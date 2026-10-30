"""Account for detached fixture successors using an exact private executable identity."""

import os
from pathlib import Path
import shutil
import signal
import subprocess
import time

from scenarios import hash_file, verify_new_output, write_json


class FixtureProcesses:
    """Own one private executable copy and every surviving process launched from it."""

    def __init__(self, binary: Path, evidence: Path):
        self.source = binary
        self.evidence = evidence
        self.executable = evidence / "letopis"
        self.stopped = set()

    def __enter__(self):
        # Fail before launching anything if this host cannot inspect exact identities.
        read_process_rows()
        verify_new_output(self.evidence)
        self.evidence.mkdir(parents=True, mode=0o700)
        shutil.copy2(self.source, self.executable)
        if hash_file(self.source) != hash_file(self.executable):
            raise ValueError("fixture executable copy differs from the pinned product")
        return self

    def __exit__(self, *_):
        try:
            self.stop_all()
        except Exception as error:
            write_json(self.evidence / "cleanup.json", {"outcome": "cleanup_failed", "error_type": type(error).__name__,
                                                       "stopped_pids": sorted(self.stopped)})
            raise
        write_json(self.evidence / "cleanup.json", {"outcome": "stopped", "stopped_pids": sorted(self.stopped),
                   "remaining": [], "executable": str(self.executable), "sha256": hash_file(self.executable)})

    def get_executable(self) -> Path:
        return self.executable

    def stop_all(self) -> list[int]:
        """Stop only exact private-path matches, including setsid drain successors."""
        for sig in (signal.SIGTERM, signal.SIGKILL):
            for pid in self.list_processes():
                self.signal_process(pid, sig)
            deadline = time.monotonic() + .3
            while self.list_processes() and time.monotonic() < deadline:
                time.sleep(.01)
            if not self.list_processes():
                return sorted(self.stopped)
        raise RuntimeError("a fixture-owned process survived bounded cleanup")

    def list_processes(self) -> list[int]:
        return [pid for pid, command in read_process_rows() if self.matches_command(command)]

    def signal_process(self, pid: int, sig: int) -> None:
        """Recheck exact identity immediately before signalling; never kill by basename."""
        if not self.matches_command(read_process_command(pid)):
            return
        try:
            if os.getpgid(pid) == pid:
                os.killpg(pid, sig)
            else:
                # A pre-exec child can still share its already-stopped parent's group.
                os.kill(pid, sig)
            self.stopped.add(pid)
        except ProcessLookupError:
            pass
        except PermissionError:
            if self.matches_command(read_process_command(pid)):
                raise

    def matches_command(self, command: str) -> bool:
        program = str(self.executable)
        return command == program or command.startswith(program + " ")


def read_process_rows() -> list[tuple[int, str]]:
    """Read process identities privately; retain no unrelated argv or environment."""
    result = subprocess.run(["/bin/ps", "-ww", "-axo", "pid=,command="], capture_output=True, check=True, timeout=2)
    rows = []
    for line in result.stdout.decode(errors="replace").splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2:
            rows.append((int(parts[0]), parts[1]))
    return rows


def read_process_command(pid: int) -> str:
    result = subprocess.run(["/bin/ps", "-ww", "-p", str(pid), "-o", "command="], capture_output=True, timeout=2)
    return result.stdout.decode(errors="replace").strip() if result.returncode == 0 else ""
