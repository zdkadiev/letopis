"""Explicit ownership of fake-vector work and controlled reader/writer schedules."""

from concurrent.futures import ThreadPoolExecutor
from collections.abc import Callable
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import threading
import time

from measure import measure_process
from provider import start_provider
from scenarios import (ROOT, encode_json, get_environment, hash_file, initialize_projection,
                       prepare_fixture, read_product_inputs, run_git, verify_new_output, write_json)


class ContentionInterrupted(KeyboardInterrupt):
    """Carry the durably recorded interrupted schedule to its campaign owner."""

    def __init__(self, record: dict):
        super().__init__("contention schedule interrupted")
        self.record = record


def run_canary(binary: Path, root: Path) -> dict:
    """Prove vector-only retrieval through real CLI and a directly owned drain."""
    verify_new_output(root)
    root.mkdir(parents=True)
    repository = root / "repository"
    repository.mkdir()
    run_git(repository, "init", "--quiet", "--template=", "--initial-branch=main")
    for filename, body in (("a.md", "document alpha"), ("b.md", "document beta")):
        (repository / filename).write_text('<!-- letopis: {"created_at":1} -->\n# Fixture\n' + body + "\n")
    (repository / ".gitignore").write_text(".letopis/\n")
    run_git(repository, "add", ".")
    run_git(repository, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "-qm", "Vector-only canary")
    (repository / ".letopis").mkdir(mode=0o700)
    configure_provider(repository, "http://127.0.0.1:1/api", False)
    _, fts = invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "vectorprobe", "limit": 1}})
    with start_provider(root / "provider") as provider:
        configure_provider(repository, provider.url, True)
        with embed_lease(repository):
            invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "", "limit": 1}})
            # A direct drain must observe the held product-compatible kernel lock.
            invoke_cli(binary, repository, {"args": ["embed", "--drain"], "request": {}})
            interoperable = provider.get_counters()["requests"] == 0
            if not interoperable:
                raise ValueError("embed lock interoperability failed")
        drain, _ = invoke_cli(binary, repository, {"args": ["embed", "--drain"], "request": {}})
        before = provider.get_counters()["requests"]
        _, vector = invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "vectorprobe", "limit": 1}})
        delta = provider.get_counters()["requests"] - before
        _, checked = invoke_cli(binary, repository, {"args": ["check"], "request": {}})
        assert_embed_released(repository)
        receipt = {"fts_ids": [item["element_id"] for item in fts],
                   "vector_ids": [item["element_id"] for item in vector], "drain": drain,
                   "query_request_delta": delta, "embed_lock_interoperable": interoperable,
                   "final_check": checked, "provider": provider.get_counters()}
        if receipt["fts_ids"] or receipt["vector_ids"] != ["b.md"] or delta < 1:
            raise ValueError("vector-only canary was not demonstrated")
        write_json(root / "receipt.json", receipt)
        return receipt


def run_contention(scenario: dict, build: dict, evidence: Path,
                   *, verify: Callable[[], None] | None = None) -> dict:
    """Persist each owned subcall before continuing; preserve and propagate cancellation."""
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    try:
        if verify is not None:
            verify()
        baseline = measure_contention_call(scenario, build, evidence, {"role": "baseline"})
        if baseline["outcome"] == "interrupted":
            raise KeyboardInterrupt
        reader, written, index_locked = coordinate_reindex(scenario, build, evidence, verify)
    except KeyboardInterrupt:
        record = preserve_interrupted_contention(evidence)
        raise ContentionInterrupted(record) from None
    receipt = build_contention_receipt(scenario, baseline, reader, written, index_locked)
    write_json(evidence / "receipt.json", receipt)
    return receipt


def coordinate_reindex(scenario, build, evidence, verify):
    cancel = threading.Event()
    with ThreadPoolExecutor(max_workers=1) as executor:
        if verify is not None:
            verify()
        writer = executor.submit(measure_contention_call, scenario, build, evidence,
                                 {"role": "writer", "cancel_event": cancel})
        try:
            index_locked = wait_for_index_writer(scenario["repository"], writer)
            if writer.done() and writer.result()["outcome"] == "interrupted":
                raise KeyboardInterrupt
            if verify is not None:
                verify()
            reader = measure_contention_call(scenario, build, evidence, {"role": "reader"})
            if reader["outcome"] == "interrupted":
                raise KeyboardInterrupt
            written = writer.result()
            if written["outcome"] == "interrupted":
                raise KeyboardInterrupt
        finally:
            cancel.set()
    return reader, written, index_locked


