#!/usr/bin/env python3
"""TK-11 transport/process receipts; Python 3.11+, prepared jsonschema, offline runtime."""
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


def create_evidence(path):
    path.mkdir(parents=True, exist_ok=False)


def hash_file(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def hash_inputs(root):
    paths = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs',
             'rust-toolchain.toml', 'clippy.toml', '.dockerignore', SCHEMA)]
    for name in ('src', 'vendor', 'examples', 'scripts', 'tests/support', 'tests/cli', 'tests/fixtures/markdown'):
        paths.extend(p for p in (root / name).rglob('*') if p.is_file()
                     and '__pycache__' not in p.parts)
    paths.extend((root / 'tests').glob('cli_*.rs'))
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
    with tempfile.TemporaryDirectory(prefix='tk11-product-') as temporary:
        repo = Path(temporary)
        execute(['git', 'init', '-q'], 'product-git-init.log', cwd=repo)
        (repo / 'knowledge.md').write_text('private corpus sentinel\n')
        execute(['git', 'add', 'knowledge.md'], 'product-git-add.log', cwd=repo)
        execute(['git', '-c', 'user.name=Test', '-c', 'user.email=test@example.org',
                 'commit', '-qm', 'fixture'], 'product-git-commit.log', cwd=repo)
        for i, (args, body, error) in enumerate([
            (['check'], b'{}', 'internal_error'),
            (['inspect'], b'{"element_ids":["knowledge.md"]}', 'internal_error'),
            (['embed', '--drain'], b'', 'internal_error'),
            (['check'], b'{"extra":1}', 'invalid_request'),
            (['inspect'], b'{}', 'invalid_request'),
            (['embed', '--drain'], b'{"extra":1}', 'invalid_request'),
        ]):
            name = f'product-{i}.json'
            execute([str(binary), *args], name, cwd=repo, expected=1, stdin=body)
            raw = (evidence / name).read_bytes()
            assert raw.endswith(b'\n') and raw.count(b'\n') == 1
            assert not Path(str(evidence / name) + '.stderr').read_bytes()
            response = json.loads(raw)
            validate_response(catalogue, ' '.join(args), response, 1)
            assert response['error']['code'] == error
            if error == 'internal_error':
                assert response['error']['message'] == 'operation unavailable in this build'
        assert not (repo / '.letopis').exists()


def capture_handler_responses(evidence, catalogue):
    responses = []
    for line in (evidence / 'handlers.log').read_text().splitlines():
        if 'CLI_RESPONSE ' not in line:
            continue
        record = json.loads(line.split('CLI_RESPONSE ', 1)[1])
        response = record['response']
        result = response.get('result', {})
        command = 'inspect' if 'elements' in result else 'check'
        if response.get('ok') and not result:
            command = 'embed --drain'
        validate_response(catalogue, command, response, record['exit'])
        responses.append({'source': 'injected handler unit test', 'command': command, **record})
    assert any(r['command'] == 'check' and r['exit'] == 0 for r in responses)
    assert any(r['command'] == 'inspect' and r['exit'] == 0 for r in responses)
    assert any(r['exit'] == 1 for r in responses)
    (evidence / 'handler-responses.json').write_text(json.dumps(responses, indent=2) + '\n')


def run_host(root, evidence, execute, manifest):
    from jsonschema import Draft202012Validator
    manifest['jsonschema'] = version('jsonschema')
    catalogue = json.loads((root / SCHEMA).read_text())
    Draft202012Validator.check_schema(catalogue)
    for tool in (['rustc', '-vV'], ['cargo', '--version'], ['git', '--version'],
                 [sys.executable, '--version'], ['uname', '-a']):
        execute(tool, 'version-' + Path(tool[0]).name + '.log')
    execute(['cargo', 'build', '--locked', '--offline', '--bin', 'letopis',
             '--example', 'cli_runtime_fixture', '--message-format=json'], 'build.log')
    artifacts = {}
    for line in (evidence / 'build.log').read_text().splitlines():
        message = json.loads(line)
        if message.get('reason') == 'compiler-artifact' and message.get('executable'):
            artifacts[message['target']['name']] = Path(message['executable'])
    assert set(artifacts) == {'letopis', 'cli_runtime_fixture'}
    manifest['executables'] = {}
    for name, artifact in artifacts.items():
        shutil.copy2(artifact, evidence / name)
        manifest['executables'][name] = {'cargo_path': str(artifact), 'sha256': hash_file(artifact)}
    execute(['cargo', 'test', '--locked', '--offline', '--lib', 'cli::tests::',
             '--', '--nocapture', '--test-threads=1'], 'handlers.log')
    capture_handler_responses(evidence, catalogue)
    execute(['cargo', 'test', '--locked', '--offline', '--lib', 'filesystem::local::tests::',
             '--', '--nocapture', '--test-threads=1'], 'locks.log')
    execute(['cargo', 'test', '--locked', '--offline', '--test', 'cli_process',
             '--test', 'cli_runtime', '--', '--nocapture', '--test-threads=1'],
            'runtime.log', extra={'LETOPIS_CLI_FIXTURE': str(artifacts['cli_runtime_fixture'])})
    observations = [json.loads(line.split('CLI_OBSERVER ', 1)[1])
                    for line in (evidence / 'runtime.log').read_text().splitlines()
                    if 'CLI_OBSERVER ' in line]
    assert any(o.get('null_fds') == [True, True, True] for o in observations)
    assert any(o.get('owned') is False for o in observations)
    (evidence / 'observers.json').write_text(json.dumps(observations, indent=2) + '\n')
    capture_transport(root, evidence, execute, artifacts['letopis'], catalogue)


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
                'limits': 'Transport and fixture runtime only; real drain requires Task 5.'}

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
