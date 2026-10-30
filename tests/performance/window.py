#!/usr/bin/env python3
"""Bound an owned workload while allowing samply's root task to finish and save."""

import argparse
import json
import signal
from pathlib import Path

from measure import measure_process
from scenarios import write_json


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--request", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    request = json.loads(args.request.read_text())
    if not 0 < request["seconds"] <= 120:
        raise ValueError("profile window must be positive and at most 120 seconds")
    with WindowOwner() as owner:
        record = measure_process(request["argv"], Path(request["repository"]),
                                 bytes.fromhex(request["stdin_hex"]), {"timeout": request["seconds"],
                                 "cleanup_grace": .1, "cancel_event": owner})
    write_json(args.output, record)
    print(json.dumps({"profile_window_only": True, "child_outcome": record["outcome"], "pid": record["pid"]}))


class WindowOwner:
    """Defer outer termination until the collector cleans its nested owned group."""

    def __init__(self):
        self.cancelled = False
        self.handlers = {}

    def __enter__(self):
        self.handlers = {sig: signal.signal(sig, self.cancel_window) for sig in (signal.SIGTERM, signal.SIGINT)}
        return self

    def __exit__(self, *_):
        for sig, handler in self.handlers.items():
            signal.signal(sig, handler)

    def is_set(self) -> bool:
        return self.cancelled

    def cancel_window(self, *_):
        # Do not abort Popen construction before the collector owns its child.
        self.cancelled = True


if __name__ == "__main__":
    main()