def measure_contention_call(scenario, build, evidence, options):
    """Publish an exclusive, synced subcall receipt before its result is consumed."""
    role = options["role"]
    argv = ["reindex"] if role == "writer" else scenario["argv"]
    data = b"{}\n" if role == "writer" else scenario["stdin"]
    settings = {"timeout": scenario.get("timeout", 120), "cancel_event": options.get("cancel_event")}
    record = measure_process([build["executable"], *argv], scenario["repository"], data, settings)
    record["contention_role"] = role
    with (evidence / f"{role}.json").open("xb") as stream:
        stream.write(encode_json(record))
        stream.flush()
        os.fsync(stream.fileno())
    return record


def preserve_interrupted_contention(evidence):
    """Retain all finished/cleaned subcalls and identify the censored schedule."""
    calls = {role: json.loads(path.read_text()) for role in ("baseline", "reader", "writer")
             if (path := evidence / f"{role}.json").exists()}
    interrupted = [call for call in calls.values() if call["outcome"] == "interrupted"]
    record = dict(interrupted[0] if interrupted else next(reversed(calls.values()),
                  {"duration_seconds": 0, "resources": {"unavailable": "no completed subcall"}}))
    record.update(outcome="interrupted", validation={"valid": False, "reason": "contention interrupted"},
                  contention_receipt=str(evidence / "receipt.json"))
    write_json(evidence / "receipt.json", {"interrupted": True, "subcalls": calls, "attempt": record})
    return record


def build_contention_receipt(scenario, baseline, reader, written, index_locked):
    overlap = max(0., min(reader["started_monotonic"] + reader["duration_seconds"],
                         written["started_monotonic"] + written["duration_seconds"])
                  - max(reader["started_monotonic"], written["started_monotonic"]))
    from report import validate_result
    duration = (max(reader["started_monotonic"] + reader["duration_seconds"],
                    written["started_monotonic"] + written["duration_seconds"])
                - min(reader["started_monotonic"], written["started_monotonic"]))
    return {"reader": reader, "writer": written, "baseline_reader": baseline,
               "baseline_validation": validate_result(scenario, baseline),
               "overlap_seconds": overlap, "writer_index_lock_observed": index_locked,
               "throughput_success_per_s": sum(call["outcome"] == "success" for call in (reader, written)) / duration,
               "reader_validation": validate_result(scenario, reader),
               "writer_validation": validate_result({"expected": {"command": "reindex"}}, written),
               "scope": "CLI lifetime overlap; no direct SQL lock-wait attribution"}


def wait_for_index_writer(repository: Path, writer) -> bool:
    """Observe this isolated writer's index lock without stealing or unlinking it."""
    deadline = time.monotonic() + 10
    with (repository / ".letopis/index.lock").open("a+b") as stream:
        while not writer.done() and time.monotonic() < deadline:
            try:
                fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
                fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
            except BlockingIOError:
                return True
            time.sleep(.005)
    return False


def run_campaign(args, build: dict, evidence: Path) -> None:
    """Assess owned drain/reindex overlap and current fake-vector coverage."""
    from scenarios import prepare_scenario
    started = time.monotonic()
    canary = run_canary(Path(build["executable"]), evidence / "canary")
    write_json(evidence / "canary.json", canary)
    fixture = prepare_fixture(evidence / "fixture", args.nodes)
    initialize_projection(fixture["repository"], build["executable"], full_audit=False)
    fixture["binary"] = build["executable"]
    scenario = prepare_scenario(fixture, "search", evidence / "concurrent-reindex")
    scenario["timeout"] = min(args.timeout, args.stage_seconds - (time.monotonic() - started))
    run_contention(scenario, build, evidence / "reindex-overlap")
    observer = build_observer(ROOT, ROOT / "target", evidence / "observer-build")
    with start_provider(evidence / "provider") as provider:
        repository = fixture["repository"]
        configure_provider(repository, provider.url, True)
        with embed_lease(repository):
            invoke_cli(Path(build["executable"]), repository,
                       {"args": ["search"], "request": {"query": "", "limit": 1}})
        scenario = {"repository": repository, "argv": ["search"],
                    "stdin": b'{"query":"\\"case 000000\\"","limit":10}\n',
                    "expected": {"command": "search", "id": "knowledge/part-0000.md#node-000000",
                                 "counts": fixture["counts"]}, "timeout": args.timeout}
        run_drain_overlap(scenario, build, provider, evidence / "drain-overlap")
        observe_vectors(scenario, Path(observer["executable"]), evidence / "coverage")
        before = provider.get_counters()["requests"]
        record, result = invoke_cli(Path(build["executable"]), repository,
                               {"args": ["search"], "request": {"query": "vectorprobe", "limit": 1}})
        delta = provider.get_counters()["requests"] - before
        record.update(embedding_mode="fake_vectors", scope="client/storage overhead, not semantic model performance")
        record.update(query_request_delta=delta, validation={"valid": delta >= 1 and len(result) == 1,
                                                           "fts_query_absent_by_fixture": True})
        write_json(evidence / "fake-query.json", record)
        if not record["validation"]["valid"]:
            raise ValueError("fake-vector query participation not demonstrated")
        _, checked = invoke_cli(Path(build["executable"]), repository, {"args": ["check"], "request": {}})
        if checked["errors"]:
            raise ValueError("post-contention integrity failure")
        write_json(evidence / "integrity.json", checked)


