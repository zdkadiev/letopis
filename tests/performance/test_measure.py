"""Process failures and resource accounting use independent tiny programs."""

import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import threading
import unittest
from unittest.mock import patch

from measure import measure_process


class MeasurementTests(unittest.TestCase):
    def test_retains_diagnostics_emitted_during_timeout_cleanup(self):
        # Given: a child writes a small diagnostic while handling TERM.
        code = ("import signal,sys,time;"
                "signal.signal(signal.SIGTERM,lambda *_:(sys.stderr.write('cleanup diagnostic'),sys.stderr.flush(),sys.exit(1)));"
                "time.sleep(60)")
        # When: timeout cleanup terminates that owned child.
        result = measure_process([sys.executable, "-c", code], Path.cwd(), b"", {"timeout": .1})
        # Then: bounded failure evidence includes bytes produced during cleanup.
        self.assertEqual(result["outcome"], "timeout")
        self.assertIn(b"cleanup diagnostic", bytes.fromhex(result["stderr"]["capture_hex"]))

    def test_timeout_retains_resource_observation_for_killed_child(self):
        # Given: a live owned child that exceeds its bounded deadline.
        result = measure_process([sys.executable, "-c", "import time;time.sleep(60)"],
                                 Path.cwd(), b"", {"timeout": .05})
        # When / Then: cancellation is censored, but available wait4 RAM/CPU are retained.
        self.assertEqual(result["outcome"], "timeout")
        self.assertIn("peak_rss", result["resources"])
        self.assertGreater(result["resources"]["peak_rss"]["value"], 0)

    def test_spawn_observer_receives_only_the_owned_pid(self):
        # Given: a callback used to attach a stack sampler to this exact child.
        observed = []
        # When: starting one owned process.
        result = measure_process([sys.executable, "-c", "print('fixture')"], Path.cwd(), b"",
                                 {"on_spawn": observed.append})
        # Then: attachment never relies on a process-name search.
        self.assertEqual(observed, [result["pid"]])

    def test_explicit_cancellation_stops_only_owned_session(self):
        # Given: an owned long-running child and an explicit cancellation event.
        cancel = threading.Event()
        cancel.set()
        # When: a scheduler cancels its writer after stopping readers.
        result = measure_process([sys.executable, "-c", "import time;time.sleep(60)"],
                                 Path.cwd(), b"", {"cancel_event": cancel, "timeout": 0.1})
        # Then: the collector stops and reaps rather than awaiting the full sleep.
        self.assertEqual(result["outcome"], "interrupted")
        self.assertLess(result["duration_seconds"], 1)

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
