"""Independent C1 observations of `--version`, general help and per-command help.

Every flag runs the built binary with standard input left open and unread, an empty
HOME and an empty working directory outside any repository, under a watchdog. Expected
values are frozen literals for the current package (`letopis 0.0.0`) and the command set
of the preparation base; a new selection changes them here, never from program output.
"""
from __future__ import annotations

import contextlib
import json
import os
import re
import signal
import subprocess
import tempfile
import time
from pathlib import Path

import evaluation

EXPECTED_VERSION = b"letopis 0.0.0\n"
INVALID_REQUEST = {"ok": False, "error": {"code": "invalid_request",
                                          "message": "invalid command input", "details": {}}}
# Stdin JSON fields of each public command (X-1 and src/cli/input.rs at the base).
COMMAND_FIELDS = {
    "check": {"paths": "optional"},
    "inspect": {"element_ids": "required"},
    "search": {"query": "required", "limit": "required", "filters": "optional"},
    "traverse": {"element_ids": "required", "mode": "required", "direction": "required",
                 "depth": "required", "limit": "required", "edge_types": "optional",
                 "edge_limit": "optional"},
    "reindex": {},
    "status": {},
}
DRAIN_ARGV = ("embed", "--drain")
INVALID_ARGV = (("bogus",), ("bogus", "--help"), ("search", "extra", "--help"),
                ("search", "--help", "extra"), ("--help", "bogus"))
FLAG_WATCHDOG_SECONDS = 10
FLAG_PATH = "/usr/bin:/bin"


def evaluate(copy: Path, task_id: str, evidence: Path) -> list[dict]:
    """Run `make check`, build the participant binary and observe the task's flags."""
    observe = {"C1-T1": observe_version, "C1-T2": observe_help,
               "C1-T3": observe_command_help}[task_id]
    return evaluation.observe_after_regression(
        copy, evidence, lambda: observe(evaluation.build_binary(copy, evidence), evidence))


def observe_version(binary: Path, evidence: Path) -> list[dict]:
    """`letopis --version` prints exactly the frozen version and leaves no state."""
    outside = run_flag(binary, ["--version"], evidence, in_repository=False)
    inside = run_flag(binary, ["--version"], evidence, in_repository=True)
    value = outside["exit"] == 0 and outside["stdout"] == EXPECTED_VERSION
    return [
        make_check("version_value", value, describe(outside)),
        make_check("version_no_stdin_or_state", is_clean_success(outside, inside),
                   f"outside: {describe(outside)}; inside repo: {describe(inside)}"),
    ]


def observe_help(binary: Path, evidence: Path) -> list[dict]:
    """General help names every command with JSON stdin examples; earlier flags hold."""
    outside = run_flag(binary, ["--help"], evidence, in_repository=False)
    inside = run_flag(binary, ["--help"], evidence, in_repository=True)
    missing = find_help_gaps(outside["stdout"].decode("utf-8", "replace"))
    earlier = find_earlier_breaks(binary, evidence)
    passed = outside["exit"] == 0 and not missing and not earlier
    return [
        make_check("help_commands_and_examples", passed,
                   f"{describe(outside)}; gaps: {missing}; earlier behaviour: {earlier}"),
        make_check("help_no_stdin_or_state", is_clean_success(outside, inside),
                   f"outside: {describe(outside)}; inside repo: {describe(inside)}"),
    ]


def observe_command_help(binary: Path, evidence: Path) -> list[dict]:
    """Each command explains its own input form; invalid argv keeps the JSON failure."""
    general = run_flag(binary, ["--help"], evidence, in_repository=False)
    general_text = general["stdout"].decode("utf-8", "replace")
    gaps = [] if general["exit"] == 0 else ["--help failed"]
    for command, fields in COMMAND_FIELDS.items():
        gaps += find_command_gaps(binary, evidence, command, fields, general_text)
    drain = run_flag(binary, [*DRAIN_ARGV, "--help"], evidence, in_repository=True)
    if drain["exit"] != 0 or not drain["stdout"].strip() or drain["created"]:
        gaps.append(f"embed --drain --help: {describe(drain)}")
    rejected = find_accepted_invalid(binary, evidence) + find_earlier_breaks(binary, evidence)
    return [
        make_check("command_help_forms", not gaps, f"gaps: {gaps}"),
        make_check("help_rejects_invalid_argv", not rejected, f"accepted or changed: {rejected}"),
    ]


def run_flag(binary: Path, argv: list[str], evidence: Path, *, in_repository: bool) -> dict:
    """Run argv with stdin open and unread; report exit, output and any created entries.

    The working directory is a new empty directory outside any repository, or a fresh
    committed Git repository; `created` lists every entry that appeared in it or HOME.
    """
    with tempfile.TemporaryDirectory(prefix="tk30-flag-") as sandbox:
        cwd, home = Path(sandbox) / "cwd", Path(sandbox) / "home"
        cwd.mkdir()
        home.mkdir()
        if in_repository:
            create_repository(cwd)
        before = list_entries(cwd) | list_entries(home)
        env = {"PATH": FLAG_PATH, "HOME": str(home), "LC_ALL": "C",
               "GIT_CEILING_DIRECTORIES": sandbox}
        observed = run_open_stdin([str(binary), *argv], cwd, env)
        observed["created"] = sorted((list_entries(cwd) | list_entries(home)) - before)
    label = " ".join(argv) + (" (repo)" if in_repository else "")
    evaluation.append_command(evidence, {**observed, "label": label})
    return observed


