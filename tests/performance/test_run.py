"""Runner budget and input controls are exercised without benchmarks."""

import unittest
import json
import io
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import signal
from types import SimpleNamespace
from unittest.mock import patch

from run import parse_arguments, run_series


class RunnerTests(unittest.TestCase):
    @unittest.skipUnless(os.environ.get("PERF_BASELINE_FILE"), "requires pinned release build receipt")
    def test_real_sigint_stops_campaign_and_records_interrupted(self):
        # Given: a real initial publication, with a later status scenario queued.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            evidence = root / "cancelled"
            argv = [sys.executable, str(Path(__file__).with_name("run.py")), "--stage", "survey", "--nodes", "60",
                    "--scenarios", "search", "status", "--baseline", os.environ["PERF_BASELINE_FILE"], "--evidence", str(evidence)]
            child = subprocess.Popen(argv, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
            try:
                deadline = time.monotonic() + 3
                while not (evidence / "samples.jsonl").exists() and time.monotonic() < deadline:
                    time.sleep(.01)
                time.sleep(.1)
                # When: Ctrl-C reaches the controller during its owned measurement.
                child.send_signal(signal.SIGINT)
                child.communicate(timeout=4)
                # Then: cancellation is durable and no following case starts.
                self.assertEqual(json.loads((evidence / "completion.json").read_text())["outcome"], "interrupted")
                self.assertFalse((evidence / "case-status-0").exists())
            finally:
                if child.poll() is None:
                    child.kill()
                child.communicate()

    def test_interrupted_measurement_is_persisted_and_aborts_remaining_series(self):
        # Given: the first invocation was interrupted by the user.
        args = SimpleNamespace(nodes=60, repetitions=2, timeout=1)
        scenario = {"repository": Path.cwd(), "argv": ["check"], "stdin": b"{}",
                    "setup_seconds": 0, "cache_label": "uncontrolled", "expected": {"command": "check"}}
        record = {"outcome": "interrupted", "duration_seconds": .1, "stdout": {"truncated": False}}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            # When / Then: cancellation reaches the campaign after durable recording.
            with (root / "samples").open("wb") as stream, patch("run.verify_build"), \
                    patch("run.prepare_scenario", return_value=scenario), \
                    patch("run.measure_process", return_value=record) as measured, patch("builtins.print"):
                with self.assertRaises(KeyboardInterrupt):
                    run_series(args, {"executable": "fixture"}, {}, root, stream, "check", time.monotonic() + 10)
                self.assertEqual(measured.call_count, 1)
            self.assertEqual(json.loads((root / "samples").read_text())["outcome"], "interrupted")

    def test_profile_window_rejects_zero_and_invalid_limits(self):
        # Given: zero, negative, non-finite and excessive recording windows.
        for value in ("0", "-1", "nan", "121"):
            arguments = ["run.py", "--stage", "profile", "--profile-window", value]
            # When / Then: malformed windows fail rather than run a different campaign.
            with patch("sys.argv", arguments), patch("sys.stderr", io.StringIO()):
                with self.assertRaises(SystemExit):
                    parse_arguments()

    def test_successful_process_with_wrong_result_is_rejected(self):
        # Given: a fast exit-zero process returns an incorrect search ID.
        args = SimpleNamespace(nodes=60, repetitions=1, timeout=120)
        scenario = {"repository": Path.cwd(), "argv": ["search"], "stdin": b"{}",
                    "setup_seconds": 0, "cache_label": "uncontrolled",
                    "expected": {"command": "search", "id": "right.md"}}
        record = {"outcome": "success", "duration_seconds": .1, "exit_code": 0,
                  "stdout": {"truncated": False, "capture_hex": json.dumps({"ok": True,
                    "result": [{"element_id": "wrong.md", "flags": []}]}).encode().hex()}}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            # When / Then: persist the rejected attempt and fail rather than quietly complete.
            with (root / "samples").open("wb") as stream, \
                    patch("run.prepare_scenario", return_value=scenario), \
                    patch("run.verify_build"), patch("run.measure_process", return_value=record):
                with self.assertRaises(ValueError):
                    run_series(args, {"executable": "fixture"}, {}, root, stream, "search", time.monotonic() + 10)
            self.assertFalse(json.loads((root / "samples").read_text())["validation"]["valid"])

    @unittest.skipUnless(os.environ.get("PERF_BASELINE_FILE"), "requires pinned release build receipt")
    def test_stage_watchdog_preserves_partial_evidence_during_setup(self):
        # Given: a fresh campaign whose budget cannot cover real first publication.
        with tempfile.TemporaryDirectory() as directory:
            evidence = Path(directory).resolve() / "budget"
            runner = Path(__file__).with_name("run.py")
            started = time.monotonic()
            # When: the global stage budget interrupts setup or the first owned call.
            result = subprocess.run([sys.executable, str(runner), "--stage", "survey", "--nodes", "60",
                                     "--stage-seconds", "0.15", "--baseline", os.environ["PERF_BASELINE_FILE"],
                                     "--evidence", str(evidence)], capture_output=True, timeout=4)
            # Then: failure is visible and bounded, with immutable partial context.
            self.assertNotEqual(result.returncode, 0)
            self.assertLess(time.monotonic() - started, 3)
            receipt = json.loads((evidence / "completion.json").read_text())
            self.assertEqual(receipt["outcome"], "failed")
            self.assertIn("watchdog", receipt["diagnostic"])
            self.assertTrue((evidence / "manifest.json").exists())

    def test_explicit_initial_publication_budget_is_bounded(self):
        # Given: first publication needs a separately recorded full-audit override.
        arguments = ["run.py", "--stage", "survey", "--nodes", "1000", "--initial-timeout", "600"]
        # When: parsing a permitted setup override.
        with patch("sys.argv", arguments):
            result = parse_arguments()
        # Then: ordinary invocations remain at their 120-second default.
        self.assertEqual(result.initial_timeout, 600)
        self.assertEqual(result.timeout, 120)


if __name__ == "__main__":
    unittest.main()
