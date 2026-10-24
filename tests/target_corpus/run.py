#!/usr/bin/env python3
"""Run isolated target-corpus CLI phases; retain immutable pins and safe receipts."""

import argparse
from concurrent.futures import ThreadPoolExecutor
from contextlib import contextmanager
from dataclasses import dataclass
import fcntl
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import signal
import sqlite3
import stat
import subprocess
import sys
import time
import urllib.request
import uuid

from validate import validate_corpus

MODEL = "embeddinggemma:300m-qat-q4_0"
BASE_URL = "http://127.0.0.1:11434/api"
CAPTURE_LIMIT = 16384
ERROR_CODES = {"invalid_request", "not_found", "validation_failed", "embedding_failed",
               "internal_error", "cancelled"}


@dataclass(frozen=True)
class _ProcessRequest:
    command: list[str]
    repository: Path
    environment: dict
    data: bytes
    timeout: float | None


def main(arguments: list[str] | None = None) -> int:
    """Execute one phase, exclusively creating evidence, including failure receipts."""
    options = _parse_arguments(arguments)
    try:
        _verify_path(options.evidence, exists=False)
        options.evidence.mkdir(parents=True, exist_ok=False)
    except (OSError, ValueError):
        print("runner failed: evidence must be new and nonsymlink", file=sys.stderr)
        return 1
    receipt = {"schema_version": 1, "phase": options.phase, "status": "failed",
               "verdict": "not_assessed", "observation": None, "error": None,
               "platform": platform.platform(), "python": platform.python_version()}
    started = time.monotonic()
    previous = signal.signal(signal.SIGTERM, _interrupt)
    try:
        _execute_phase(options, receipt)
    except (OSError, ValueError, KeyError, TypeError, sqlite3.Error, subprocess.SubprocessError, KeyboardInterrupt) as error:
        # Exception text may include provider bodies or input: retain only its type.
        receipt["error"] = type(error).__name__
    finally:
        signal.signal(signal.SIGTERM, previous)
        receipt["duration_seconds"] = time.monotonic() - started
        _write_json(options.evidence / "runner-receipt.json", receipt)
    print(json.dumps({key: receipt[key] for key in ("phase", "status", "verdict", "error")}))
    return 0 if receipt["status"] in {"backlog", "populated"} else 1


