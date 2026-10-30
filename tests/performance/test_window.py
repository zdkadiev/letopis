"""A successful sampler wrapper must never mean a successful timed-out CLI."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest


class WindowTests(unittest.TestCase):
    def test_window_records_child_timeout_separately_from_wrapper_success(self):
        # Given: an owned child that cannot finish inside the recording window.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            request = root / "request.json"
            request.write_text(json.dumps({"argv": [sys.executable, "-c", "import time;time.sleep(60)"],
                                           "repository": str(root), "stdin_hex": "", "seconds": .05}))
            output = root / "child.json"
            # When: the wrapper bounds only its owned child and exits for samply to save.
            result = subprocess.run([sys.executable, str(Path(__file__).with_name("window.py")),
                                     "--request", str(request), "--output", str(output)],
                                    capture_output=True, timeout=4)
            # Then: wrapper success cannot be counted as CLI completion.
            self.assertEqual(result.returncode, 0)
            receipt = json.loads(output.read_text())
            self.assertEqual(receipt["outcome"], "timeout")
            self.assertEqual(json.loads(result.stdout)["profile_window_only"], True)


if __name__ == "__main__":
    unittest.main()
