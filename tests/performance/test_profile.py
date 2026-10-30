"""Profiler validity and transparent Git instrumentation."""

import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
from types import SimpleNamespace
from unittest.mock import patch

from profile import record_profile, record_wall_stacks, run_profile_pair, summarize_stacks, validate_profile


class ProfileTests(unittest.TestCase):
    def test_missing_samply_returns_a_failed_measurement_receipt(self):
        # Given: this host has no profiler executable.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            scenario = {"repository": root, "argv": ["check"], "stdin": b"{}"}
            # When: attempting a profile through the normal adapter.
            with patch("profile.shutil.which", return_value=None):
                result = record_profile(scenario, {"executable": "fixture"}, root / "capture")
            # Then: callers get a persisted unavailable result, not a missing-key crash.
            self.assertEqual(result["outcome"], "unavailable")
            self.assertEqual(result["measurement"]["outcome"], "spawn_failed")
            self.assertTrue((root / "capture/receipt.json").exists())

    def test_missing_sampler_artifact_cannot_enter_profiled_latency_series(self):
        # Given: the CLI exits successfully but sampler produces no valid profile.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            scenario = {"repository": root, "argv": ["check"], "stdin": b"{}",
                        "expected": {"command": "check"}}
            def make_result():
                return {"outcome": "success", "duration_seconds": .1, "exit_code": 0,
                        "stdout": {"truncated": False, "capture_hex":
                            b'{"ok":true,"result":{"errors":[]}}'.hex()}}
            # When: recording an ordinary/profiled pair with an unavailable artifact.
            with (root / "samples").open("wb") as stream, patch("profile.verify_build"), \
                    patch("profile.prepare_scenario", return_value=scenario), \
                    patch("profile.measure_process", return_value=make_result()), \
                    patch("profile.record_profile", return_value={"measurement": make_result(), "outcome": "unavailable"}), \
                    patch("builtins.print"):
                run_profile_pair(SimpleNamespace(timeout=1, nodes=60), {"executable": "fixture"},
                                 {}, root, stream, "check", 0)
            rows = [json.loads(line) for line in (root / "samples").read_text().splitlines()]
            # Then: only the unprofiled call is a valid successful timing observation.
            self.assertTrue(rows[0]["validation"]["valid"])
            self.assertFalse(rows[1]["validation"]["valid"])
    def test_wall_sampler_attaches_by_owned_pid_and_finishes(self):
        # Given: a tiny fake sampler and a child that lives long enough to observe.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            sampler = root / "wall-sampler"
            sampler.write_text(f'#!{sys.executable}\nimport sys\nfrom pathlib import Path\n'
                               'Path(sys.argv[-1]).write_text(sys.argv[1])\n')
            sampler.chmod(0o755)
            code = 'import time;time.sleep(.1);print(\'{"ok":true,"result":{"errors":[]}}\')'
            scenario = {"repository": root, "argv": ["-c", code], "stdin": b"",
                        "expected": {"command": "check"}}
            # When: observing only the owned process using its exact PID.
            result = record_wall_stacks(scenario, {"executable": sys.executable,
                                                   "wall_sampler": str(sampler)}, root / "capture")
            # Then: the sample belongs to this invocation and its child is reaped.
            self.assertEqual(Path(result["path"]).read_text(), str(result["child"]["pid"]))
            self.assertEqual(result["child"]["outcome"], "success")
            self.assertTrue(result["validation"]["valid"])

    def test_symbolized_stacks_keep_inclusive_costs_nonadditive(self):
        # Given: one ten-microsecond CPU delta observed under two nested frames.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            thread = {"name": "fixture", "pid": 1, "samples": {"stack": [1], "threadCPUDelta": [10]},
                      "stackTable": {"prefix": [None, 0], "frame": [0, 1]},
                      "frameTable": {"address": [100, 200], "func": [0, 1]},
                      "funcTable": {"resource": [0, 0], "name": [0, 1]},
                      "resourceTable": {"lib": [0]}, "stringArray": ["0x64", "0xc8"]}
            data = {"meta": {"sampleUnits": {"threadCPUDelta": "µs"}}, "libs": [{"name": "letopis"}],
                    "threads": [thread]}
            with gzip.open(root / "profile.gz", "wt") as stream:
                json.dump(data, stream)
            symbols = {"string_table": ["source_check", "__wait4"], "data": [{"debug_name": "letopis",
                "symbol_table": [{"symbol": 0}, {"symbol": 1}], "known_addresses": [[100, 0], [200, 1]]}]}
            (root / "symbols.json").write_text(json.dumps(symbols))
            # When: summarizing inclusive stack observations.
            summary = summarize_stacks(root / "profile.gz", root / "symbols.json")
            # Then: each frame sees ten, while total observed CPU remains ten, not twenty.
            costs = {item["function"]: item["inclusive_cpu_delta"] for item in summary["functions"]}
            self.assertEqual(costs["source_check"], 10)
            self.assertEqual(costs["__wait4"], 10)
            self.assertEqual(summary["observed_cpu_delta"], 10)
            self.assertTrue(summary["inclusive_costs_overlap"])

    def test_rejects_empty_truncated_and_missing_profiles(self):
        # Given: a known two-sample profile and several invalid artifacts.
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory).resolve() / "profile.json.gz"
            with gzip.open(path, "wt") as stream:
                json.dump({"threads": [{"samples": {"length": 2, "stack": [0, 0]}}]}, stream)
            # When / Then: sample counts are checked, not just process exit.
            self.assertEqual(validate_profile(path)["samples"], 2)
            for content in (b"broken", gzip.compress(b'{"threads":[]}')):
                path.write_bytes(content)
                with self.assertRaises(ValueError):
                    validate_profile(path)
            with self.assertRaises(ValueError):
                validate_profile(path.with_name("missing"))

    def test_samply_adapter_preserves_input_and_disables_ui(self):
        # Given: a fake sampler that records argv/input and emits a known profile.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            sampler = root / "sampler"
            sampler.write_text(f'#!{sys.executable}\n' +
                'import gzip,json,sys\nfrom pathlib import Path\n'
                'args=sys.argv[1:]\nout=Path(args[args.index("--output")+1])\n'
                'out.with_suffix(".args").write_text(json.dumps(args))\n'
                'data=sys.stdin.buffer.read();out.with_suffix(".input").write_bytes(data)\n'
                'with gzip.open(out,"wt") as f:json.dump({"threads":[{"samples":{"length":1,"stack":[0]}}]},f)\n'
                'print("{}")\n')
            sampler.chmod(0o755)
            scenario = {"repository": root, "argv": ["search"], "stdin": b'{"query":"fixture"}\n'}
            # When: recording with the installed adapter interface.
            result = record_profile(scenario, {"executable": "/unused", "samply": str(sampler)}, root / "capture")
            # Then: sampler gets the exact request without launching any UI.
            output = Path(result["path"])
            argv = json.loads(output.with_suffix(".args").read_text())
            self.assertIn("--save-only", argv)
            self.assertEqual(argv[argv.index("--rate") + 1], "1000")
            self.assertEqual(output.with_suffix(".input").read_bytes(), scenario["stdin"])
            self.assertEqual(result["profile"]["samples"], 1)

    def test_git_wrapper_is_transparent_and_does_not_log_payloads(self):
        # Given: a fake Git receiving binary input and arguments containing spaces.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fake = root / "real git"
            fake.write_text(f'#!{sys.executable}\nimport sys\n'
                            'sys.stdout.buffer.write(sys.stdin.buffer.read())\n'
                            'sys.stderr.write("private-payload")\nsys.exit(7)\n')
            fake.chmod(0o755)
            log = root / "calls.jsonl"
            environment = dict(os.environ, PERF_REAL_GIT=str(fake), PERF_GIT_LOG=str(log))
            # When: wrapper invokes Git with unchanged argv and stdin.
            wrapper = Path(__file__).with_name("git_wrapper.py")
            result = subprocess.run([sys.executable, str(wrapper), "cat-file", "a secret path"],
                                    input=b"private\x00blob", capture_output=True, env=environment)
            # Then: exit and streams survive, but private data is absent in counters.
            self.assertEqual(result.returncode, 7)
            self.assertEqual(result.stdout, b"private\x00blob")
            self.assertEqual(result.stderr, b"private-payload")
            record = json.loads(log.read_text())
            self.assertEqual(record["operation"], "cat-file")
            self.assertNotIn("secret", log.read_text())
            self.assertNotIn("private", log.read_text())


if __name__ == "__main__":
    unittest.main()
