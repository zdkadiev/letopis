#!/usr/bin/env python3
"""CLI transport, search, inspection and real-drain receipts with local HTTP."""
import argparse
import hashlib
from importlib.metadata import version
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

COMMANDS = []
SCHEMA = 'docs/zkit/letopis/contracts/schemas/X-1-cli.yaml'
LOCK_TESTS = (
    'filesystem::local::tests::index_owner_writes_pid_after_acquiring_lock',
    'filesystem::local::tests::std_file_and_rustix_locks_exclude_each_other_for_both_names',
    'filesystem::lock::tests::lock_open_rejects_replacement_and_never_recreates_observed_deletion',
    'filesystem::lock::tests::owner_drop_unlocks_while_duplicate_descriptor_remains_open',
    'filesystem::lock::tests::retries_interrupted_lock_acquisition_until_success',
    'filesystem::lock_tests::lock_observation_is_read_only_and_ownership_persists_until_drop',
)


def create_evidence(path):
    path.mkdir(parents=True, exist_ok=False)


def hash_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def hash_inputs(root):
    paths = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'Makefile',
             'rust-toolchain.toml', 'clippy.toml', '.dockerignore', 'skills/letopis/SKILL.md', SCHEMA)]
    for name in ('src', 'vendor', 'examples', 'scripts', 'tests/support', 'tests/cli', 'tests/fixtures'):
        paths.extend(p for p in (root / name).rglob('*') if p.is_file()
                     and '__pycache__' not in p.parts)
    paths.extend((root / 'tests').glob('*.rs'))
    return {str(p.relative_to(root)): hash_file(p) for p in sorted(paths)}


def child_environment(evidence, extra=None):
    env = dict(os.environ)
    # A supplied fixture must never bypass building the observed source.
    for key in tuple(env):
        if key.startswith('LETOPIS_FIXTURE_') or key == 'LETOPIS_CLI_FIXTURE':
            del env[key]
    env.update(LETOPIS_TEST_EVIDENCE=str(evidence / 'process'))
    env.update(extra or {})
    return env


def run_command(argv, *, cwd, env, log, timeout=600, expected=0, stdin=None):
    record = {'argv': [str(arg) for arg in argv], 'cwd': str(cwd),
              'stdout': log.name, 'stderr': log.name + '.stderr',
              'expected_exit': expected, 'timeout_seconds': timeout}
    COMMANDS.append(record)
    if stdin is not None:
        record['stdin_hex'] = stdin.hex()
        record['stdin_sha256'] = hashlib.sha256(stdin).hexdigest()
    with log.open('wb') as output, Path(str(log) + '.stderr').open('wb') as error:
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdout=output, stderr=error,
                                 stdin=subprocess.PIPE if stdin is not None else subprocess.DEVNULL,
                                 start_new_session=True)
        try:
            child.communicate(input=stdin, timeout=timeout)
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
            record.update(exit_code=child.returncode, interrupted=True)
            raise
        record['exit_code'] = child.returncode
    if child.returncode != expected:
        raise subprocess.CalledProcessError(child.returncode, argv)


def validate_response(catalogue, command, response, code):
    from jsonschema import Draft202012Validator
    branch = 'success' if response.get('ok') is True else 'error'
    schema = {'$schema': catalogue['$schema'], '$defs': catalogue['$defs'],
              **catalogue['x-commands'][command][branch]}
    Draft202012Validator.check_schema(schema)
    Draft202012Validator(schema).validate(response)
    assert code == catalogue['x-commands'][command]['exit'][branch]