def run_drain_overlap(scenario: dict, build: dict, provider, evidence: Path) -> dict:
    """Supervise early writer exit and own any detached fixture successor."""
    from ownership import FixtureProcesses
    from report import validate_result
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    with FixtureProcesses(Path(build["executable"]), evidence / "processes") as owner:
        owned = dict(build, executable=str(owner.get_executable()))
        result = coordinate_drain(scenario, owned, provider)
        stopped = owner.stop_all()
        reader, writer = result["reader"], result["writer"]
        validation = (validate_result(scenario, reader) if reader
                      else {"valid": False, "reason": "reader did not start"})
        overlap = (min(reader["duration_seconds"], max(0., writer["started_monotonic"]
                   + writer["duration_seconds"] - reader["started_monotonic"])) if reader else 0.)
        assert_embed_released(scenario["repository"])
        result.update(reader_validation=validation, overlap_seconds=overlap,
                      scope="reader during paused inference, not exact SQLite wait time",
                      provider=provider.get_counters(), owned_embed_lock_released=True,
                      detached_cleanup_pids=stopped)
        write_json(evidence / "receipt.json", result)
    if result["interrupted"] or (reader and reader["outcome"] == "interrupted" and not result["early_writer_exit"]):
        raise KeyboardInterrupt
    if (result["error_type"] or result["early_writer_exit"] or stopped or not validation["valid"]
            or (not scenario.get("cancel_writer") and writer["outcome"] != "success")):
        raise ValueError("drain/reader schedule aborted or failed correctness")
    return result


def coordinate_drain(scenario: dict, build: dict, provider) -> dict:
    """Stop/reap readers before requesting writer cleanup, including exception paths."""
    reader_cancel, writer_cancel = threading.Event(), threading.Event()
    reader = None
    read_record, write_record = None, None
    early, interrupted, error_type = False, False, None
    provider.arm_barrier(timeout=scenario.get("timeout", 120) + 5)
    with ThreadPoolExecutor(max_workers=2) as executor:
        writer = executor.submit(measure_process, [build["executable"], "embed", "--drain"],
                                 scenario["repository"], b"{}\n",
                                 {"timeout": scenario.get("timeout", 120), "cancel_event": writer_cancel})
        try:
            if not await_inference(provider, writer):
                early = writer.done()
                raise ValueError("owned drain did not reach inference barrier")
            reader = executor.submit(measure_process, [build["executable"], *scenario["argv"]],
                                     scenario["repository"], scenario["stdin"],
                                     {"timeout": scenario.get("timeout", 120), "cancel_event": reader_cancel})
            early = await_reader(reader, writer, reader_cancel)
            read_record = reader.result()
            if scenario.get("cancel_writer"):
                writer_cancel.set()
            provider.release.set()
            write_record = writer.result()
        except KeyboardInterrupt:
            interrupted = True
        except Exception as error:
            error_type = type(error).__name__
        finally:
            reader_cancel.set()
            if reader is not None:
                read_record = reader.result()
            writer_cancel.set()
            provider.release.set()
            write_record = writer.result()
    return {"reader": read_record, "writer": write_record, "early_writer_exit": early,
            "interrupted": interrupted, "error_type": error_type}


def await_reader(reader, writer, cancellation: threading.Event) -> bool:
    while not reader.done():
        if writer.done():
            cancellation.set()
            return True
        time.sleep(.005)
    return False


def await_inference(provider, writer) -> bool:
    deadline = time.monotonic() + 10
    while not provider.arrived.is_set() and time.monotonic() < deadline:
        if writer.done():
            return False
        provider.arrived.wait(timeout=.005)
    return provider.arrived.is_set()


