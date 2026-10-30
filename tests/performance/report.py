"""Independent correctness checks and descriptive, scope-aware summaries."""

from collections import defaultdict
import hashlib
import json
import math
from pathlib import Path, PurePosixPath
import re
import statistics
import sqlite3
import subprocess

from scenarios import run_git, write_json

GROUP_KEYS = ("name", "nodes", "phase", "embedding_mode", "profile_mode")


def validate_result(scenario: dict, measurement: dict) -> dict:
    """Validate command behavior outside its timer; exit zero alone is insufficient."""
    if measurement["outcome"] != "success" or measurement["stdout"]["truncated"]:
        return {"valid": False, "reason": "process or capture failure"}
    try:
        response = json.loads(bytes.fromhex(measurement["stdout"]["capture_hex"]).decode("utf-8"))
        if response.get("ok") is not True or response.get("warnings"):
            raise ValueError("CLI failure or warning")
        observed = validate_command(scenario, response["result"])
        if "counts" in scenario["expected"]:
            observed["projection_counts"] = verify_projection_counts(scenario)
        return {"valid": True, "observed": observed}
    except (ValueError, KeyError, TypeError, AttributeError, OSError, IndexError,
            sqlite3.Error, subprocess.CalledProcessError) as error:
        return {"valid": False, "reason": type(error).__name__}


def summarize_samples(records: list[dict]) -> dict:
    """Refuse incomparable series and exclude failures from successful quantiles."""
    groups = {tuple(record.get(key) for key in GROUP_KEYS) for record in records}
    if len(groups) > 1:
        raise ValueError("incomparable measurement series")
    values = sorted(record["duration_seconds"] for record in records
                    if record["outcome"] == "success" and record["validation"]["valid"])
    failures = sum(record["outcome"] != "success" or not record["validation"]["valid"]
                   for record in records)
    occupied = sum(record["duration_seconds"] for record in records)
    return {"schema_version": 1, "n": len(values), "attempts": len(records), "failures": failures,
            "min": min(values) if values else None, "max": max(values) if values else None,
            "median": statistics.median(values) if values else None,
            "p95": values[math.ceil(len(values) * .95) - 1] if len(values) >= 100 else None,
            "p99": values[math.ceil(len(values) * .99) - 1] if len(values) >= 1000 else None,
            "quantile_method": "nearest_rank; median averages middle pair",
            "throughput_success_per_s": len(values) / occupied if occupied else None,
            "throughput_scope": "sequential occupied call time including failed attempts, excludes setup",
            "tail_limit": "p95 requires 100 successes; p99 requires 1000 successes",
            "values_seconds": values}


def write_report(evidence: Path) -> Path:
    """Summarize immutable per-call records into a new JSON artifact."""
    groups = defaultdict(list)
    with (evidence / "samples.jsonl").open() as stream:
        for line in stream:
            record = json.loads(line)
            groups[tuple(record.get(key) for key in GROUP_KEYS)].append(record)
    summaries = [dict(zip(GROUP_KEYS, key), **summarize_samples(records))
                 for key, records in groups.items()]
    output = evidence / "summary.json"
    write_json(output, {"schema_version": 1, "series": summaries})
    return output


def validate_command(scenario: dict, result) -> dict:
    expected = scenario["expected"]
    command = expected["command"]
    if command == "search":
        identifiers = [item["element_id"] for item in result]
        if identifiers != [expected["id"]] or any(item.get("flags") != [] for item in result):
            raise ValueError("unexpected search IDs")
        return {"ids": identifiers}
    if command == "check":
        if result["errors"] != []:
            raise ValueError("corpus validation findings")
        return {"errors": 0}
    if command == "status":
        if result["problems"] or result["revision"] is None or result["locks"]:
            raise ValueError("unexpected problems or locks")
        return {"revision": result["revision"], "backlog": result["embedding_backlog"]}
    if command == "reindex":
        if type(result["revision"]) is not int or result["revision"] < 1:
            raise ValueError("invalid publication")
        return {"revision": result["revision"]}
    if command == "inspect":
        return validate_inspection(scenario, result)
    if command == "traverse":
        return validate_traversal(expected, result)
    if command == "embed":
        if result != {}:
            raise ValueError("invalid drain result")
        return {}
    raise ValueError("missing correctness oracle")


def validate_inspection(scenario: dict, result: dict) -> dict:
    expected_id = scenario["expected"]["id"]
    if set(result["elements"]) != {expected_id} or not re.fullmatch(r"[0-9a-f]{40}|[0-9a-f]{64}", result["snapshot"]):
        raise ValueError("invalid inspect identity")
    element = result["elements"][expected_id]
    relative = PurePosixPath(element["path"])
    if relative.is_absolute() or ".." in relative.parts or str(relative) != expected_id.split("#")[0]:
        raise ValueError("unexpected inspect path")
    data = run_git(scenario["repository"], "show", f'{result["snapshot"]}:{relative}')
    if "lines" in element:
        start, end = element["lines"]
        lines = data.splitlines(keepends=True)
        if not (1 <= start <= end <= len(lines)):
            raise ValueError("invalid captured line range")
        data = b"".join(lines[start - 1:end])
    if hashlib.sha256(data).hexdigest() != element["hash"]:
        raise ValueError("captured bytes do not match inspect hash")
    return {"id": expected_id, "snapshot": result["snapshot"], "hash": element["hash"]}


def validate_traversal(expected: dict, result: dict) -> dict:
    edges = {(item["source"], item["target"], item["type"]) for item in result["edges"]}
    required = {tuple(edge) for edge in expected.get("edges", [])}
    if not required or edges != required:
        raise ValueError("unexpected traversal edges")
    ids = {item["element_id"] for item in result["elements"]}
    if ids != set(expected["element_ids"]) or any(item.get("flags") != [] for item in result["elements"]):
        raise ValueError("unexpected traversal IDs")
    return {"ids": sorted(ids), "edge_count": len(edges)}


def verify_projection_counts(scenario: dict) -> dict:
    """Read ordinary metadata after timing; never load vec1 through Python SQLite."""
    path = scenario["repository"] / ".letopis/cache.db"
    connection = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
    try:
        nodes = dict(connection.execute("SELECT kind, count(*) FROM nodes GROUP BY kind"))
        edges = connection.execute("SELECT count(*) FROM edges").fetchone()[0]
    finally:
        connection.close()
    observed = {"knowledge": nodes.get("vertex", 0) + nodes.get("metavertex", 0),
                "source": nodes.get("source", 0), "edges": edges}
    expected = scenario["expected"]["counts"]
    if any(observed[key] != expected[key] for key in observed):
        raise ValueError("projection counts differ from independent fixture counts")
    return observed
