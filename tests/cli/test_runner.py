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
                     'tests/cli/Dockerfile', '.dockerignore', run.SCHEMA,
                     'tests/cli_drain.rs', 'tests/support/drain.rs',
                     'tests/document_refresh.rs', 'tests/embedding_service.rs']:
            self.assertIn(name, inputs)
        self.assertFalse(any('__pycache__' in name for name in inputs))

    def test_artifact_resolution_respects_target_specific_cargo_path(self):
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'build.log'
            artifact = '/target/aarch64-apple-darwin/debug/examples/cli_runtime_fixture'
            log.write_text(json.dumps({'reason':'compiler-artifact',
                'target':{'name':'cli_runtime_fixture'}, 'executable':artifact}) + '\n')
            self.assertEqual(run.read_artifacts(log)['cli_runtime_fixture'], Path(artifact))


class MakeWorkflowTests(unittest.TestCase):
    def test_success_is_one_ok_line_for_check_and_verify(self):
        with tempfile.TemporaryDirectory() as directory:
            for target in ('check', 'verify'):
                result = self.run_make(directory, target)
                self.assertEqual(result.returncode, 0, result.stdout)
                self.assertEqual(result.stdout, 'ok\n')

    def test_verbose_verify_retains_every_steps_output(self):
        with tempfile.TemporaryDirectory() as directory:
            result = self.run_make(directory, 'verify', verbose=True)
            self.assertEqual(result.returncode, 0, result.stdout)
            for step in ('fmt', 'check', 'clippy', 'test', 'doc'):
                self.assertIn('stdout ' + step + '\n', result.stdout)
                self.assertIn('stderr ' + step + '\n', result.stdout)
            self.assertEqual(result.stdout.count('stdout test\n'), 2)
            self.assertTrue(result.stdout.endswith('ok\n'))

    def test_failure_prints_complete_failed_step_and_stops(self):
        with tempfile.TemporaryDirectory() as directory:
            for failed in ('clippy', 'doc'):
                result = self.run_make(directory, 'verify', fail_step=failed)
                self.assertNotEqual(result.returncode, 0)
                self.assertTrue(result.stdout.startswith(
                    f'stdout {failed}\nstderr {failed}\n'), result.stdout)
                self.assertNotIn('stdout test\n', result.stdout)
                self.assertNotIn('ok\n', result.stdout)

    def run_make(self, directory, target, *, verbose=False, fail_step=None):
        root = Path(__file__).resolve().parents[2]
        fake = Path(directory) / 'cargo.py'
        fake.write_text(
            "import os,sys\n"
            "step=sys.argv[1]\n"
            "print('stdout '+step, flush=True)\n"
            "print('stderr '+step, file=sys.stderr, flush=True)\n"
            "raise SystemExit(23 if os.environ.get('FAIL_STEP') == step else 0)\n"
        )
        env = {key: value for key, value in os.environ.items()
               if key not in ('MAKEFLAGS', 'MFLAGS', 'MAKEOVERRIDES', 'FAIL_STEP')}
        if fail_step:
            env['FAIL_STEP'] = fail_step
        return subprocess.run(
            ['make', '-C', directory, '-f', str(root / 'Makefile'), target,
             f'CARGO={sys.executable} {fake}', f'V={int(verbose)}'],
            cwd=root, env=env, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, timeout=10,
        )
