"""Profiler validity and transparent Git instrumentation."""

import gzip
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from profile import record_profile, validate_profile


class ProfileTests(unittest.TestCase):
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
