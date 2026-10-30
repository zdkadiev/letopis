"""Runner budget and input controls are exercised without benchmarks."""

import unittest
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
from unittest.mock import patch

from run import parse_arguments


class RunnerTests(unittest.TestCase):
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