def capture_transport(root, evidence, execute, binary, catalogue):
    observations = []
    cases = [
        (['check'], b'{}', None, False),
        (['reindex'], b'{}', None, False),
        (['reindex'], b'{"paths":[]}', 'invalid_request', True),
        (['inspect'], b'{"element_ids":["knowledge.md"]}', 'not_found', False),
        (['check'], b'{"extra":1}', 'invalid_request', True),
        (['inspect'], b'{}', 'invalid_request', True),
        (['embed', '--drain'], b'{"extra":1}', 'invalid_request', True),
        (['search'], b'{"query":"absent","limit":1}', None, False),
        (['embed', '--drain'], b'', None, False),
    ]
    for i, (args, body, error, pre_io) in enumerate(cases):
        # Each case owns a fresh repository: a real inspect can initialize storage.
        with tempfile.TemporaryDirectory(prefix='letopis-product-') as temporary:
            repo = Path(temporary)
            execute(['git', 'init', '-q'], f'product-{i}-git-init.log', cwd=repo)
            (repo / 'knowledge.md').write_text('private corpus sentinel\n')
            execute(['git', 'add', 'knowledge.md'], f'product-{i}-git-add.log', cwd=repo)
            execute(['git', '-c', 'user.name=Test', '-c', 'user.email=test@example.org',
                     'commit', '-qm', 'fixture'], f'product-{i}-git-commit.log', cwd=repo)
            name = f'product-{i}.json'
            expected = 1 if error else 0
            execute([str(binary), *args], name, cwd=repo, expected=expected, stdin=body)
            raw = (evidence / name).read_bytes()
            assert raw.endswith(b'\n') and raw.count(b'\n') == 1
            assert not Path(str(evidence / name) + '.stderr').read_bytes()
            response = json.loads(raw)
            validate_response(catalogue, ' '.join(args), response, expected)
            if error:
                assert response['error']['code'] == error
            if args == ['reindex'] and not error:
                assert response == {'ok': True, 'result': {'revision': 1}}
            if args == ['check'] and not error:
                assert response == {'ok': True, 'result': {'errors': []}}
            if pre_io:
                assert not (repo / '.letopis').exists()
            observations.append({'source': 'actual product executable', 'command': ' '.join(args),
                                 'exit': expected, 'response': response})
    (evidence / 'product-transport-responses.json').write_text(json.dumps(observations, indent=2) + '\n')


def capture_handler_responses(evidence, catalogue):
    responses = []
    for line in (evidence / 'handlers.log').read_text().splitlines():
        if 'CLI_RESPONSE ' not in line:
            continue
        record = json.loads(line.split('CLI_RESPONSE ', 1)[1])
        response = record['response']
        result = response.get('result', {})
        if isinstance(result, list):
            command = 'search'
        else:
            command = 'inspect' if 'elements' in result else 'check'
            if response.get('ok') and 'revision' in result:
                command = 'status' if 'embedding_backlog' in result else 'reindex'
            if response.get('ok') and not result:
                command = 'embed --drain'
        validate_response(catalogue, command, response, record['exit'])
        responses.append({'source': 'injected handler unit test', 'command': command, **record})
    assert any(r['command'] == 'check' and r['exit'] == 0 for r in responses)
    assert any(r['command'] == 'inspect' and r['exit'] == 0 for r in responses)
    assert any(r['command'] == 'search' and r['exit'] == 0 for r in responses)
    assert any(r['exit'] == 1 for r in responses)
    (evidence / 'handler-responses.json').write_text(json.dumps(responses, indent=2) + '\n')


def capture_reading_responses(evidence, catalogue):
    responses = []
    expected = {'search_success': ('search', 0, None),
                'search_fallback': ('search', 0, None),
                'inspect_success': ('inspect', 0, None),
                'inspect_mixed': ('inspect', 0, None),
                'inspect_removed': ('inspect', 1, 'not_found'),
                'not_found': ('inspect', 1, 'not_found'),
                'malformed_input': ('search', 1, 'invalid_request')}
    for line in (evidence / 'runtime.log').read_text().splitlines():
        if 'PRODUCT_READING ' not in line:
            continue
        record = json.loads(line.split('PRODUCT_READING ', 1)[1])
        validate_response(catalogue, record['command'], record['response'], record['exit'])
        assert record['case'] in expected
        command, code, error = expected[record['case']]
        assert (record['command'], record['exit']) == (command, code)
        if error:
            assert record['response']['error']['code'] == error
        if record['case'] == 'inspect_mixed':
            elements = record['response']['result']['elements']
            assert set(elements) == {'knowledge.md', 'src/cache.rs#L2-L2',
                                     'src/missing.rs', 'https://example.invalid/a?q=1#fragment'}
            assert elements['src/cache.rs#L2-L2']['path'] == 'src/cache.rs'
            assert elements['src/cache.rs#L2-L2']['lines'] == [2, 2]
            assert elements['src/cache.rs#L2-L2']['hash'] is not None
            assert elements['src/missing.rs'] == {'path': 'src/missing.rs', 'hash': None}
            assert elements['https://example.invalid/a?q=1#fragment'] == {
                'path': 'https://example.invalid/a?q=1#fragment', 'hash': None}
        responses.append({**record, 'source': 'actual product executable'})
    assert {record['case'] for record in responses} == set(expected)
    (evidence / 'product-reading-responses.json').write_text(json.dumps(responses, indent=2) + '\n')


