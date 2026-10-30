"""One-variable external-binding experiment; no product algorithm is changed."""

import argparse
import copy
import json
import signal
from pathlib import Path

from measure import measure_process
from profile import record_git_calls
from report import summarize_samples, validate_result
from scenarios import (hash_file, initialize_projection, prepare_fixture, prepare_scenario,
                       verify_build, verify_new_output, write_json)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--baseline", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--nodes", type=int, default=60)
    parser.add_argument("--variable", choices=("external", "packing"), default="external")
    args = parser.parse_args()
    build = json.loads(args.baseline.read_text())
    verify_build(build)
    evidence = args.evidence.absolute()
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    from run import capture_harness, stop_stage
    write_json(evidence / "manifest.json", {"build": build, "harness_archive": capture_harness(evidence),
               "variable": args.variable, "nodes": args.nodes,
               "fixed": ["knowledge count", "fragment sizes", "source bytes/tree", "build"],
               "packing_note": "derived path IDs and includes boundaries can differ; first requested fragment bytes remain fixed"})
    previous = signal.signal(signal.SIGALRM, stop_stage)
    signal.setitimer(signal.ITIMER_REAL, 1200)
    try:
        results = {}
        variants = ("with_external", "masked_external") if args.variable == "external" else ("packing30", "packing10")
        for variant in variants:
            fixture = prepare_fixture(evidence / variant, args.nodes, elements_per_file=10 if variant == "packing10" else 30)
            if variant == "masked_external":
                fixture = mask_external_edges(fixture)
            initialize_projection(fixture["repository"], build["executable"], full_audit=False)
            fixture["binary"] = build["executable"]
            scenario = prepare_scenario(fixture, "check" if args.variable == "external" else "inspect",
                                        evidence / f"case-{variant}")
            results[variant] = measure_control(scenario, build, evidence / f"results-{variant}")
        write_json(evidence / "comparison.json", {"results": results,
                   "interpretation": "workload control changes required behavior, not an equivalent optimization"})
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def mask_external_edges(fixture: dict) -> dict:
    """Remove declarations, preserving bytes per line and all author source metadata."""
    result = copy.deepcopy(fixture)
    removed = 0
    for path in sorted(fixture["repository"].glob("knowledge/*.md")):
        lines = path.read_bytes().splitlines(keepends=True)
        for index, line in enumerate(lines):
            if line.startswith(b"[source](") and b'"letopis:type=external"' in line:
                lines[index] = b" " * (len(line) - 1) + b"\n"
                removed += 1
        path.write_bytes(b"".join(lines))
        result["input_sha256"][path.relative_to(fixture["repository"]).as_posix()] = hash_file(path)
    result["counts"]["source"] = 0
    result["counts"]["edges"] -= removed
    result["control"] = {"removed_external_edges": removed, "source_snapshot_unchanged": True}
    return result


def measure_control(scenario: dict, build: dict, evidence: Path) -> dict:
    """Record three successful observations plus a separate instrumented counter run."""
    from run import append_record
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    records = []
    with (evidence / "samples.jsonl").open("xb") as stream:
        for _ in range(3):
            record = measure_process([build["executable"], *scenario["argv"]],
                                     scenario["repository"], scenario["stdin"], {"timeout": 120})
            record.update(name=scenario["name"], nodes=scenario["expected"]["counts"]["knowledge"],
                          phase="measured", embedding_mode="fts_disabled", profile_mode="none",
                          validation=validate_result(scenario, record))
            append_record(stream, record)
            records.append(record)
            print(json.dumps({"control": str(evidence.name), "seconds": record["duration_seconds"],
                              "valid": record["validation"]["valid"]}), flush=True)
            if not record["validation"]["valid"]:
                break
    counted = record_git_calls(scenario, build, evidence / "git")
    result = {"summary": summarize_samples(records), "git": counted,
              "counts": scenario["expected"]["counts"]}
    write_json(evidence / "receipt.json", result)
    return result


if __name__ == "__main__":
    main()
