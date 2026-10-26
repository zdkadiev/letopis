"""Transport behavior: wrong routing, validation, timeout, cancellation or redaction must fail."""
import json
import os
import signal
import socket
import subprocess
import tempfile
import time
import unittest
from fixtures import COMPATIBLE, KEY, NATIVE, TEXTS, endpoint, invoke, payload, start


class TransportTests(unittest.TestCase):
    def assert_race_outcome(self, code, out, err):
        value = json.loads(out)
        self.assertEqual(len(out.splitlines()), 1)
        if value['ok']: self.assertIn(code, (0, 1))
        else: self.assertEqual(code, 1)
        if not value['ok']: self.assertEqual(value['error']['code'], 'cancelled')
        self.assertEqual(err, '')

    def assert_safe_failure(self, outcome):
        code, value, out, err = outcome
        self.assertEqual(code, 1, (out, err))
        self.assertFalse(value['ok'])
        self.assertEqual(value['error']['code'], 'embedding_failed')
        for canary in (KEY, TEXTS[0], 'RESPONSE_BODY_CANARY', 'RESPONSE_HEADER_CANARY', 'QUERY_CANARY', 'USERINFO_CANARY'):
            self.assertNotIn(canary, out + err)

    def test_exact_native_and_compatible_wire(self):
        # Given
        for provider, response, suffix, body in [
            ('ollama', NATIVE, 'embed', {'model': 'fixture-model', 'input': TEXTS, 'truncate': False}),
            ('openai', COMPATIBLE, 'embeddings', {'model': 'fixture-model', 'input': TEXTS, 'encoding_format': 'float'}),
        ]:
            with self.subTest(provider=provider), endpoint(body=response) as server:
                # When
                code, value, out, err = invoke(payload(server['url'], provider))
                # Then
                self.assertEqual(code, 0, (out, err))
                self.assertEqual(value, {'ok': True, 'result': [[1, 2, 3], [4, 5, 6]]})
                self.assertEqual(len(server['requests']), 1)
                request = server['requests'][0]
                self.assertEqual(request['method'], 'POST')
                self.assertEqual(request['path'], '/api/' + suffix)
                self.assertEqual(request['body'], body)
                self.assertEqual(request['headers'].get('authorization'), 'Bearer ' + KEY if provider == 'openai' else None)

    def test_http_errors_and_non_200_are_not_success_or_retried(self):
        # Given
        for status in [201, 202, 401, 429, 500]:
            with self.subTest(status=status), endpoint(status=status, body=NATIVE if status < 300 else b'RESPONSE_BODY_CANARY') as server:
                # When / Then
                self.assert_safe_failure(invoke(payload(server['url'])))
                self.assertEqual(len(server['requests']), 1)

    def test_malformed_body_and_provider_error_are_safe(self):
        # Given
        for body in [b'RESPONSE_BODY_CANARY', {'error': 'RESPONSE_BODY_CANARY'}, {'model': 'm', 'embeddings': [[1, 2, 3]]}]:
            with self.subTest(body=body), endpoint(body=body) as server:
                # When / Then
                self.assert_safe_failure(invoke(payload(server['url'])))

    def test_refusal_reset_and_truncated_response(self):
        # Given
        with socket.socket() as s:
            s.bind(('127.0.0.1', 0))
            url = f'http://127.0.0.1:{s.getsockname()[1]}'
            # When / Then
            self.assert_safe_failure(invoke(payload(url)))
        for mode in ['reset', 'truncated']:
            with self.subTest(mode=mode), endpoint(mode=mode) as server:
                # When / Then
                self.assert_safe_failure(invoke(payload(server['url'])))

    def test_short_deadline_covers_headers_body_and_trickle(self):
        # Given
        for mode in ['silent_headers', 'silent_body', 'trickle']:
            with self.subTest(mode=mode), endpoint(mode=mode) as server:
                begun = time.monotonic()
                try:
                    # When
                    result = invoke(payload(server['url']), ['--deadline-ms', '250'], timeout=4)
                except subprocess.TimeoutExpired:
                    # When / Then
                    self.assertTrue(server['accepted'].is_set())
                    # When / Then
                    self.fail('HTTP attempt survived its deadline and outer safety timeout')
                # When / Then
                self.assert_safe_failure(result)
                self.assertTrue(server['accepted'].is_set())
                self.assertLess(time.monotonic() - begun, 3)
                self.assertIn('timeout', result[1]['error']['message'])

    def test_actual_default_deadline_is_60_seconds(self):
        # Given
        with endpoint(mode='silent_headers') as server:
            begun = time.monotonic()
            # When
            result = invoke(payload(server['url']), timeout=75)
            elapsed = time.monotonic() - begun
            # When / Then
            self.assert_safe_failure(result)
            self.assertTrue(server['accepted'].is_set())
            self.assertGreaterEqual(elapsed, 58)
            self.assertLess(elapsed, 73)
            self.assertIn('timeout', result[1]['error']['message'])

    def test_foreground_sigint_is_one_cancelled_outcome(self):
        # Given
        for mode in ['silent_headers', 'silent_body']:
            with self.subTest(mode=mode), endpoint(mode=mode) as server:
                p = start(payload(server['url']))
                try:
                    # When / Then
                    self.assertTrue(server['accepted'].wait(3))
                    p.send_signal(signal.SIGINT)
                    out, err = p.communicate(timeout=3)
                    self.assertEqual(p.returncode, 1, (out, err))
                    value = json.loads(out)
                    self.assertFalse(value['ok'])
                    self.assertEqual(value['error']['code'], 'cancelled')
                    self.assertEqual(len(out.splitlines()), 1)
                    self.assertEqual(err, '')
                finally:
                    if p.poll() is None: p.kill(); p.communicate()

    def test_sigint_after_output_never_prints_second_json(self):
        # Given
        with endpoint() as server:
            p = start(payload(server['url']), ['--hold-after-output'])
            try:
                first = p.stdout.readline()
                # When / Then
                self.assertTrue(json.loads(first)['ok'])
                p.send_signal(signal.SIGINT)
                rest, err = p.communicate(timeout=3)
                self.assertEqual(rest, '')
                self.assertEqual(err, '')
                self.assertEqual(p.returncode, 1)
                # When / Then
                self.assert_race_outcome(p.returncode, first + rest, err)
            finally:
                if p.poll() is None: p.kill(); p.communicate()

    def test_sigint_response_race_has_one_complete_outcome(self):
        # Given
        for _ in range(12):
            with endpoint() as server:
                p = start(payload(server['url']))
                try:
                    # When / Then
                    self.assertTrue(server['accepted'].wait(3))
                    try: p.send_signal(signal.SIGINT)
                    except ProcessLookupError: pass
                    out, err = p.communicate(timeout=3)
                    # When / Then
                    self.assert_race_outcome(p.returncode, out, err)
                finally:
                    if p.poll() is None: p.kill(); p.communicate()

    def test_cancelled_output_wins_exit_status_over_completed_http_success(self):
        # Given
        with tempfile.TemporaryDirectory() as temporary, endpoint() as server:
            gate = os.path.join(temporary, 'release')
            p = start(payload(server['url']), ['--hold-after-cancel-output', '--before-output-gate', gate])
            try:
                from pathlib import Path
                ready = Path(gate).with_suffix('.ready')
                deadline = time.monotonic() + 3
                while not ready.exists() and time.monotonic() < deadline:
                    time.sleep(0.001)
                # When / Then
                self.assertTrue(ready.exists(), 'client did not reach output gate')
                self.assertEqual(ready.read_text(), 'success', 'fixture must have a completed HTTP success')
                p.send_signal(signal.SIGINT)
                first = p.stdout.readline()
                self.assertEqual(json.loads(first)['error']['code'], 'cancelled')
                # HTTP already succeeded; release its result only after cancellation printed.
                Path(gate).touch()
                rest, err = p.communicate(timeout=3)
                # When / Then
                self.assert_race_outcome(p.returncode, first + rest, err)
            finally:
                if p.poll() is None: p.kill(); p.communicate()

    def test_drain_sigint_retains_default_signal_death(self):
        # Given
        with endpoint(mode='silent_body') as server:
            p = start(payload(server['url']), ['--drain-fixture'])
            try:
                # When / Then
                self.assertTrue(server['accepted'].wait(3))
                p.send_signal(signal.SIGINT)
                out, err = p.communicate(timeout=3)
                self.assertEqual(p.returncode, -signal.SIGINT)
                self.assertEqual((out, err), ('', ''))
            finally:
                if p.poll() is None: p.kill(); p.communicate()

    def test_redirect_sink_receives_no_request_or_key(self):
        # Given
        for status in [301, 302, 303, 307, 308]:
            with self.subTest(status=status), endpoint() as sink, endpoint(status=status, location=sink['url'] + '/leak') as server:
                # When / Then
                self.assert_safe_failure(invoke(payload(server['url'], 'openai')))
                self.assertEqual(sink['requests'], [])

    def test_proxy_environment_cannot_change_http_or_https_route(self):
        # Given
        for variable in ['HTTP_PROXY', 'http_proxy', 'HTTPS_PROXY', 'https_proxy', 'ALL_PROXY', 'all_proxy']:
            with self.subTest(variable=variable), endpoint() as trap, endpoint(body=COMPATIBLE) as server:
                value = payload(server['url'], 'openai')
                if 'https' in variable.lower(): value['config']['parameters']['base_url'] = server['url'].replace('http:', 'https:') + '/api'
                # When
                result = invoke(value, ['--deadline-ms', '500'], {variable: trap['url']})
                # Then
                if 'https' in variable.lower(): self.assert_safe_failure(result)
                else: self.assertEqual(result[0], 0, result[2:])
                self.assertEqual(trap['requests'], [])

    def test_untrusted_tls_is_rejected(self):
        # Given
        with endpoint(tls='localhost') as server:
            # When / Then
            self.assert_safe_failure(invoke(payload(server['url']), ['--deadline-ms', '1500']))
            self.assertEqual(server['requests'], [])

    def test_invalid_url_and_credentials_fail_without_traffic(self):
        # Given
        with endpoint() as server:
            # When
            value = payload(server['url'], 'openai')
            # When / Then
            self.assert_safe_failure(invoke(value, env={'OPENAI_API_KEY': 'USERINFO_CANARY\r\nINJECTED: yes'}))
            value['config']['parameters']['base_url'] = server['url'].replace('://', '://USERINFO_CANARY@') + '?QUERY_CANARY'
            # When / Then
            self.assert_safe_failure(invoke(value))
            self.assertEqual(server['requests'], [])

    def test_query_is_preserved_but_never_leaks_on_failure(self):
        # Given
        with endpoint(status=500, body=b'RESPONSE_BODY_CANARY') as server:
            # When
            value = payload(server['url'])
            value['config']['parameters']['base_url'] += '?secret=QUERY_CANARY#not-on-wire'
            # When / Then
            self.assert_safe_failure(invoke(value))
            self.assertEqual(server['requests'][0]['path'], '/api/embed?secret=QUERY_CANARY')


if __name__ == '__main__':
    unittest.main()