def _parse_arguments(arguments: list[str] | None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("corpus", "binary", "observer", "evidence"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    parser.add_argument("--phase", choices=("load", "drain", "observe"), required=True)
    parser.add_argument("--max-seconds", type=float)
    parser.add_argument("--config", type=Path, help="explicit local Ollama config; pinned on first load")
    options = parser.parse_args(arguments)
    if options.max_seconds is not None and not 0 < options.max_seconds < float("inf"):
        parser.error("--max-seconds must be finite and positive")
    return options


def _interrupt(signum: int, frame: object) -> None:
    raise KeyboardInterrupt


def _execute_phase(options: argparse.Namespace, receipt: dict) -> None:
    for path in (options.corpus, options.binary, options.observer):
        _verify_path(path)
    manifest = validate_corpus(options.corpus)
    repository = options.corpus.parent / "repository"
    service = repository / ".letopis"
    if options.phase == "load":
        service.mkdir(mode=0o700, exist_ok=True)
    _verify_path(service)
    with _own_lock(service / "corpus-runner.lock"):
        pin = _prepare_pin(options, service)
        receipt.update(pin=pin, provenance=_get_provenance(), counts=manifest["counts"],
                       fragment_bytes=manifest["fragment_bytes"], file_bytes=manifest["file_bytes"])
        if options.phase == "drain":
            with _own_lock(service / "embed.lock"):
                receipt["before_observation"] = _observe(options, manifest, pin, "before-")
            process = _run_product(options, repository, ["embed", "--drain"], b"")
            with _own_lock(service / "embed.lock"):
                observation = _observe(options, manifest, pin)
        else:
            with _own_lock(service / "embed.lock"):
                process = None
                if options.phase == "load":
                    if _has_initialized_identity(service / "vectors.db", pin["config_identity"]):
                        receipt["before_observation"] = _observe(options, manifest, pin, "before-")
                    process = _run_product(options, repository, ["search"],
                                           b'{"query":"","limit":1}')
                observation = _observe(options, manifest, pin)
        receipt["observation"] = observation
        if process is not None and process["outcome"] != "success":
            raise ValueError("product command failed or interrupted")
        _set_verdict(options.phase, observation, receipt)


def _prepare_pin(options: argparse.Namespace, service: Path) -> dict:
    path = options.corpus.parent / "runner-pin.json"
    existing = _read_json(path) if path.exists() or path.is_symlink() else None
    if existing is None and options.phase != "load":
        raise ValueError("initial load pin required")
    config = _read_json(options.config) if options.config else (
        existing["config"] if existing else {"provider": "ollama", "model": MODEL,
                                           "parameters": {"base_url": BASE_URL}, "max_retries": 2})
    _verify_config(config)
    config_bytes = _encode_json(config)
    canonical = json.dumps(config, sort_keys=True, separators=(",", ":")).encode("ascii")
    pin = {"schema_version": 1, "corpus_sha256": _hash_file(options.corpus),
           "binaries": {"product": _hash_file(options.binary), "observer": _hash_file(options.observer)},
           "config": config, "config_sha256": hashlib.sha256(config_bytes).hexdigest(),
           "config_identity": hashlib.sha256(canonical).hexdigest(), "model": inspect_model(config)}
    if existing is not None and pin != existing:
        raise ValueError("pinned input, config, model or binary drift")
    config_path = service / "config.json"
    if existing is None:
        if any((service / leaf).exists() or (service / leaf).is_symlink()
               for leaf in ("config.json", "cache.db", "vectors.db")):
            raise ValueError("unowned existing product state")
        _publish_bytes(path, _encode_json(pin))
    if not config_path.exists() and not config_path.is_symlink():
        if options.phase != "load" or any((service / leaf).exists()
                                         for leaf in ("cache.db", "vectors.db")):
            raise ValueError("missing pinned config after product initialization")
        _publish_bytes(config_path, config_bytes)
    _verify_path(config_path)
    if _hash_file(config_path) != pin["config_sha256"]:
        raise ValueError("config bytes differ from durable pin")
    return pin


def _has_initialized_identity(path: Path, expected: str) -> bool:
    """Reject stale identity; permit only provably empty first-initialization metadata.

    This read-only guard never reads vector payloads or validates product coverage;
    matching initialized stores still require the Rust observer before refresh.
    """
    if not path.exists() and not path.is_symlink():
        return False
    _verify_path(path)
    before = path.stat(follow_symlinks=False)
    if not stat.S_ISREG(before.st_mode):
        raise ValueError("vectors database must be a regular file")
    database = sqlite3.connect(path.as_uri() + "?mode=ro", uri=True)
    try:
        database.execute("BEGIN")
        version = database.execute("PRAGMA user_version").fetchone()[0]
        definitions = database.execute("SELECT name FROM sqlite_schema WHERE sql IS NOT NULL").fetchall()
        if version == 0 and not definitions:
            return False
        if version != 1:
            raise ValueError("unsupported existing vectors database")
        identities = database.execute("SELECT id, config_identity FROM meta").fetchall()
        if identities == [(1, expected)]:
            return True
        if identities:
            raise ValueError("stored config identity differs from durable pin")
        occupied = database.execute("""
            SELECT EXISTS(SELECT 1 FROM vector_keys)
                OR EXISTS(SELECT 1 FROM embed_failures)
                OR EXISTS(SELECT 1 FROM query_error)
        """).fetchone()[0]
        if occupied:
            raise ValueError("unconfigured database contains embedding state")
    finally:
        database.close()
        _verify_path(path)
        after = path.stat(follow_symlinks=False)
        if (before.st_dev, before.st_ino) != (after.st_dev, after.st_ino):
            raise ValueError("vectors database identity changed")
    return False


def _get_provenance() -> dict:
    root = Path(__file__).resolve().parents[2]
    paths = [root / name for name in ("Cargo.toml", "Cargo.lock", "build.rs", "rust-toolchain.toml",
                                      "tests/target_corpus.rs", "tests/support/target_corpus.rs")]
    paths.extend((root / "src").rglob("*.rs"))
    paths.extend((root / "tests/target_corpus").glob("*.py"))
    revision = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, env=_get_environment(),
                              capture_output=True, timeout=10, check=True).stdout.decode().strip()
    if not re.fullmatch(r"[0-9a-f]{40}", revision):
        raise ValueError("invalid source revision")
    return {"source_revision": revision,
            "source_sha256": {str(path.relative_to(root)): _hash_file(path) for path in sorted(paths)}}


def _run_product(options: argparse.Namespace, repository: Path, arguments: list[str], data: bytes) -> dict:
    label = "refresh" if arguments == ["search"] else "drain"
    request = _ProcessRequest([str(options.binary), *arguments], repository,
                              _get_environment(), data, options.max_seconds)
    record, output = _run_process(request)
    if record["outcome"] in {"success", "nonzero"}:
        try:
            _verify_response(output, arguments[0], record["exit_code"])
        except (ValueError, KeyError, TypeError, UnicodeError):
            record["outcome"] = "invalid_response"
    _write_json(options.evidence / f"{label}-process.json", record)
    return record


def _observe(options: argparse.Namespace, manifest: dict, pin: dict, prefix: str = "") -> dict:
    repository = options.corpus.parent / "repository"
    request = {"root": str(repository), "output": str(options.evidence / f"{prefix}observation.json"),
               "expected_knowledge": manifest["counts"]["knowledge"],
               "expected_sources": manifest["counts"]["source"],
               "expected_edges": manifest["counts"]["edges"]}
    path = options.evidence / f"{prefix}observer-request.json"
    _write_json(path, request)
    environment = _get_environment()
    environment["LETOPIS_CORPUS_REQUEST"] = str(path)
    command = [str(options.observer), "--ignored", "--exact", "observe_existing_corpus", "--nocapture"]
    record, _ = _run_process(_ProcessRequest(command, repository, environment, b"", 600))
    _write_json(options.evidence / f"{prefix}observer-process.json", record)
    if record["outcome"] != "success":
        raise ValueError("observer failed")
    observation = _read_json(options.evidence / f"{prefix}observation.json")
    _verify_observation(observation, manifest["counts"], pin)
    return observation


def _set_verdict(phase: str, observation: dict, receipt: dict) -> None:
    if phase == "load":
        if observation["covered_nodes"] != 0 or observation["eligible_hashes"] == 0:
            raise ValueError("initial configured backlog was consumed or unavailable")
        receipt["status"] = "backlog"
    elif observation["verdict"] == "populated":
        receipt.update(status="populated", verdict="populated")
    else:
        receipt["status"] = "incomplete"


def _verify_config(config: dict) -> None:
    if (set(config) != {"provider", "model", "parameters", "max_retries"}
            or config["provider"] != "ollama" or config["model"] != MODEL
            or type(config["max_retries"]) is not int or config["max_retries"] != 2
            or set(config["parameters"]) != {"base_url"}
            or not re.fullmatch(r"http://127\.0\.0\.1:[1-9][0-9]{0,4}/api",
                                config["parameters"]["base_url"])):
        raise ValueError("only explicit local pinned Ollama configuration is supported")


def inspect_model(config: dict) -> dict:
    """Inspect installed model metadata without inference, pulls or remote proxies."""
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    try:
        with opener.open(config["parameters"]["base_url"] + "/tags", timeout=10) as response:
            data = response.read(2 * 1024 * 1024 + 1)
        if len(data) > 2 * 1024 * 1024:
            raise ValueError("model inventory too large")
        matches = [item for item in json.loads(data)["models"] if item.get("name") == MODEL]
        if len(matches) != 1:
            raise ValueError("installed model missing or ambiguous")
        model = matches[0]
        if (model.get("remote_host") or model.get("remote_model")
                or not re.fullmatch(r"[0-9a-f]{64}", model.get("digest", ""))):
            raise ValueError("model is remote or has invalid digest")
        return {"name": MODEL, "digest": model["digest"]}
    except (OSError, KeyError, TypeError, UnicodeError) as error:
        raise ValueError("cannot inspect local model") from error


def _verify_response(output: bytes, command: str, exit_code: int) -> None:
    if not output.endswith(b"\n") or output.count(b"\n") != 1:
        raise ValueError("CLI must emit one JSON line")
    response = json.loads(output, object_pairs_hook=_reject_duplicate_keys)
    if not isinstance(response, dict):
        raise ValueError("CLI response must be an object")
    if exit_code == 0:
        expected = [] if command == "search" else {}
        if (response.get("ok") is not True or response.get("result") != expected
                or set(response) - {"ok", "result", "warnings"}):
            raise ValueError("unexpected CLI success shape")
        warnings = response.get("warnings", [])
        if not isinstance(warnings, list):
            raise ValueError("CLI warnings must be an array")
        for problem in warnings:
            _verify_problem(problem)
    elif (exit_code != 1 or response.get("ok") is not False
          or set(response) != {"ok", "error"}
          or not isinstance(response["error"], dict)):
        raise ValueError("unexpected CLI failure shape")
    else:
        _verify_problem(response["error"])


def _verify_problem(problem: dict) -> None:
    if (not isinstance(problem, dict) or set(problem) != {"code", "message", "details"}
            or problem["code"] not in ERROR_CODES or not isinstance(problem["message"], str)
            or not isinstance(problem["details"], dict)):
        raise ValueError("unexpected CLI problem shape")


def _verify_observation(observation: dict, counts: dict, pin: dict) -> None:
    numeric = ("project_revision", "knowledge_nodes", "source_nodes", "edges", "unique_search_hashes",
               "covered_nodes", "missing_nodes", "eligible_hashes", "failed_hashes", "exhausted_hashes",
               "invalid_files")
    if any(type(observation[key]) is not int or observation[key] < 0 for key in numeric):
        raise ValueError("invalid observation counts")
    knowledge = counts["knowledge"]
    if (observation["schema_version"] != 1 or observation["config_identity"] != pin["config_identity"]
            or observation["knowledge_nodes"] != knowledge or observation["source_nodes"] != counts["source"]
            or observation["edges"] != counts["edges"] or observation["invalid_files"] != 0
            or observation["unique_search_hashes"] != knowledge
            or observation["covered_nodes"] + observation["missing_nodes"] != knowledge
            or observation["eligible_hashes"] + observation["exhausted_hashes"] != observation["missing_nodes"]
            or observation["failed_hashes"] > observation["missing_nodes"]):
        raise ValueError("observation differs from pinned corpus or coverage invariants")
    dimensions = observation["dimensions"]
    if dimensions is not None and (type(dimensions) is not int or not 2 <= dimensions <= 4096):
        raise ValueError("invalid native dimensions")
    populated = observation["covered_nodes"] == knowledge and dimensions is not None
    if observation["verdict"] != ("populated" if populated else "not_assessed"):
        raise ValueError("false coverage verdict")


@contextmanager
def _own_lock(path: Path):
    """Own a stable private flock descriptor; never unlink or steal another lock."""
    _verify_path(path.parent)
    descriptor = os.open(path, os.O_RDWR | os.O_CREAT | os.O_NOFOLLOW | os.O_NONBLOCK, 0o600)
    try:
        directory = path.parent.stat(follow_symlinks=False)
        before = os.fstat(descriptor)
        if (not stat.S_ISREG(before.st_mode) or before.st_nlink != 1 or before.st_uid != os.getuid()
                or before.st_mode & 0o077):
            raise ValueError("unsafe coordination lock")
        fcntl.flock(descriptor, fcntl.LOCK_EX | fcntl.LOCK_NB)
        _verify_lock_identity(path, before, directory)
        yield
        _verify_lock_identity(path, before, directory)
    finally:
        os.close(descriptor)


def _run_process(request: _ProcessRequest) -> tuple[dict, bytes]:
    record = {"argv": request.command, "cwd": str(request.repository), "stdin_hex": request.data.hex(),
              "watchdog_seconds": request.timeout, "outcome": "spawn_failed", "exit_code": None}
    started, captured, child = time.monotonic(), b"", None
    try:
        with ThreadPoolExecutor(max_workers=2) as readers:
            try:
                child = subprocess.Popen(request.command, cwd=request.repository, env=request.environment,
                    stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
                record.update(child_pid=child.pid, owned_session=child.pid)
                output = readers.submit(_read_stream, child.stdout)
                errors = readers.submit(_read_stream, child.stderr)
                try:
                    child.stdin.write(request.data)
                    child.stdin.close()
                    child.wait(timeout=request.timeout)
                    record["outcome"] = "success" if child.returncode == 0 else "nonzero"
                except (subprocess.TimeoutExpired, KeyboardInterrupt, BrokenPipeError) as error:
                    record["outcome"] = "timeout" if isinstance(error, subprocess.TimeoutExpired) else "interrupted"
                    _stop_child(child)
                stdout, captured = output.result()
                stderr, _ = errors.result()
                record.update(exit_code=child.returncode, stdout=stdout, stderr=stderr)
            finally:
                if child is not None and child.poll() is None:
                    _stop_child(child)
    except OSError:
        pass
    record["duration_seconds"] = time.monotonic() - started
    return record, captured


def _get_environment() -> dict:
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith(("GIT_", "LETOPIS_"))
                   and key not in {"OPENAI_API_KEY", "OLLAMA_API_KEY", "OLLAMA_HOST"}}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, LC_ALL="C", TZ="UTC")
    return environment