def build_observer(source: Path, target: Path, evidence: Path) -> dict:
    """Build the existing read-only native observer; compilation is never timed."""
    verify_new_output(evidence)
    evidence.mkdir(parents=True)
    environment = get_environment()
    environment.update(CARGO_TARGET_DIR=str(target), CARGO_PROFILE_RELEASE_DEBUG="1")
    command = ["cargo", "test", "--release", "--locked", "--offline", "--test", "target_corpus",
               "--no-run", "--message-format=json"]
    with (evidence / "cargo.log").open("wb") as errors:
        result = subprocess.run(command, cwd=source, env=environment, stdout=subprocess.PIPE,
                                stderr=errors, timeout=600, check=True)
    messages = [json.loads(line) for line in result.stdout.splitlines()]
    artifact = next(item for item in reversed(messages) if item.get("executable")
                    and item.get("target", {}).get("name") == "target_corpus")
    path = Path(artifact["executable"])
    receipt = {"executable": str(path), "sha256": hash_file(path), "inputs": read_product_inputs(source),
               "observer_inputs": {str(p.relative_to(source)): hash_file(p)
                                   for p in sorted((source / "tests/support").rglob("*.rs"))},
               "entrypoint_hash": hash_file(source / "tests/target_corpus.rs")}
    write_json(evidence / "build.json", receipt)
    return receipt


def observe_vectors(scenario: dict, observer: Path, evidence: Path) -> dict:
    """Use native vec1 read-only checks; a Python SQLite connection lacks vec1."""
    counts = scenario["expected"]["counts"]
    evidence.mkdir(parents=True)
    output = evidence / "observation.json"
    request = evidence / "request.json"
    write_json(request, {"root": str(scenario["repository"]), "output": str(output),
                         "expected_knowledge": counts["knowledge"], "expected_sources": counts["source"],
                         "expected_edges": counts["edges"]})
    environment = get_environment()
    environment["LETOPIS_CORPUS_REQUEST"] = str(request)
    record = measure_process([str(observer), "observe_existing_corpus", "--ignored", "--exact"],
                             scenario["repository"], b"", {"environment": environment, "timeout": 120})
    write_json(evidence / "process.json", record)
    if record["outcome"] != "success" or not output.exists():
        raise ValueError("native vector observation failed")
    receipt = json.loads(output.read_text())
    verify_coverage(receipt, counts["knowledge"])
    return receipt


def verify_coverage(receipt: dict, expected_nodes: int) -> None:
    """Accept only a complete native observation of current config/hash coverage."""
    if (receipt["covered_nodes"] != expected_nodes or receipt["knowledge_nodes"] != expected_nodes
            or receipt["missing_nodes"] or receipt["eligible_hashes"] or receipt["invalid_files"]
            or receipt["dimensions"] != 3 or receipt["verdict"] != "populated"
            or not re.fullmatch(r"[0-9a-f]{64}", receipt["config_identity"])):
        raise ValueError("current vector coverage is incomplete")


def configure_provider(repository: Path, url: str, enabled: bool) -> None:
    """Replace only this generated fixture's config, never a user's config."""
    config = {"embedding_model": {"enabled": enabled, "provider": "ollama", "model": "performance-fixture",
                                  "parameters": {"base_url": url}, "max_retries": 0}}
    (repository / ".letopis/config.json").write_bytes(encode_json(config))


def invoke_cli(binary: Path, repository: Path, invocation: dict) -> tuple[dict, object]:
    record = measure_process([str(binary), *invocation["args"]], repository,
                             encode_json(invocation["request"]), {"timeout": invocation.get("timeout", 120)})
    if record["outcome"] == "interrupted":
        raise KeyboardInterrupt
    if record["outcome"] != "success" or record["stdout"]["truncated"]:
        raise ValueError("CLI invocation failed")
    response = json.loads(bytes.fromhex(record["stdout"]["capture_hex"]))
    if response.get("ok") is not True or response.get("warnings"):
        raise ValueError("CLI returned error/warning")
    return record, response["result"]


@contextmanager
def embed_lease(repository: Path):
    """Own the persistent embed lock inode; never unlink or replace it."""
    with (repository / ".letopis/embed.lock").open("a+b") as stream:
        fcntl.flock(stream.fileno(), fcntl.LOCK_EX)
        yield
        fcntl.flock(stream.fileno(), fcntl.LOCK_UN)


def assert_embed_released(repository: Path) -> None:
    with (repository / ".letopis/embed.lock").open("a+b") as stream:
        fcntl.flock(stream.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
        fcntl.flock(stream.fileno(), fcntl.LOCK_UN)
