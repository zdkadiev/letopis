#!/usr/bin/env python3
"""Bounded historical-provenance campaigns on frozen releases and synthetic inputs."""

import argparse
import json
import os
from pathlib import Path
import platform
import random
import shutil
import signal

from compare import verify_equivalent_cases, verify_frozen_build, summarize_comparison
from measure import measure_process
from profile import record_profile
from report import summarize_samples, validate_result
from run import append_record, capture_harness, get_cpu_model
from scenarios import (build_request, encode_json, hash_file, prepare_fixture,
                       run_git, verify_new_output, write_json)

SOURCE_BYTES = b"".join(f"Historical source line {index:03d}: stable evidence.\n".encode()
                        for index in range(1, 101))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--after", type=Path)
    parser.add_argument("--nodes", type=int, choices=(10000, 100000), required=True)
    parser.add_argument("--variant", choices=("shared", "distinct"), required=True)
    parser.add_argument("--stage", choices=("timings", "profiles"), required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--scenarios", nargs="+", choices=("check", "status"))
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--initial-timeout", type=float, default=600)
    parser.add_argument("--stage-seconds", type=float, default=1200)
    arguments = parser.parse_args()
    if not (0 < arguments.timeout <= 120 and 0 < arguments.initial_timeout <= 600
            and 0 < arguments.stage_seconds <= 1200):
        parser.error("budgets must be positive and at most 120/600/1200 seconds")
    result = run_campaign(arguments)
    if result["outcome"] != "complete":
        raise SystemExit(1)


def run_campaign(arguments: argparse.Namespace) -> dict:
    """Persist each attempt before starting another, without retrying failed series."""
    evidence = arguments.evidence
    verify_new_output(evidence)
    sides = ("before", "after") if arguments.after else ("before",)
    builds = {side: json.loads(getattr(arguments, side).read_text()) for side in sides}
    for build in builds.values():
        verify_frozen_build(build)
    if len({build["rustc"] for build in builds.values()}) != 1:
        raise ValueError("same compiler required")
    evidence.mkdir(parents=True, mode=0o700)
    rows, outcome = [], "complete"
    manifest = build_manifest(arguments, builds)
    arguments.stage_expired = False
    def stop_stage(*_):
        arguments.stage_expired = True
        raise KeyboardInterrupt("stage watchdog expired")
    previous = signal.signal(signal.SIGALRM, stop_stage)
    signal.setitimer(signal.ITIMER_REAL, arguments.stage_seconds)
    try:
        fixtures = {}
        for side in sides:
            fixtures[side] = prepare_historical_fixture(evidence / f"fixture-{side}", arguments.nodes, arguments.variant)
            manifest["fixtures"][side] = {key: value for key, value in fixtures[side].items() if key != "repository"}
        write_json(evidence / "manifest.json", manifest)
        with (evidence / "samples.jsonl").open("xb") as stream:
            run_series(arguments, builds, fixtures, stream, rows)
        if any(not row["validation"]["valid"] for row in rows):
            outcome = "incomplete"
    except (Exception, KeyboardInterrupt) as error:
        outcome = "interrupted" if isinstance(error, KeyboardInterrupt) and not arguments.stage_expired else "incomplete"
        write_json(evidence / "failure.json", {"error_type": type(error).__name__, "diagnostic": str(error)[:1000]})
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        if not (evidence / "manifest.json").exists():
            write_json(evidence / "manifest.json", manifest)
        result = build_summary(rows, sides, outcome)
        result.update(manifest=str(evidence / "manifest.json"), samples=str(evidence / "samples.jsonl"),
                      summary=str(evidence / "summary.json"))
        write_json(evidence / "summary.json", result)
    return result


def build_manifest(arguments, builds):
    return {"schema_version": 1, "builds": builds,
            "settings": {key: str(value) if isinstance(value, Path) else value for key, value in vars(arguments).items()},
            "harness_archive": capture_harness(arguments.evidence), "fixtures": {},
            "host": {"platform": platform.platform(), "machine": platform.machine(),
                     "cpu_model": get_cpu_model(), "cpu_count": os.cpu_count(), "load_average": os.getloadavg()},
            "conditioning_calls_per_side_and_scenario": 2, "ordinary_samples_per_side_and_scenario": 3,
            "cpu_rss_scope": "inherited per-child wait4; see attempt resources; not whole-stack peak",
            "cache_state": "separate stable repositories; OS page cache uncontrolled"}


def prepare_historical_fixture(output: Path, nodes: int, variant: str) -> dict:
    """Create standalone inputs with a complete, byte-identical historical source tree."""
    if variant not in ("shared", "distinct"):
        raise ValueError("unknown historical variant")
    fixture = prepare_fixture(output, nodes)
    repository = fixture["repository"]
    shutil.rmtree(repository / "sources")
    (repository / "sources").mkdir()
    paths = ["sources/shared.txt"] if variant == "shared" else [f"sources/source-{index}.txt" for index in range(nodes)]
    for relative in paths:
        (repository / relative).write_bytes(SOURCE_BYTES)
    run_git(repository, "add", "-A", "--", "sources")
    source_tree = run_git(repository, "write-tree").decode().strip()
    randomizer = random.Random(65)
    selectors, index = set(), 0
    for relative in sorted(fixture["input_sha256"]):
        if not relative.startswith("knowledge/"):
            continue
        count = (repository / relative).read_bytes().count(b"## Synthetic knowledge")
        with (repository / relative).open("wb") as stream:
            for _ in range(count):
                source = get_source_selector(index, variant)
                selectors.add(source)
                stream.write(build_fragment(index, source, source_tree, randomizer))
                index += 1
    inputs = {str(path.relative_to(repository)): hash_file(path)
              for directory in ("sources", "knowledge") for path in sorted((repository / directory).iterdir())}
    counts = {"knowledge": nodes, "source": len(selectors), "edges": nodes, "includes": 0}
    requests = {name: build_request(name)[1] for name in ("check", "status")}
    expected = {name: {"command": name, "counts": counts} for name in requests}
    manifest = {"schema_version": 1, "variant": variant, "options": fixture["options"],
                "source_tree": source_tree, "input_sha256": dict(sorted(inputs.items())),
                "counts": counts, "requests": requests, "expected": expected,
                "source_file_count": len(paths), "source_bytes_per_file": len(SOURCE_BYTES),
                "config_sha256": hash_file(repository / ".letopis/config.json"),
                "packing": fixture["packing"],
                "comparison_limit": "variants differ in source cardinality; absolute timings do not isolate one variable"}
    # Retain the original generator receipt; this new receipt describes actual rewritten bytes.
    write_json(output / "historical.json", manifest)
    return dict(manifest, repository=repository)


def get_source_selector(index: int, variant: str) -> str:
    if variant == "distinct":
        return f"sources/source-{index}.txt"
    if index % 16 == 0:
        return "sources/shared.txt"
    line = 1 + index % 64
    return f"sources/shared.txt#L{line}-L{line}"


def build_fragment(index: int, source: str, source_tree: str, randomizer: random.Random) -> bytes:
    metadata = json.dumps({"created_at": 1792792800, "source_snapshot": source_tree}, separators=(",", ":"))
    prefix = (f"## Synthetic knowledge {index:06d} {{#node-{index:06d}}}\n"
              f"<!-- letopis: {metadata} -->\n"
              f'[source](../{source} "letopis:type=external;path={source}")\n'
              f"\nHistorical provenance marker, sample {randomizer.randrange(1000000):06d}.\n").encode()
    padding = b"Stable historical evidence for the dependent knowledge. "
    remaining = 2000 - len(prefix) - 2
    return prefix + (padding * (remaining // len(padding) + 1))[:remaining] + b"\n\n"


def run_series(arguments, builds, fixtures, stream, rows):
    sides = tuple(builds)
    ready = set()
    for side in sides:
        case = build_case(fixtures[side], "check")
        record = get_measurement(arguments, builds[side], case, side, "first_publication", 0, "none")
        save_attempt(stream, rows, record)
        if record["validation"]["valid"]:
            ready.add(side)
    for name in arguments.scenarios or ("check", "status"):
        cases = {side: build_case(fixtures[side], name) for side in sides}
        if len(sides) == 2:
            verify_equivalent_cases(cases["before"], cases["after"])
        active = set(ready)
        for index in range(5):
            phase = "conditioning" if index < 2 else "measured"
            pair = index - 2
            order = sides if index % 2 == 0 else tuple(reversed(sides))
            for side in order:
                if side not in active:
                    continue
                modes = ("none",)
                if arguments.stage == "profiles" and phase == "measured":
                    modes = ("none", "samply") if pair % 2 == 0 else ("samply", "none")
                for mode in modes:
                    record = get_measurement(arguments, builds[side], cases[side], side, phase, pair, mode)
                    save_attempt(stream, rows, record)
                    if not record["validation"]["valid"]:
                        active.discard(side)
                        break


def build_case(fixture, name):
    return {"repository": fixture["repository"], "name": name, "argv": [name],
            "stdin": encode_json(fixture["requests"][name]), "expected": fixture["expected"][name],
            "state_hashes": fixture["input_sha256"], "changed_paths": []}


def get_measurement(arguments, build, case, side, phase, pair, mode):
    verify_frozen_build(build)
    timeout = arguments.initial_timeout if phase == "first_publication" else arguments.timeout
    receipt = None
    if mode == "samply":
        receipt = arguments.evidence / f"profile-{case['name']}-{pair}-{side}"
        profile = record_profile(dict(case, timeout=timeout), build, receipt)
        record = dict(profile["measurement"], profile_outcome=profile["outcome"])
    else:
        record = measure_process([build["executable"], *case["argv"]], case["repository"], case["stdin"], {"timeout": timeout})
    record.update(name=case["name"], nodes=arguments.nodes, variant=arguments.variant, side=side,
                  phase=phase, pair=pair, profile_mode=mode, embedding_mode="fts_disabled", timeout_seconds=timeout,
                  validation=validate_result(case, record))
    if arguments.stage_expired:
        record["censor_reason"] = "stage watchdog expired"
    if receipt:
        record["profile_receipt"] = str(receipt / "receipt.json")
        if record["profile_outcome"] != "recorded":
            record["validation"] = {"valid": False, "reason": "profile unavailable"}
    return record


def save_attempt(stream, rows, record):
    rows.append(record)
    append_record(stream, record)
    print(json.dumps({key: record[key] for key in ("side", "name", "phase", "pair", "profile_mode", "outcome", "duration_seconds", "validation")}), flush=True)


def build_summary(rows, sides, outcome):
    series = []
    names = sorted({row["name"] for row in rows if row["phase"] == "measured"})
    for name in names:
        ordinary = {side: [row for row in rows if row["name"] == name and row["phase"] == "measured"
                           and row["profile_mode"] == "none" and row["side"] == side] for side in sides}
        values = summarize_comparison(ordinary["before"], ordinary["after"]) if len(sides) == 2 else {"before": summarize_samples(ordinary["before"])}
        series.append(dict(name=name, **values))
    return {"schema_version": 1, "outcome": outcome, "mode": "comparison" if len(sides) == 2 else "baseline_only",
            "series": series, "profile_attempts": sum(row["profile_mode"] == "samply" for row in rows)}


if __name__ == "__main__":
    main()
