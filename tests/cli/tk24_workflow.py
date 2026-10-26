#!/usr/bin/env python3
"""Run the TK-24 agent workflow against an isolated real Git repository."""
import argparse
from dataclasses import dataclass
import hashlib
import json
import os
from pathlib import Path
import subprocess

from run import SCHEMA, validate_response

ORIGINAL_SOURCE = b'fn cache() {\n    println!("v1");\n    println!("ok");\n}\n'
EDITED_SOURCE = b'fn cache() {\n    println!("v2");\n    println!("ok");\n}\n'
ORIGINAL_RANGE = b'    println!("v1");\n    println!("ok");\n}\n'
EDITED_RANGE = b'    println!("v2");\n    println!("ok");\n}\n'
KNOWLEDGE_ID = 'knowledge/decision.md#cache'
SOURCE_ID = 'src/cache.rs#L2-L4'
RENAMED_ID = 'knowledge/new.md#cache-new'


@dataclass
class Workflow:
    binary: Path
    evidence: Path
    repo: Path
    catalogue: dict
    env: dict
    records: list


@dataclass
class Fixture:
    decision: Path
    referer: Path
    initial_tree: str
    head_before: str
    index_before: bytes


def run(argv, cwd, records, *, body=None, env=None, expected=0):
    data = None if body is None else json.dumps(body, separators=(',', ':')).encode()
    if env is None:
        env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
        env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    result = subprocess.run(argv, cwd=cwd, env=env, input=data, capture_output=True, check=False)
    record = {'argv': argv, 'cwd': str(cwd), 'exit': result.returncode,
              'stdin': body, 'stdout': result.stdout.decode(errors='replace'),
              'stderr': result.stderr.decode(errors='replace')}
    if 'GIT_INDEX_FILE' in env:
        record['git_index_file'] = env['GIT_INDEX_FILE']
    records.append(record)
    assert result.returncode == expected, record
    return result.stdout


def command(workflow, name, body, *, error=None):
    raw = run([str(workflow.binary), name], workflow.repo, workflow.records,
              body=body, expected=1 if error else 0)
    assert raw.endswith(b'\n') and raw.count(b'\n') == 1
    assert workflow.records[-1]['stderr'] == ''
    response = json.loads(raw)
    validate_response(workflow.catalogue, name, response, 1 if error else 0)
    if error:
        assert response['error']['code'] == error and 'result' not in response
    return response


def git(workflow, *args, env=None):
    return run(['git', *args], workflow.repo, workflow.records,
               env=workflow.env if env is None else env).strip()


def git_bytes(workflow, *args):
    return run(['git', *args], workflow.repo, workflow.records, env=workflow.env)


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


def check_empty(workflow):
    response = command(workflow, 'check', {})
    assert response['result']['errors'] == [], response


def prepare_repository(workflow):
    git(workflow, 'init', '-q')
    git(workflow, 'config', 'user.name', 'Fixture')
    git(workflow, 'config', 'user.email', 'fixture@example.invalid')
    (workflow.repo / 'src').mkdir()
    (workflow.repo / 'src/cache.rs').write_bytes(ORIGINAL_SOURCE)
    (workflow.repo / '.gitignore').write_text('.letopis/\n')
    git(workflow, 'add', '.gitignore', 'src/cache.rs')
    git(workflow, 'commit', '-qm', 'source basis')
    initial_tree = git(workflow, 'rev-parse', 'HEAD^{tree}').decode()
    head_before = git(workflow, 'rev-parse', 'HEAD').decode()
    (workflow.repo / 'knowledge').mkdir()
    decision = write_knowledge(workflow.repo, initial_tree)
    referer = workflow.repo / 'knowledge/referer.md'
    referer.write_text('<!-- letopis: {"created_at":1792800000} -->\n'
                       '[decision](knowledge/decision.md#cache "letopis:type=depends_on")\n')
    (workflow.repo / 'unrelated.txt').write_text('user staged change\n')
    git(workflow, 'add', 'unrelated.txt')
    index_before = (workflow.repo / '.git/index').read_bytes()
    return Fixture(decision, referer, initial_tree, head_before, index_before)


def verify_initial_read(workflow, fixture):
    check_empty(workflow)
    found = command(workflow, 'search', {'query': 'Checked cache source', 'limit': 10})
    assert any(row['element_id'] == KNOWLEDGE_ID for row in found['result'])
    walked = command(workflow, 'traverse', {'element_ids': ['knowledge/referer.md'],
                     'mode': 'relations', 'direction': 'outgoing', 'depth': 1,
                     'limit': 10, 'edge_limit': 10})
    assert any(row['element_id'] == KNOWLEDGE_ID for row in walked['result']['elements'])
    mixed = command(workflow, 'inspect', {'element_ids': [KNOWLEDGE_ID, SOURCE_ID]})
    snapshot = mixed['result']['snapshot']
    assert git_bytes(workflow, 'show', f'{snapshot}:knowledge/decision.md') == fixture.decision.read_bytes()
    assert git_bytes(workflow, 'show', f'{snapshot}:src/cache.rs') == ORIGINAL_SOURCE
    assert mixed['result']['elements'][SOURCE_ID]['hash'] == hashlib.sha256(ORIGINAL_RANGE).hexdigest()
    return snapshot


