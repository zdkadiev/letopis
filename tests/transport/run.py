"""Reproducible TK-6 proof. Existing receipts are never overwritten."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import sys

ROOT = Path(__file__).resolve().parent
REPO = ROOT.parent.parent
EXCLUDED = {'target', '__pycache__', '.build'}


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def sources(root=ROOT):
    result = {}
    for directory, subdirs, files in os.walk(root):
        subdirs[:] = sorted(d for d in subdirs if d not in EXCLUDED)
        for name in sorted(files):
            p = Path(directory) / name
            result[str(p.relative_to(root))] = digest(p)
    return result


def require_detection(build_code, test_code, output, witness):
    if build_code != 0 or test_code == 0 or witness not in output:
        raise RuntimeError('Mutation was not detected by its named behavioral test')
    if not any(marker in output for marker in ('AssertionError', 'assertion failed', 'called `Result::unwrap_err()`')):
        raise RuntimeError('Infrastructure/runtime error is not a behavioral mutation detection')


def prior_proofs():
    result = []
    for number, folder in [(1, 'storage-build'), (2, 'publication'), (3, 'embedding'), (4, 'runtime'), (5, 'pilot_protocol')]:
        for directory in (['macos', 'linux'] if number == 5 else ['macos-arm64', 'linux-arm64']):
            path = REPO / f'docs/zkit/letopis/checks/TK-{number}' / directory / 'manifest.json'
            receipt = json.loads(path.read_text())
            base = REPO if number == 5 else REPO / 'tests' / folder
            hashes = receipt.get('files_sha256', receipt.get('source_sha256', {}))
            mismatches = [name for name, expected in hashes.items() if not (base / name).is_file() or digest(base / name) != expected]
            # Validate that the claimed historical PASS manifest is in this HEAD.
            relative = str(path.relative_to(REPO))
            committed = subprocess.run(['git', '-c', f'safe.directory={REPO}', 'show', f'HEAD:{relative}'], cwd=REPO, capture_output=True, check=True).stdout
            if committed != path.read_bytes():
                raise RuntimeError(f'Uncommitted historical evidence: {relative}')
            entry = {'task': f'TK-{number}', 'platform': directory, 'manifest': relative, 'manifest_sha256': digest(path), 'source_files': len(hashes), 'mismatches': mismatches, 'result': receipt.get('result')}
            result.append(entry)
            if not hashes or mismatches or receipt.get('result') != 'PASS':
                raise RuntimeError(f'Historical proof does not match current sources: {entry}')
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--evidence', type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    before = sources()
    env = dict(os.environ)
    target = Path(env.get('CARGO_TARGET_DIR', ROOT / 'target')).resolve()
    env['TK6_BINARY'] = str(target / 'debug/transport_client')
    env['TK6_REQUESTS'] = str(evidence / 'requests.jsonl')
    manifest = {'result': 'FAIL', 'scope': 'Synthetic X-2 transport; no real model inference/utility pilot', 'started_utc': datetime.now(timezone.utc).isoformat(), 'platform': platform.platform(), 'machine': platform.machine(), 'python': sys.version, 'provider_calls': 0, 'ollama_inference': False, 'ollama_version_exercised': None, 'x86_64_status': 'assumed until TK-43', 'source_sha256': before, 'commands': []}

    def execute(argv, cwd=REPO, timeout=300):
        argv = [str(x) for x in argv]
        log = f'{len(manifest["commands"]) + 1:02d}-{Path(argv[0]).name}.log'
        entry = {'argv': argv, 'cwd': str(cwd), 'log': log}
        manifest['commands'].append(entry)
        try:
            p = subprocess.run(argv, cwd=cwd, env=env, capture_output=True, text=True, timeout=timeout)
        except subprocess.TimeoutExpired as error:
            entry['timeout'] = timeout
            (evidence / log).write_bytes((error.stdout or b'') + (error.stderr or b''))
            raise RuntimeError(f'Command exceeded outer safety timeout: {log}') from error
        output = p.stdout + p.stderr
        (evidence / log).write_text(output)
        entry['exit_code'] = p.returncode
        print(f'{log}: exit {p.returncode}', flush=True)
        if p.returncode:
            raise RuntimeError(f'Command failed: {log}')
        return output

    try:
        manifest['rustc'] = execute(['rustc', '--version', '--verbose'])
        if manifest['rustc'].splitlines()[0].split()[1] != '1.95.0':
            raise RuntimeError('Requires Rust 1.95.0')
        manifest['base_commit'] = execute(['git', '-c', f'safe.directory={REPO}', 'rev-parse', 'HEAD']).strip()
        execute(['cargo', '--version'])
        cargo = ['cargo']
        execute(cargo + ['test', '--locked', '--manifest-path', ROOT / 'Cargo.toml'])
        execute(cargo + ['build', '--locked', '--manifest-path', ROOT / 'Cargo.toml', '--bin', 'transport_client'])
        execute([sys.executable, '-B', '-m', 'unittest', 'discover', '-s', ROOT, '-p', 'test_runner.py', '-v'])
        execute(cargo + ['fmt', '--manifest-path', ROOT / 'Cargo.toml', '--', '--check'])
        execute(cargo + ['clippy', '--locked', '--manifest-path', ROOT / 'Cargo.toml', '--all-targets', '--', '-D', 'warnings'])
        tree = execute(cargo + ['tree', '--locked', '--manifest-path', ROOT / 'Cargo.toml', '--prefix', 'none'])
        names = [line.split()[0] for line in tree.splitlines() if line.split()]
        if set(names) & {'tokio', 'reqwest'} or 'rustls-platform-verifier' not in names:
            raise RuntimeError('Dependency tree violates D-96')
        execute(cargo + ['tree', '--locked', '--manifest-path', ROOT / 'Cargo.toml', '-e', 'features'])
        if sys.platform == 'linux':
            manifest['system_trust'] = json.loads(execute([sys.executable, '-B', ROOT / 'linux_trust.py']))
        else:
            manifest['system_trust'] = {'local_untrusted_issuer': 'PASS', 'positive_system_trust': 'separate public-HTTPS receipt required; no host Keychain edits'}
        execute([sys.executable, '-B', ROOT / 'negative_controls.py', '--evidence', evidence / 'negative-controls'], timeout=600)
        # Mutations used disposable copies; recheck real transport after controls.
        execute([sys.executable, '-B', '-m', 'unittest', 'discover', '-s', ROOT, '-p', 'test_*.py', '-v'])
        manifest['prior_proofs'] = prior_proofs()
        execute([sys.executable, '-B', REPO / 'docs/zkit/letopis/roadmap/schedule.py', '--check'])
        if sources() != before:
            raise RuntimeError('Source bytes changed during proof')
        manifest['result'] = 'PASS'
    except Exception as error:
        manifest['error'] = str(error)
        raise
    finally:
        manifest['finished_utc'] = datetime.now(timezone.utc).isoformat()
        manifest['source_after_sha256'] = sources()
        manifest['evidence_sha256'] = {str(p.relative_to(evidence)): digest(p) for p in sorted(evidence.rglob('*')) if p.is_file()}
        (evidence / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')
        print(f'TK-6 {manifest["result"]}: {evidence}', flush=True)


if __name__ == '__main__':
    main()
