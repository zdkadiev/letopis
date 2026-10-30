"""Bounded measurement of owned process sessions, including pipe EOF."""

import hashlib
import os
from pathlib import Path
import selectors
import signal
import subprocess
import sys
import time

from scenarios import get_environment

CAPTURE_LIMIT = 65536


def measure_process(argv: list[str], repository: Path, data: bytes, options: dict) -> dict:
    """Measure an owned child; retain failure evidence and bounded binary output."""
    started = time.monotonic()
    record = {"started_monotonic": started, "duration_seconds": 0, "pid": None,
              "outcome": "spawn_failed", "exit_code": None, "resources": {}}
    streams = {name: StreamCapture(options.get("capture_limit", CAPTURE_LIMIT))
               for name in ("stdout", "stderr")}
    child = None
    try:
        child = subprocess.Popen(argv, cwd=repository, env=options.get("environment", get_environment()),
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                 start_new_session=True)
        record["pid"] = child.pid
        usage = collect_process(child, data, streams, started + options.get("timeout", 120))
        record.update(exit_code=child.returncode, outcome="success" if child.returncode == 0 else "nonzero",
                      resources=describe_resources(usage))
    except (TimeoutError, KeyboardInterrupt, OSError) as error:
        record["outcome"] = ("timeout" if isinstance(error, TimeoutError) else
                             "interrupted" if isinstance(error, KeyboardInterrupt) else "spawn_failed")
    finally:
        if child is not None:
            stop_session(child)
            record["exit_code"] = child.returncode
            for pipe in (child.stdin, child.stdout, child.stderr):
                pipe.close()
    record["duration_seconds"] = time.monotonic() - started
    if not record["resources"]:
        record["resources"] = {"unavailable": "invocation did not complete resource collection"}
    record.update({name: stream.get_record() for name, stream in streams.items()})
    return record


class StreamCapture:
    """Hash complete streams while keeping only bounded synthetic diagnostics."""

    def __init__(self, limit: int):
        self.limit = limit
        self.digest = hashlib.sha256()
        self.length = 0
        self.capture = bytearray()

    def append_bytes(self, data: bytes) -> None:
        self.digest.update(data)
        self.length += len(data)
        self.capture.extend(data[:max(0, self.limit - len(self.capture))])

    def get_record(self) -> dict:
        return {"bytes": self.length, "sha256": self.digest.hexdigest(),
                "capture_hex": self.capture.hex(), "truncated": self.length > self.limit}


def collect_process(child: subprocess.Popen, data: bytes, streams: dict, deadline: float):
    """Drain pipes and per-child wait4 within one deadline, without global counters."""
    usage = None
    remaining = memoryview(data)
    with selectors.DefaultSelector() as selector:
        for name in ("stdout", "stderr"):
            pipe = getattr(child, name)
            os.set_blocking(pipe.fileno(), False)
            selector.register(pipe, selectors.EVENT_READ, name)
        os.set_blocking(child.stdin.fileno(), False)
        if remaining:
            selector.register(child.stdin, selectors.EVENT_WRITE, "stdin")
        else:
            child.stdin.close()
        while selector.get_map() or child.returncode is None:
            if time.monotonic() >= deadline:
                raise TimeoutError("owned process or stream exceeded deadline")
            for key, _ in selector.select(min(0.005, max(0, deadline - time.monotonic()))):
                if key.data == "stdin":
                    remaining = write_input(selector, key.fileobj, remaining)
                else:
                    read_output(selector, key, streams)
            if child.returncode is None:
                pid, status, observed = os.wait4(child.pid, os.WNOHANG)
                if pid:
                    child.returncode = os.waitstatus_to_exitcode(status)
                    usage = observed
    return usage


def describe_resources(usage) -> dict:
    """Keep wait4 scope explicit; it is not simultaneous CLI-plus-children peak."""
    if usage is None:
        return {"unavailable": "no successful wait4 observation"}
    scope = "wait4_direct_child_and_OS_accounted_reaped_descendants"
    scale = 1 if sys.platform == "darwin" else 1024
    return {"cpu_seconds": {"value": usage.ru_utime + usage.ru_stime, "unit": "seconds",
                            "scope": scope, "method": "wait4"},
            "peak_rss": {"value": usage.ru_maxrss * scale, "unit": "bytes", "scope": scope,
                         "method": "wait4_lifetime_max_not_simultaneous_tree_peak"},
            "io_blocks": {"read": usage.ru_inblock, "written": usage.ru_oublock,
                          "unit": "OS_block_operations", "scope": scope, "method": "wait4"},
            "io_bytes": {"unavailable": "portable per-process byte counters not collected"}}


def write_input(selector, pipe, remaining: memoryview) -> memoryview:
    try:
        remaining = remaining[os.write(pipe.fileno(), remaining[:8192]):]
    except BrokenPipeError:
        remaining = remaining[:0]
    if not remaining:
        selector.unregister(pipe)
        pipe.close()
    return remaining


def read_output(selector, key, streams: dict) -> None:
    data = os.read(key.fd, 65536)
    if data:
        streams[key.data].append_bytes(data)
    else:
        selector.unregister(key.fileobj)


def stop_session(child: subprocess.Popen) -> None:
    """Close this session even when an exited parent left pipe-holding children."""
    for sig in (signal.SIGTERM, signal.SIGKILL):
        try:
            os.killpg(child.pid, sig)
        except ProcessLookupError:
            break
        if sig == signal.SIGTERM and child.returncode is None:
            try:
                child.wait(timeout=0.2)
            except subprocess.TimeoutExpired:
                continue
    if child.returncode is None:
        child.wait()
