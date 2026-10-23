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
    ('kernel-exclusion', 'src/filesystem/local.rs', 'retry_lock(|| file.try_lock())',
     'retry_lock(|| { let _ = file; Ok::<(), TryLockError>(()) })',
     'two_released_probes_yield_exactly_one_active_drain',
     'left: Bool(true)\n right: false'),
]


READING_CONTROLS = [
    ('captured-config', 'src/service/embedding.rs',
     '        if self.provider.validate_config(request.config).is_err() {',
     '        let _ = self.read_configuration()?;\n        if self.provider.validate_config(request.config).is_err() {',
     ['--lib'], 'service::embedding::tests::captured_query_never_rereads_or_resets_configuration',
     'left: 1'),
    ('candidate-hash', 'src/service/search.rs',
     'if node.search_hash != hit.search_hash {',
     'if false {',
     ['--lib'], 'service::search::tests::candidate_metadata_rejects_wrong_rows_and_search_hashes',
     'assertion failed:'),
    ('reopen-cache', 'src/service/search.rs',
     '        let fts = read.match_text(&expression).map_err(map_search_error)?;',
     '        drop(read);\n        let mut read = self.storage.begin_read().map_err(map_search_error)?;\n        let fts = read.match_text(&expression).map_err(map_search_error)?;',
     ['--test', 'search_consistency'], 'concurrent_publication_preserves_old_membership_coordinates_and_revision',
     'CommandError { kind: InternalError, message: "invalid search index data", .. }'),
    ('live-inspect-reread', 'src/service/inspection.rs',
     '        let mut captured = CapturedKnowledge::default();\n        captured.collect_metadata(&path, parsed, requested);',
     '        drop(parsed);\n        let live = self.filesystem.read_file(&path).map_err(map_document_error)?.unwrap();\n        let parsed = self.markdown.parse_file(&path, &live.bytes).map_err(|_| CommandError::new(CommandErrorKind::ValidationFailed, "invalid control input"))?.unwrap();\n        let mut captured = CapturedKnowledge::default();\n        captured.collect_metadata(&path, parsed, requested);',
     ['--lib'], 'service::inspection::tests::capture_writes_each_blob_before_reading_the_next_file',
     'assertion `left == right` failed'),
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


def run_reading_control(copy, evidence, control, manifest):
    name, relative, needle, replacement, target, test, oracle = control
    path = copy / relative
    original = path.read_bytes()
    assert original.decode().count(needle) == 1
    env = child_environment(evidence, {'CARGO_TARGET_DIR': str(copy / 'target')})
    command = ['cargo', 'test', '--locked', '--offline', *target, test,
               '--', '--exact', '--nocapture']
    record = {'source': relative, 'test': test, 'original_sha256': hash_file(path)}
    manifest['controls'][name] = record
    try:
        path.write_text(original.decode().replace(needle, replacement))
        record['mutated_sha256'] = hash_file(path)
        run_command(['cargo', 'test', '--locked', '--offline', *target, '--no-run'],
                    cwd=copy, env=env, log=evidence / f'{name}-compile.log')
        try:
            run_command(command, cwd=copy, env=env, log=evidence / f'{name}-red.log')
        except subprocess.CalledProcessError:
            output = (evidence / f'{name}-red.log').read_text()
            error = (evidence / f'{name}-red.log.stderr').read_text()
            assert oracle in error + output, error + output
            assert 'test result: FAILED. 0 passed; 1 failed;' in output
            record['behavioral_oracle'] = oracle
        else:
            raise AssertionError(name + ' unexpectedly passed')
    finally:
        path.write_bytes(original)
        assert path.read_bytes() == original
        record['restored_sha256'] = hash_file(path)
    run_command(command, cwd=copy, env=env, log=evidence / f'{name}-green.log')
    record['status'] = 'PASS'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--evidence', required=True, type=Path)
    parser.add_argument('--suite', choices=['runtime', 'reading'], default='runtime')
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
            controls = READING_CONTROLS if args.suite == 'reading' else CONTROLS
            run_selected = run_reading_control if args.suite == 'reading' else run_control
            for control in controls:
                run_selected(copy, evidence, control, manifest)
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
