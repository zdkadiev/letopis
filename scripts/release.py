#!/usr/bin/env python3
"""Package and smoke-test a native Letopis release; requires Python 3.11+."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import platform
import subprocess
import tarfile
import tempfile
import tomllib

ROOT = Path(__file__).resolve().parents[1]
TARGETS = {
    'aarch64-apple-darwin', 'x86_64-apple-darwin',
    'aarch64-unknown-linux-gnu', 'x86_64-unknown-linux-gnu',
}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    package = commands.add_parser('package')
    package.add_argument('--binary', type=Path, required=True)
    package.add_argument('--target', required=True, choices=sorted(TARGETS))
    package.add_argument('--output', type=Path, required=True)
    smoke = commands.add_parser('smoke')
    smoke.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'package':
        print(build_package(args.binary, args.output, args.target))
    else:
        run_smoke(args.binary)


def build_package(binary: Path, output: Path, target: str) -> Path:
    if target not in TARGETS:
        raise ValueError(f'unsupported target: {target}')
    version = get_version()
    binary = binary.resolve(strict=True)
    observed = subprocess.check_output([binary, '--version'], text=True).strip()
    if observed != f'letopis {version}':
        raise ValueError(f'binary version does not match source: {observed}')
    license_bytes = (ROOT / 'LICENSE').read_bytes()
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=ROOT, text=True).strip()
    receipt = json.dumps({
        'version': version, 'target': target, 'commit': commit,
        'builder_os': platform.platform(),
        'rustc': subprocess.check_output(['rustc', '--version'], cwd=ROOT, text=True).strip(),
    }, indent=2).encode() + b'\n'
    output.mkdir(parents=True, exist_ok=True)
    archive = output / f'letopis-{version}-{target}.tar.gz'
    checksum = archive.with_suffix('.gz.sha256')
    if checksum.exists():
        raise FileExistsError(checksum)
    with archive.open('xb') as destination:
        with tarfile.open(fileobj=destination, mode='w:gz') as package:
            for name, data, mode in [
                ('letopis', binary.read_bytes(), 0o755),
                ('LICENSE', license_bytes, 0o644),
                ('BUILD.json', receipt, 0o644),
                ('THIRD_PARTY_NOTICES.txt', get_third_party_notices(), 0o644),
            ]:
                entry = tarfile.TarInfo(name)
                entry.size, entry.mode = len(data), mode
                package.addfile(entry, io.BytesIO(data))
    digest = hashlib.sha256(archive.read_bytes()).hexdigest()
    with checksum.open('x') as destination:
        destination.write(f'{digest}  {archive.name}\n')
    return archive


def get_third_party_notices() -> bytes:
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--locked', '--offline', '--format-version', '1',
    ], cwd=ROOT, text=True))
    notices = [b'Third-party dependencies distributed with Letopis.\n',
               b'This bundle conservatively includes build, test and other-platform dependencies.\n']
    for package in sorted(metadata['packages'], key=lambda item: (item['name'], item['version'])):
        if package['name'] == 'letopis':
            continue
        package_root = Path(package['manifest_path']).parent
        heading = (f"\n=== {package['name']} {package['version']} ===\n"
                   f"License: {package.get('license')}\n"
                   f"Authors: {', '.join(package.get('authors', []))}\n"
                   f"Repository: {package.get('repository')}\n")
        notices.append(heading.encode())
        files = {path for path in package_root.rglob('*')
                 if path.is_file() and path.name.lower().startswith(
                     ('license', 'licence', 'copying', 'notice', 'copyright'))}
        if package.get('license_file'):
            files.add(package_root / package['license_file'])
        for path in sorted(files):
            notices.append(f"\n--- {path.relative_to(package_root)} ---\n".encode())
            notices.append(path.read_bytes())
            notices.append(b'\n')
    rust_root = Path(subprocess.check_output(['rustc', '--print', 'sysroot'], text=True).strip())
    notices.append(b'\n=== Rust standard library copyright and licenses ===\n')
    notices.append((rust_root / 'share/doc/rust/COPYRIGHT-library.html').read_bytes())
    notices.append(b'\n=== SQLite vec1 upstream notice ===\n')
    source = (ROOT / 'vendor/vec1/vec1.c').read_bytes()
    notices.append(source.split(b'*/', 1)[0] + b'*/\n')
    return b''.join(notices)


def run_smoke(binary: Path):
    binary = binary.resolve(strict=True)
    version = subprocess.check_output([binary, '--version'], text=True).strip()
    if version != f'letopis {get_version()}':
        raise ValueError('unexpected executable version')
    with tempfile.TemporaryDirectory(prefix='letopis-release-') as directory:
        root = Path(directory)
        subprocess.run(['git', 'init', '-q', directory], check=True)
        invoke(binary, root, 'init', {})
        knowledge = root / 'letopis'
        knowledge.mkdir()
        content = '<!-- letopis: {"created_at":1793491200} -->\n# Retry budget\nKeep retries bounded.\n'
        (knowledge / 'retry-budget.md').write_text(content)
        nested = root / 'nested'
        nested.mkdir()
        element_id = 'letopis/retry-budget.md'
        report = invoke(binary, nested, 'check', {})
        if report != {'errors': []}:
            raise ValueError(f'unexpected validation report: {report}')
        links = invoke(binary, nested, 'search', {'query': 'retry', 'limit': 10})
        if not any(link['element_id'] == element_id for link in links):
            raise ValueError('search did not find the fixture knowledge')
        traversal = invoke(binary, nested, 'traverse', {
            'element_ids': [element_id], 'mode': 'both', 'direction': 'both',
            'depth': 2, 'limit': 10, 'edge_limit': 10,
        })
        if traversal != {'elements': [], 'edges': []}:
            raise ValueError('unexpected traversal result')
        inspected = invoke(binary, nested, 'inspect', {'element_ids': [element_id]})
        snapshot_bytes = subprocess.check_output([
            'git', 'show', f'{inspected["snapshot"]}:{element_id}',
        ], cwd=root)
        if snapshot_bytes != content.encode():
            raise ValueError('inspect snapshot differs from the authored content')
        invoke(binary, nested, 'status', {})
        invoke(binary, nested, 'config', {})
        invoke(binary, nested, 'reindex', {})
    print(json.dumps({'status': 'PASS', 'binary': str(binary), 'version': version,
                      'platform': platform.platform(), 'machine': platform.machine()}))


def invoke(binary: Path, root: Path, command: str, request: dict):
    process = subprocess.run([binary, command], input=json.dumps(request), text=True,
                             capture_output=True, cwd=root, timeout=60)
    response = json.loads(process.stdout)
    if process.returncode != 0 or response.get('ok') is not True:
        raise ValueError(f'{command} failed: {process.stdout}')
    return response['result']


def get_version() -> str:
    with (ROOT / 'Cargo.toml').open('rb') as source:
        return tomllib.load(source)['package']['version']


if __name__ == '__main__':
    main()
