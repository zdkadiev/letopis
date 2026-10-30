#!/usr/bin/env python3
"""Run bounded, model-free diagnostics in isolated synthetic repositories."""

import argparse
import json
import os
from pathlib import Path
import platform
import signal
import shutil
import subprocess
import time

from measure import measure_process
from report import validate_result, write_report
from scenarios import (ROOT, SCENARIOS, build_release, encode_json, hash_file,
                       initialize_projection, prepare_fixture, prepare_scenario,
                       run_git, verify_build, verify_new_output, write_json)


def main() -> None:
    args = parse_arguments()
    if args.list_scenarios:
        print("\n".join(SCENARIOS))
        return
    if not args.stage or args.evidence is None:
        raise SystemExit("--stage and --evidence are required")
    evidence = args.evidence.absolute()
    verify_new_output(evidence)
    evidence.mkdir(parents=True, mode=0o700)
    manifest = get_manifest(args)
    manifest["harness_archive"] = capture_harness(evidence)
    write_json(evidence / "manifest.json", manifest)
    previous = signal.signal(signal.SIGALRM, stop_stage)
    signal.setitimer(signal.ITIMER_REAL, args.stage_seconds)
    try:
        build = read_build(args, evidence)
        if args.stage in ("survey", "scale"):
            run_survey(args, build, evidence)
        elif args.stage == "profile":
            from profile import run_profiles
            run_profiles(args, build, evidence)
        else:
            from contention import run_campaign
            run_campaign(args, build, evidence)
        write_json(evidence / "completion.json", {"outcome": "complete", "build": build})
    except (Exception, KeyboardInterrupt) as error:
        write_json(evidence / "completion.json", {"outcome": "interrupted" if
                   isinstance(error, KeyboardInterrupt) else "failed", "error_type": type(error).__name__,
                   "diagnostic": str(error)[:1000]})
        raise
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)


def parse_arguments():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list-scenarios", action="store_true")
    parser.add_argument("--stage", choices=("survey", "profile", "scale", "contention"))
    parser.add_argument("--nodes", type=int, default=1000)
    parser.add_argument("--evidence", type=Path)
    parser.add_argument("--baseline", type=Path)
    parser.add_argument("--fixture-manifest", type=Path, help="copy a verified idle disabled synthetic fixture")
    parser.add_argument("--scenarios", nargs="+", choices=SCENARIOS)
    parser.add_argument("--repetitions", type=int, help="explicit smoke/extended sampling override")
    parser.add_argument("--timeout", type=float, default=120)
    parser.add_argument("--initial-timeout", type=float, default=120,
                        help="recorded first-publication/full-source-audit override, at most 600s")
    parser.add_argument("--stage-seconds", type=float, default=1200)
    args = parser.parse_args()
    if not 0 < args.timeout <= 600 or not 0 < args.stage_seconds <= 1200:
        parser.error("timeouts must be positive and at most 600s/call, 1200s/stage")
    if not 0 < args.initial_timeout <= 600:
        parser.error("initial timeout must be positive and at most 600 seconds")
    if args.repetitions is not None and args.repetitions < 1:
        parser.error("repetitions must be positive")
    if args.timeout > 120 and args.nodes < 100000:
        parser.error("extended timeout requires the 100k/full-audit campaign")
    if args.fixture_manifest and args.stage != "profile":
        parser.error("fixture reuse is currently supported only for profiling")
    return args


def read_build(args, evidence: Path) -> dict:
    if args.baseline:
        pin = json.loads(args.baseline.read_text())
        build = pin.get("build", pin)
        verify_build(build)
        write_json(evidence / "build.json", build)
        return build
    return build_release(ROOT, ROOT / "target", evidence / "build")


def get_manifest(args) -> dict:
    return {"schema_version": 1, "stage": args.stage, "nodes": args.nodes,
            "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "host": {"platform": platform.platform(), "machine": platform.machine(),
                     "cpu_count": os.cpu_count(), "load_average": os.getloadavg(),
                     "physical_ram_bytes": os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE"),
                     "cpu_model": get_cpu_model(), "git_executable": shutil.which("git"),
                     "git_version": subprocess.check_output(["git", "--version"], text=True).strip(),
                     "filesystem_free_bytes": os.statvfs(ROOT).f_bavail * os.statvfs(ROOT).f_frsize,
                     "disk_model": "unavailable: not measured"},
            "harness_commit": run_git(ROOT, "rev-parse", "HEAD").decode().strip(),
            "harness_hashes": {str(path.relative_to(ROOT)): hash_file(path)
                               for path in sorted((ROOT / "tests/performance").glob("*.py"))},
            "settings": {"timeout": args.timeout, "stage_seconds": args.stage_seconds,
                         "initial_timeout": args.initial_timeout,
                         "fixture_manifest": str(args.fixture_manifest) if args.fixture_manifest else None,
                         "repetitions_override": args.repetitions, "selected_scenarios": args.scenarios},
            "coverage_limits": {"true_hybrid": "not_assessed: no real model authorization",
                                "real_model_drain": "not_assessed: no authorization",
                                "linux": "not_assessed: initial host is macOS",
                                "parallel_drain": "not_assessed: product drain is sequential"},
            "cpu_scope": "see each wait4 record; not whole-stack simultaneous aggregate",
            "cache_state": "OS page cache uncontrolled; fresh CLI process per call"}