def capture_reindex_responses(evidence, catalogue):
    responses = []
    for line in (evidence / 'reindex.log').read_text().splitlines():
        if 'PRODUCT_REINDEX ' not in line:
            continue
        response = json.loads(line.split('PRODUCT_REINDEX ', 1)[1])
        validate_response(catalogue, 'reindex', response, 0)
        responses.append({'source': 'actual product executable', 'command': 'reindex',
                          'exit': 0, 'response': response})
    assert responses, 'actual reindex responses missing'
    (evidence / 'product-reindex-responses.json').write_text(json.dumps(responses, indent=2) + '\n')


def run_host(root, evidence, execute, manifest):
    from jsonschema import Draft202012Validator
    manifest['jsonschema'] = version('jsonschema')
    catalogue = json.loads((root / SCHEMA).read_text())
    Draft202012Validator.check_schema(catalogue)
    for tool in (['rustc', '-vV'], ['cargo', '--version'], ['git', '--version'],
                 [sys.executable, '--version'], ['uname', '-a']):
        execute(tool, 'version-' + Path(tool[0]).name + '.log')
    execute(['make', 'verify', 'V=1'], 'root-check.log')
    execute(['cargo', 'build', '--locked', '--offline', '--bin', 'letopis',
             '--example', 'cli_runtime_fixture', '--message-format=json'], 'build.log')
    artifacts = read_artifacts(evidence / 'build.log')
    assert set(artifacts) == {'letopis', 'cli_runtime_fixture'}
    manifest['executables'] = {}
    for name, artifact in artifacts.items():
        shutil.copy2(artifact, evidence / name)
        manifest['executables'][name] = {'cargo_path': str(artifact), 'sha256': hash_file(artifact)}
    execute(['cargo', 'test', '--locked', '--offline', '--lib', 'cli::',
             '--', '--nocapture', '--test-threads=1'], 'handlers.log')
    capture_handler_responses(evidence, catalogue)
    capture_lock_tests(evidence, execute)
    execute(['cargo', 'test', '--locked', '--offline', '--test', 'cli_process',
             '--test', 'cli_runtime', '--test', 'cli_reading', '--', '--nocapture', '--test-threads=1'],
            'runtime.log', extra={'LETOPIS_CLI_FIXTURE': str(artifacts['cli_runtime_fixture'])})
    capture_reading_responses(evidence, catalogue)
    execute(['cargo', 'test', '--locked', '--offline', '--test', 'reindex_command',
             '--', '--nocapture', '--test-threads=1'], 'reindex.log')
    capture_reindex_responses(evidence, catalogue)
    execute(['cargo', 'test', '--locked', '--offline', '--test', 'thin_slice',
             '--', '--nocapture', '--test-threads=1'], 'thin-slice.log')
    capture_thin_slice_responses(evidence, catalogue)
    observations = [json.loads(line.split('CLI_OBSERVER ', 1)[1])
                    for line in (evidence / 'runtime.log').read_text().splitlines()
                    if 'CLI_OBSERVER ' in line]
    assert any(o.get('null_fds') == [True, True, True] for o in observations)
    assert any(o.get('owned') is False for o in observations)
    (evidence / 'observers.json').write_text(json.dumps(observations, indent=2) + '\n')
    execute(['cargo', 'test', '--locked', '--offline', '--test', 'cli_drain',
             '--', '--nocapture', '--test-threads=1'], 'product-drain.log',
            extra={'LETOPIS_CLI_FIXTURE': str(artifacts['cli_runtime_fixture'])})
    captures = []
    for line in (evidence / 'product-drain.log').read_text().splitlines():
        for marker in ('PRODUCT_RESPONSE ', 'PRODUCT_FOREGROUND ', 'PRODUCT_DRAIN '):
            if marker not in line:
                continue
            value = json.loads(line.split(marker, 1)[1])
            if marker == 'PRODUCT_RESPONSE ':
                validate_response(catalogue, 'embed --drain', value, 0)
            captures.append({'kind': marker.strip(), 'observation': value})
    assert any(item['kind'] == 'PRODUCT_DRAIN' for item in captures)
    (evidence / 'product-observations.json').write_text(json.dumps(captures, indent=2) + '\n')
    capture_transport(root, evidence, execute, evidence / 'letopis', catalogue)


def capture_lock_tests(evidence, execute):
    execute(['cargo', 'test', '--locked', '--offline', '--lib', 'filesystem::',
             '--', '--nocapture', '--test-threads=1'], 'locks.log')
    passed = {line[len('test '):-len(' ... ok')]
              for line in (evidence / 'locks.log').read_text().splitlines()
              if line.startswith('test ') and line.endswith(' ... ok')}
    missing = set(LOCK_TESTS) - passed
    assert not missing, f'required lock tests missing: {sorted(missing)}'


