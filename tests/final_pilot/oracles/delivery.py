"""Independent C2 observations of the local package, install and installed-check scripts.

Inputs are literal bytes written by the oracle; expected checksums are `hashlib` over
those known bytes, never values read back from a participant manifest. Each failure
scenario compares the complete inventory (bytes and modes) before and after the run.
"""
from __future__ import annotations

import hashlib
import json
import os
import shutil
import stat
import subprocess
from pathlib import Path

import evaluation

TOY_BINARY = b"#!/bin/sh\necho tk30-literal-binary\n"
TOY_SKILL = b"---\nname: letopis\n---\n\nTK-30 literal skill fixture.\n"
DAMAGED_BINARY = b"#!/bin/sh\necho tk30-damaged-binary\n"
GARBAGE_BINARY = b"\x00tk30 not an executable image\x00"
EXPECTED_VERSION = "0.0.0"
BINARY_PATH = "bin/letopis"
SKILL_PATH = "share/letopis/skills/letopis/SKILL.md"
MANIFEST_PATH = "manifest.json"
SCRIPTS = {"C2-T1": "scripts/package.py", "C2-T2": "scripts/install.py",
           "C2-T3": "scripts/check_install.py"}
CHECK_IDS = {"C2-T1": ("bundle_layout", "manifest_integrity", "bundle_rejects_invalid_input"),
             "C2-T2": ("install_layout_and_repeat", "install_failure_preserves_previous"),
             "C2-T3": ("installed_workflow", "installed_missing_or_damaged",
                       "installed_independence")}
WORKFLOW_COMMANDS = ("check", "search", "inspect", "reindex")
SCRIPT_WATCHDOG_SECONDS = 120
# Build output, HOME and Git metadata of the copy are not part of the participant result.
PACKAGE_SKIP = ("target", "home", "repo/.git")


def evaluate(copy: Path, task_id: str, evidence: Path) -> list[dict]:
    """Run `make check`, then observe one delivery script in the disposable copy."""
    return evaluation.observe_after_regression(
        copy, evidence, lambda: observe_script(copy, task_id, evidence))


def observe_script(copy: Path, task_id: str, evidence: Path) -> list[dict]:
    work = copy / "delivery"
    work.mkdir()
    if not (copy / "repo" / SCRIPTS[task_id]).is_file():
        return [make_check(check_id, False, f"{SCRIPTS[task_id]} is absent")
                for check_id in CHECK_IDS[task_id]]
    if task_id == "C2-T1":
        return observe_package(copy, work, evidence)
    if task_id == "C2-T2":
        return observe_install(copy, work, evidence)
    real = evaluation.build_binary(copy, evidence)
    return observe_installed_check(copy, work, real, evidence)


def observe_package(copy: Path, work: Path, evidence: Path) -> list[dict]:
    """Package literal inputs, then reject each invalid input without side effects."""
    inputs = write_inputs(work / "inputs")
    repo = copy / "repo"
    before = read_inventory(repo, skip=(".git",))
    bundle = work / "bundle"
    run = run_script(copy, "package", ["--binary", inputs[0], "--skill", inputs[1],
                                       "--output", bundle], evidence)
    unchanged = read_inventory(repo, skip=(".git",)) == before
    layout = find_layout_gaps(bundle, include_manifest=True)
    manifest = find_manifest_gaps(bundle / MANIFEST_PATH, copy)
    ok = run["exit"] == 0
    rejected = find_package_acceptances(copy, work, inputs, evidence) if ok else ["no valid run"]
    return [
        make_check("bundle_layout", ok and unchanged and not layout,
                   f"exit {run['exit']}; layout gaps {layout}; source unchanged {unchanged}"),
        make_check("manifest_integrity", ok and not manifest, f"manifest gaps {manifest}"),
        make_check("bundle_rejects_invalid_input", ok and not rejected,
                   f"accepted or changed: {rejected}"),
    ]


