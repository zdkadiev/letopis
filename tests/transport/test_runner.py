import importlib.util
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('tk6_runner', ROOT / 'run.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)


class RunnerTests(unittest.TestCase):
    def test_existing_evidence_is_rejected_without_changes(self):
        # Given
        with tempfile.TemporaryDirectory() as temporary:
            evidence = Path(temporary) / 'existing'
            evidence.mkdir()
            marker = evidence / 'keep.bin'
            marker.write_bytes(b'untouched\x00receipt')
            # When
            result = subprocess.run([sys.executable, '-B', ROOT / 'run.py', '--evidence', evidence], capture_output=True, text=True)
            # Then
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual({p.name: p.read_bytes() for p in evidence.iterdir()}, {'keep.bin': b'untouched\x00receipt'})

    def test_build_or_unrelated_failure_cannot_count_as_detection(self):
        # Given
        for build, code, output in [(1, 1, 'FAIL: target\nAssertionError'), (0, 0, 'FAIL: target\nAssertionError'), (0, 1, 'ImportError: target'), (0, 1, 'FAIL: other\nAssertionError')]:
            # When / Then
            with self.subTest(output=output), self.assertRaises(RuntimeError):
                runner.require_detection(build, code, output, 'FAIL: target')

    def test_named_behavioral_assertion_is_required(self):
        # Given: inputs are passed inline
        # When
        runner.require_detection(0, 1, 'FAIL: target\nAssertionError', 'FAIL: target')
        # When / Then
        with self.assertRaises(RuntimeError):
            runner.require_detection(0, 1, 'FAIL: target\nTimeoutExpired', 'FAIL: target')


if __name__ == '__main__':
    unittest.main()
