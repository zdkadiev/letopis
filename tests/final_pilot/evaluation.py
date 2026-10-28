"""TK-30 private evaluation: capture, one disposable copy and exactly one task oracle.

Model-free and standard-library only. The participant state is captured before any
evaluator code runs, the oracle sees only a disposable copy, and the participant audit
is re-checked afterwards even when the oracle fails or is interrupted. A broken build,
toolchain or fixture is `infra_error`, never a feature verdict; manual criteria stay
`null` until a separate operator annotation pins them to the captured commit.

Public API: evaluate, InfraError, run_observed, build_binary, observe_after_regression,
run_regression,
read_audit, build_environment, read_captured_commit.
"""
from __future__ import annotations

import importlib
import json
import os
import re
import shutil
import subprocess
import time
from pathlib import Path, PurePosixPath

import copies
import study

ORACLE_MODULES = {"C1": "oracles.cli", "C2": "oracles.delivery", "C3": "oracles.drain"}
TASK_PATTERN = re.compile(r"C[1-3]-T[1-3]")
STATUS_ASSESSED = "assessed"
STATUS_INFRA = "infra_error"
CAUSE_BUILD = "participant_build"
CAUSE_PROBE = "probe_compile"
CAUSE_ENVIRONMENT = "environment"
REGRESSION_ID = "regression"
SELECTION_INPUT = "inputs/selection.json"
EVIDENCE_DIR = "operator/evidence"
ANNOTATION_DIR = "operator/annotations"
COMMANDS_LOG = "commands.jsonl"
RESULT_FILE = "result.json"
BUILD_TIMEOUT_SECONDS = 1800
REGRESSION_TIMEOUT_SECONDS = 3600
GIT_TIMEOUT_SECONDS = 30
FAILURE_TAIL_LINES = 40
OID_PATTERN = re.compile(r"[0-9a-f]{40}")


class InfraError(RuntimeError):
    """The evaluation could not observe the feature; `cause` names why.

    `participant_build` and `probe_compile` come from the captured source; `environment`
    is reserved for real host faults (a tool cannot start, rustc/git fail, no loopback).
    `checks` keeps verdicts computed before the failure, such as a failed regression.
    """

    def __init__(self, message: str, cause: str = "environment") -> None:
        super().__init__(message)
        self.cause = cause
        self.checks: list[dict] = []


def evaluate(root: Path, slot: str) -> dict:
    """Capture the slot if needed, run its single oracle on a disposable copy and grade it.

    Returns `{task_id, status, checks, execution}`: status is `assessed` or `infra_error`
    and every check is `{id, passed: bool | None, evidence}` in the frozen selection order.
    `execution.infra_cause` is null, `participant_build`, `probe_compile` or `environment`;
    an infra_error keeps verdicts observed before it (for example a failed regression).
    The participant audit is verified in `finally`, so a write-back or an interrupted
    oracle still raises. Raises ValueError for an unknown slot/task or a check ID mismatch.
    """
    root = Path(root)
    task_id = read_task_id(slot)
    expected = read_check_ids(root, task_id)
    oracle = load_oracle(task_id)
    capture = read_or_capture(root, slot)
    try:
        return run_oracle(root, slot, oracle, expected)
    finally:
        copies.require_unchanged(root, capture)


def run_observed(argv: list[str], *, cwd: Path, env: dict, evidence: Path, label: str,
                 timeout: float, stdin: bytes | None = None) -> dict:
    """Run one owned process to completion or watchdog and log its exit before grading.

    Returns `{label, exit, elapsed, timed_out, stdout, stderr}`; a timed-out process is
    killed (it is our own child) and reported with `exit: None`. A missing executable
    raises InfraError.
    """
    started = time.monotonic()
    try:
        result = subprocess.run(argv, cwd=cwd, env=env, input=stdin, capture_output=True,
                                timeout=timeout)
        observed = {"exit": result.returncode, "timed_out": False,
                    "stdout": result.stdout, "stderr": result.stderr}
    except subprocess.TimeoutExpired as expired:
        observed = {"exit": None, "timed_out": True, "stdout": expired.stdout or b"",
                    "stderr": expired.stderr or b""}
    except OSError as error:
        raise InfraError(f"{label}: cannot start {argv[0]}: {error.strerror}") from error
    observed.update(label=label, elapsed=round(time.monotonic() - started, 3))
    append_command(evidence, observed)
    return observed


