"""Recompute summaries from captured probe output; does not open the DB."""
import hashlib
import json
import math
from pathlib import Path
import re
import statistics

ROOT = Path(__file__).resolve().parent


def stats(values):
    values = sorted(values)
    assert values
    return dict(n=len(values), min=min(values), median=statistics.median(values),
                p95=values[math.ceil(.95 * len(values)) - 1], max=max(values))


results = {}
active = 0
for mode in ["load", "read", "mixed", "readers_1", "readers_2", "readers_4"]:
    rows = [json.loads(s) for s in (ROOT / f"{mode}-stdout.jsonl").read_text().splitlines()]
    complete = [r for r in rows if r["event"] == "complete"]
    assert len(complete) == 1
    active += complete[0]["seconds"]
    captured = (ROOT / f"{mode}-time.txt").read_text()
    result = dict(program_seconds=complete[0]["seconds"],
                  real_seconds=float(re.search(r"^real ([\d.]+)", captured, re.M)[1]),
                  peak_rss_bytes=int(re.search(r"(\d+)  maximum resident set size", captured)[1]),
                  peak_footprint_bytes=int(re.search(r"(\d+)  peak memory footprint", captured)[1]))
    groups = {}
    for row in rows:
        if row["event"] == "query":
            groups.setdefault(row["kind"], []).append(row)
    for kind, queries in groups.items():
        assert all(q["rows"] > 0 for q in queries)
        expected = 10 if kind == "burst_10_readers" else 150 if kind.startswith("readers_") or kind == "steady_10qps" else 30
        assert len(queries) == expected, (kind, len(queries))
        if expected == 150:
            assert sorted(q["i"] for q in queries) == list(range(expected))
        summary = {"service_ms": stats([q["ms"] for q in queries])}
        if "dispatch_lag_ms" in queries[0]:
            totals = [q["ms"] + q["dispatch_lag_ms"] for q in queries]
            for q, total in zip(queries, totals):
                if "scheduled_to_finish_ms" in q:
                    assert abs(q["scheduled_to_finish_ms"] - total) < .01
            summary.update(dispatch_lag_ms=stats([q["dispatch_lag_ms"] for q in queries]),
                           scheduled_to_finish_ms=stats(totals))
        groups[kind] = summary
    result["queries"] = groups
    results[mode] = result
assert active < 300, active
load = [json.loads(s) for s in (ROOT / "load-stdout.jsonl").read_text().splitlines()]
counts = {r["table"]: r["rows"] for r in load if r["event"] == "count"}
assert counts == {"nodes": 200000, "edges": 500000, "text_index": 100000, "vector_index": 100000}
assert any(r["event"] == "verify" and r["result"] == "PASS" for r in load)
manifest = json.loads((ROOT / "build-manifest.json").read_text())
binary = ROOT / "target/release/parallelism"
if binary.exists():
    manifest["parallelism_binary_sha256"] = hashlib.sha256(binary.read_bytes()).hexdigest()
    (ROOT / "build-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
output = dict(units="milliseconds and bytes; nearest-rank p95", counts=counts,
              active_program_seconds=active,
              observed_wal_max_bytes=max(r.get("wal_bytes", 0) for r in load),
              db_breakdown=json.loads((ROOT / "db-breakdown.json").read_text()), runs=results)
(ROOT / "analysis.json").write_text(json.dumps(output, indent=2) + "\n")
print(json.dumps({"active_seconds": active, "runs": results}, indent=2))
