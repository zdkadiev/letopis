"""Compare frozen sequential/one-worker/four-worker products with a controlled provider."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import ExitStack
import json
import os
from pathlib import Path
import platform
import shutil
import signal
import sqlite3
import statistics
import subprocess
import threading
import time

from compare import verify_frozen_build
from contention import embed_lease, invoke_cli
from drain_provider import DrainProvider, MODEL, QUERY_TEXT
from measure import measure_process
from ownership import FixtureProcesses
from report import summarize_samples
from run import append_record, capture_harness, get_cpu_model
from scenarios import (ROOT, encode_json, get_environment, hash_file, read_product_inputs,
                       verify_new_output, write_json)

VARIANTS = ("before", "one-worker", "after")
LIMITS = {"before": 1, "one-worker": 1, "after": 4}


class DrainInterrupted(KeyboardInterrupt):
    def __init__(self, record: dict):
        super().__init__("owned drain interrupted; evidence retained")
        self.record = record


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--before", type=Path, required=True)
    parser.add_argument("--one-worker", type=Path, required=True)
    parser.add_argument("--after", type=Path, required=True)
    parser.add_argument("--evidence", type=Path, required=True)
    args = parser.parse_args()
    result = run_campaign(args)
    print(json.dumps({"outcome": result["outcome"], "summary": str(args.evidence.absolute() / "summary.json")}))
    if result["outcome"] != "complete":
        raise SystemExit(1)


def run_campaign(args: argparse.Namespace) -> dict:
    started = time.monotonic()
    evidence = args.evidence.absolute()
    prepare_campaign(evidence)
    builds = {label: json.loads(getattr(args, label.replace("-", "_")).read_text()) for label in VARIANTS}
    for build in builds.values():
        verify_frozen_build(build)
    if len({build["rustc"] for build in builds.values()}) != 1:
        raise ValueError("all variants require the same toolchain")
    if read_product_inputs(ROOT) != builds["after"]["inputs"]:
        raise ValueError("observer source must match the frozen final product")
    spec = build_corpus_spec()
    observer = build_observer(builds["after"], evidence / "observer")
    write_json(evidence / "manifest.json", {"builds": builds, "corpus": spec,
        "host": {"platform": platform.platform(), "machine": platform.machine(), "cpu": get_cpu_model(),
                 "load_average": os.getloadavg()}, "harness_inputs": capture_harness(evidence),
        "observer": observer, "conditioning_per_variant": 1, "measured_per_variant": 5,
        "service_delay_seconds": .025, "query_overlap_delay_seconds": .1,
        "cache_state": "fresh initialized projections; OS page cache uncontrolled",
        "scope": "synthetic native-protocol provider; no model or external inference"})
    records, outcome, expired = [], "complete", False
    def stop_stage(*_):
        nonlocal expired
        expired = True
        raise KeyboardInterrupt
    previous = signal.signal(signal.SIGALRM, stop_stage)
    signal.setitimer(signal.ITIMER_REAL, max(.001, 1200 - (time.monotonic() - started)))
    try:
        with ExitStack() as stack, (evidence / "samples.jsonl").open("xb") as stream:
            provider = stack.enter_context(DrainProvider(spec["by_text"], evidence / "provider"))
            owners = {label: stack.enter_context(FixtureProcesses(Path(builds[label]["executable"]),
                       evidence / "ownership" / label)) for label in VARIANTS}
            cases = [(label, "conditioning" if number == 0 else "measured", number)
                     for number in range(6) for label in VARIANTS]
            cases += [(label, "query-overlap", 0) for label in VARIANTS]
            for label, kind, number in cases:
                case = {"variant": label, "kind": kind, "number": number, "spec": spec,
                        "observer": observer, "build": builds[label]}
                output = evidence / "cases" / f"{kind}-{number}-{label}"
                try:
                    record = measure_case(owners[label], provider, case, output)
                except DrainInterrupted as error:
                    record = error.record
                records.append(record)
                append_record(stream, record)
                if not record["validation"]["valid"]:
                    outcome = "incomplete"
    except KeyboardInterrupt:
        outcome = "incomplete" if expired else "interrupted"
    except Exception as error:
        outcome = "incomplete"
        write_json(evidence / "failure.json", {"error_type": type(error).__name__, "message": str(error)})
    finally:
        signal.setitimer(signal.ITIMER_REAL, 0)
        signal.signal(signal.SIGALRM, previous)
    result = build_summary(records, outcome)
    write_json(evidence / "summary.json", result)
    return result


def build_corpus_spec() -> dict:
    texts, vectors, by_text = {}, {}, {}
    for number in range(64):
        text = f"SyntheticDrainText{number:03} " + "detail " * 120
        text = text.rstrip()
        vector = [0., 1., 0.] if number == 0 else [float(number + 1), 1., 0.]
        relative = f"knowledge/doc-{number:03}.md"
        texts[relative], vectors[relative], by_text[text] = text, vector, vector
    for number in range(4):
        relative = f"knowledge/shared-{number}.md"
        texts[relative] = texts["knowledge/doc-000.md"]
        vectors[relative] = [0., 1., 0.]
    return {"texts": texts, "vectors": vectors, "by_text": by_text,
            "knowledge_nodes": 68, "unique_vectors": 64, "shared_extra_nodes": 4}


def build_observer(build: dict, evidence: Path) -> dict:
    prepare_campaign(evidence)
    environment = get_environment()
    environment.update(CARGO_TARGET_DIR=build["target"], CARGO_PROFILE_RELEASE_DEBUG="1")
    command = ["cargo", "test", "--release", "--locked", "--offline", "--test",
               "drain_measurement", "--no-run", "--message-format=json"]
    with (evidence / "cargo.log").open("xb") as errors:
        result = subprocess.run(command, cwd=ROOT, env=environment, stdout=subprocess.PIPE,
                                stderr=errors, timeout=600, check=True)
    messages = [json.loads(line) for line in result.stdout.splitlines()]
    artifact = next(message["executable"] for message in messages
                    if message.get("reason") == "compiler-artifact" and message.get("executable")
                    and message["target"]["name"] == "drain_measurement")
    binary = evidence / "drain-observer"
    shutil.copy2(artifact, binary)
    binary.chmod(0o555)
    source = ROOT / "tests/drain_measurement.rs"
    shutil.copy2(source, evidence / "source.rs")
    receipt = {"executable": str(binary), "sha256": hash_file(binary),
               "source_sha256": hash_file(source), "product_inputs": read_product_inputs(ROOT),
               "fixture_helper_sha256": hash_file(ROOT / "tests/support/storage.rs"), "command": command}
    write_json(evidence / "build.json", receipt)
    return receipt


def measure_case(owner: FixtureProcesses, provider: DrainProvider, case: dict, evidence: Path) -> dict:
    prepare_campaign(evidence)
    verify_frozen_build(case["build"])
    binary = owner.get_executable()
    if hash_file(binary) != case["build"]["sha256"]:
        raise ValueError("private executable differs from frozen product")
    inputs = prepare_fixture(evidence / "repository", binary, case["spec"], provider.url)
    write_json(evidence / "inputs.json", inputs)
    overlap = case["kind"] == "query-overlap"
    provider.begin_window(evidence.name, .1 if overlap else .025)
    interrupted, query_error, queries = False, None, []
    try:
        if overlap:
            record, queries, query_error = run_overlap(binary, evidence / "repository", evidence / "drain")
        else:
            record = run_drain(binary, evidence / "repository", evidence / "drain")
    except KeyboardInterrupt:
        interrupted = True
        record = json.loads((evidence / "drain/process.json").read_text())
    counters = provider.get_counters()
    write_json(evidence / "provider.json", counters)
    expected = {"vectors": case["spec"]["vectors"], "unique_vectors": 64,
                "document_requests": 64, "query_requests": 3 if overlap else 0,
                "concurrency_limit": LIMITS[case["variant"]]}
    observed, validation = {}, {"valid": False, "reason": "interrupted"}
    if not interrupted:
        try:
            observed = observe_fixture(case["observer"], evidence / "repository", expected, evidence)
            validate_sample(record, observed, expected, counters)
            if query_error or (overlap and counters["queries_during_document_work"] < 1):
                raise ValueError(query_error or "no query overlapped document work")
            validation = {"valid": True}
        except Exception as error:
            validation = {"valid": False, "reason": str(error), "error_type": type(error).__name__}
    record.update(variant=case["variant"], kind=case["kind"], number=case["number"],
                  input_sha256=inputs["sha256"], completed_vectors=observed.get("unique_vectors", 0),
                  provider=counters, queries=queries, validation=validation)
    write_json(evidence / "receipt.json", record)
    if interrupted:
        raise DrainInterrupted(record)
    return record


def prepare_fixture(root: Path, binary: Path, spec: dict, url: str) -> dict:
    root.mkdir()
    environment = get_environment()
    environment.update(GIT_AUTHOR_DATE="2000-01-01T00:00:00+0000", GIT_COMMITTER_DATE="2000-01-01T00:00:00+0000")
    commands = [["init", "--object-format=sha1", "-q"], ["add", "--", ".gitignore", "knowledge"],
                ["-c", "user.name=Drain fixture", "-c", "user.email=fixture@example.invalid", "commit", "-qm", "synthetic corpus"]]
    (root / "knowledge").mkdir()
    (root / ".gitignore").write_text(".letopis/\n")
    for relative, text in spec["texts"].items():
        (root / relative).write_text('<!-- letopis: {"created_at":1} -->\n' + text)
    for command in commands:
        subprocess.run(["git", *command], cwd=root, env=environment, check=True,
                       stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=30)
    (root / ".letopis").mkdir(mode=0o700)
    write_config(root, url, False)
    with embed_lease(root):
        initialization, _ = invoke_cli(binary, root, {"args": ["reindex"], "request": {}})
    write_json(root.parent / "initialization.json", initialization)
    with sqlite3.connect(root / ".letopis/cache.db") as connection:
        if connection.execute("SELECT count(*) FROM nodes").fetchone()[0] != 68:
            raise ValueError("synthetic projection is incomplete before measurement")
    write_config(root, url, True)
    names = [*spec["texts"], ".gitignore", ".letopis/config.json"]
    return {"sha256": {name: hash_file(root / name) for name in names},
            "bytes": sum((root / name).stat().st_size for name in spec["texts"]),
            "knowledge_nodes": 68, "unique_texts": 64}


def write_config(root: Path, url: str, enabled: bool) -> None:
    # This configuration belongs to the new private fixture, rather than immutable evidence.
    config = {"embedding_model": {"enabled": enabled, "provider": "ollama", "model": MODEL,
                               "parameters": {"base_url": url}, "max_retries": 0}}
    (root / ".letopis/config.json").write_bytes(encode_json(config))


def run_overlap(binary: Path, root: Path, evidence: Path) -> tuple:
    cancel = threading.Event()
    executor = ThreadPoolExecutor(max_workers=1)
    future = executor.submit(run_drain, binary, root, evidence, {"cancel_event": cancel})
    queries, failure, interrupted = [], None, False
    try:
        wait_canary(root, future)
        for number in range(3):
            record, result = invoke_cli(binary, root, {"args": ["search"],
                "request": {"query": QUERY_TEXT, "limit": 1}})
            write_json(evidence.parent / f"query-{number}.json", record)
            if not isinstance(result, list) or not result or result[0]["element_id"] != "knowledge/doc-000.md":
                raise ValueError("synthetic vector-only query returned the wrong knowledge")
            queries.append(record)
    except KeyboardInterrupt:
        interrupted = True
        cancel.set()
    except Exception as error:
        failure = str(error)
    finally:
        try:
            record = future.result(timeout=125)
        except KeyboardInterrupt:
            interrupted = True
            record = json.loads((evidence / "process.json").read_text())
        finally:
            cancel.set()
            executor.shutdown(wait=True)
    if interrupted:
        raise KeyboardInterrupt
    return record, queries, failure


def wait_canary(root: Path, future) -> None:
    deadline = time.monotonic() + 5
    while time.monotonic() < deadline:
        with sqlite3.connect(root / ".letopis/cache.db") as connection:
            connection.execute("ATTACH DATABASE ? AS vec", (str(root / ".letopis/vectors.db"),))
            ready = connection.execute("""SELECT 1 FROM nodes n JOIN vec.vector_keys k
                ON n.search_hash = k.search_hash WHERE n.id = ? LIMIT 1""",
                ("knowledge/doc-000.md",)).fetchone()
        if ready:
            return
        if future.done():
            raise ValueError("drain finished before vector canary became ready")
        time.sleep(.005)
    raise ValueError("vector canary did not become ready in the bounded overlap window")


def observe_fixture(observer: dict, root: Path, expected: dict, evidence: Path) -> dict:
    binary = Path(observer["executable"])
    if hash_file(binary) != observer["sha256"]:
        raise ValueError("native observer changed")
    output, request = evidence / "observation.json", evidence / "observation-request.json"
    write_json(request, {"root": str(root), "expected": expected["vectors"], "output": str(output)})
    environment = get_environment()
    environment["LETOPIS_DRAIN_OBSERVATION"] = str(request)
    record = measure_process([str(binary), "observe_existing_drain", "--ignored", "--exact"], root, b"",
                             {"timeout": 120, "environment": environment})
    write_json(evidence / "observation-process.json", record)
    if record["outcome"] != "success" or not output.exists():
        raise ValueError("native literal-vector observation failed")
    return json.loads(output.read_text())


def build_summary(records: list[dict], outcome: str) -> dict:
    result = {"outcome": outcome, "variants": {}, "real_provider": "not assessed; separate authorization required"}
    identities = {json.dumps(row["input_sha256"], sort_keys=True) for row in records}
    if len(identities) > 1:
        result["outcome"] = "incomplete"
        result["input_parity_error"] = True
    for label in VARIANTS:
        samples = [row for row in records if row["variant"] == label and row["kind"] == "measured"]
        overlap = [row for row in records if row["variant"] == label and row["kind"] == "query-overlap"]
        summary = summarize(samples)
        summary["max_document_active"] = max((row["provider"]["max_document_active"] for row in samples), default=None)
        summary["document_requests"] = [row["provider"]["document_requests"] for row in samples]
        summary["overlap"] = overlap
        result["variants"][label] = summary
        if len(samples) != 5 or len(overlap) != 1 or any(not row["validation"]["valid"] for row in samples + overlap):
            result["outcome"] = "incomplete"
    return result


def prepare_campaign(evidence: Path) -> None:
    """Refuse artifact reuse before creating a private campaign directory."""
    verify_new_output(evidence)
    evidence.mkdir(parents=True, mode=0o700)


def run_drain(binary: Path, repository: Path, evidence: Path, options: dict | None = None) -> dict:
    """Persist the real owned process before propagating interruption."""
    prepare_campaign(evidence)
    record = measure_process([str(binary), "embed", "--drain"], repository, b"",
                             {"timeout": 120, **(options or {})})
    record["cli_ok"] = False
    if record["outcome"] == "success" and not record["stdout"]["truncated"]:
        try:
            response = json.loads(bytes.fromhex(record["stdout"]["capture_hex"]))
            record["cli_ok"] = response.get("ok") is True and not response.get("warnings")
        except (ValueError, TypeError, AttributeError):
            pass
    write_json(evidence / "process.json", record)
    if record["outcome"] == "interrupted":
        raise KeyboardInterrupt
    return record


def summarize(records: list[dict]) -> dict:
    """Keep failed/censored time in throughput and avoid unsupported quantiles."""
    common = summarize_samples(records)
    occupied = sum(record["duration_seconds"] for record in records)
    vectors = sum(record.get("completed_vectors", 0) for record in records)
    rss = [record.get("resources", {}).get("peak_rss", {}).get("value") for record in records]
    rss = [value for value in rss if value is not None]
    return {"attempts": common["attempts"], "successes": common["n"], "failures": common["failures"],
            "seconds_total": occupied, "completed_vectors": vectors,
            "vectors_per_second": vectors / occupied if occupied else None,
            "median_seconds": common["median"], "min_seconds": common["min"], "max_seconds": common["max"],
            "p95_seconds": common["p95"], "p99_seconds": common["p99"],
            "peak_rss_bytes": max(rss) if rss else None,
            "median_peak_rss_bytes": statistics.median(rss) if rss else None,
            "rss_scope": "direct drain process lifetime peak; not simultaneous whole-stack RAM"}


def validate_sample(record: dict, observed: dict, expected: dict, provider: dict) -> None:
    """Validate process, literal vectors, attempts and the document-only cap."""
    if record.get("outcome") != "success" or record.get("cli_ok") is not True:
        raise ValueError("drain process did not complete successfully")
    if (observed.get("vectors") != expected["vectors"]
            or observed.get("unique_vectors") != expected["unique_vectors"]
            or observed.get("missing") != [] or observed.get("failed_hashes") != 0
            or observed.get("query_error") is not None):
        raise ValueError("native vector observation differs from the literal fixture")
    if (provider.get("document_requests") != expected["document_requests"]
            or provider.get("query_requests") != expected.get("query_requests", 0)
            or provider.get("failures") != 0):
        raise ValueError("provider request/attempt accounting differs from the fixture")
    maximum = provider.get("max_document_active", 0)
    if not 1 <= maximum <= expected["concurrency_limit"]:
        raise ValueError("document concurrency exceeds its bound or was not observed")
    if expected["concurrency_limit"] > 1 and maximum < 2:
        raise ValueError("parallel document work was not observed")


if __name__ == "__main__":
    main()
