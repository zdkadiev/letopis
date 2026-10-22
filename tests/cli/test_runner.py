import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import run


class RunnerTests(unittest.TestCase):
    def test_nonzero_command_is_recorded_and_propagated(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaises(subprocess.CalledProcessError):
                run.run_command([sys.executable, '-c', 'raise SystemExit(42)'],
                                cwd=root, env=dict(os.environ), log=root / 'out', timeout=10)
            self.assertEqual(run.COMMANDS[-1]['exit_code'], 42)

    def test_timeout_kills_and_reaps_only_owned_session(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outsider = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
            try:
                script = "import os,pathlib,time; pathlib.Path('pid').write_text(str(os.getpid())); time.sleep(60)"
                with self.assertRaises(subprocess.TimeoutExpired):
                    run.run_command([sys.executable, '-c', script], cwd=root,
                                    env=dict(os.environ), log=root / 'out', timeout=1)
                with self.assertRaises(ProcessLookupError):
                    os.kill(int((root / 'pid').read_text()), 0)
                self.assertIsNone(outsider.poll())
            finally:
                outsider.kill()
                outsider.wait()

    def test_existing_evidence_is_never_reused(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'sentinel').write_text('keep')
            with self.assertRaises(FileExistsError):
                run.create_evidence(root)
            self.assertEqual((root / 'sentinel').read_text(), 'keep')

    def test_environment_uses_new_evidence_and_explicit_built_fixture(self):
        with patch.dict(os.environ, {'LETOPIS_TEST_EVIDENCE': 'old',
                        'LETOPIS_CLI_FIXTURE': '/untrusted/fixture',
                        'LETOPIS_FIXTURE_EXECUTABLE': '/untrusted/exe', 'KEEP_SENTINEL': 'keep'}):
            env = run.child_environment(Path('/new'), {'RUSTDOCFLAGS': '-D warnings'})
            self.assertEqual(env['LETOPIS_TEST_EVIDENCE'], '/new/process')
            self.assertNotIn('LETOPIS_CLI_FIXTURE', env)
            self.assertNotIn('LETOPIS_FIXTURE_EXECUTABLE', env)
            self.assertEqual(env['KEEP_SENTINEL'], 'keep')
            self.assertEqual(env['RUSTDOCFLAGS'], '-D warnings')
            self.assertEqual(run.child_environment(Path('/new'),
                {'LETOPIS_CLI_FIXTURE': '/built/fixture'})['LETOPIS_CLI_FIXTURE'], '/built/fixture')

    def test_preflight_failure_writes_fail_receipt(self):
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / 'new'
            with patch.object(sys, 'argv', ['run.py', '--platform', 'host', '--evidence', str(evidence)]), \
                 patch.object(run, 'hash_inputs', side_effect=FileNotFoundError('missing source')):
                with self.assertRaises(FileNotFoundError):
                    run.main()
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertIn('FileNotFoundError', manifest['error'])

    def test_schema_selects_command_and_exit_not_catalogue_union(self):
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        valid = {'ok': True, 'result': {'errors': []}}
        run.validate_response(catalogue, 'check', valid, 0)
        with self.assertRaises(ValidationError):
            run.validate_response(catalogue, 'inspect', valid, 0)
        with self.assertRaises(AssertionError):
            run.validate_response(catalogue, 'check', valid, 1)
        with self.assertRaises(ValidationError):
            run.validate_response(catalogue, 'check', {**valid, 'extra': 1}, 0)

    def test_inventory_covers_support_schema_and_ignores_bytecode(self):
        root = Path(__file__).resolve().parents[2]
        inputs = run.hash_inputs(root)
        for name in ['tests/support/cli.rs', 'src/main.rs', 'tests/cli/run.py',
                     'tests/cli/Dockerfile', '.dockerignore', run.SCHEMA]:
            self.assertIn(name, inputs)
        self.assertFalse(any('__pycache__' in name for name in inputs))
