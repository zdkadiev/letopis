#!/usr/bin/env python3
"""TK-30 operator CLI: model-free prepare, audit and (not yet wired) evaluate.

There is deliberately no session or exec subcommand: this tool never reads credentials,
starts an agent or contacts a provider.
"""
from __future__ import annotations

import json
import sys

if sys.version_info < (3, 11):
    print(json.dumps({"error": "Python 3.11 or newer is required"}))
    sys.exit(2)

import argparse
import hashlib
import subprocess
from pathlib import Path

import study

HERE = Path(__file__).resolve().parent
FINAL_DIR = HERE.parent.parent / "docs/zkit/letopis/pilot/final"
SELECTION_FILE = "selection.json"
INPUT_FILES = (SELECTION_FILE, "instructions.md", "readiness.md")
MANIFEST_FILE = "manifest.json"
EXIT_OK = 0
EXIT_INVALID = 2
GIT_TIMEOUT_SECONDS = 30


class UsageError(ValueError):
    """Command-line arguments did not match any supported command."""


class JsonArgumentParser(argparse.ArgumentParser):
    def error(self, message: str):
        raise UsageError(message)


def main(argv: list[str]) -> int:
    """Run one command; print one JSON object and return the process exit code."""
    try:
        args = build_parser().parse_args(argv)
        return args.handler(args)
    except (UsageError, OSError, ValueError, KeyError, TypeError,
            subprocess.SubprocessError) as error:
        return emit({"error": str(error)}, EXIT_INVALID)


def build_parser() -> argparse.ArgumentParser:
    parser = JsonArgumentParser(add_help=False)
    commands = parser.add_subparsers(dest="command", required=True,
                                     parser_class=JsonArgumentParser)
    prepare = commands.add_parser("prepare", add_help=False)
    prepare.add_argument("--repo", type=Path, required=True)
    prepare.add_argument("--base", required=True)
    prepare.add_argument("--output", type=Path, required=True)
    prepare.set_defaults(handler=run_prepare)
    audit = commands.add_parser("audit", add_help=False)
    audit.add_argument("--root", type=Path, required=True)
    audit.set_defaults(handler=run_audit)
    evaluate = commands.add_parser("evaluate", add_help=False)
    evaluate.add_argument("--root", type=Path, required=True)
    evaluate.add_argument("--slot", required=True)
    evaluate.set_defaults(handler=run_evaluate)
    return parser


def run_prepare(args: argparse.Namespace) -> int:
    """Write the unready manifest and copies of the operator inputs into a new directory."""
    selection = study.load_selection(FINAL_DIR / SELECTION_FILE)
    receipts = {name: hash_file(FINAL_DIR / name) for name in INPUT_FILES}
    manifest = study.build_manifest(
        selection, {"original_base": args.base, "file_sha256": receipts})
    check_commit_exists(args.repo, args.base)
    args.output.mkdir(parents=True)
    inputs = args.output / "inputs"
    inputs.mkdir()
    for name in INPUT_FILES:
        (inputs / name).write_bytes((FINAL_DIR / name).read_bytes())
    (args.output / MANIFEST_FILE).write_text(
        json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    return emit({"state": "unready", "pending": manifest["pending"],
                 "output": str(args.output)}, EXIT_OK)


def run_audit(args: argparse.Namespace) -> int:
    """Report readiness from the manifest values and the recomputed copied inputs.

    The stored `state` claim is ignored; the selection hash and input file hashes are
    recomputed from `inputs/` so a post-recheck edit is reported as drift.
    """
    manifest = json.loads((args.root / MANIFEST_FILE).read_text(encoding="utf-8"))
    readiness = study.assess_readiness(manifest, observe_inputs(args.root / "inputs"))
    return emit(readiness, EXIT_OK if readiness["state"] == "ready" else EXIT_INVALID)


def run_evaluate(args: argparse.Namespace) -> int:
    return emit({"error": "evaluate is not wired until Task 3"}, EXIT_INVALID)


def observe_inputs(inputs: Path) -> dict:
    selection = study.load_selection(inputs / SELECTION_FILE)
    return {"selection_sha256": study.hash_selection(selection),
            "file_sha256": {name: hash_file(inputs / name) for name in INPUT_FILES}}


def check_commit_exists(repo: Path, base: str) -> None:
    result = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "-e", f"{base}^{{commit}}"],
        capture_output=True, timeout=GIT_TIMEOUT_SECONDS)
    if result.returncode != 0:
        raise ValueError("base commit is not present in the repository")


def hash_file(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def emit(payload: dict, code: int) -> int:
    print(json.dumps(payload))
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