def run_open_stdin(argv: list[str], cwd: Path, env: dict) -> dict:
    """Wait for the owned child while its stdin pipe stays open; kill it at the watchdog.

    The child runs in its own new session, so the watchdog also ends any helper it
    started that could otherwise keep the output pipes open.
    """
    started = time.monotonic()
    with subprocess.Popen(argv, cwd=cwd, env=env, stdin=subprocess.PIPE,
                          stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                          start_new_session=True) as child:
        try:
            child.wait(timeout=FLAG_WATCHDOG_SECONDS)
            timed_out = False
        except subprocess.TimeoutExpired:
            timed_out = True
        with contextlib.suppress(ProcessLookupError):
            os.killpg(child.pid, signal.SIGKILL)
        child.wait()
        child.stdin.close()
        stdout, stderr = child.stdout.read(), child.stderr.read()
    return {"exit": None if timed_out else child.returncode, "timed_out": timed_out,
            "stdout": stdout, "stderr": stderr,
            "elapsed": round(time.monotonic() - started, 3)}


def find_help_gaps(text: str) -> list[str]:
    """Semantic help content: JSON on stdin and an example invocation per command."""
    gaps = []
    lowered = text.lower()
    if "json" not in lowered or not ("stdin" in lowered or "standard input" in lowered):
        gaps.append("does not say commands take JSON on standard input")
    lines = text.splitlines()
    for command in COMMAND_FIELDS:
        invocation = re.compile(rf"\bletopis\s+{command}\b")
        if not any(invocation.search(line) and "{" in line for line in lines):
            gaps.append(f"no example invocation with JSON for {command}")
    return gaps


def find_command_gaps(binary: Path, evidence: Path, command: str, fields: dict,
                      general_text: str) -> list[str]:
    observed = run_flag(binary, [command, "--help"], evidence, in_repository=True)
    text = observed["stdout"].decode("utf-8", "replace")
    if observed["exit"] != 0 or not text.strip() or observed["created"]:
        return [f"{command} --help: {describe(observed)}"]
    gaps = [] if re.search(rf"\b{command}\b", general_text) else [f"--help omits {command}"]
    if not fields and "{}" not in text:
        gaps.append(f"{command} --help does not show its empty object input")
    for field, presence in fields.items():
        pattern = re.compile(rf"\b{field}\b")
        if not any(pattern.search(line) and presence in line.lower()
                   for line in text.splitlines()):
            gaps.append(f"{command} --help does not mark {field} as {presence}")
    return gaps


def find_accepted_invalid(binary: Path, evidence: Path) -> list[str]:
    accepted = []
    for argv in INVALID_ARGV:
        observed = run_flag(binary, list(argv), evidence, in_repository=False)
        if observed["exit"] != 1 or read_json(observed["stdout"]) != INVALID_REQUEST:
            accepted.append(f"{' '.join(argv)}: {describe(observed)}")
    return accepted


def find_earlier_breaks(binary: Path, evidence: Path) -> list[str]:
    """The frozen version output and the existing invalid-argv failure still hold."""
    version = run_flag(binary, ["--version"], evidence, in_repository=False)
    invalid = run_flag(binary, ["bogus"], evidence, in_repository=False)
    breaks = []
    if version["exit"] != 0 or version["stdout"] != EXPECTED_VERSION:
        breaks.append(f"--version: {describe(version)}")
    if invalid["exit"] != 1 or read_json(invalid["stdout"]) != INVALID_REQUEST:
        breaks.append(f"bogus: {describe(invalid)}")
    return breaks


def is_clean_success(*runs: dict) -> bool:
    return all(run["exit"] == 0 and not run["created"] for run in runs)


def create_repository(directory: Path) -> None:
    env = {"PATH": FLAG_PATH, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1",
           "HOME": str(directory)}
    (directory / "note.md").write_text("# Literal TK-30 note\n", encoding="utf-8")
    for argv in (["init", "-q"], ["add", "note.md"],
                 ["-c", "user.name=Oracle", "-c", "user.email=oracle@invalid.local",
                  "commit", "-qm", "note"]):
        subprocess.run(["git", "-C", str(directory), *argv], env=env, check=True,
                       capture_output=True, timeout=evaluation.GIT_TIMEOUT_SECONDS)


def list_entries(root: Path) -> set[str]:
    """Every path below root except Git's own directory, which the oracle created."""
    entries = set()
    for directory, dirnames, filenames in os.walk(root):
        dirnames[:] = [name for name in dirnames if name != ".git"]
        for name in dirnames + filenames:
            entries.add(str(Path(directory, name).relative_to(root.parent)))
    return entries


def read_json(data: bytes):
    try:
        return json.loads(data)
    except ValueError:
        return None


def make_check(check_id: str, passed: bool, evidence: str) -> dict:
    return {"id": check_id, "passed": bool(passed), "evidence": evidence}


def describe(run: dict) -> str:
    if run["timed_out"]:
        return "watchdog expired with stdin open"
    stdout = run["stdout"][:120].decode("utf-8", "replace")
    return f"exit {run['exit']}, stdout {stdout!r}, created {run.get('created', [])}"
