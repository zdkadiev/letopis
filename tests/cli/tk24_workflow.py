#!/usr/bin/env python3
"""Run the TK-24 agent workflow against an isolated real Git repository."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess

from run import SCHEMA, validate_response


def run(argv, cwd, records, *, body=None, env=None, expected=0):
    data = None if body is None else json.dumps(body, separators=(',', ':')).encode()
    if env is None:
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    result = subprocess.run(argv, cwd=cwd, env=env, input=data, capture_output=True, check=False)
    record = {'argv': argv, 'cwd': str(cwd), 'exit': result.returncode,
              'stdin': body, 'stdout': result.stdout.decode(errors='replace'),
              'stderr': result.stderr.decode(errors='replace')}
    records.append(record)
    assert result.returncode == expected, record
    return result.stdout


def command(binary, name, body, repo, records, catalogue, *, error=None):
    raw = run([str(binary), name], repo, records, body=body, expected=1 if error else 0)
    assert raw.endswith(b'\n') and raw.count(b'\n') == 1
    assert records[-1]['stderr'] == ''
    response = json.loads(raw)
    validate_response(catalogue, name, response, 1 if error else 0)
    if error:
        assert response['error']['code'] == error and 'result' not in response
    return response


def git(repo, records, *args, env=None):
    return run(['git', *args], repo, records, env=env).strip()


def git_bytes(repo, records, *args, env=None):
    return run(['git', *args], repo, records, env=env)


def write_knowledge(repo, basis, *, renamed=False):
    if renamed:
        path = repo / 'knowledge/new.md'
        heading = '## Cache choice {#cache-new}'
    else:
        path = repo / 'knowledge/decision.md'
        heading = '## Cache choice {#cache}'
    text = (f'{heading}\n'
            f'<!-- letopis: {{"created_at":1792800000,"source_snapshot":"{basis}"}} -->\n'
            'Checked cache source.\n'
            '[source](src/cache.rs#L2-L4 "letopis:type=external")\n')
    path.parent.mkdir(exist_ok=True)
    path.write_text(''.join(text))
    return path


def check_empty(binary, repo, records, catalogue):
    response = command(binary, 'check', {}, repo, records, catalogue)
    assert response['result']['errors'] == [], response


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    repo = evidence / 'repo'
    repo.mkdir()
    records = []
    catalogue = json.loads((Path(__file__).resolve().parents[2] / SCHEMA).read_text())
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    binary = args.binary.resolve()
    try:
        git(repo, records, 'init', '-q', env=env)
        git(repo, records, 'config', 'user.name', 'Fixture', env=env)
        git(repo, records, 'config', 'user.email', 'fixture@example.invalid', env=env)
        (repo / 'src').mkdir()
        original = b'fn cache() {\n    println!("v1");\n    println!("ok");\n}\n'
        edited = b'fn cache() {\n    println!("v2");\n    println!("ok");\n}\n'
        (repo / 'src/cache.rs').write_bytes(original)
        (repo / '.gitignore').write_text('.letopis/\n')
        git(repo, records, 'add', '.gitignore', 'src/cache.rs', env=env)
        git(repo, records, 'commit', '-qm', 'source basis', env=env)
        initial_tree = git(repo, records, 'rev-parse', 'HEAD^{tree}', env=env).decode()
        head_before = git(repo, records, 'rev-parse', 'HEAD', env=env).decode()
        (repo / 'knowledge').mkdir()
        decision = write_knowledge(repo, initial_tree)
        referer = repo / 'knowledge/referer.md'
        referer.write_text('<!-- letopis: {"created_at":1792800000} -->\n'
                           '[decision](knowledge/decision.md#cache "letopis:type=depends_on")\n')
        (repo / 'unrelated.txt').write_text('user staged change\n')
        git(repo, records, 'add', 'unrelated.txt', env=env)
        index_before = (repo / '.git/index').read_bytes()
        check_empty(binary, repo, records, catalogue)
        found = command(binary, 'search', {'query': 'Checked cache source', 'limit': 10},
                        repo, records, catalogue)
        assert any(row['element_id'] == 'knowledge/decision.md#cache' for row in found['result'])
        walked = command(binary, 'traverse', {'element_ids': ['knowledge/referer.md'],
                         'mode': 'relations', 'direction': 'outgoing', 'depth': 1,
                         'limit': 10, 'edge_limit': 10}, repo, records, catalogue)
        assert any(row['element_id'] == 'knowledge/decision.md#cache'
                   for row in walked['result']['elements'])
        mixed = command(binary, 'inspect', {'element_ids': [
            'knowledge/decision.md#cache', 'src/cache.rs#L2-L4']}, repo, records, catalogue)
        snap = mixed['result']['snapshot']
        assert git_bytes(repo, records, 'show', f'{snap}:knowledge/decision.md', env=env) == decision.read_bytes()
        assert git_bytes(repo, records, 'show', f'{snap}:src/cache.rs', env=env) == original
        assert mixed['result']['elements']['src/cache.rs#L2-L4']['hash'] == hashlib.sha256(
            b'    println!("v1");\n    println!("ok");\n}\n').hexdigest()
        (repo / 'src/cache.rs').write_bytes(edited)
        check_empty(binary, repo, records, catalogue)
        stale = command(binary, 'search', {'query': 'Checked cache source', 'limit': 10,
                        'filters': {'needs_recheck': True}}, repo, records, catalogue)
        assert any(row['element_id'] == 'knowledge/decision.md#cache' for row in stale['result'])
        current = command(binary, 'inspect', {'element_ids': ['knowledge/decision.md#cache',
                          'src/cache.rs#L2-L4']}, repo, records, catalogue)
        new_reading = current['result']['snapshot']
        assert git_bytes(repo, records, 'show', f'{new_reading}:src/cache.rs', env=env) == edited
        assert git_bytes(repo, records, 'show', f'{initial_tree}:src/cache.rs', env=env) == original
        temp_index = evidence / 'temporary-index'
        temp_env = {**env, 'GIT_INDEX_FILE': str(temp_index)}
        git(repo, records, 'read-tree', 'HEAD', env=temp_env)
        git(repo, records, 'add', '--', 'src/cache.rs', env=temp_env)
        verified_tree = git(repo, records, 'write-tree', env=temp_env).decode()
        assert git_bytes(repo, records, 'show', f'{verified_tree}:src/cache.rs', env=env) == edited
        assert verified_tree != initial_tree
        write_knowledge(repo, verified_tree)
        check_empty(binary, repo, records, catalogue)
        decision.unlink()
        write_knowledge(repo, verified_tree, renamed=True)
        dangling = command(binary, 'check', {}, repo, records, catalogue)
        assert dangling['result']['errors'], dangling
        referer.write_text('<!-- letopis: {"created_at":1792800000} -->\n'
                           '[decision](knowledge/new.md#cache-new "letopis:type=depends_on")\n')
        check_empty(binary, repo, records, catalogue)
        renamed = command(binary, 'inspect', {'element_ids': ['knowledge/new.md#cache-new',
                          'src/cache.rs#L2-L4']}, repo, records, catalogue)
        assert set(renamed['result']['elements']) == {
            'knowledge/new.md#cache-new', 'src/cache.rs#L2-L4'}
        command(binary, 'inspect', {'element_ids': ['knowledge/decision.md#cache',
                'knowledge/new.md#cache-new']}, repo, records, catalogue, error='not_found')
        assert (repo / '.git/index').read_bytes() == index_before
        assert git(repo, records, 'rev-parse', 'HEAD', env=env).decode() == head_before
        result = {'status': 'PASS', 'initial_tree': initial_tree, 'verified_tree': verified_tree,
                  'initial_reading': snap, 'edited_reading': new_reading,
                  'final_reading': renamed['result']['snapshot'],
                  'index_sha256_before': hashlib.sha256(index_before).hexdigest(),
                  'index_sha256_after': hashlib.sha256((repo / '.git/index').read_bytes()).hexdigest(),
                  'head_before': head_before, 'head_after': head_before, 'commands': records}
    except BaseException as error:
        result = {'status': 'FAIL', 'error': repr(error), 'commands': records}
        raise
    finally:
        (evidence / 'workflow.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
