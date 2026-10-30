"""A successful sampler wrapper must never mean a successful timed-out CLI."""

import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest
import os
import signal

from measure import measure_process


class WindowTests(unittest.TestCase):
    def test_outer_timeout_stops_nested_group_and_pipe_holding_descendant(self):
        # Given: nested processes ignore TERM and retain the wrapper's output pipes.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            child_pid, grand_pid = root / "pid", root / "grand"
            code = ("import os,signal,subprocess,sys,time;from pathlib import Path;"
                    "signal.signal(signal.SIGTERM,signal.SIG_IGN);"
                    f"Path({str(child_pid)!r}).write_text(str(os.getpid()));"
                    "p=subprocess.Popen([sys.executable,'-c','import time;time.sleep(60)']);"
                    f"Path({str(grand_pid)!r}).write_text(str(p.pid));time.sleep(60)")
            request = root / "request.json"
            request.write_text(json.dumps({"argv": [sys.executable, "-c", code],
                "repository": str(root), "stdin_hex": "", "seconds": 30}))
            # When: an outer watchdog terminates the wrapper before its inner deadline.
            try:
                proxy = "import subprocess,sys;subprocess.run(sys.argv[1:])"
                result = measure_process([sys.executable, "-c", proxy, sys.executable, str(Path(__file__).with_name("window.py")),
                    "--request", str(request), "--output", str(root / "child.json")], root, b"", {"timeout": .5})
                # Then: all nested owned work is stopped and interruption is retained.
                self.assertEqual(result["outcome"], "timeout")
                for path in (child_pid, grand_pid):
                    state = subprocess.run(["ps", "-o", "stat=", "-p", path.read_text()], capture_output=True, text=True)
                    self.assertTrue(not state.stdout.strip() or state.stdout.lstrip().startswith("Z"))
                self.assertTrue((root / "child.json").exists(), {"wrapper_exit": result["exit_code"],
                    "stderr": bytes.fromhex(result["stderr"]["capture_hex"]).decode(errors="replace")})
                self.assertEqual(json.loads((root / "child.json").read_text())["outcome"], "interrupted")
            finally:
                if child_pid.exists():
                    try:
                        os.killpg(int(child_pid.read_text()), signal.SIGKILL)
                    except ProcessLookupError:
                        pass

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