def observe_install(copy: Path, work: Path, evidence: Path) -> list[dict]:
    """Install an oracle-written bundle twice, then damaged bundles and a collision."""
    bundle = write_bundle(work / "bundle", copy)
    prefix = work / "prefix"
    first = run_script(copy, "install", ["--bundle", bundle, "--prefix", prefix], evidence)
    installed = read_inventory(prefix)
    second = run_script(copy, "install", ["--bundle", bundle, "--prefix", prefix], evidence)
    gaps = find_layout_gaps(prefix, include_manifest=False, exact=False)
    stray = sorted({entry.name for entry in work.iterdir()} - {"bundle", "prefix"})
    repeat = (first["exit"] == 0 and second["exit"] == 0 and not gaps and not stray
              and read_inventory(prefix) == installed)
    failures = find_install_damage(copy, work, bundle, evidence) if first["exit"] == 0 else [
        "no valid install"]
    return [
        make_check("install_layout_and_repeat", repeat,
                   f"exits {first['exit']}/{second['exit']}; gaps {gaps}; outside {stray}"),
        make_check("install_failure_preserves_previous", not failures,
                   f"accepted or changed: {failures}"),
    ]


def observe_installed_check(copy: Path, work: Path, real: Path, evidence: Path) -> list[dict]:
    """Run the installed check on a logged prefix, then on missing or damaged components."""
    prefix, log = create_logged_prefix(work, real, copy)
    installed = read_inventory(prefix)
    good = run_script(copy, "check_install", ["--prefix", prefix], evidence)
    calls = read_calls(log)
    workflow = good["exit"] == 0 and find_call_gaps(calls, prefix) == []
    damaged = find_unreported_damage(copy, work, prefix, evidence) if workflow else [
        "no valid check"]
    independent = observe_independence(copy, work, prefix, log, evidence)
    return [
        make_check("installed_workflow", workflow and read_inventory(prefix) == installed,
                   f"exit {good['exit']}; calls {calls}; gaps {find_call_gaps(calls, prefix)}"),
        make_check("installed_missing_or_damaged", not damaged, f"unreported: {damaged}"),
        make_check("installed_independence", workflow and not independent,
                   f"violations: {independent}"),
    ]


def find_package_acceptances(copy: Path, work: Path, inputs: tuple, evidence: Path) -> list:
    """Each invalid input must fail without a partial bundle or any source change."""
    empty = work / "invalid/empty"
    empty.parent.mkdir()
    empty.write_bytes(b"")
    existing = work / "invalid/existing"
    existing.mkdir()
    (existing / "sentinel").write_bytes(b"keep")
    binary, skill = inputs
    cases = {"missing binary": (work / "invalid/none", skill, work / "out-1"),
             "empty binary": (empty, skill, work / "out-2"),
             "directory binary": (work / "invalid", skill, work / "out-3"),
             "missing skill": (binary, work / "invalid/none", work / "out-4"),
             "empty skill": (binary, empty, work / "out-5"),
             "existing output": (binary, skill, existing)}
    accepted = []
    for name, (case_binary, case_skill, output) in cases.items():
        before = read_inventory(copy, skip=PACKAGE_SKIP)
        run = run_script(copy, "package", ["--binary", case_binary, "--skill", case_skill,
                                           "--output", output], evidence)
        # A run killed at the watchdog (exit None) did not reject anything.
        if run["exit"] in (0, None) or read_inventory(copy, skip=PACKAGE_SKIP) != before:
            accepted.append(name)
    return accepted


def find_install_damage(copy: Path, work: Path, bundle: Path, evidence: Path) -> list[str]:
    """Damaged bundles and a destination collision must leave the prefix as it was."""
    collision = work / "collision"
    run_script(copy, "install", ["--bundle", bundle, "--prefix", collision], evidence)
    (collision / SKILL_PATH).unlink()
    (collision / SKILL_PATH).mkdir()
    (collision / SKILL_PATH / "sentinel").write_bytes(b"keep")
    cases = {"changed binary": (damage_bundle(bundle, work / "d1", BINARY_PATH), "prefix"),
             "missing skill": (damage_bundle(bundle, work / "d2", SKILL_PATH), "prefix"),
             "broken manifest": (damage_bundle(bundle, work / "d3", MANIFEST_PATH), "prefix"),
             "directory collision": (bundle, "collision")}
    changed = []
    for name, (case_bundle, prefix_name) in cases.items():
        prefix = work / prefix_name
        before = read_inventory(prefix)
        run = run_script(copy, "install", ["--bundle", case_bundle, "--prefix", prefix],
                         evidence)
        if run["exit"] in (0, None) or read_inventory(prefix) != before:
            changed.append(name)
    return changed


