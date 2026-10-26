import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import run
import negative_controls


class RunnerTests(unittest.TestCase):
    def test_nonzero_command_is_recorded_and_propagated(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            # When / Then
            with self.assertRaises(subprocess.CalledProcessError):
                run.run_command([sys.executable, '-c', 'raise SystemExit(42)'],
                                cwd=root, env=dict(os.environ), log=root / 'out', timeout=10)
            self.assertEqual(run.COMMANDS[-1]['exit_code'], 42)

    def test_timeout_kills_and_reaps_only_owned_session(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            outsider = subprocess.Popen([sys.executable, '-c', 'import time; time.sleep(60)'])
            try:
                script = "import os,pathlib,time; pathlib.Path('pid').write_text(str(os.getpid())); time.sleep(60)"
                # When / Then
                with self.assertRaises(subprocess.TimeoutExpired):
                    run.run_command([sys.executable, '-c', script], cwd=root,
                                    env=dict(os.environ), log=root / 'out', timeout=1)
                # When / Then
                with self.assertRaises(ProcessLookupError):
                    os.kill(int((root / 'pid').read_text()), 0)
                self.assertIsNone(outsider.poll())
            finally:
                outsider.kill()
                outsider.wait()

    def test_existing_evidence_is_never_reused(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'sentinel').write_text('keep')
            # When / Then
            with self.assertRaises(FileExistsError):
                run.create_evidence(root)
            self.assertEqual((root / 'sentinel').read_text(), 'keep')

    def test_environment_uses_new_evidence_and_explicit_built_fixture(self):
        # Given
        with patch.dict(os.environ, {'LETOPIS_TEST_EVIDENCE': 'old',
                        'LETOPIS_CLI_FIXTURE': '/untrusted/fixture',
                        'LETOPIS_FIXTURE_EXECUTABLE': '/untrusted/exe', 'KEEP_SENTINEL': 'keep'}):
            # When
            env = run.child_environment(Path('/new'), {'RUSTDOCFLAGS': '-D warnings'})
            # Then
            self.assertEqual(env['LETOPIS_TEST_EVIDENCE'], '/new/process')
            self.assertNotIn('LETOPIS_CLI_FIXTURE', env)
            self.assertNotIn('LETOPIS_FIXTURE_EXECUTABLE', env)
            self.assertEqual(env['KEEP_SENTINEL'], 'keep')
            self.assertEqual(env['RUSTDOCFLAGS'], '-D warnings')
            self.assertEqual(run.child_environment(Path('/new'),
                {'LETOPIS_CLI_FIXTURE': '/built/fixture'})['LETOPIS_CLI_FIXTURE'], '/built/fixture')

    def test_preflight_failure_writes_fail_receipt(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory) / 'new'
            with patch.object(sys, 'argv', ['run.py', '--platform', 'host', '--evidence', str(evidence)]), \
                 patch.object(run, 'hash_inputs', side_effect=FileNotFoundError('missing source')):
                # When / Then
                with self.assertRaises(FileNotFoundError):
                    run.main()
            manifest = json.loads((evidence / 'manifest.json').read_text())
            self.assertEqual(manifest['status'], 'FAIL')
            self.assertIn('FileNotFoundError', manifest['error'])

    def test_schema_selects_command_and_exit_not_catalogue_union(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        valid = {'ok': True, 'result': {'errors': []}}
        # When
        run.validate_response(catalogue, 'check', valid, 0)
        # When / Then
        with self.assertRaises(ValidationError):
            run.validate_response(catalogue, 'inspect', valid, 0)
        # When / Then
        with self.assertRaises(AssertionError):
            run.validate_response(catalogue, 'check', valid, 1)
        # When / Then
        with self.assertRaises(ValidationError):
            run.validate_response(catalogue, 'check', {**valid, 'extra': 1}, 0)

    def test_handler_capture_classifies_lists_without_loosening_validation(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = [
            {'exit': 0, 'response': {'ok': True, 'result': {'errors': []}}},
            {'exit': 0, 'response': {'ok': True, 'result': {'snapshot': 'a' * 40, 'elements': {}}}},
            {'exit': 1, 'response': {'ok': False, 'error': {'code': 'internal_error', 'message': 'safe', 'details': {}}}},
            {'exit': 0, 'response': {'ok': True, 'result': []}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            def capture(items):
                (evidence / 'handlers.log').write_text(''.join('CLI_RESPONSE ' + json.dumps(item) + '\n' for item in items))
                run.capture_handler_responses(evidence, catalogue)
            # When
            capture(records)
            results = json.loads((evidence / 'handler-responses.json').read_text())
            # Then
            self.assertEqual(results[-1]['command'], 'search')
            self.assertEqual(results[-1]['source'], 'injected handler unit test')
            # When / Then
            with self.assertRaises(AssertionError):
                capture(records[:-1])
            # When / Then
            with self.assertRaises(AssertionError):
                capture([*records[:-1], {**records[-1], 'exit': 1}])
            malformed = {'ok': True, 'result': [{'element_id': 'x.md', 'path': 'x.md', 'lines': [1, 2], 'revision': 1, 'flags': [], 'text': 'private'}]}
            # When / Then
            with self.assertRaises(ValidationError):
                capture([*records[:-1], {'exit': 0, 'response': malformed}])

    def test_product_reading_capture_requires_all_real_classifications(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = self.make_reading_records()
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            # When
            self.capture_reading(evidence, catalogue, records)
            observed = json.loads((evidence / 'product-reading-responses.json').read_text())
            # Then
            self.assertEqual([r['case'] for r in observed],
                             ['search_success', 'search_fallback', 'inspect_success',
                              'inspect_mixed', 'inspect_removed', 'not_found', 'malformed_input'])
            self.assertTrue(all(r['source'] == 'actual product executable' for r in observed))
            for index in range(len(records)):
                with self.subTest(missing=records[index]['case']):
                    # When / Then
                    with self.assertRaises(AssertionError):
                        self.capture_reading(evidence, catalogue, records[:index] + records[index+1:])

    def test_product_reading_rejects_wrong_shape_exit_and_classification(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = self.make_reading_records()
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            for field, value, error in [('exit', 1, AssertionError),
                    ('response', {'ok': True, 'result': {'errors': []}}, ValidationError),
                    ('command', 'inspect', ValidationError),
                    ('case', 'not_found', AssertionError)]:
                # When / Then
                with self.subTest(field=field), self.assertRaises(error):
                    self.capture_reading(evidence, catalogue, [{**records[0], field: value}, *records[1:]])

    def test_injected_receipt_cannot_replace_product_reading(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = self.make_reading_records()
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            injected = 'CLI_RESPONSE ' + json.dumps(records[0]) + '\n'
            # When
            self.capture_reading(evidence, catalogue, records, injected)
            observed = json.loads((evidence / 'product-reading-responses.json').read_text())
            # Then
            self.assertEqual(len(observed), 7)
            # When / Then
            with self.assertRaises(AssertionError):
                self.capture_reading(evidence, catalogue, records[1:], injected)

    def make_reading_records(self):
        success = {'ok': True, 'result': []}
        return [
            {'command': 'search', 'case': 'search_success', 'exit': 0, 'response': success},
            {'command': 'search', 'case': 'search_fallback', 'exit': 0, 'response': success},
            {'command': 'inspect', 'case': 'inspect_success', 'exit': 0,
             'response': {'ok': True, 'result': {'snapshot': 'a' * 40, 'elements': {}}}},
            {'command': 'inspect', 'case': 'inspect_mixed', 'exit': 0,
             'response': {'ok': True, 'result': {'snapshot': 'a' * 40, 'elements': {
                 'knowledge.md': {'path': 'knowledge.md', 'lines': [1, 2], 'hash': 'b' * 64},
                 'src/cache.rs#L2-L2': {'path': 'src/cache.rs', 'lines': [2, 2], 'hash': 'c' * 64},
                 'src/missing.rs': {'path': 'src/missing.rs', 'hash': None},
                 'https://example.invalid/a?q=1#fragment': {
                     'path': 'https://example.invalid/a?q=1#fragment', 'hash': None}}}}},
            {'command': 'inspect', 'case': 'inspect_removed', 'exit': 1,
             'response': {'ok': False, 'error': {'code': 'not_found', 'message': 'safe', 'details': {}}}},
            {'command': 'inspect', 'case': 'not_found', 'exit': 1,
             'response': {'ok': False, 'error': {'code': 'not_found', 'message': 'safe', 'details': {}}}},
            {'command': 'search', 'case': 'malformed_input', 'exit': 1,
             'response': {'ok': False, 'error': {'code': 'invalid_request', 'message': 'safe', 'details': {}}}},
        ]

    def capture_reading(self, evidence, catalogue, records, prefix=''):
        (evidence / 'runtime.log').write_text(prefix + ''.join(
            'PRODUCT_READING ' + json.dumps(record) + '\n' for record in records))
        run.capture_reading_responses(evidence, catalogue)

    def test_handler_capture_distinguishes_status_from_reindex_revision(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = [
            {'exit': 0, 'response': {'ok': True, 'result': {'errors': []}}},
            {'exit': 0, 'response': {'ok': True, 'result': {'snapshot': 'a' * 40, 'elements': {}}}},
            {'exit': 0, 'response': {'ok': True, 'result': []}},
            {'exit': 1, 'response': {'ok': False, 'error': {'code': 'internal_error', 'message': 'safe', 'details': {}}}},
            {'exit': 0, 'response': {'ok': True, 'result': {'revision': 7, 'embedding_backlog': 2,
                'embedding_failed': 1, 'locks': [], 'problems': []}}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            (evidence / 'handlers.log').write_text(''.join(
                'CLI_RESPONSE ' + json.dumps(r) + '\n' for r in records))
            # When
            run.capture_handler_responses(evidence, catalogue)
            observed = json.loads((evidence / 'handler-responses.json').read_text())
            # Then
            self.assertEqual(observed[-1]['command'], 'status')

    def test_reindex_product_receipts_require_valid_actual_output(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            log = evidence / 'reindex.log'
            log.write_text('PRODUCT_REINDEX {"ok":true,"result":{"revision":7}}\n')
            # When
            run.capture_reindex_responses(evidence, catalogue)
            observed = json.loads((evidence / 'product-reindex-responses.json').read_text())
            # Then
            self.assertEqual(observed[0]['response']['result'], {'revision': 7})
            log.write_text('PRODUCT_REINDEX {"ok":true,"result":{"revision":"7"}}\n')
            # When / Then
            with self.assertRaises(ValidationError):
                run.capture_reindex_responses(evidence, catalogue)
            log.write_text('CLI_RESPONSE {"ok":true,"result":{"revision":7}}\n')
            # When / Then
            with self.assertRaises(AssertionError):
                run.capture_reindex_responses(evidence, catalogue)

    def test_handler_capture_validates_reindex_command_shape(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        base = [
            {'exit': 0, 'response': {'ok': True, 'result': {'errors': []}}},
            {'exit': 0, 'response': {'ok': True, 'result': {'snapshot': 'a' * 40, 'elements': {}}}},
            {'exit': 0, 'response': {'ok': True, 'result': []}},
            {'exit': 1, 'response': {'ok': False, 'error': {'code': 'internal_error', 'message': 'safe', 'details': {}}}},
        ]
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            def capture(result):
                records = [*base, {'exit': 0, 'response': {'ok': True, 'result': result}}]
                (evidence / 'handlers.log').write_text(''.join(
                    'CLI_RESPONSE ' + json.dumps(r) + '\n' for r in records))
                run.capture_handler_responses(evidence, catalogue)
            # When
            capture({'revision': 7})
            records = json.loads((evidence / 'handler-responses.json').read_text())
            # Then
            self.assertEqual(records[-1]['command'], 'reindex')
            for invalid in ({'revision': '7'}, {'revision': 7, 'extra': 1}):
                # When / Then
                with self.assertRaises(ValidationError):
                    capture(invalid)

    def test_transport_accepts_check_success_and_rejects_invalid_input_before_io(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        repositories = []
        check_cases = []
        terminator = "\n"
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            def execute(argv, name, *, cwd, expected=0, stdin=None):
                if argv[0] == 'git':
                    return
                repositories.append(str(cwd))
                if argv[1:] == ['check'] and stdin == b'{}':
                    self.assertEqual(expected, 0)
                    (cwd / '.letopis').mkdir()
                    response = {'ok': True, 'result': {'errors': []}}
                elif argv[1] == 'inspect' and stdin != b'{}':
                    (cwd / '.letopis').mkdir(exist_ok=True)
                    response = {'ok': False, 'error': {'code': 'not_found', 'message': 'unknown', 'details': {}}}
                elif argv[1:] == ['reindex'] and stdin == b'{}':
                    (cwd / '.letopis').mkdir(exist_ok=True)
                    response = {'ok': True, 'result': {'revision': 1}}
                elif argv[1] == 'search':
                    (cwd / '.letopis').mkdir(exist_ok=True)
                    response = {'ok': True, 'result': []}
                elif argv[1:] == ['embed', '--drain'] and not stdin:
                    response = {'ok': True, 'result': {}}
                else:
                    self.assertEqual(expected, 1)
                    self.assertFalse((cwd / '.letopis').exists())
                    response = {'ok': False, 'error': {'code': 'invalid_request', 'message': 'invalid command input', 'details': {}}}
                if argv[1:] == ['check']:
                    check_cases.append((stdin, expected, (cwd / '.letopis').exists()))
                (evidence / name).write_text(json.dumps(response) + terminator)
                Path(str(evidence / name) + '.stderr').write_bytes(b'')
            # When
            run.capture_transport(root, evidence, execute, Path('/product'), catalogue)
            # Then
            self.assertEqual(len(repositories), len(set(repositories)))
            self.assertEqual(check_cases, [(b'{}', 0, True), (b'{"extra":1}', 1, False)])
            observed = json.loads((evidence / 'product-transport-responses.json').read_text())
            self.assertEqual(observed[0]['response'], {'ok': True, 'result': {'errors': []}})
            self.assertTrue(any(r['command'] == 'reindex' and r['exit'] == 0 for r in observed))
            terminator = "\n{}\n"
            # When / Then
            with self.assertRaises(AssertionError):
                run.capture_transport(root, evidence, execute, Path('/product'), catalogue)

    def test_thin_slice_requires_all_actual_cases(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = self.make_thin_slice_records()
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            # When
            self.capture_thin_slice(evidence, catalogue, records)
            observed = json.loads((evidence / 'thin-slice-responses.json').read_text())
            # Then
            self.assertEqual(len(observed), 7)
            self.assertTrue(all(r['source'] == 'actual product executable' for r in observed))
            for index in range(len(records)):
                with self.subTest(missing=records[index]['case']):
                    # When / Then
                    with self.assertRaises(AssertionError):
                        self.capture_thin_slice(evidence, catalogue, records[:index] + records[index+1:])

    def test_thin_slice_rejects_invalid_envelope_or_exit(self):
        # Given
        from jsonschema import ValidationError
        root = Path(__file__).resolve().parents[2]
        catalogue = json.loads((root / run.SCHEMA).read_text())
        records = self.make_thin_slice_records()
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory)
            for change, error in [({'exit': 1}, AssertionError),
                                  ({'response': {'ok': True, 'result': {'errors': []}}}, ValidationError),
                                  ({'case': 'invented'}, AssertionError)]:
                with self.subTest(change=change):
                    # When / Then
                    with self.assertRaises(error):
                        self.capture_thin_slice(evidence, catalogue, [{**records[0], **change}, *records[1:]])
            bad = {**records[-1], 'response': {'ok': False, 'error':
                {'code': 'internal_error', 'message': 'wrong classification', 'details': {}}}}
            # When / Then
            with self.assertRaises(AssertionError):
                self.capture_thin_slice(evidence, catalogue, [*records[:-1], bad])

    def make_thin_slice_records(self):
        search = {'ok': True, 'result': [{'element_id': 'knowledge/cache.md',
            'path': 'knowledge/cache.md', 'lines': [1, 3], 'revision': 1, 'flags': []}]}
        records = [{'case': case, 'command': 'search', 'exit': 0, 'response': search}
                   for case in ['load', 'background', 'provider_failure', 'invalid_neighbor', 'text_race']]
        records.append({'case': 'inspect', 'command': 'inspect', 'exit': 0,
                        'response': {'ok': True, 'result': {'snapshot': 'a' * 40,
                            'elements': {'knowledge/cache.md': {'path': 'knowledge/cache.md',
                                'lines': [1, 3], 'hash': 'b' * 64}}}}})
        records.append({'case': 'invalid_inspect', 'command': 'inspect', 'exit': 1,
                        'response': {'ok': False, 'error': {'code': 'validation_failed',
                            'message': 'invalid current file', 'details': {}}}})
        return records

    def capture_thin_slice(self, evidence, catalogue, records):
        (evidence / 'thin-slice.log').write_text(''.join(
            'THIN_SLICE_RESPONSE ' + json.dumps(record) + '\n' for record in records))
        run.capture_thin_slice_responses(evidence, catalogue)

    def test_inventory_covers_support_schema_and_ignores_bytecode(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        # When
        inputs = run.hash_inputs(root)
        for name in ['tests/support/cli.rs', 'src/main.rs', 'tests/cli/run.py',
                     'tests/cli/Dockerfile', '.dockerignore', run.SCHEMA,
                     'tests/cli_drain.rs', 'tests/support/drain.rs',
                     'tests/document_refresh.rs', 'tests/embedding_service.rs',
                     'tests/thin_slice.rs', 'tests/support/thin_slice.rs', 'skills/letopis/SKILL.md',
                     'tests/fixtures/markdown/thin_slice/cache.rs', 'tests/fixtures/markdown/thin_slice/cache.md.in',
                     'tests/fixtures/markdown/thin_slice/flow.md.in']:
            # Then
            self.assertIn(name, inputs)
        self.assertFalse(any('__pycache__' in name for name in inputs))

    def test_every_negative_control_matches_current_source_exactly_once(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        for name, relative, needle, _replacement, _test, _oracle in negative_controls.CONTROLS:
            with self.subTest(control=name):
                occurrences = (root / relative).read_text().count(needle)
                # When / Then
                self.assertEqual(occurrences, 1, f'{name}: stale or ambiguous source anchor')

    def test_reading_controls_match_current_source_exactly_once(self):
        # Given
        root = Path(__file__).resolve().parents[2]
        for name, relative, needle, *_rest in negative_controls.READING_CONTROLS:
            with self.subTest(control=name):
                # When / Then
                self.assertEqual((root / relative).read_text().count(needle), 1)

    def test_artifact_resolution_respects_target_specific_cargo_path(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            log = Path(directory) / 'build.log'
            artifact = '/target/aarch64-apple-darwin/debug/examples/cli_runtime_fixture'
            log.write_text(json.dumps({'reason':'compiler-artifact',
                'target':{'name':'cli_runtime_fixture'}, 'executable':artifact}) + '\n')
            # When / Then
            self.assertEqual(run.read_artifacts(log)['cli_runtime_fixture'], Path(artifact))


class MakeWorkflowTests(unittest.TestCase):
    def test_success_is_one_ok_line_for_check_and_verify(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            for target in ('check', 'verify'):
                # When
                result = self.run_make(directory, target)
                # Then
                self.assertEqual(result.returncode, 0, result.stdout)
                self.assertEqual(result.stdout, 'ok\n')

    def test_verbose_verify_retains_every_steps_output(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            # When
            result = self.run_make(directory, 'verify', verbose=True)
            # Then
            self.assertEqual(result.returncode, 0, result.stdout)
            for step in ('fmt', 'check', 'clippy', 'test', 'doc'):
                self.assertIn('stdout ' + step + '\n', result.stdout)
                self.assertIn('stderr ' + step + '\n', result.stdout)
            self.assertEqual(result.stdout.count('stdout test\n'), 2)
            self.assertTrue(result.stdout.endswith('ok\n'))

    def test_failure_prints_complete_failed_step_and_stops(self):
        # Given
        with tempfile.TemporaryDirectory() as directory:
            for failed in ('clippy', 'doc'):
                # When
                result = self.run_make(directory, 'verify', fail_step=failed)
                # Then
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


if __name__ == '__main__':
    unittest.main()