def run_survey(args, build: dict, evidence: Path) -> None:
    started = time.monotonic()
    fixture = prepare_fixture(evidence / "fixture", args.nodes)
    fixture["binary"] = build["executable"]
    write_json(evidence / "corpus.json", {key: str(value) if isinstance(value, Path) else value
                                          for key, value in fixture.items()})
    deadline = started + args.stage_seconds
    coverage = []
    with (evidence / "samples.jsonl").open("xb") as stream:
        first = measure_process([build["executable"], "search"], fixture["repository"],
                                b'{"query":"\\"case 000000\\"","limit":10}\n',
                                {"timeout": min(args.initial_timeout, deadline - time.monotonic())})
        first.update(name="search", nodes=args.nodes, phase="first_use", embedding_mode="fts_disabled",
                     profile_mode="none", cache_label="uninitialized_projection_os_cache_uncontrolled",
                     validation=validate_result({"expected": {"command": "search",
                                                             "id": "knowledge/part-0000.md#node-000000"}}, first))
        append_record(stream, first)
        if not first["validation"]["valid"]:
            raise ValueError("initial projection failed; inspect first-use receipt")
        for name in args.scenarios or SCENARIOS:
            if time.monotonic() >= deadline:
                coverage.append({"name": name, "state": "not_assessed", "reason": "stage budget"})
                continue
            run_series(args, build, fixture, evidence, stream, name, deadline)
            coverage.append({"name": name, "state": "attempted"})
    write_json(evidence / "coverage.json", {"rows": coverage})
    write_report(evidence)


def run_series(args, build: dict, fixture: dict, evidence: Path, stream, name: str, deadline: float):
    expensive = name.startswith("refresh") or name == "reindex" or args.nodes >= 100000
    repetitions = args.repetitions or (3 if expensive else 20)
    reusable = None
    for index in range(repetitions + (0 if expensive else 2)):
        if not expensive and index >= repetitions + 2:
            break
        if time.monotonic() >= deadline:
            break
        if reusable is None or expensive:
            scenario = prepare_scenario(fixture, name, evidence / f"case-{name}-{index}")
            if not expensive:
                reusable = scenario
        else:
            scenario = reusable
        phase = "conditioning" if not expensive and index < 2 else "measured"
        verify_build(build)
        remaining = min(args.timeout, deadline - time.monotonic())
        if remaining <= 0:
            break
        record = measure_process([build["executable"], *scenario["argv"]], scenario["repository"],
                                 scenario["stdin"], {"timeout": remaining})
        record.update(name=name, nodes=args.nodes, phase=phase, embedding_mode="fts_disabled",
                      profile_mode="none", cache_label=scenario["cache_label"],
                      setup_seconds=scenario["setup_seconds"], validation=validate_result(scenario, record))
        append_record(stream, record)
        if not expensive and args.repetitions is None and record["duration_seconds"] > 2:
            repetitions = min(repetitions, 3)
        print(json.dumps({"scenario": name, "phase": phase, "index": index,
                          "seconds": round(record["duration_seconds"], 4),
                          "valid": record["validation"]["valid"]}), flush=True)
        if not record["validation"]["valid"]:
            break


def append_record(stream, record: dict) -> None:
    """Persist completed attempts so interruption cannot erase prior evidence."""
    stream.write(encode_json(record))
    stream.flush()
    os.fsync(stream.fileno())


def stop_stage(*_) -> None:
    """Interrupt setup as well as calls; cleanup runs through normal finally blocks."""
    raise RuntimeError("stage watchdog expired; partial evidence retained")


def capture_harness(evidence: Path) -> dict:
    """Preserve runnable source bytes even when the harness has pending local edits."""
    files = list((ROOT / "tests/performance").glob("*.py"))
    files.extend(ROOT / "tests/target_corpus" / name for name in ("generate.py", "validate.py"))
    hashes = {}
    for source in sorted(files):
        relative = source.relative_to(ROOT)
        output = evidence / "harness" / relative
        output.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, output)
        hashes[str(relative)] = hash_file(output)
    return hashes


def get_cpu_model() -> str:
    try:
        return subprocess.check_output(["sysctl", "-n", "machdep.cpu.brand_string"],
                                       stderr=subprocess.DEVNULL, text=True, timeout=2).strip()
    except (OSError, subprocess.CalledProcessError):
        return "unavailable on this host or under current permissions"


if __name__ == "__main__":
    main()
