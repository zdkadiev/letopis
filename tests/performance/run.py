#!/usr/bin/env python3
"""Run bounded, model-free diagnostics in isolated synthetic repositories."""

import argparse
from pathlib import Path

from scenarios import SCENARIOS


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list-scenarios", action="store_true")
    parser.add_argument("--stage", choices=("survey", "profile", "scale", "contention"))
    parser.add_argument("--nodes", type=int, default=1000)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--baseline", type=Path)
    arguments = parser.parse_args()
    if arguments.list_scenarios:
        print("\n".join(SCENARIOS))
        return
    parser.error("campaign execution is not implemented yet")


if __name__ == "__main__":
    main()
