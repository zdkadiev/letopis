import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from run import create_evidence, hash_inputs, run_command, run_host


class RunnerTests(unittest.TestCase):
    def test_child_receipts_use_current_evidence_and_keep_command_overrides(self):
        # Given
        import run
        from unittest.mock import patch
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / 'receipt'
            argv = ['run.py', '--platform', 'host', '--evidence', str(evidence)]

            def execute_probe(_root, _evidence, execute):
                execute(['probe'], 'probe.log', extra={'RUSTDOCFLAGS': '-D warnings'})

            with patch.object(sys, 'argv', argv), \
                    patch.object(run, 'hash_inputs', return_value={}), \
                    patch.object(run, 'run_host', side_effect=execute_probe), \
                    patch.object(run, 'run_command') as command:
                run.main()
            environment = command.call_args.kwargs['env']
            # When / Then
            self.assertEqual(environment.get('LETOPIS_TEST_EVIDENCE'),
                             str(evidence.resolve() / 'process'))
            self.assertEqual(environment.get('RUSTDOCFLAGS'), '-D warnings')

    def test_nonzero_command_prevents_pass_manifest(self):
        # Catches swallowed subprocess errors.
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # When / Then
            with self.assertRaises(subprocess.CalledProcessError):
                run_command([sys.executable, '-c', 'raise SystemExit(42)'], cwd=root,
                            env=dict(os.environ), log=root / 'command.log', timeout=10)
            self.assertFalse((root / 'manifest.json').exists())

    def test_existing_evidence_directory_is_rejected(self):
        # Catches destructive reuse of historical receipts.
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = root / 'sentinel'
            sentinel.write_text('keep')
            # When / Then
            with self.assertRaises(FileExistsError):
                create_evidence(root)
            self.assertEqual(sentinel.read_text(), 'keep')

    def test_timeout_reaps_owned_process(self):
        # Catches killing without waiting, or leaving the owned child alive.
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = "import os,pathlib,time; pathlib.Path('pid').write_text(str(os.getpid())); time.sleep(30)"
            # When / Then
            with self.assertRaises(subprocess.TimeoutExpired):
                run_command([sys.executable, '-c', script], cwd=root,
                            env=dict(os.environ), log=root / 'command.log', timeout=1)
            pid = int((root / 'pid').read_text())
            # When / Then
            with self.assertRaises(ProcessLookupError):
                os.kill(pid, 0)


class PreparationTests(unittest.TestCase):
    def run_copy(self, root, script):
        import shutil
        scripts = root / 'tests/storage'
        scripts.mkdir(parents=True)
        for name in ('run.py', 'negative_controls.py'):
            shutil.copy2(Path(__file__).parent / name, scripts / name)
        evidence = root / 'receipt'
        argv = [sys.executable, '-B', str(scripts / script), '--evidence', str(evidence)]
        if script == 'run.py':
            argv += ['--platform', 'host']
        result = subprocess.run(argv, capture_output=True, text=True)
        return result, evidence

    def test_hash_preparation_failure_retains_fail_receipt(self):
        # Missing build inputs must not escape without a failure receipt.
        # Given
        import json
        with tempfile.TemporaryDirectory() as directory:
            # When
            result, evidence = self.run_copy(Path(directory), 'run.py')
            # Then
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((evidence / 'manifest.json').is_file(), result.stderr)
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertIn('FileNotFoundError', manifest['error'])

    def test_git_preparation_failure_retains_command_exit_and_fail_receipt(self):
        # Real Git failure must be recorded rather than bypassing the manifest.
        # Given
        import json
        with tempfile.TemporaryDirectory() as directory:
            # When
            result, evidence = self.run_copy(Path(directory), 'negative_controls.py')
            # Then
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((evidence / 'manifest.json').is_file(), result.stderr)
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertEqual(manifest['commands'][0]['argv'], ['git', 'rev-parse', 'HEAD'])
            self.assertNotEqual(manifest['commands'][0]['exit_code'], 0)
            self.assertTrue((evidence / manifest['commands'][0]['log']).is_file())

    def test_successful_git_preparation_records_both_commands(self):
        # Given
        import json
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            subprocess.run(['git', '-c', 'user.name=Test', '-c', 'user.email=test@example.org',
                            'commit', '--allow-empty', '-qm', 'fixture'], cwd=root, check=True)
            # When
            result, evidence = self.run_copy(root, 'negative_controls.py')
            # Then
            self.assertNotEqual(result.returncode, 0)  # Empty archive lacks native inputs.
            self.assertTrue((evidence / 'manifest.json').is_file(), result.stderr)
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertEqual([c['exit_code'] for c in manifest['commands'][:2]], [0, 0])
            self.assertEqual(manifest['commands'][1]['argv'][:2], ['git', 'archive'])
            self.assertEqual(manifest['commands'][1]['argv'][3], manifest['commit'])


