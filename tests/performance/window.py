#!/usr/bin/env python3
"""Bound an owned workload while allowing samply's root task to finish and save."""

import argparse
import json
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
    record = measure_process(request["argv"], Path(request["repository"]),
                             bytes.fromhex(request["stdin_hex"]), {"timeout": request["seconds"]})
    write_json(args.output, record)
    print(json.dumps({"profile_window_only": True, "child_outcome": record["outcome"], "pid": record["pid"]}))


if __name__ == "__main__":
    main()
