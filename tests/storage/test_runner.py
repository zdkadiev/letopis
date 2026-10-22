import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from run import create_evidence, run_command


class RunnerTests(unittest.TestCase):
    def test_nonzero_command_prevents_pass_manifest(self):
        # Catches swallowed subprocess errors.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(subprocess.CalledProcessError):
                run_command([sys.executable, '-c', 'raise SystemExit(42)'], cwd=root,
                            env=dict(os.environ), log=root / 'command.log', timeout=10)
            self.assertFalse((root / 'manifest.json').exists())

    def test_existing_evidence_directory_is_rejected(self):
        # Catches destructive reuse of historical receipts.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            sentinel = root / 'sentinel'
            sentinel.write_text('keep')
            with self.assertRaises(FileExistsError):
                create_evidence(root)
            self.assertEqual(sentinel.read_text(), 'keep')

    def test_timeout_reaps_owned_process(self):
        # Catches killing without waiting, or leaving the owned child alive.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            script = "import os,pathlib,time; pathlib.Path('pid').write_text(str(os.getpid())); time.sleep(30)"
            with self.assertRaises(subprocess.TimeoutExpired):
                run_command([sys.executable, '-c', script], cwd=root,
                            env=dict(os.environ), log=root / 'command.log', timeout=1)
            pid = int((root / 'pid').read_text())
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
        import json
        with tempfile.TemporaryDirectory() as directory:
            result, evidence = self.run_copy(Path(directory), 'run.py')
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((evidence / 'manifest.json').is_file(), result.stderr)
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertIn('FileNotFoundError', manifest['error'])

    def test_git_preparation_failure_retains_command_exit_and_fail_receipt(self):
        # Real Git failure must be recorded rather than bypassing the manifest.
        import json
        with tempfile.TemporaryDirectory() as directory:
            result, evidence = self.run_copy(Path(directory), 'negative_controls.py')
            self.assertNotEqual(result.returncode, 0)
            self.assertTrue((evidence / 'manifest.json').is_file(), result.stderr)
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertEqual(manifest['commands'][0]['argv'], ['git', 'rev-parse', 'HEAD'])
            self.assertNotEqual(manifest['commands'][0]['exit_code'], 0)
            self.assertTrue((evidence / manifest['commands'][0]['log']).is_file())

    def test_successful_git_preparation_records_both_commands(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            subprocess.run(['git', 'init', '-q', str(root)], check=True)
            subprocess.run(['git', '-c', 'user.name=Test', '-c', 'user.email=test@example.org',
                            'commit', '--allow-empty', '-qm', 'fixture'], cwd=root, check=True)
            result, evidence = self.run_copy(root, 'negative_controls.py')
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
            good = subprocess.run([sys.executable, str(script), str(root)], capture_output=True)
            self.assertEqual(good.returncode, 0, good.stderr)
            path = root / 'src/storage/cache.sql'
            path.write_text(path.read_text().replace('revision INTEGER', 'revision TEXT'))
            bad = subprocess.run([sys.executable, str(script), str(root)], capture_output=True)
            self.assertNotEqual(bad.returncode, 0)
            self.assertIn(b'canonical DDL differs', bad.stderr)
