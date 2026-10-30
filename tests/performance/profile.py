"""Samply capture and separate, explicitly instrumented Git-count experiments."""

from collections import Counter
import gzip
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import time

from measure import measure_process
from report import validate_result
from scenarios import (get_environment, hash_file, initialize_projection, prepare_fixture,
                       prepare_existing_fixture, prepare_scenario, verify_build, verify_new_output, write_json)


def record_profile(scenario: dict, build: dict, evidence: Path) -> dict:
    """Record without a UI/server; permission failures cannot become empty success."""
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    sampler = build.get("samply") or shutil.which("samply")
    if not sampler:
        return {"outcome": "unavailable", "reason": "samply is not installed"}
    output = evidence / "profile.json.gz"
    argv = [sampler, "record", "--save-only", "--rate", "1000", "--unstable-presymbolicate",
            "--output", str(output), build["executable"], *scenario["argv"]]
    measurement = measure_process(argv, scenario["repository"], scenario["stdin"],
                                  {"timeout": scenario.get("timeout", 120)})
    result = {"argv": argv, "measurement": measurement, "path": str(output),
              "scope": "samply observed processes; inspect profile before claiming Git coverage"}
    try:
        if measurement["outcome"] != "success":
            raise ValueError("sampler or measured child did not complete successfully")
        result.update(outcome="recorded", profile=validate_profile(output))
        symbols = evidence / "profile.json.syms.json"
        if symbols.exists():
            write_json(evidence / "stacks.json", summarize_stacks(output, symbols))
    except ValueError as error:
        result.update(outcome="unavailable", reason=str(error))
    write_json(evidence / "receipt.json", result)
    return result


def validate_profile(path: Path) -> dict:
    """Reject absent/truncated profiles and recordings without usable stack samples."""
    try:
        with gzip.open(path, "rt") as stream:
            profile = json.load(stream)
        threads = profile["threads"]
        count = sum(thread.get("samples", {}).get("length", 0) for thread in threads)
        usable = sum(stack is not None for thread in threads
                     for stack in thread.get("samples", {}).get("stack", []))
        if not count or not usable:
            raise ValueError("profile has no usable stack samples")
        return {"samples": count, "usable_stacks": usable, "threads": len(threads),
                "sha256": hash_file(path), "bytes": path.stat().st_size,
                "libraries": [library.get("name") for library in profile.get("libs", [])]}
    except (OSError, ValueError, KeyError, TypeError, EOFError) as error:
        raise ValueError("missing, invalid or empty samply profile") from error


def summarize_stacks(profile_path: Path, symbols_path: Path) -> dict:
    """Attribute observed CPU deltas inclusively; frames/threads cannot be summed twice."""
    with gzip.open(profile_path, "rt") as stream:
        profile = json.load(stream)
    symbols = json.loads(symbols_path.read_text())
    lookup = build_symbol_lookup(symbols)
    cpu, observations = Counter(), Counter()
    total = 0
    for thread in profile["threads"]:
        samples = thread["samples"]
        for index, stack in enumerate(samples["stack"]):
            if stack is None:
                continue
            delta = samples.get("threadCPUDelta", [0] * len(samples["stack"]))[index] or 0
            names = read_stack_functions(thread, stack, profile["libs"], lookup)
            total += delta
            for name in names:
                cpu[name] += delta
                observations[name] += 1
    functions = [{"function": name, "inclusive_cpu_delta": cpu[name],
                  "stack_observations": observations[name]} for name in observations]
    return {"observed_cpu_delta": total, "cpu_delta_unit": profile["meta"].get("sampleUnits", {}).get("threadCPUDelta"),
            "inclusive_costs_overlap": True, "wall_percentages": "unavailable: stacks are not blocked-time accounting",
            "process_ids": sorted({thread["pid"] for thread in profile["threads"]}),
            "functions": sorted(functions, key=lambda item: item["inclusive_cpu_delta"], reverse=True)}


def build_symbol_lookup(symbols: dict) -> dict:
    lookup = {}
    strings = symbols["string_table"]
    for library in symbols["data"]:
        for address, index in library["known_addresses"]:
            if index is None:
                continue
            entry = library["symbol_table"][index]
            names = {strings[entry["symbol"]]}
            names.update(strings[frame["function"]] for frame in entry.get("frames", []))
            lookup[library["debug_name"], address] = names
    return lookup


