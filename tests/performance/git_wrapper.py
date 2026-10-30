#!/usr/bin/env python3
"""Transparent diagnostic Git launcher: never read or retain stdin/blob content."""

import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import time

OPERATIONS = {"rev-parse", "cat-file", "hash-object", "ls-files", "ls-tree", "mktree",
              "update-ref", "for-each-ref", "config", "version", "diff", "status",
              "show", "show-ref", "check-ref-format", "rev-list", "read-tree", "write-tree"}


def main() -> None:
    program = Path(os.environ["PERF_REAL_GIT"])
    if program.resolve() == Path(sys.argv[0]).resolve():
        raise SystemExit("recursive Git wrapper")
    started = time.monotonic()
    child = subprocess.run([str(program), *sys.argv[1:]])
    operation = next((arg for arg in sys.argv[1:] if arg in OPERATIONS), "other")
    record = {"operation": operation, "pid": os.getpid(), "returncode": child.returncode,
              "started_monotonic": started, "duration_seconds": time.monotonic() - started}
    descriptor = os.open(os.environ["PERF_GIT_LOG"], os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    with os.fdopen(descriptor, "ab") as stream:
        stream.write((json.dumps(record) + "\n").encode())
    if child.returncode < 0:
        signum = -child.returncode
        signal.signal(signum, signal.SIG_DFL)
        os.kill(os.getpid(), signum)
    raise SystemExit(child.returncode)


if __name__ == "__main__":
    main()
