"""Stop the actual outer runner during a mutation; sources and descendants must survive safely."""
import hashlib
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT))
from negative_controls import copy_crates


class Interruption(unittest.TestCase):
    def test_outer_interrupt_keeps_sources_and_terminates_nested_command(self):
        # The outer workspace is itself disposable so the RED reproducer cannot
        # corrupt the developer's untracked files. The actual full runner is used.
        # Given
        with tempfile.TemporaryDirectory(prefix="tk4-interrupt-") as temp, tempfile.TemporaryDirectory(prefix="tk4-sock-", dir="/tmp") as channel:
            root = copy_crates(Path(temp))
            build = root / ".build"
            build.mkdir()
            (build / "vec1.c").write_bytes((ROOT / ".build/vec1.c").read_bytes())
            original = {p: hashlib.sha256(p.read_bytes()).hexdigest() for p in root.rglob("*") if p.is_file()}
            address = str(Path(channel) / "control")
            listener = socket.socket(socket.AF_UNIX)
            listener.bind(address)
            listener.listen()
            # Compilation may precede the control event; active peer and exit
            # checks below have 20-second bounds, unrelated to compile startup.
            listener.settimeout(120)
            env = dict(os.environ, TK4_MUTATION_CONTROL=address)
            conn = None
            with tempfile.TemporaryFile() as log:
                runner = subprocess.Popen([sys.executable, "-B", str(root / "run.py"), "--evidence", str(build / "evidence")],
                                          cwd=root, env=env, stdout=log, stderr=log, start_new_session=True)
                try:
                    conn, _ = listener.accept()
                    conn.settimeout(20)
                    line = conn.makefile("rb").readline()
                    event = json.loads(line)
                    mutant = Path(event["path"])
                    # Given: validate the live fixture before the interruption.
                    self.assertTrue(mutant.is_file(), "hook must observe an actual written mutant")
                    self.assertNotEqual(hashlib.sha256(mutant.read_bytes()).hexdigest(), original[root / "src/launch.rs"], "hook did not observe an actual mutant")
                    # When
                    runner.send_signal(signal.SIGINT)  # direct owned unreaped Popen
                    try:
                        returncode = runner.wait(timeout=20)
                    except subprocess.TimeoutExpired:
                        returncode = None
                    try:
                        conn.settimeout(20)
                        gone = conn.recv(1) == b""
                    except socket.timeout:
                        gone = False
                    changed = [str(p.relative_to(root)) for p, h in original.items() if hashlib.sha256(p.read_bytes()).hexdigest() != h]
                    print(json.dumps({"changed_sources": changed, "nested_command_eof": gone, "runner_exit": returncode,
                                      "nested_group": event["group"], "mutation_group": event["parent_group"]}), flush=True)
                    # Then
                    self.assertEqual(changed, [], "outer interruption left a mutant in original sources")
                    self.assertTrue(gone, "nested command escaped outer interruption")
                    self.assertIsNotNone(returncode, "outer runner did not finish after interruption")
                    self.assertNotEqual(returncode, 0)
                    self.assertEqual(event["group"], event["parent_group"], "nested command escaped the mutation process group")
                finally:
                    # An escaped RED child can terminate ONLY itself through the
                    # still-owned connection; never kill a claimed numeric PID.
                    if conn is not None:
                        try:
                            conn.sendall(b"K")
                            conn.settimeout(20)
                            while conn.recv(4096):
                                pass
                        except (BrokenPipeError, ConnectionResetError):
                            pass
                        conn.close()
                    listener.close()
                    if runner.poll() is None:
                        runner.kill()
                    runner.wait(timeout=20)
                    log.seek(0)
                    if runner.returncode == 0:
                        # When / Then
                        self.fail("interrupted full runner unexpectedly succeeded")


if __name__ == "__main__":
    unittest.main()
