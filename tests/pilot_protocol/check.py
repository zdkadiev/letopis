#!/usr/bin/env python3
"""Read-only P-1 supplied-record checker and descriptive accounting CLI."""
import argparse
from collections import Counter
import json
import math
from pathlib import Path

from validation import CONDITIONS, require, validate

TOKEN_FIELDS = ("input_tokens", "output_tokens", "cached_input_tokens", "reasoning_output_tokens")


def token_totals(requests):
    result = {"request_count": len(requests)}
    for key in TOKEN_FIELDS:
        values = [r[key] for r in requests]
        result[key] = None if None in values else sum(values)
    result["known_total"] = sum(r[key] for r in requests for key in TOKEN_FIELDS[:2]
                                if r[key] is not None)
    result["unknown_request_ids"] = sorted(r["id"] for r in requests
                                           if r["input_tokens"] is None or r["output_tokens"] is None)
    result["complete"] = not result["unknown_request_ids"]
    result["total"] = result["known_total"] if result["complete"] else None
    return result


def summarize(study: dict, sessions: list[dict]) -> dict:
    """Describe already-validated records; do not infer a winner or execute agents."""
    slots = {(r["condition"], r["chain_id"], r["task_id"]): r for r in sessions}
    result = {"protocol_valid": True, "study_id": study["study_id"], "kind": study["kind"],
              "stage": study["stage"], "scope": "supplied-record validation and descriptive accounting only",
              "agent_executions": 0, "conditions": {}}
    for condition in CONDITIONS:
        records = [slots[condition, chain["id"], task["id"]]
                   for chain in study["chains"] for task in chain["tasks"]]
        outcomes = {key: sum(r["outcome"] == key for r in records)
                    for key in ("correct", "failed", "incomplete", "not_run")}
        attempted = [r for r in records if r["outcome"] != "not_run"]
        correctness_checks = {
            "planned": sum(len(r["checks"]) for r in records),
            "assessed": sum(c["passed"] is not None for r in attempted for c in r["checks"]),
            "not_run": sum(len(r["checks"]) for r in records if r["outcome"] == "not_run"),
            "unresolved_check_ids": [f"{r['task_id']}/{c['id']}" for r in attempted
                                     for c in r["checks"] if c["passed"] is None],
        }
        research = {key: sum(e["kind"] == key for r in records for e in r["research"])
                    for key in ("first", "repeat", "revalidation", "unresolved")}
        research["repeat_count"] = research["repeat"] if not research["unresolved"] else None
        observed = [a for r in records if r["outcome"] != "not_run" for a in r["agreements"]]
        planned = sum(len(r["agreements"]) for r in records)
        assessed = sum(a["violated"] is not None for a in observed)
        violations = sum(a["violated"] is True for a in observed)
        unresolved = len(observed) - assessed
        agreements = {"planned": planned, "assessed": assessed, "violations": violations,
                      "unresolved": unresolved, "not_run": planned - len(observed),
                      "violation_rate": violations / assessed if assessed and not unresolved else None}
        requests = [request for r in records for request in r["requests"]]
        tokens = token_totals(requests)
        for source, target in (("model", "by_model"), ("purpose", "by_purpose")):
            tokens[target] = {key: token_totals([r for r in requests if r[source] == key])
                              for key in sorted({r[source] for r in requests})}
        profile = [{"session_id": r["session_id"], "chain_id": r["chain_id"], "task_id": r["task_id"], **row}
                   for r in records for row in r["workload_profile"]]
        workload = {"events": len(profile), "by_operation": dict(Counter(r["operation"] for r in profile)),
                    "known_bytes": sum(r["bytes"] for r in profile if r["bytes"] is not None),
                    "complete": all(r[k] is not None for r in profile
                                    for k in ("bytes", "timestamp_ms", "elapsed_ms")), "records": profile}
        result["conditions"][condition] = {"outcomes": outcomes, "research": research,
                                           "correctness_checks": correctness_checks,
                                           "agreements": agreements, "tokens": tokens,
                                           "workload_profile": workload}
    result["correctness_preserved"] = all(r["outcome"] == "correct" for r in sessions)
    result["paired_correct_slots"] = [task["id"] for chain in study["chains"] for task in chain["tasks"]
                                      if all(slots[c, chain["id"], task["id"]]["outcome"] == "correct"
                                             for c in CONDITIONS)]
    result["data_complete"] = all(c["tokens"]["complete"] and not c["research"]["unresolved"]
                                  and not c["correctness_checks"]["unresolved_check_ids"]
                                  and not c["agreements"]["unresolved"] and c["agreements"]["assessed"] > 0
                                  and not c["outcomes"]["incomplete"] for c in result["conditions"].values())
    if study["kind"] == "demonstration":
        result["status"] = "demonstration_only"
    elif not result["data_complete"]:
        result["status"] = "incomplete_data"
    elif not result["correctness_preserved"]:
        result["status"] = "correctness_not_preserved"
    else:
        result["status"] = "descriptive_only"
    result["limits"] = ["No utility threshold, winner, causal or statistical-significance conclusion.",
                        "Checksums do not attest OS isolation, actual model configuration or annotation truth.",
                        "Raw token counts across different model tokenizers are bookkeeping, not equal compute.",
                        "Unknown workload fields remain incomplete inputs for P-2.7, separately from P-1."]
    return result


def read_json(path):
    def pairs(items):
        result = {}
        for key, value in items:
            if key in result:
                raise ValueError(f"{path}: duplicate JSON key {key!r}")
            result[key] = value
        return result

    def constant(value):
        raise ValueError(f"{path}: non-finite JSON number {value}")

    def real(value):
        parsed = float(value)
        if not math.isfinite(parsed):
            constant(value)
        return parsed

    return json.loads(Path(path).read_text(), object_pairs_hook=pairs,
                      parse_constant=constant, parse_float=real)


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("study", type=Path)
    parser.add_argument("sessions", type=Path)
    parser.add_argument("--evidence-root", type=Path, required=True)
    args = parser.parse_args(argv)
    try:
        study = read_json(args.study)
        directory = args.sessions.resolve(strict=True)
        require(directory.is_dir(), "sessions", "expected session directory")
        sessions = []
        for path in sorted(directory.rglob("*.json")):
            source = path.resolve(strict=True)
            require(source.is_relative_to(directory) and source.is_file(), "sessions", "session input escapes directory or is not a file")
            sessions.append(read_json(source))
        validate(study, sessions, args.evidence_root)
        result = summarize(study, sessions)
    except (ValueError, OSError, UnicodeError, RuntimeError) as error:
        print(json.dumps({"protocol_valid": False, "error": str(error)}, ensure_ascii=False))
        return 2
    print(json.dumps(result, ensure_ascii=False, sort_keys=True, indent=2, allow_nan=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