def verify_edited_source(workflow, fixture):
    (workflow.repo / 'src/cache.rs').write_bytes(EDITED_SOURCE)
    check_empty(workflow)
    stale = command(workflow, 'search', {'query': 'Checked cache source', 'limit': 10,
                    'filters': {'needs_recheck': True}})
    matches = [row for row in stale['result'] if row['element_id'] == KNOWLEDGE_ID]
    assert len(matches) == 1 and matches[0]['flags'] == ['source_changed'], stale
    current = command(workflow, 'inspect', {'element_ids': [KNOWLEDGE_ID, SOURCE_ID]})
    reading = current['result']['snapshot']
    assert git_bytes(workflow, 'show', f'{reading}:src/cache.rs') == EDITED_SOURCE
    assert git_bytes(workflow, 'show', f'{fixture.initial_tree}:src/cache.rs') == ORIGINAL_SOURCE
    assert current['result']['elements'][SOURCE_ID]['hash'] == hashlib.sha256(EDITED_RANGE).hexdigest()
    temp_env = {**workflow.env, 'GIT_INDEX_FILE': str(workflow.evidence / 'temporary-index')}
    git(workflow, 'read-tree', 'HEAD', env=temp_env)
    git(workflow, 'add', '--', 'src/cache.rs', env=temp_env)
    verified_tree = git(workflow, 'write-tree', env=temp_env).decode()
    assert git_bytes(workflow, 'show', f'{verified_tree}:src/cache.rs') == EDITED_SOURCE
    assert verified_tree != fixture.initial_tree
    write_knowledge(workflow.repo, verified_tree)
    check_empty(workflow)
    resolved = command(workflow, 'search', {'query': 'Checked cache source', 'limit': 10,
                       'filters': {'needs_recheck': True}})
    assert resolved['result'] == [], resolved
    return reading, verified_tree, matches[0]['flags']


def verify_rename_and_repair(workflow, fixture, verified_tree):
    fixture.decision.unlink()
    write_knowledge(workflow.repo, verified_tree, renamed=True)
    dangling = command(workflow, 'check', {})
    errors = dangling['result']['errors']
    assert len(errors) == 1, dangling
    assert errors[0]['code'] == 'validation_failed', dangling
    assert errors[0]['details']['reason'] == 'dangling_target', dangling
    assert errors[0]['details']['element_id'] == 'knowledge/referer.md', dangling
    assert errors[0]['details']['target'] == KNOWLEDGE_ID, dangling
    fixture.referer.write_text('<!-- letopis: {"created_at":1792800000} -->\n'
                               '[decision](knowledge/new.md#cache-new "letopis:type=depends_on")\n')
    check_empty(workflow)
    renamed = command(workflow, 'inspect', {'element_ids': [RENAMED_ID, SOURCE_ID]})
    assert set(renamed['result']['elements']) == {RENAMED_ID, SOURCE_ID}
    command(workflow, 'inspect', {'element_ids': [KNOWLEDGE_ID, RENAMED_ID]},
            error='not_found')
    return renamed['result']['snapshot'], errors[0]


def verify_user_git_state(workflow, fixture):
    index_after = (workflow.repo / '.git/index').read_bytes()
    head_after = git(workflow, 'rev-parse', 'HEAD').decode()
    assert index_after == fixture.index_before
    assert head_after == fixture.head_before
    return {'index_sha256_before': hashlib.sha256(fixture.index_before).hexdigest(),
            'index_sha256_after': hashlib.sha256(index_after).hexdigest(),
            'head_before': fixture.head_before, 'head_after': head_after}


def run_workflow(workflow):
    fixture = prepare_repository(workflow)
    first_reading = verify_initial_read(workflow, fixture)
    edited_reading, verified_tree, source_flags = verify_edited_source(
        workflow, fixture)
    final_reading, dangling_error = verify_rename_and_repair(
        workflow, fixture, verified_tree)
    git_state = verify_user_git_state(workflow, fixture)
    return {'status': 'PASS', 'initial_tree': fixture.initial_tree,
            'verified_tree': verified_tree, 'initial_reading': first_reading,
            'edited_reading': edited_reading, 'final_reading': final_reading,
            'observed_source_flags': source_flags,
            'observed_dangling_error': dangling_error, **git_state}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    repo = evidence / 'repo'
    repo.mkdir()
    catalogue = json.loads((Path(__file__).resolve().parents[2] / SCHEMA).read_text())
    env = {k: v for k, v in os.environ.items() if not k.startswith('GIT_')}
    env.update(GIT_CONFIG_NOSYSTEM='1', GIT_CONFIG_GLOBAL='/dev/null')
    workflow = Workflow(args.binary.resolve(), evidence, repo, catalogue, env, [])
    try:
        result = run_workflow(workflow)
    except BaseException as error:
        result = {'status': 'FAIL', 'error': repr(error)}
        raise
    finally:
        result['helper_sha256'] = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
        result['binary_sha256'] = hashlib.sha256(workflow.binary.read_bytes()).hexdigest()
        result['commands'] = workflow.records
        (evidence / 'workflow.json').write_text(json.dumps(result, indent=2) + '\n')


if __name__ == '__main__':
    main()
