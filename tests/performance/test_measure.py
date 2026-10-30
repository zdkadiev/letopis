"""Process failures and resource accounting use independent tiny programs."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

from measure import measure_process


class MeasurementTests(unittest.TestCase):
    def test_collects_binary_streams_and_exit_status(self):
        # Given: a process echoing binary stdin and returning a nonzero status.
        code = "import sys;sys.stdout.buffer.write(sys.stdin.buffer.read());sys.stderr.buffer.write(b'\\xff');sys.exit(7)"
        # When: measuring its complete lifetime.
        result = measure_process([sys.executable, "-c", code], Path.cwd(), b"a\x00b", {})
        # Then: raw bytes survive without requiring UTF-8 or successful exit.
        self.assertEqual(result["exit_code"], 7)
        self.assertEqual(result["outcome"], "nonzero")
        self.assertEqual(bytes.fromhex(result["stdout"]["capture_hex"]), b"a\x00b")
        self.assertEqual(bytes.fromhex(result["stderr"]["capture_hex"]), b"\xff")
        self.assertGreaterEqual(result["duration_seconds"], 0)
        self.assertIn("scope", result["resources"]["cpu_seconds"])

    def test_bounds_capture_but_hashes_complete_stream(self):
        # Given: more output than a 64-KiB capture limit.
        code = "import sys;sys.stdout.buffer.write(b'x'*100000)"
        # When: recording an output-heavy subprocess.
        result = measure_process([sys.executable, "-c", code], Path.cwd(), b"", {})
        # Then: length is exact while retained bytes are bounded.
        self.assertEqual(result["stdout"]["bytes"], 100000)
        self.assertEqual(len(result["stdout"]["capture_hex"]), 65536 * 2)
        self.assertTrue(result["stdout"]["truncated"])

    def test_timeout_reaps_grandchild_retaining_pipe(self):
        # Given: the parent exits while its own grandchild still holds stdout.
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory).resolve() / "grandchild"
            code = ("import subprocess,sys; p=subprocess.Popen([sys.executable,'-c',"
                    "'import time;time.sleep(60)']);open(sys.argv[1],'w').write(str(p.pid))")
            sentinel = subprocess.Popen([sys.executable, "-c", "import time;time.sleep(60)"])
            # When: pipe EOF exceeds the owned deadline.
            try:
                started = time.monotonic()
                result = measure_process([sys.executable, "-c", code, str(path)], Path.cwd(),
                                         b"", {"timeout": 0.2})
                # Then: cleanup is bounded and leaves unrelated work alone.
                self.assertEqual(result["outcome"], "timeout")
                self.assertLess(time.monotonic() - started, 2)
                self.assertIsNone(sentinel.poll())
                pid = path.read_text()
                state = subprocess.run(["ps", "-o", "stat=", "-p", pid], capture_output=True, text=True)
                self.assertTrue(not state.stdout.strip() or state.stdout.lstrip().startswith("Z"))
            finally:
                if path.exists():
                    try:
                        os.kill(int(path.read_text()), 15)
                    except ProcessLookupError:
                        pass
                sentinel.terminate()
                sentinel.wait()

    def test_spawn_failure_and_interruption_have_distinct_outcomes(self):
        # Given: a missing executable and a separately interrupted running child.
        missing = measure_process(["/no/such/executable"], Path.cwd(), b"", {})
        # When: the collector is interrupted after process creation.
        with patch("measure.collect_process", side_effect=KeyboardInterrupt):
            interrupted = measure_process([sys.executable, "-c", "import time;time.sleep(60)"],
                                          Path.cwd(), b"", {})
        # Then: neither outcome can be summarized as a successful latency.
        self.assertEqual(missing["outcome"], "spawn_failed")
        self.assertEqual(interrupted["outcome"], "interrupted")
        self.assertIsNotNone(interrupted["exit_code"])


if __name__ == "__main__":
    unittest.main()
