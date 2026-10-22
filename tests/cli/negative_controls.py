#!/usr/bin/env python3
"""Compiling controls in a disposable copy of the actual current source, then exact restore/GREEN."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
from run import COMMANDS, child_environment, create_evidence, hash_file, hash_inputs, read_artifacts, run_command

CONTROLS = [
    ('null-output', 'src/process/detached.rs',
     '.stdout(Stdio::null())\n            .stderr(Stdio::null())',
     '.stdout(Stdio::inherit())\n            .stderr(Stdio::inherit())',
     'detached_child_has_exact_context_null_fds_and_no_service_descriptors',
     'stdout EOF before child release'),
    ('kernel-exclusion', 'src/filesystem/local.rs', 'match file.try_lock() {',
     'match { let _ = file; Ok::<(), TryLockError>(()) } {',
     'two_released_probes_yield_exactly_one_active_drain',
     'left: Bool(true)\n right: false'),
]


def run_control(copy, evidence, control, manifest):
    name, relative, needle, replacement, test, oracle = control
    path = copy / relative
    original = path.read_bytes()
    assert original.decode().count(needle) == 1
    env = child_environment(evidence, {'CARGO_TARGET_DIR': str(copy / 'target')})

    def execute(argv, suffix):
        run_command(argv, cwd=copy, env=env, log=evidence / f'{name}-{suffix}.log')

    def build_fixture(suffix):
        execute(['cargo', 'build', '--locked', '--offline', '--example',
                 'cli_runtime_fixture', '--message-format=json'], suffix)
        fixture = read_artifacts(evidence / f'{name}-{suffix}.log')['cli_runtime_fixture']
        env['LETOPIS_CLI_FIXTURE'] = str(fixture)
        return fixture

    command = ['cargo', 'test', '--locked', '--offline', '--test', 'cli_process', test,
               '--', '--exact', '--nocapture']
    record = {'source': relative, 'test': test, 'original_sha256': hash_file(path)}
    manifest['controls'][name] = record
    try:
        path.write_text(original.decode().replace(needle, replacement))
        record['mutated_sha256'] = hash_file(path)
        fixture = build_fixture('fixture')
        record['mutated_fixture_sha256'] = hash_file(fixture)
        execute(['cargo', 'test', '--locked', '--offline', '--test', 'cli_process', '--no-run'], 'compile')
        try:
            execute(command, 'red')
        except subprocess.CalledProcessError:
            error = (evidence / f'{name}-red.log.stderr').read_text()
            assert oracle in error, error
            assert 'test result: FAILED. 0 passed; 1 failed;' in (evidence / f'{name}-red.log').read_text()
            record['behavioral_oracle'] = oracle
        else:
            raise AssertionError(name + ' unexpectedly passed')
    finally:
        path.write_bytes(original)
        assert path.read_bytes() == original
        record['restored_sha256'] = hash_file(path)
    fixture = build_fixture('restore-fixture')
    record['restored_fixture_sha256'] = hash_file(fixture)
    execute(command, 'green')
    record['status'] = 'PASS'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    create_evidence(evidence)
    COMMANDS.clear()
    root = Path(__file__).resolve().parents[2]
    manifest = {'status': 'FAIL', 'commands': COMMANDS, 'controls': {}}
    try:
        manifest['inputs'] = hash_inputs(root)
        run_command(['git', 'rev-parse', 'HEAD'], cwd=root, env=dict(os.environ),
                    log=evidence / 'source-commit.log')
        with tempfile.TemporaryDirectory(prefix='tk11-controls-') as directory:
            copy = Path(directory)
            for name in manifest['inputs']:
                target = copy / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(root / name, target)
            for control in CONTROLS:
                run_control(copy, evidence, control, manifest)
                assert hash_inputs(copy) == manifest['inputs'], 'copy was not restored exactly'
        assert hash_inputs(root) == manifest['inputs'], 'original source changed'
        manifest['status'] = 'PASS'
    except BaseException as error:
        manifest['error'] = repr(error)
        raise
    finally:
        (evidence / 'manifest.json').write_text(json.dumps(manifest, indent=2) + '\n')


if __name__ == '__main__':
    main()