def _read_json(path: Path) -> dict:
    _verify_path(path)
    with path.open("rb") as stream:
        return json.load(stream, object_pairs_hook=_reject_duplicate_keys)


def _write_json(path: Path, value: dict) -> None:
    _publish_bytes(path, _encode_json(value))


def _encode_json(value: dict) -> bytes:
    return (json.dumps(value, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def _hash_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _publish_bytes(path: Path, data: bytes) -> None:
    """Publish a complete immutable file, surviving interruption before config creation."""
    temporary = path.with_name(f".{path.name}.{uuid.uuid4().hex}.tmp")
    try:
        descriptor = os.open(temporary, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
        with os.fdopen(descriptor, "wb") as stream:
            stream.write(data)
            stream.flush()
            os.fsync(stream.fileno())
        os.link(temporary, path)
        directory = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
        try:
            os.fsync(directory)
        finally:
            os.close(directory)
    finally:
        temporary.unlink(missing_ok=True)


def _verify_lock_identity(path: Path, before: os.stat_result, directory: os.stat_result) -> None:
    _verify_path(path)
    parent = path.parent.stat(follow_symlinks=False)
    if (parent.st_dev, parent.st_ino) != (directory.st_dev, directory.st_ino):
        raise ValueError("coordination directory identity changed")
    current = path.stat(follow_symlinks=False)
    if (current.st_dev, current.st_ino, current.st_nlink) != (before.st_dev, before.st_ino, 1):
        raise ValueError("coordination lock identity changed")


def _verify_path(path: Path, *, exists: bool = True) -> None:
    if ".." in path.parts:
        raise ValueError("path contains parent traversal")
    if not path.is_absolute() or path.is_symlink() or any(parent.is_symlink() for parent in path.parents):
        raise ValueError("absolute nonsymlink path required")
    if exists and not path.exists():
        raise ValueError("required path is absent")
    if not exists and path.exists():
        raise ValueError("output already exists")


def _reject_duplicate_keys(pairs: list[tuple[str, object]]) -> dict:
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate JSON key")
        result[key] = value
    return result


def _read_stream(stream) -> tuple[dict, bytes]:
    digest, length, captured = hashlib.sha256(), 0, bytearray()
    with stream:
        for block in iter(lambda: stream.read(8192), b""):
            digest.update(block)
            length += len(block)
            captured.extend(block[:max(0, CAPTURE_LIMIT - len(captured))])
    return {"bytes": length, "sha256": digest.hexdigest()}, bytes(captured) if length <= CAPTURE_LIMIT else b""


def _stop_child(child: subprocess.Popen) -> None:
    if child.poll() is None:
        try:
            os.killpg(child.pid, signal.SIGTERM)
        except ProcessLookupError:
            pass
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            os.killpg(child.pid, signal.SIGKILL)
    child.wait()


if __name__ == "__main__":
    sys.exit(main())
