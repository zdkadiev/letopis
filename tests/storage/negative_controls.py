#!/usr/bin/env python3
"""Run destructive controls only inside disposable git-archive copies."""
import argparse
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import tarfile
import tempfile
from run import create_evidence, run_command, COMMANDS


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    args = parser.parse_args()
    create_evidence(args.evidence)
    evidence = args.evidence.resolve()
    root = Path(__file__).resolve().parents[2]
    commit = subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip()
    archive = subprocess.check_output(['git', 'archive', commit], cwd=root)
    results = {'commit': commit, 'commands': COMMANDS, 'status': 'FAIL'}
    try:
        for control in ('checksum', 'registration'):
            with tempfile.TemporaryDirectory(prefix='tk9-control-') as directory:
                copy = Path(directory)
                with tarfile.open(fileobj=io.BytesIO(archive)) as source:
                    source.extractall(copy, filter='data')
                path = copy / ('vendor/vec1/vec1.c' if control == 'checksum' else 'src/storage/native.rs')
                if control == 'checksum':
                    path.write_bytes(path.read_bytes() + b'\n')
                else:
                    text = path.read_text()
                    needle = 'unsafe { sqlite3_auto_extension(Some(letopis_vec1_init)) }'
                    assert text.count(needle) == 1
                    path.write_text(text.replace(needle,
                        '{ let _ = sqlite3_auto_extension; let _ = letopis_vec1_init; SQLITE_OK }'))
                results[control + '_sha256'] = hashlib.sha256(path.read_bytes()).hexdigest()
                env = {**os.environ, 'CARGO_TARGET_DIR': str(copy / 'target')}
                def execute(argv, name):
                    run_command(argv, cwd=copy, env=env, log=evidence / name, timeout=600)
                if control == 'registration':
                    execute(['cargo', 'test', '--locked', '--offline', '--no-run'], 'registration-compile.log')
                try:
                    execute(['cargo', 'test', '--locked', '--offline',
                             'storage::native::tests::native_capabilities_match_pinned_stack',
                             '--', '--nocapture'], control + '-reject.log')
                except subprocess.CalledProcessError:
                    log = (evidence / (control + '-reject.log')).read_text()
                    expected = 'checksum' if control == 'checksum' else 'no such function: vec1_info'
                    assert expected in log, log
                    if control == 'checksum':
                        assert 'cargo:rustc-link-lib=static=letopis_vec1' not in log
                else:
                    raise AssertionError(control + ' unexpectedly passed')
        results['status'] = 'PASS'
    finally:
        (evidence / 'manifest.json').write_text(json.dumps(results, indent=2) + '\n')


if __name__ == '__main__':
    main()