class SchemaParityTests(unittest.TestCase):
    def test_parity_accepts_current_schema_and_rejects_deliberate_drift(self):
        # Product DDL drift must fail independently of runtime self-recognition.
        # Given
        import shutil
        project = Path(__file__).resolve().parents[2]
        script = project / 'tests/storage/check_schema.py'
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ('src/storage/cache.sql', 'src/storage/vectors.sql',
                         'docs/zkit/letopis/mechanics/storage-schema.sql'):
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(project / name, target)
            # When
            good = subprocess.run([sys.executable, str(script), str(root)], capture_output=True)
            # Then
            self.assertEqual(good.returncode, 0, good.stderr)
            path = root / 'src/storage/cache.sql'
            path.write_text(path.read_text().replace('revision INTEGER', 'revision TEXT'))
            bad = subprocess.run([sys.executable, str(script), str(root)], capture_output=True)
            self.assertNotEqual(bad.returncode, 0)
            self.assertIn(b'canonical DDL differs', bad.stderr)


class InputInventoryTests(unittest.TestCase):
    def test_inventory_excludes_nested_build_caches_and_keeps_source_hashes(self):
        # Catches inventories bloated by ignored historical Cargo/build output.
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sources = (
                'Cargo.toml', 'Cargo.lock', 'build.rs', 'rust-toolchain.toml', 'Makefile',
                'clippy.toml', '.dockerignore',
                'docs/zkit/letopis/mechanics/storage-schema.sql',
                'src/lib.rs', 'tests/storage/test_runner.py',
                'tests/fixtures/target-name.txt', 'vendor/vec1/vec1.c',
            )
            for name in sources:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('abc')
            caches = ('tests/runtime/target/debug/build.o',
                      'tests/storage-build/.build/sqlite3.c',
                      'tests/storage/__pycache__/run.pyc',
                      'src/nested/target/ignored.rs')
            for name in caches:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_bytes(b'generated build output')

            # When.
            actual = hash_inputs(root)

            # Then: legitimate source names survive; generated trees do not.
            # When / Then
            self.assertEqual(set(actual), set(sources))
            self.assertEqual(actual['src/lib.rs'],
                             'ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad')


class ProcessEvidenceTests(unittest.TestCase):
    def test_process_receipt_environment_and_real_deadline_are_explicit(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            calls = []
            def execute(argv, name, **kwargs):
                calls.append((argv, name, kwargs))
                if name == 'build.log':
                    raise RuntimeError('stop before standalone fixture')
            # When / Then
            with self.assertRaisesRegex(RuntimeError, 'stop before'):
                run_host(evidence, evidence, execute)
            root = next(call for call in calls if call[1] == 'rust-check.log')
            deadline = next(call for call in calls if call[1] == 'provider-real-deadline.log')
            self.assertEqual(root[2]['extra'],
                             {'LETOPIS_TEST_EVIDENCE': str(evidence / 'process')})
            self.assertEqual(deadline[2]['extra'], root[2]['extra'])
            self.assertEqual(deadline[0], ['cargo', 'test', '--locked', '--offline',
                '--test', 'embedding_provider', 'attempt_obeys_real_sixty_second_deadline',
                '--', '--ignored', '--exact', '--nocapture'])
            self.assertEqual(len([call for call in calls if call[1] == deadline[1]]), 1)
