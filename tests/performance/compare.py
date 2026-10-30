#!/usr/bin/env python3
"""Compare independently frozen release builds on equivalent synthetic inputs."""

import argparse
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import time

from measure import measure_process
from report import summarize_samples, validate_result
from run import append_record, capture_harness, get_cpu_model
from scenarios import (ROOT, SCENARIOS, build_release, build_request, encode_json,
                       hash_file, prepare_fixture, prepare_scenario, read_product_inputs,
                       verify_new_output, write_json)

SIDES = ("before", "after")
STAGE_SECONDS = 1200
STABLE_SCENARIOS = {"check", "inspect", "traverse", "search", "status"}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--freeze-build", action="store_true")
    parser.add_argument("--source", type=Path, default=ROOT)
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--stage", choices=("timings", "initialization", "contention", "profiles"))
    parser.add_argument("--before", type=Path)
    parser.add_argument("--after", type=Path)
    parser.add_argument("--nodes", type=int, choices=(60, 1000), default=60)
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIOS)
    parser.add_argument("--pairs", type=int)
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--initial-timeout", type=float, default=600)
    parser.add_argument("--stage-seconds", type=float, default=STAGE_SECONDS)
    args = parser.parse_args()
    if args.freeze_build:
        if args.stage or args.before or args.after or args.pairs is not None or args.scenarios:
            parser.error("comparison flags cannot be used in freeze mode")
        freeze_build(args.source, args.evidence)
        return
    if not args.stage or not args.before or not args.after:
        parser.error("comparison requires --stage, --before and --after")
    if args.pairs is not None and (args.stage != "timings" or args.pairs < 1):
        parser.error("positive --pairs is a timings-only override")
    if not 0 < args.timeout <= 120 or not 0 < args.initial_timeout <= 600 or not 0 < args.stage_seconds <= 1200:
        parser.error("budgets must be positive, at most 120s/call, 600s/initialization, 1200s/stage")
    if args.stage in ("profiles", "contention") and args.scenarios and any(
            name not in ("check", "inspect", "traverse", "search") for name in args.scenarios):
        parser.error("profile/contention probes require a read scenario")
    args.pairs = args.pairs or 10
    result = run_comparison(args)
    if result["outcome"] != "complete":
        raise SystemExit(1)


def freeze_build(source: Path, evidence: Path) -> dict:
    """Archive a clean release's exact product inputs, binary, symbols and receipt."""
    source = source.resolve()
    verify_new_output(evidence)
    inputs = read_product_inputs(source)
    evidence.mkdir(parents=True, mode=0o700)
    archive = evidence / "source"
    for relative in inputs:
        output = archive / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source / relative, output)
        output.chmod(0o444)
    build = build_release(source, source / "target", evidence / "release")
    if build["inputs"] != inputs or read_product_inputs(source) != inputs:
        raise ValueError("source changed while freezing release")
    product = evidence / "release/product"
    artifacts = {str(path.relative_to(product)): hash_file(path)
                 for path in sorted(product.rglob("*")) if path.is_file()}
    for path in product.rglob("*"):
        if path.is_file():
            path.chmod(0o555 if path.name == "letopis" else 0o444)
    receipt = evidence / "release/build.json"
    receipt.chmod(0o444)
    pin = dict(build, frozen_schema=1, source_archive=str(archive), artifacts=artifacts,
               release_receipt=str(receipt), release_receipt_sha256=hash_file(receipt),
               receipt=str(evidence / "build.json"))
    write_json(evidence / "build.json", pin)
    verify_frozen_build(pin)
    return pin