def find_unreported_damage(copy: Path, work: Path, prefix: Path, evidence: Path) -> list:
    """Every broken component fails the check with a diagnostic naming that component."""
    cases = {"missing binary": (BINARY_PATH, None, 0), "empty binary": (BINARY_PATH, b"", 0o755),
             "non-executable binary": (BINARY_PATH, TOY_BINARY, 0o644),
             "damaged binary": (BINARY_PATH, GARBAGE_BINARY, 0o755),
             "missing skill": (SKILL_PATH, None, 0), "empty skill": (SKILL_PATH, b"", 0o644)}
    unreported = []
    for number, (name, (path, data, mode)) in enumerate(cases.items()):
        broken = work / f"broken-{number}"
        shutil.copytree(prefix, broken, symlinks=True)
        replace_file(broken / path, data, mode)
        run = run_script(copy, "check_install", ["--prefix", broken], evidence)
        text = (run["stdout"] + run["stderr"]).decode("utf-8", "replace").lower()
        named = ("skill" in text) if path == SKILL_PATH else ("bin/letopis" in text
                                                              or "binary" in text)
        if run["exit"] in (0, None) or not named:
            unreported.append(name)
    return unreported


def observe_independence(copy: Path, work: Path, prefix: Path, log: Path,
                         evidence: Path) -> list[str]:
    """Run from another directory with no letopis on PATH and the build target sealed."""
    outside = work / "outside"
    outside.mkdir()
    log.write_text("")
    installed = read_inventory(prefix)
    source = read_inventory(copy / "repo", skip=(".git",))
    target = copy / "target"
    mode = stat.S_IMODE(target.stat().st_mode)
    target.chmod(0)
    try:
        run = run_script(copy, "check_install", ["--prefix", prefix], evidence, cwd=outside)
    finally:
        target.chmod(mode)
    calls = read_calls(log)
    violations = [] if run["exit"] == 0 else [f"exit {run['exit']}"]
    violations += find_call_gaps(calls, prefix)
    violations += [f"flag {call}" for call in calls if "--version" in call or "--help" in call]
    if read_inventory(prefix) != installed:
        violations.append("installed files changed")
    if read_inventory(copy / "repo", skip=(".git",)) != source or any(outside.iterdir()):
        violations.append("source tree or working directory changed")
    return violations


def find_layout_gaps(root: Path, *, include_manifest: bool, exact: bool = True) -> list[str]:
    expected = {BINARY_PATH: TOY_BINARY, SKILL_PATH: TOY_SKILL}
    files = read_inventory(root) if root.is_dir() else {}
    gaps = [f"{path} bytes differ" for path, data in expected.items()
            if files.get(path, (None,))[0] != hash_bytes(data)]
    if files.get(BINARY_PATH, (None, 0))[1] & 0o111 == 0:
        gaps.append(f"{BINARY_PATH} is not executable")
    allowed = set(expected) | ({MANIFEST_PATH} if include_manifest else set())
    regular = {path for path, (kind, _mode) in files.items() if kind not in ("dir", "other")}
    if exact and regular != allowed:
        gaps.append(f"files {sorted(regular)}")
    return gaps


def find_manifest_gaps(path: Path, copy: Path) -> list[str]:
    manifest = read_json_file(path)
    if not isinstance(manifest, dict):
        return ["manifest.json is missing or not a JSON object"]
    expected = build_manifest(copy)
    return [f"{key} is {manifest.get(key)!r}" for key in expected
            if manifest.get(key) != expected[key]]


def build_manifest(copy: Path) -> dict:
    """The manifest the contract fixes for the literal inputs of this captured commit."""
    files = [{"path": BINARY_PATH, "sha256": hash_bytes(TOY_BINARY)},
             {"path": SKILL_PATH, "sha256": hash_bytes(TOY_SKILL)}]
    return {"version": EXPECTED_VERSION, "source_commit": evaluation.read_captured_commit(copy),
            "target": read_host_target(copy), "files": files}


def write_inputs(directory: Path) -> tuple[Path, Path]:
    directory.mkdir()
    binary, skill = directory / "letopis", directory / "SKILL.md"
    binary.write_bytes(TOY_BINARY)
    binary.chmod(0o755)
    skill.write_bytes(TOY_SKILL)
    return binary, skill


def write_bundle(directory: Path, copy: Path) -> Path:
    replace_file(directory / BINARY_PATH, TOY_BINARY, 0o755)
    replace_file(directory / SKILL_PATH, TOY_SKILL, 0o644)
    (directory / MANIFEST_PATH).write_text(json.dumps(build_manifest(copy), indent=2))
    return directory


def damage_bundle(bundle: Path, directory: Path, path: str) -> Path:
    shutil.copytree(bundle, directory)
    if path == BINARY_PATH:
        replace_file(directory / path, DAMAGED_BINARY, 0o755)
    elif path == MANIFEST_PATH:
        (directory / path).write_text("{ not json")
    else:
        (directory / path).unlink()
    return directory


