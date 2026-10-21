#!/usr/bin/env python3
"""Reproduce TK-5 synthetic protocol checks; no real agent/provider execution."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import platform
import subprocess
import sys

from fixtures import make_dataset, write_dataset

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def inputs():
    paths = [p for p in HERE.rglob("*") if p.is_file()
             and not {"__pycache__", ".build"}.intersection(p.relative_to(HERE).parts)]
    protocol = REPO / "docs/zkit/letopis/pilot"
    paths.extend(p for p in protocol.rglob("*") if p.is_file())
    return {p.relative_to(REPO).as_posix(): digest(p) for p in sorted(paths)}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    evidence = args.evidence.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    manifest = {"result": "FAIL", "scope": "synthetic protocol verification only",
                "started_utc": datetime.now(timezone.utc).isoformat(),
                "platform": platform.platform(), "python": sys.version,
                "agent_executions": 0, "provider_calls": 0, "utility_pilot_runs": 0,
                "commands": [], "negative_controls": [], "source_sha256": inputs()}

    def execute(argv, log, expected=0):
        argv = [str(x) for x in argv]
        result = subprocess.run(argv, cwd=REPO, capture_output=True, text=True, timeout=120)
        (evidence / log).write_text(result.stdout + result.stderr)
        manifest["commands"].append({"argv": argv, "log": log, "expected_exit": expected,
                                     "exit_code": result.returncode})
        if result.returncode != expected:
            raise RuntimeError(f"expected exit {expected}, got {result.returncode}: {log}")
        return result

    def cli(folder, log, expected=0):
        result = execute([sys.executable, "-B", HERE / "check.py", folder / "study.json",
                          folder / "sessions", "--evidence-root", folder], log, expected)
        return json.loads(result.stdout)

    try:
        execute([sys.executable, "-B", "-m", "unittest", "discover", "-s", HERE,
                 "-p", "test_*.py", "-v"], "unit-tests.txt")
        example = cli(HERE / "examples/demonstration", "early-example.json")
        assert example["status"] == "demonstration_only"
        assert example["conditions"]["baseline"]["tokens"]["total"] == 480
        assert example["conditions"]["memory"]["tokens"]["total"] == 462
        manifest["early_example"] = "demonstration_only"
        final_dir = evidence / "final-demonstration"
        study, sessions = make_dataset(final_dir, "final")
        write_dataset(final_dir, study, sessions)
        final = cli(final_dir, "final-example.json")
        assert all(final["conditions"][c]["outcomes"]["correct"] == 9 for c in ("baseline", "memory"))
        manifest["final_example_slots"] = 18
        cases = [("missing-request", "request_inventory"), ("reused-session", "session_id"),
                 ("prefilled-memory", "memory_before"), ("corrupt-evidence", "checksum"),
                 ("suppressed-failed-task", "missing planned slot")]
        for name, expected in cases:
            folder = evidence / "cases" / name
            study, sessions = make_dataset(folder)
            if name == "missing-request":
                sessions[0]["request_inventory"]["ids"].append("unreported-request")
            elif name == "reused-session":
                sessions[1]["session_id"] = sessions[0]["session_id"]
            elif name == "prefilled-memory":
                sessions[2]["memory_before"] = {"items": 1, "sha256": "f" * 64}
            elif name == "corrupt-evidence":
                (folder / "evidence/demonstration.txt").write_text("changed raw evidence\n")
            else:
                sessions[0]["outcome"] = "failed"
                sessions[0]["checks"][0]["passed"] = False
                sessions.pop(0)
            write_dataset(folder, study, sessions)
            report = cli(folder, name + ".json", 2)
            assert report["protocol_valid"] is False and expected in report["error"], report
            manifest["negative_controls"].append({"name": name, "result": "DETECTED",
                                                   "expected_error": expected, "actual_error": report["error"]})
        execute([sys.executable, "-B", REPO / "docs/zkit/letopis/roadmap/schedule.py", "--check"], "roadmap.json")
        legacy = REPO / "docs/zkit/letopis/checks/TK-4/macos-arm64/manifest.json"
        prior = json.loads(legacy.read_text())
        assert prior["result"] == "PASS"
        for relative, expected in prior["files_sha256"].items():
            assert digest((REPO / "tests/runtime" / relative).resolve()) == expected, relative
        manifest["legacy"] = {"source_files_match_prior_pass": len(prior["files_sha256"]),
                               "receipt_sha256": digest(legacy), "legacy_executions": 0}
        assert inputs() == manifest["source_sha256"], "checker modified its inputs"
        manifest["result"] = "PASS"
    finally:
        manifest["finished_utc"] = datetime.now(timezone.utc).isoformat()
        manifest["evidence_sha256"] = {p.relative_to(evidence).as_posix(): digest(p)
                                        for p in sorted(evidence.rglob("*")) if p.is_file()}
        (evidence / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print(json.dumps({"result": manifest["result"], "evidence": str(evidence),
                          "negative_controls": len(manifest["negative_controls"]),
                          "agent_executions": 0, "provider_calls": 0, "utility_pilot_runs": 0}))


if __name__ == "__main__":
    main()