def run_comparison(args) -> dict:
    """Persist a bounded matched campaign, retaining failed/censored attempts."""
    evidence = args.evidence
    verify_new_output(evidence)
    builds = {side: json.loads(getattr(args, side).read_text()) for side in SIDES}
    for build in builds.values():
        verify_frozen_build(build)
    if builds["before"]["rustc"] != builds["after"]["rustc"]:
        raise ValueError("comparison requires the same toolchain")
    evidence.mkdir(parents=True, mode=0o700)
    manifest = {"schema_version": 1, "stage": args.stage, "nodes": args.nodes,
                "builds": builds, "settings": {key: str(value) if isinstance(value, Path) else value
                                               for key, value in vars(args).items()},
                "harness_archive": capture_harness(evidence),
                "host": {"platform": platform.platform(), "machine": platform.machine(),
                         "cpu_model": get_cpu_model(), "cpu_count": os.cpu_count(),
                         "load_average": os.getloadavg()},
                "cache_state": "initialized independent copies; OS page cache uncontrolled",
                "warmups_per_side": 2 if args.stage == "timings" else 0,
                "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    write_json(evidence / "manifest.json", manifest)
    args.stage_expired = False
    def stop_campaign(*_):
        # KeyboardInterrupt lets measure_process persist its owned child's receipt.
        args.stage_expired = True
        raise KeyboardInterrupt
    previous = signal.signal(signal.SIGALRM, stop_campaign)
    signal.setitimer(signal.ITIMER_REAL, args.stage_seconds)
    rows, outcome = [], "complete"
    try:
        with (evidence / "samples.jsonl").open("xb") as stream:
            run_stages(args, builds, evidence, stream, rows)
        if any(not row["validation"]["valid"] for row in rows):
            outcome = "incomplete"
    except (Exception, KeyboardInterrupt) as error:
        write_json(evidence / "failure.json", {"error_type": type(error).__name__,
                                               "diagnostic": str(error)[:1000]})
        outcome = "interrupted" if isinstance(error, KeyboardInterrupt) and not args.stage_expired else "incomplete"
        if isinstance(error, KeyboardInterrupt) and not args.stage_expired:
            raise
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
        result = build_campaign_summary(rows, outcome)
        write_json(evidence / "summary.json", result)
    return result


def run_stages(args, builds: dict, evidence: Path, stream, rows: list) -> None:
    fixture = prepare_fixture(evidence / "fixture", args.nodes)
    fixtures = {}
    for side in SIDES:
        repository = evidence / f"initialized-{side}"
        shutil.copytree(fixture["repository"], repository)
        fixtures[side] = dict(fixture, repository=repository, binary=builds[side]["executable"])
        command, request = build_request("search")
        case = {"repository": repository, "argv": [command], "stdin": encode_json(request),
                "expected": {"command": "search", "id": "knowledge/part-0000.md#node-000000",
                             "counts": fixture["counts"]}}
        record = measure_case(args, builds[side], case, "search", side, "first_publication", 0,
                              args.initial_timeout)
        save_attempt(stream, rows, record)
    if not all(row["validation"]["valid"] for row in rows):
        return
    names = args.scenarios or get_default_scenarios(args.stage, args.nodes)
    for name in names:
        if args.stage == "timings":
            run_timing_series(args, builds, fixtures, evidence, stream, rows, name)
        else:
            run_probe_series(args, builds, fixtures, evidence, stream, rows, name)


def run_timing_series(args, builds, fixtures, evidence, stream, rows, name):
    stable = None
    for index in range(args.pairs + 2):
        if stable is None or name not in STABLE_SCENARIOS:
            stable = prepare_pair(fixtures, name, evidence, f"{name}-{index}", builds)
        phase = "conditioning" if index < 2 else "measured"
        pair = index - 2
        for side in get_order(index):
            case = stable[side]
            record = measure_case(args, builds[side], case, name, side, phase, pair, args.timeout)
            record["projection_revision"] = read_projection_revision(case["repository"])
            save_attempt(stream, rows, record)
        if any(not row["validation"]["valid"] for row in rows[-2:]):
            return


def run_probe_series(args, builds, fixtures, evidence, stream, rows, name):
    censored_sides = set()
    for index in range(3):
        cases = prepare_pair(fixtures, name, evidence, f"{name}-{index}", builds)
        for side in get_order(index):
            if side in censored_sides:
                continue
            case, build = cases[side], builds[side]
            case["timeout"] = args.timeout
            verify_frozen_build(build)
            record = run_probe(args, case, build, evidence / f"probe-{name}-{index}-{side}", index)
            record.update(name=name, nodes=args.nodes, side=side, pair=index, phase="measured",
                          embedding_mode="fts_disabled", profile_mode=args.stage)
            if args.stage == "initialization" and record["outcome"] == "timeout":
                censored_sides.add(side)
                record["subsequent_attempts_skipped"] = 2 - index
            save_attempt(stream, rows, record)


def run_probe(args, case, build, output, index):
    if args.stage == "initialization":
        return measure_case(args, build, case, case["name"], "pending", "measured", index, args.timeout)
    if args.stage == "contention":
        from contention import ContentionInterrupted, run_contention
        try:
            receipt = run_contention(case, build, output, verify=lambda: verify_frozen_build(build))
        except ContentionInterrupted as error:
            return error.record
        record = receipt["reader"]
        valid = (receipt["reader_validation"]["valid"] and receipt["writer_validation"]["valid"]
                 and receipt["baseline_validation"]["valid"])
        record.update(contention_receipt=str(output / "receipt.json"),
                      overlap_assessed=receipt["writer_index_lock_observed"] and receipt["overlap_seconds"] > 0,
                      validation={"valid": valid})
        return record
    return run_profile_probe(args, case, build, output, index)


def run_profile_probe(args, case, build, output, index):
    from profile import record_git_calls, record_profile
    output.mkdir()
    modes = ("none", "samply") if index % 2 == 0 else ("samply", "none")
    plain, profiled = None, None
    for mode in modes:
        verify_frozen_build(build)
        if mode == "none":
            plain = measure_case(args, build, case, case["name"], "pending", "measured", index, args.timeout)
            write_json(output / "ordinary.json", plain)
            if plain["outcome"] == "interrupted":
                return plain
        else:
            receipt = record_profile(case, build, output / "samply")
            profiled = dict(receipt["measurement"], profile_outcome=receipt["outcome"],
                            validation=validate_result(case, receipt["measurement"]))
            if receipt["outcome"] != "recorded":
                profiled["validation"] = {"valid": False, "reason": "no usable profile"}
            if profiled["outcome"] == "interrupted":
                return profiled
    if index == 0:
        verify_frozen_build(build)
        counter = record_git_calls(case, build, output / "git-counts")
        profiled.update(counter_outcome=counter["measurement"]["outcome"],
                        counter_validation=counter["validation"], counter_receipt=str(output / "git-counts/receipt.json"))
        if counter["measurement"]["outcome"] == "interrupted":
            return dict(counter["measurement"], counter_outcome="interrupted",
                        counter_receipt=profiled["counter_receipt"], validation={"valid": False, "reason": "counter interrupted"})
        if counter["measurement"]["outcome"] != "success" or not counter["validation"]["valid"]:
            profiled["validation"] = {"valid": False, "reason": "required Git counter failed"}
    if not plain["validation"]["valid"]:
        profiled["validation"] = {"valid": False, "reason": "ordinary profile control failed"}
    profiled["ordinary_duration_seconds"] = plain["duration_seconds"]
    profiled["receipt_directory"] = str(output)
    return profiled


def prepare_pair(fixtures, name, evidence, label, builds):
    cases = {}
    for side in SIDES:
        # prepare_scenario refreshes copied inode identities outside the measurement.
        verify_frozen_build(builds[side])
        cases[side] = prepare_scenario(fixtures[side], name, evidence / f"case-{label}-{side}")
    verify_equivalent_cases(cases["before"], cases["after"])
    return cases


def measure_case(args, build: dict, case: dict, name: str, side: str,
                 phase: str, index: int, timeout: float) -> dict:
    verify_frozen_build(build)
    record = measure_process([build["executable"], *case["argv"]], case["repository"],
                             case["stdin"], {"timeout": timeout})
    record.update(name=name, nodes=args.nodes, side=side, phase=phase, pair=index,
                  embedding_mode="fts_disabled", profile_mode="none", timeout_seconds=timeout,
                  cache_label="OS cache uncontrolled", validation=validate_result(case, record))
    if getattr(args, "stage_expired", False):
        record["censor_reason"] = "stage watchdog"
    return record


def save_attempt(stream, rows: list, record: dict) -> None:
    rows.append(record)
    append_record(stream, record)
    print(json.dumps({key: record[key] for key in ("side", "name", "phase", "pair", "outcome",
                                                 "duration_seconds", "validation")}), flush=True)


def get_default_scenarios(stage: str, nodes: int) -> list[str]:
    if stage == "initialization":
        return ["check", "reindex", "status", "refresh_bulk"]
    if stage == "contention":
        return ["search"]
    if stage == "profiles":
        return ["check", "inspect", "traverse"]
    return ["check"] if nodes == 60 else ["inspect", "traverse", "search"]


def get_order(index: int) -> tuple:
    return SIDES if index % 2 == 0 else tuple(reversed(SIDES))


def read_projection_revision(repository: Path):
    """Record metadata outside timing without loading native vector extensions."""
    import sqlite3
    connection = sqlite3.connect((repository / ".letopis/cache.db").as_uri() + "?mode=ro", uri=True)
    try:
        row = connection.execute("SELECT revision FROM project WHERE id = 1").fetchone()
        return row[0] if row else None
    except sqlite3.OperationalError:
        return None
    finally:
        connection.close()


def build_campaign_summary(rows: list, outcome: str) -> dict:
    groups = sorted({(row["name"], row["phase"], row["profile_mode"]) for row in rows
                     if row["phase"] != "conditioning"})
    series = []
    for name, phase, mode in groups:
        sides = [[row for row in rows if row["name"] == name and row["phase"] == phase
                  and row["profile_mode"] == mode and row["side"] == side] for side in SIDES]
        series.append(dict(name=name, phase=phase, profile_mode=mode,
                           **summarize_comparison(*sides)))
    return {"schema_version": 1, "outcome": outcome, "series": series}


def summarize_comparison(before: list[dict], after: list[dict]) -> dict:
    """Report successful distributions while retaining every failed attempt count."""
    left, right = summarize_samples(before), summarize_samples(after)
    old, new = left["median"], right["median"]
    return {"before": left, "after": right, "before_median_seconds": old,
            "after_median_seconds": new, "speedup": old / new if old is not None and new else None,
            "reduction_percent": 100 * (old - new) / old if old and new is not None else None}


def verify_equivalent_cases(before: dict, after: dict) -> None:
    """Reject mismatched requests, independent oracles or actual corpus bytes."""
    for key in ("argv", "stdin", "expected", "state_hashes", "changed_paths"):
        if before[key] != after[key]:
            raise ValueError(f"unequal fixture {key}")
    for relative in before["state_hashes"]:
        if hash_file(before["repository"] / relative) != hash_file(after["repository"] / relative):
            raise ValueError("unequal actual fixture bytes")
    for relative in (".letopis/config.json", ".gitignore"):
        if (before["repository"] / relative).read_bytes() != (after["repository"] / relative).read_bytes():
            raise ValueError("unequal fixture configuration")


def verify_frozen_build(pin: dict) -> None:
    """Verify archived inputs and release artifacts without consulting live sources.

    Receipts establish local build provenance, not cryptographic compiler attestation.
    Keep the original builder receipt; an executable hash alone is insufficient.
    """
    required = {"frozen_schema", "source_archive", "artifacts", "release_receipt",
                "release_receipt_sha256", "inputs", "executable", "sha256"}
    if not required <= pin.keys() or pin["frozen_schema"] != 1:
        raise ValueError("missing frozen release provenance")
    receipt = Path(pin["release_receipt"])
    if receipt.is_symlink() or hash_file(receipt) != pin["release_receipt_sha256"]:
        raise ValueError("original release receipt changed")
    original = json.loads(receipt.read_text())
    fields = {"executable", "sha256", "inputs", "product_commit", "rustc", "command", "release_debug", "target"}
    if (not fields <= original.keys() or any(pin.get(key) != original[key] for key in fields)
            or not {"--release", "--locked", "--offline"} <= set(original["command"])
            or original["release_debug"] != "1"):
        raise ValueError("frozen build differs from original locked release receipt")
    verify_manifest(Path(pin["source_archive"]), pin["inputs"])
    binary = Path(pin["executable"])
    verify_manifest(binary.parent, pin["artifacts"])
    if binary.is_symlink() or hash_file(binary) != pin["sha256"]:
        raise ValueError("binary differs from frozen release")


def verify_manifest(root: Path, expected: dict) -> None:
    if root.is_symlink() or any(path.is_symlink() for path in root.rglob("*")):
        raise ValueError("symlink in frozen archive")
    actual = {str(path.relative_to(root)): hash_file(path)
              for path in sorted(root.rglob("*")) if path.is_file()}
    if not expected or actual != expected:
        raise ValueError("frozen archive manifest changed")


if __name__ == "__main__":
    main()