def create_logged_prefix(work: Path, real: Path, copy: Path) -> tuple[Path, Path]:
    """Install the real binary behind a wrapper that logs each absolute invocation."""
    prefix, log = work / "prefix", work / "calls.log"
    product = work / "product/letopis"
    product.parent.mkdir()
    shutil.copy2(real, product)
    wrapper = f'#!/bin/sh\nprintf \'%s\\n\' "$0 $*" >> "{log}"\nexec "{product}" "$@"\n'
    replace_file(prefix / BINARY_PATH, wrapper.encode(), 0o755)
    replace_file(prefix / SKILL_PATH, (copy / "repo/skills/letopis/SKILL.md").read_bytes(),
                 0o644)
    log.write_text("")
    return prefix, log


def find_call_gaps(calls: list[str], prefix: Path) -> list[str]:
    """Each logged call names the installed binary by absolute path (`$0 ARGS`)."""
    binary = os.path.realpath(prefix / BINARY_PATH)
    split = [call.split(" ", 2) for call in calls]
    gaps = [f"not absolute: {call}" for call, parts in zip(calls, split)
            if not os.path.isabs(parts[0]) or os.path.realpath(parts[0]) != binary]
    used = {parts[1] for parts in split if len(parts) > 1}
    return gaps + [f"{command} not run" for command in WORKFLOW_COMMANDS if command not in used]


def run_script(copy: Path, name: str, args: list, evidence: Path, cwd: Path | None = None):
    """Run `python3 scripts/NAME.py` with the copy's HOME and no `letopis` on PATH."""
    env = {**evaluation.build_environment(copy), "PATH": build_tool_path(),
           "TMPDIR": str(make_tmp(copy))}
    argv = [shutil.which("python3", path=env["PATH"]) or "python3",
            str(copy / "repo/scripts" / f"{name}.py"), *map(str, args)]
    return evaluation.run_observed(argv, cwd=cwd or copy / "repo", env=env, evidence=evidence,
                                   label=" ".join(argv[1:3]), timeout=SCRIPT_WATCHDOG_SECONDS)


def build_tool_path() -> str:
    """The host PATH without any directory that offers a `letopis` executable."""
    return os.pathsep.join(directory for directory in os.environ.get("PATH", "").split(os.pathsep)
                           if directory and not os.access(Path(directory, "letopis"), os.X_OK))


def read_inventory(root: Path, skip: tuple[str, ...] = ()) -> dict[str, tuple]:
    """Map every entry below root to (sha256 or kind, mode); never follows links."""
    entries = {}
    if not root.exists():
        return entries
    for directory, dirnames, filenames in os.walk(root):
        dirnames[:] = [name for name in dirnames
                       if (Path(directory) / name).relative_to(root).as_posix() not in skip]
        for name in dirnames + filenames:
            path = Path(directory) / name
            info = path.lstat()
            kind = (hash_bytes(path.read_bytes()) if stat.S_ISREG(info.st_mode)
                    else "dir" if stat.S_ISDIR(info.st_mode) else "other")
            entries[path.relative_to(root).as_posix()] = (kind, stat.S_IMODE(info.st_mode))
    return entries


def read_host_target(copy: Path) -> str:
    result = subprocess.run(["rustc", "-vV"], capture_output=True, text=True,
                            cwd=copy / "repo", env=evaluation.build_environment(copy),
                            timeout=evaluation.GIT_TIMEOUT_SECONDS)
    hosts = [line[6:] for line in result.stdout.splitlines() if line.startswith("host: ")]
    if result.returncode != 0 or len(hosts) != 1:
        raise evaluation.InfraError("rustc -vV did not report the host target")
    return hosts[0]


def read_calls(log: Path) -> list[str]:
    return [line for line in log.read_text().splitlines() if line]


def replace_file(path: Path, data: bytes | None, mode: int) -> None:
    if path.exists():
        path.unlink()
    if data is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    path.chmod(mode)


def make_tmp(copy: Path) -> Path:
    tmp = copy / "tmp"
    tmp.mkdir(exist_ok=True)
    return tmp


def read_json_file(path: Path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, ValueError):
        return None


def hash_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def make_check(check_id: str, passed: bool, evidence: str) -> dict:
    return {"id": check_id, "passed": bool(passed), "evidence": evidence}