def read_stack_functions(thread: dict, stack: int, libraries: list, lookup: dict) -> set:
    names = set()
    while stack is not None:
        frame = thread["stackTable"]["frame"][stack]
        function = thread["frameTable"]["func"][frame]
        resource = thread["funcTable"]["resource"][function]
        library = thread["resourceTable"]["lib"][resource] if resource >= 0 else None
        if library is not None and library < 0:
            library = None
        address = thread["frameTable"]["address"][frame]
        fallback = thread["stringArray"][thread["funcTable"]["name"][function]]
        key = (libraries[library]["name"], address) if library is not None else ("unknown", address)
        names.update(lookup.get(key, {fallback}))
        stack = thread["stackTable"]["prefix"][stack]
    return names


def record_git_calls(scenario: dict, build: dict, evidence: Path) -> dict:
    """Collect counts separately: Python wrappers add material startup overhead."""
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    real_git = shutil.which("git")
    if not real_git or Path(real_git).resolve() == evidence / "git":
        raise ValueError("cannot resolve independent Git executable")
    launcher = evidence / "git"
    wrapper = Path(__file__).with_name("git_wrapper.py")
    launcher.write_text(f'#!{sys.executable}\nimport runpy\nrunpy.run_path({str(wrapper)!r},run_name="__main__")\n')
    launcher.chmod(0o755)
    log = evidence / "calls.jsonl"
    environment = get_environment()
    environment.update(PATH=str(evidence) + os.pathsep + environment["PATH"],
                       PERF_REAL_GIT=real_git, PERF_GIT_LOG=str(log))
    measured = measure_process([build["executable"], *scenario["argv"]], scenario["repository"],
                               scenario["stdin"], {"environment": environment,
                                                    "timeout": scenario.get("timeout", 120)})
    calls = [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []
    result = {"measurement": measured, "validation": validate_result(scenario, measured),
              "count": len(calls), "operations": dict(Counter(call["operation"] for call in calls)),
              "real_git": real_git, "timing_scope": "instrumented; nested intervals may overlap"}
    write_json(evidence / "receipt.json", result)
    return result


def run_profiles(args, build: dict, evidence: Path) -> None:
    """Pair three ordinary/profiled calls on equivalent copied input states."""
    from run import append_record
    started = time.monotonic()
    fixture = (prepare_existing_fixture(args.fixture_manifest.absolute(), evidence / "fixture")
               if args.fixture_manifest else prepare_fixture(evidence / "fixture", args.nodes))
    if fixture["counts"]["knowledge"] != args.nodes:
        raise ValueError("fixture scale differs from request")
    initialize_projection(fixture["repository"], build["executable"], full_audit=False)
    fixture["binary"] = build["executable"]
    write_json(evidence / "profiler.json", {"samply_version": subprocess.check_output(
        [build.get("samply", "samply"), "--version"], text=True).strip(),
        "rate_hz": 1000, "symbols": "release debug=1; presymbolicated sidecar"})
    with (evidence / "samples.jsonl").open("xb") as stream:
        for name in args.scenarios or ("check", "inspect"):
            for pair in range(args.repetitions or 3):
                if time.monotonic() - started >= args.stage_seconds:
                    break
                run_profile_pair(args, build, fixture, evidence, stream, name, pair)
            if time.monotonic() - started < args.stage_seconds:
                scenario = prepare_scenario(fixture, name, evidence / f"counter-{name}")
                scenario["timeout"] = args.timeout
                record_git_calls(scenario, build, evidence / f"git-{name}")
    from report import write_report
    write_report(evidence)


def run_profile_pair(args, build: dict, fixture: dict, evidence: Path, stream, name: str, pair: int):
    """Alternate pair order; keep input-reset and correctness outside all timers."""
    from run import append_record
    order = ("none", "samply") if pair % 2 == 0 else ("samply", "none")
    for mode in order:
        verify_build(build)
        case = prepare_scenario(fixture, name, evidence / f"{name}-{pair}-{mode}")
        case["timeout"] = args.timeout
        if mode == "samply":
            profile = record_profile(case, build, evidence / f"capture-{name}-{pair}")
            record = profile["measurement"]
            record["profile_outcome"] = profile["outcome"]
        else:
            record = measure_process([build["executable"], *case["argv"]], case["repository"],
                                     case["stdin"], {"timeout": args.timeout})
        record.update(name=name, nodes=args.nodes, phase="measured", embedding_mode="fts_disabled",
                      profile_mode=mode, validation=validate_result(case, record))
        append_record(stream, record)
        print(json.dumps({"scenario": name, "pair": pair, "mode": mode,
                          "seconds": record["duration_seconds"], "valid": record["validation"]["valid"]}), flush=True)
