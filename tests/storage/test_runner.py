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
