#!/usr/bin/env python3
"""Product-linked acceptance receipts; Python 3.11+, no runtime network."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import uuid

COMMANDS = []
IGNORED_INPUT_DIRECTORIES = frozenset({'target', '.build', '__pycache__'})


def create_evidence(path: Path) -> None:
    path.mkdir(parents=True, exist_ok=False)


def run_command(argv: list[str], *, cwd: Path, env: dict[str, str],
                log: Path, timeout: float) -> None:
    record = {'argv': argv, 'cwd': str(cwd), 'log': log.name}
    COMMANDS.append(record)
    with log.open('wb') as output:
        child = subprocess.Popen(argv, cwd=cwd, env=env, stdout=output,
                                 stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = child.wait(timeout=timeout)
        except BaseException:
            os.killpg(child.pid, signal.SIGKILL)
            child.wait()
            record['exit_code'] = child.returncode
            record['interrupted'] = True
            raise
        record['exit_code'] = code
    if code:
        raise subprocess.CalledProcessError(code, argv)


def hash_inputs(root):
    paths = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs', 'Makefile', '.cargo/mutants.toml',
              'rust-toolchain.toml', 'clippy.toml', '.dockerignore',
              'docs/zkit/letopis/mechanics/storage-schema.sql')]
    for name in ('src', 'vendor', 'examples', 'tests', 'scripts'):
        for directory, children, files in os.walk(root / name):
            children[:] = [child for child in children
                           if child not in IGNORED_INPUT_DIRECTORIES]
            paths.extend(Path(directory) / filename for filename in files
                         if (Path(directory) / filename).is_file())
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(paths)}


def run_host(root, evidence, execute):
    execute([sys.executable, '-B', 'tests/storage/check_schema.py', str(root)],
            'schema-parity.log')
    execute([sys.executable, '-B', '-m', 'unittest', 'discover', '-s',
             'tests/storage', '-p', 'test_runner.py', '-v'], 'runner-tests.log')
    for tool in (['rustc', '-vV'], ['cargo', '--version'], ['git', '--version'],
                 [sys.executable, '--version'], ['uname', '-a']):
        execute(tool, 'version-' + Path(tool[0]).name + '.log')
    process_env = {'LETOPIS_TEST_EVIDENCE': str(evidence / 'process')}
    execute(['make', 'verify', 'V=1'], 'rust-check.log', extra=process_env)
    execute(['cargo', 'test', '--locked', '--offline', '--test',
             'embedding_provider', 'attempt_obeys_real_sixty_second_deadline',
             '--', '--ignored', '--exact', '--nocapture'],
            'provider-real-deadline.log', extra=process_env)
    execute(['cargo', 'doc', '--locked', '--offline', '--no-deps'],
            'rustdoc.log', extra={'RUSTDOCFLAGS': '-D warnings'})
    execute(['cargo', 'test', '--locked', '--offline',
             'storage::native::tests::native_capabilities_match_pinned_stack',
             '--', '--nocapture'], 'native.log')
    execute(['cargo', 'build', '--locked', '--offline', '--example',
             'storage_check', '--message-format=json'], 'build.log')
    artifacts = []
    for line in (evidence / 'build.log').read_text().splitlines():
        try:
            message = json.loads(line)
        except json.JSONDecodeError:
            continue
        if message.get('reason') == 'compiler-artifact' and message.get('executable'):
            artifacts.append(message['executable'])
    if len(artifacts) != 1:
        raise RuntimeError('expected one actual compiler executable artifact')
    with tempfile.TemporaryDirectory(prefix='tk9-standalone-') as directory:
        isolated = Path(directory)
        binary = isolated / 'storage_check'
        shutil.copy2(artifacts[0], binary)
        execute(['otool', '-L', str(binary)] if sys.platform == 'darwin'
                else ['ldd', str(binary)], 'dynamic.log')
        dependencies = (evidence / 'dynamic.log').read_text().lower()
        if re.search(r'(libsqlite|libvec1|vec1\.(so|dylib))', dependencies):
            raise RuntimeError('unexpected native shared dependency')
        execute([str(binary), str(isolated / 'scratch')], 'standalone.log', cwd=isolated)
        (evidence / 'executable.sha256').write_text(hashlib.sha256(binary.read_bytes()).hexdigest() + '\n')


def run_linux(root, evidence, execute):
    tag = 'letopis-tk9-' + uuid.uuid4().hex
    container = tag + '-run'
    execute(['docker', 'version'], 'docker-version.log')
    execute(['docker', 'build', '-f', 'tests/storage/Dockerfile', '-t', tag, '--build-arg', 'SOURCE_COMMIT=' + (evidence / 'source-commit.log').read_text().strip(), '.'],
            'docker-build.log')
    execute(['docker', 'image', 'inspect', tag], 'image.json')
    try:
        execute(['docker', 'run', '--name', container, '--network', 'none', tag],
                'linux.log')
    finally:
        # Container ownership survives a killed Docker client. Always reap it.
        try:
            execute(['docker', 'cp', container + ':/receipt/.', str(evidence / 'container')],
                    'docker-copy.log')
        finally:
            execute(['docker', 'rm', '-f', container], 'docker-cleanup.log')
    inner = json.loads((evidence / 'container/manifest.json').read_text())
    if inner['status'] != 'PASS':
        raise RuntimeError('Linux acceptance failed')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--platform', choices=['host', 'linux'], required=True)
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    create_evidence(evidence)
    root = Path(__file__).resolve().parents[2]
    manifest = {'status': 'FAIL', 'platform': platform.platform(),
                'machine': platform.machine(), 'python': sys.version,
                'inputs': {}, 'commands': COMMANDS}

    def execute(argv, name, *, cwd=root, extra=None):
        run_command(argv, cwd=cwd, env={**os.environ,
                    'LETOPIS_TEST_EVIDENCE': str(evidence / 'process'),
                    **(extra or {})},
                    log=evidence / name, timeout=1800)

    try:
        manifest['inputs'] = hash_inputs(root)
        if (root / '.git').exists():
            execute(['git', 'rev-parse', 'HEAD'], 'source-commit.log')
            execute(['git', 'status', '--short'], 'source-dirty.log')
        elif (root / 'SOURCE_COMMIT').exists():
            manifest['source_commit'] = (root / 'SOURCE_COMMIT').read_text().strip()
        if args.platform == 'host':
            run_host(root, evidence, execute)
        else:
            run_linux(root, evidence, execute)
        manifest['status'] = 'PASS'
    except BaseException as error:
        manifest['error'] = repr(error)
        raise
    finally:
        (evidence / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