def capture_thin_slice_responses(evidence, catalogue):
    expected = {case: ('search', 0, None) for case in
                ('load', 'background', 'provider_failure', 'invalid_neighbor', 'text_race')}
    expected.update(inspect=('inspect', 0, None),
                    invalid_inspect=('inspect', 1, 'validation_failed'))
    responses = []
    for line in (evidence / 'thin-slice.log').read_text().splitlines():
        if 'THIN_SLICE_RESPONSE ' not in line:
            continue
        record = json.loads(line.split('THIN_SLICE_RESPONSE ', 1)[1])
        assert record['case'] in expected, 'unknown thin-slice scenario'
        command, code, error = expected[record['case']]
        assert (record['command'], record['exit']) == (command, code)
        validate_response(catalogue, command, record['response'], code)
        if error:
            assert record['response']['error']['code'] == error
        responses.append({**record, 'source': 'actual product executable'})
    assert {record['case'] for record in responses} == set(expected)
    (evidence / 'thin-slice-responses.json').write_text(json.dumps(responses, indent=2) + '\n')


def read_artifacts(log):
    artifacts = {}
    for line in log.read_text().splitlines():
        message = json.loads(line)
        if message.get('reason') == 'compiler-artifact' and message.get('executable'):
            artifacts[message['target']['name']] = Path(message['executable'])
    return artifacts


def run_linux(root, evidence, execute, manifest):
    tag = 'letopis-tk11-' + uuid.uuid4().hex
    container = tag + '-run'
    execute(['docker', 'version'], 'docker-version.log')
    execute(['docker', 'build', '-f', 'tests/cli/Dockerfile', '-t', tag,
             '--build-arg', 'SOURCE_COMMIT=' + manifest['source_commit'], '.'],
            'docker-build.log', timeout=1800)
    execute(['docker', 'image', 'inspect', tag], 'image.json')
    try:
        execute(['docker', 'run', '--name', container, '--network', 'none', tag],
                'linux.log', timeout=1800)
    finally:
        try:
            execute(['docker', 'cp', container + ':/receipt/.', str(evidence / 'container')],
                    'docker-copy.log')
        finally:
            execute(['docker', 'rm', '-f', container], 'docker-cleanup.log')
    inner = json.loads((evidence / 'container/manifest.json').read_text())
    assert inner['status'] == 'PASS'
    assert inner['inputs'] == manifest['inputs'], 'host/Linux source inventory differs'
    manifest['container_manifest_sha256'] = hash_file(evidence / 'container/manifest.json')


def main():
    if sys.version_info < (3, 11):
        raise SystemExit('Python 3.11+ required')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['host', 'linux'], required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    create_evidence(evidence)
    COMMANDS.clear()
    root = Path(__file__).resolve().parents[2]
    manifest = {'status': 'FAIL', 'platform': platform.platform(), 'machine': platform.machine(),
                'python': sys.version, 'commands': COMMANDS,
                'environment': {k: os.environ[k] for k in ('PATH', 'CARGO_HOME', 'RUSTUP_HOME',
                   'CARGO_TARGET_DIR', 'RUSTFLAGS') if k in os.environ},
                'limits': 'Real product check/search/mixed inspect/drain with controlled local HTTP; no real model quality or unrun platform evidence.'}

    def execute(argv, name, *, cwd=root, extra=None, **kwargs):
        run_command(argv, cwd=cwd, env=child_environment(evidence, extra),
                    log=evidence / name, **kwargs)

    try:
        manifest['inputs'] = hash_inputs(root)
        if (root / '.git').exists():
            execute(['git', 'rev-parse', 'HEAD'], 'source-commit.log')
            execute(['git', 'status', '--short'], 'source-dirty.log')
            manifest['source_commit'] = (evidence / 'source-commit.log').read_text().strip()
        else:
            manifest['source_commit'] = (root / 'SOURCE_COMMIT').read_text().strip()
        if args.platform == 'host':
            run_host(root, evidence, execute, manifest)
        else:
            run_linux(root, evidence, execute, manifest)
        assert hash_inputs(root) == manifest['inputs'], 'source changed during acceptance'
        manifest['status'] = 'PASS'
    except BaseException as error:
        manifest['error'] = repr(error)
        raise
    finally:
        (evidence / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
