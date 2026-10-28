#!/usr/bin/env python3
"""TK-30 operator CLI: model-free prepare, audit, private evaluate, context and preflight.

There is deliberately no session or exec subcommand: this tool never reads credentials,
starts an agent or contacts a provider.
"""
from __future__ import annotations

import json
import sys

# tarfile extraction with filter="data" exists from 3.11.4 on.
MINIMUM_PYTHON = (3, 11, 4)
if sys.version_info < MINIMUM_PYTHON:
    print(json.dumps({"error": "Python 3.11.4 or newer is required"}))
    sys.exit(2)

import argparse
import shutil
import subprocess
from pathlib import Path

import copies
import environment
import evaluation
import study
import telemetry

HERE = Path(__file__).resolve().parent
FINAL_DIR = HERE.parent.parent / "docs/zkit/letopis/pilot/final"
SELECTION_FILE = "selection.json"
INPUT_FILES = (SELECTION_FILE, "instructions.md", "readiness.md")
MANIFEST_FILE = "manifest.json"
SEEDS_FILE = "seeds.json"
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
            subprocess.SubprocessError, evaluation.InfraError) as error:
        return emit({"error": str(error)}, EXIT_INVALID)


def build_parser() -> argparse.ArgumentParser:
    parser = JsonArgumentParser(add_help=False)
    commands = parser.add_subparsers(dest="command", required=True,
                                     parser_class=JsonArgumentParser)
    prepare = commands.add_parser("prepare", add_help=False)
    prepare.add_argument("--repo", type=to_absolute_path, required=True)
    prepare.add_argument("--base", required=True)
    prepare.add_argument("--output", type=to_absolute_path, required=True)
    prepare.set_defaults(handler=run_prepare)
    audit = commands.add_parser("audit", add_help=False)
    audit.add_argument("--root", type=to_absolute_path, required=True)
    audit.set_defaults(handler=run_audit)
    evaluate = commands.add_parser("evaluate", add_help=False)
    evaluate.add_argument("--root", type=to_absolute_path, required=True)
    evaluate.add_argument("--slot", required=True)
    evaluate.set_defaults(handler=run_evaluate)
    context = commands.add_parser("context", add_help=False)
    context.add_argument("--source", type=to_absolute_path, required=True)
    context.add_argument("--assets", type=to_absolute_path, required=True)
    context.add_argument("--output", type=to_absolute_path, required=True)
    context.set_defaults(handler=run_context)
    preflight = commands.add_parser("preflight", add_help=False)
    preflight.add_argument("--root", type=to_absolute_path, required=True)
    preflight.add_argument("--image", required=True)
    preflight.set_defaults(handler=run_preflight)
    return parser


def to_absolute_path(value: str) -> Path:
    """Resolve a path argument against the operator's cwd before any command changes cwd."""
    return Path(value).resolve()


def run_prepare(args: argparse.Namespace) -> int:
    """Write the manifest, copied inputs and the six isolated seeds into a new directory."""
    selection = study.load_selection(FINAL_DIR / SELECTION_FILE)
    receipts = {name: copies.hash_file(FINAL_DIR / name) for name in INPUT_FILES}
    study.require_full_oid(args.base, "base")
    check_commit_exists(args.repo, args.base)
    args.output.mkdir(parents=True)
    try:
        manifest = write_prepared(args, selection, receipts)
    except BaseException:
        shutil.rmtree(args.output, ignore_errors=True)
        raise
    return emit({"state": "unready", "pending": manifest["pending"],
                 "output": str(args.output)}, EXIT_OK)


def write_prepared(args: argparse.Namespace, selection: dict, receipts: dict) -> dict:
    """Write the inputs, seeds and manifest into the new output directory."""
    inputs = args.output / "inputs"
    inputs.mkdir()
    for name in INPUT_FILES:
        (inputs / name).write_bytes((FINAL_DIR / name).read_bytes())
    seeds = copies.prepare_seeds(args.repo, args.base, args.output)
    manifest = study.build_manifest(selection, {
        "original_base": args.base, "file_sha256": receipts,
        "seed_commit": seeds["seed_commit"],
        "starting_inventory": seeds["starting_inventory"]})
    manifest["source_receipts"]["tk17_sources"] = {
        "commit": seeds["source_receipts"]["commit"],
        "files": {**seeds["source_receipts"]["files"], **environment.TK17_SOURCES,
                  **telemetry.TK17_SOURCES}}
    write_json(args.output / SEEDS_FILE, seeds)
    write_json(args.output / MANIFEST_FILE, manifest)
    return manifest


def run_audit(args: argparse.Namespace) -> int:
    """Report readiness from the manifest values, the copied inputs and the seeds on disk.

    The stored `state` claim is ignored; the selection hash and input file hashes are
    recomputed from `inputs/` so a post-recheck edit is reported as drift, and supplied
    seed values are re-verified against the six seed repositories.
    """
    manifest = json.loads((args.root / MANIFEST_FILE).read_text(encoding="utf-8"))
    readiness = study.assess_readiness(manifest, observe_inputs(args.root / "inputs"))
    if "seeds" not in readiness["pending"]:
        add_seed_drift(readiness, args.root, manifest)
    return emit(readiness, EXIT_OK if readiness["state"] == "ready" else EXIT_INVALID)


def run_evaluate(args: argparse.Namespace) -> int:
    """Grade one captured slot; exit 0 even when checks fail or the status is infra_error."""
    return emit(evaluation.evaluate(args.root, args.slot), EXIT_OK)


def run_context(args: argparse.Namespace) -> int:
    """Create a build context from supplied pinned assets; exit 2 while any is missing."""
    result = environment.prepare_context(args.source, args.assets, args.output)
    return emit(result, EXIT_INVALID if result["pending"] else EXIT_OK)


def run_preflight(args: argparse.Namespace) -> int:
    """Probe an already-local image without a model; exit 2 while anything is pending.

    Model and embedding access always stay pending, so a local smoke never exits 0.
    """
    result = environment.preflight(args.root, args.image)
    return emit(result, EXIT_INVALID if result["pending"] else EXIT_OK)


def add_seed_drift(readiness: dict, root: Path, manifest: dict) -> None:
    """Mark the readiness unready when the seeds no longer match the manifest values."""
    try:
        copies.audit_seeds(root, manifest["original_base"], manifest["seed_commit"],
                           manifest["starting_inventory"])
    except (ValueError, KeyError, TypeError, OSError, subprocess.SubprocessError) as error:
        readiness["pending"].append("seed_drift")
        readiness["state"] = "unready"
        readiness["seed_problem"] = str(error)


def write_json(path: Path, value: dict) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n", encoding="utf-8")


def observe_inputs(inputs: Path) -> dict:
    selection = study.load_selection(inputs / SELECTION_FILE)
    return {"selection_sha256": study.hash_selection(selection),
            "file_sha256": {name: copies.hash_file(inputs / name) for name in INPUT_FILES}}


def check_commit_exists(repo: Path, base: str) -> None:
    result = subprocess.run(
        ["git", "-C", str(repo), "cat-file", "-e", f"{base}^{{commit}}"],
        capture_output=True, timeout=GIT_TIMEOUT_SECONDS)
    if result.returncode != 0:
        raise ValueError("base commit is not present in the repository")


def emit(payload: dict, code: int) -> int:
    print(json.dumps(payload))
    return code


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
