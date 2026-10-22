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
    paths = [root / name for name in ('Cargo.toml', 'Cargo.lock', 'build.rs',
              'rust-toolchain.toml', 'clippy.toml')]
    for name in ('src', 'vendor', 'examples', 'tests', 'scripts'):
        paths.extend(p for p in (root / name).rglob('*') if p.is_file()
                     and '__pycache__' not in p.parts)
    return {str(p.relative_to(root)): hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(paths)}


def run_host(root, evidence, execute):
    for tool in (['rustc', '-vV'], ['cargo', '--version'], ['git', '--version'],
                 [sys.executable, '--version'], ['uname', '-a']):
        execute(tool, 'version-' + Path(tool[0]).name + '.log')
    execute(['./scripts/check-rust.sh'], 'rust-check.log')
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
                'inputs': hash_inputs(root), 'commands': COMMANDS}

    def execute(argv, name, *, cwd=root, extra=None):
        run_command(argv, cwd=cwd, env={**os.environ, **(extra or {})},
                    log=evidence / name, timeout=1800)

    try:
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