def build_binary(copy: Path, evidence: Path) -> Path:
    """Build the participant binary offline into the copy's own target; InfraError on failure."""
    result = run_observed(["cargo", "build", "--locked", "--offline"], cwd=copy / "repo",
                          env=build_environment(copy), evidence=evidence, label="cargo build",
                          timeout=BUILD_TIMEOUT_SECONDS)
    binary = copy / "target/debug/letopis"
    if result["exit"] != 0 or not binary.is_file():
        save_tail(evidence, "cargo-build.log", result)
        raise InfraError("cargo build of the captured source failed", CAUSE_BUILD)
    return binary


def observe_after_regression(copy: Path, evidence: Path, observe) -> list[dict]:
    """Run `make check`, then `observe()`; an InfraError keeps the regression verdict."""
    regression = run_regression(copy, evidence)
    try:
        return [*observe(), regression]
    except InfraError as error:
        error.checks = [regression]
        raise


def run_regression(copy: Path, evidence: Path) -> dict:
    """Run `make check` in the disposable source; compact `ok` or the saved failure tail.

    A run stopped by the watchdog is unobserved (`passed: None`), not a failure verdict.
    """
    result = run_observed(["make", "check"], cwd=copy / "repo", env=build_environment(copy),
                          evidence=evidence, label="make check",
                          timeout=REGRESSION_TIMEOUT_SECONDS)
    if result["timed_out"]:
        return {"id": REGRESSION_ID, "passed": None, "evidence": "make check watchdog expired"}
    if result["exit"] == 0:
        return {"id": REGRESSION_ID, "passed": True, "evidence": "make check: ok"}
    save_tail(evidence, "make-check.log", result)
    return {"id": REGRESSION_ID, "passed": False,
            "evidence": f"make check exit {result['exit']}; tail in make-check.log"}


def read_audit(copy: Path, check_id: str) -> tuple[bool | None, str]:
    """Read the operator's manual verdict for one check of this evaluation copy.

    The annotation lives at `ROOT/operator/annotations/SLOT.json`, outside participant and
    copy. Absent, malformed or pinned to another captured commit means `None`; a valid
    verdict needs symbol/line refs that exist in the captured source and a conclusion.
    """
    path = copy.parents[2] / ANNOTATION_DIR / f"{copy.name}.json"
    if not path.is_file():
        return None, "operator audit absent"
    annotation = json.loads(path.read_text(encoding="utf-8"))
    captured = read_captured_commit(copy)
    if annotation.get("captured_commit") != captured:
        return None, "operator audit rejected: it refers to another captured commit"
    entry = annotation.get("checks", {}).get(check_id)
    if not isinstance(entry, dict) or not isinstance(entry.get("passed"), bool):
        return None, "operator audit has no verdict for this check"
    problem = find_ref_problem(copy / "repo", entry.get("refs"))
    if problem or not str(entry.get("conclusion", "")).strip():
        return None, f"operator audit rejected: {problem or 'conclusion missing'}"
    return entry["passed"], f"operator audit at {captured}: {entry['conclusion']}"


def build_environment(copy: Path) -> dict:
    """Environment for offline builds: the copy's HOME and target, the host toolchain."""
    host_home = Path.home()
    return {
        "PATH": os.environ.get("PATH", ""), "HOME": str(copy / "home"), "LC_ALL": "C",
        "CARGO_HOME": os.environ.get("CARGO_HOME", str(host_home / ".cargo")),
        "RUSTUP_HOME": os.environ.get("RUSTUP_HOME", str(host_home / ".rustup")),
        "CARGO_TARGET_DIR": str(copy / "target"), "CARGO_NET_OFFLINE": "true",
        "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
    }


def read_captured_commit(copy: Path) -> str:
    """The commit that `prepare_evaluation` made from exactly the captured bytes."""
    result = subprocess.run(["git", "-C", str(copy / "repo"), "rev-parse", "HEAD"],
                            capture_output=True, env=copies.GIT_ENVIRONMENT,
                            timeout=GIT_TIMEOUT_SECONDS)
    commit = result.stdout.decode().strip()
    if result.returncode != 0 or OID_PATTERN.fullmatch(commit) is None:
        raise InfraError("evaluation copy has no captured commit")
    return commit


def read_task_id(slot: str) -> str:
    if copies.SLOT_PATTERN.fullmatch(slot) is None:
        raise ValueError(f"unknown slot: {slot!r}")
    return slot.rsplit("-", 1)[0]


def read_check_ids(root: Path, task_id: str) -> list[str]:
    """Frozen check IDs of one task from the prepared copy of the selection."""
    selection = study.load_selection(root / SELECTION_INPUT)
    for chain in selection["chains"]:
        for task in chain["tasks"]:
            if task["id"] == task_id:
                return list(task["check_ids"])
    raise ValueError(f"unknown task id: {task_id!r}")


def load_oracle(task_id: str):
    if TASK_PATTERN.fullmatch(task_id) is None or task_id[:2] not in ORACLE_MODULES:
        raise ValueError(f"no oracle for task {task_id!r}")
    return importlib.import_module(ORACLE_MODULES[task_id[:2]])


def read_or_capture(root: Path, slot: str) -> dict:
    """Reuse the slot's single capture or take it now, before any evaluator code runs."""
    if (root / "operator/captures" / f"{slot}.json").is_file():
        return copies.read_capture(root, slot)
    return copies.capture_slot(root, slot)


def run_oracle(root: Path, slot: str, oracle, expected: list[str]) -> dict:
    """Run the oracle in a fresh copy; the copy is removed afterwards, evidence is kept."""
    evidence = create_evidence_directory(root, slot)
    copy = copies.prepare_evaluation(root, slot)
    started = time.monotonic()
    try:
        commit = read_captured_commit(copy)
        status, checks, cause = grade_oracle(oracle, copy, slot, evidence, expected)
    finally:
        shutil.rmtree(copy, ignore_errors=True)
    result = {"task_id": read_task_id(slot), "status": status, "checks": checks,
              "execution": {"elapsed_seconds": round(time.monotonic() - started, 3),
                            "captured_commit": commit, "infra_cause": cause,
                            "evidence": str(evidence)}}
    (evidence / RESULT_FILE).write_text(json.dumps(result, indent=2) + "\n", encoding="utf-8")
    return result


def grade_oracle(oracle, copy: Path, slot: str, evidence: Path,
                 expected: list[str]) -> tuple[str, list[dict], str | None]:
    """Return status, checks and the infra cause (None when the oracle completed)."""
    try:
        checks = oracle.evaluate(copy, read_task_id(slot), evidence)
    except InfraError as error:
        return grade_failure(error, expected)
    require_check_ids(checks, expected)
    return STATUS_ASSESSED, checks, None


def grade_failure(error: InfraError, expected: list[str]) -> tuple[str, list[dict], str]:
    """Unobserved checks become null; verdicts computed before the failure are kept."""
    kept = {check["id"]: check for check in error.checks}
    checks = [kept.get(check_id) or {"id": check_id, "passed": None,
                                     "evidence": f"infra ({error.cause}): {error}"}
              for check_id in expected]
    return STATUS_INFRA, checks, error.cause


def require_check_ids(checks: list[dict], expected: list[str]) -> None:
    observed = [check.get("id") for check in checks]
    if observed != expected:
        raise ValueError(f"oracle check IDs {observed} differ from the frozen {expected}")
    for check in checks:
        if check.get("passed") not in (True, False, None):
            raise ValueError(f"{check['id']}: passed must be true, false or null")


def create_evidence_directory(root: Path, slot: str) -> Path:
    """A new `run-N` directory per evaluation; earlier evidence is never overwritten."""
    parent = root / EVIDENCE_DIR / slot
    parent.mkdir(parents=True, exist_ok=True)
    number = 1
    while (parent / f"run-{number}").exists():
        number += 1
    directory = parent / f"run-{number}"
    directory.mkdir()
    return directory


def find_ref_problem(repo: Path, refs) -> str | None:
    """A ref is `{path, line, symbol}`; the symbol must appear on that captured line."""
    if not isinstance(refs, list) or not refs:
        return "symbol/line refs missing"
    for ref in refs:
        path = PurePosixPath(str(ref.get("path", "")) if isinstance(ref, dict) else "")
        if not path.parts or path.is_absolute() or ".." in path.parts:
            return f"unsafe ref {ref!r}"
        file = repo / path
        lines = file.read_text(encoding="utf-8").splitlines() if file.is_file() else []
        line = ref.get("line")
        symbol = str(ref.get("symbol", ""))
        if not (isinstance(line, int) and 1 <= line <= len(lines) and symbol
                and symbol in lines[line - 1]):
            return f"ref {path}:{line} does not show {symbol!r} in the captured source"
    return None


def append_command(evidence: Path, observed: dict) -> None:
    row = {key: observed[key] for key in ("label", "exit", "elapsed", "timed_out")}
    with (evidence / COMMANDS_LOG).open("a", encoding="utf-8") as stream:
        stream.write(json.dumps(row) + "\n")


def save_tail(evidence: Path, name: str, result: dict) -> None:
    text = (result["stdout"] + result["stderr"]).decode("utf-8", "replace")
    tail = "\n".join(text.splitlines()[-FAILURE_TAIL_LINES:])
    (evidence / name).write_text(tail + "\n", encoding="utf-8")
