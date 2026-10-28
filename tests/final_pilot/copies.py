"""TK-30 isolated paired seeds, participant capture and disposable evaluation copies.

Model-free and standard-library only. The snapshot format is the P-1 canonical inventory;
Unix modes are captured separately because they are not part of the P-1 byte hash. The
snapshot, preserve/restore and isolation checks are adapted from the pinned TK-17 sources
listed in SOURCE_RECEIPTS; none of its seeds, study or order are carried over.

Public API: prepare_seeds, audit_seeds, capture_slot, create_home, prepare_evaluation,
require_unchanged, preserve_bytes, snapshot, read_modes. Functions are ordered callers first,
so the low-level snapshot primitives sit below the code that uses them.
"""
from __future__ import annotations

import difflib
import hashlib
import io
import json
import os
import re
import shutil
import stat
import subprocess
import tarfile
from pathlib import Path, PurePosixPath

import study

CHAINS = ("C1", "C2", "C3")
HOST_STATE = (".artifacts", ".superpowers", ".engram", ".agents", ".claude", ".codex",
              ".letopis")
# The pilot task evidence (TK-30..TK-33) holds check IDs, seed RED reasons and oracle rules.
OPERATOR_PATHS = ("docs/zkit/letopis/pilot", "tests/pilot_protocol", "docs/superpowers",
                  "tests/final_pilot", "docs/zkit/letopis/checks/TK-30",
                  "docs/zkit/letopis/checks/TK-31", "docs/zkit/letopis/checks/TK-32",
                  "docs/zkit/letopis/checks/TK-33")
ARCHIVE_EXCLUSIONS = (".git", "target", *HOST_STATE, *OPERATOR_PATHS)
# Participant work in docs/ must survive capture, so only VCS and build outputs are skipped.
CODE_EXCLUSIONS = (".git", "target", ".letopis")
MEMORY_EXCLUSIONS = (".letopis",)
FIXTURE_MARKDOWN = "tests/fixtures/markdown"
PILOT_INSTRUCTION = (
    "Work only inside this repository directory. Do not use the network, other directories "
    "on this machine or any other copy of the project, and leave unrelated files as they are.")
SUPERPOWERS_BULLET = re.compile(
    r"^- Keep agreed specifications and implementation plans in [^\n]*Superpowers[^\n]*\n",
    re.MULTILINE)
WORKSPACE_SECTION = re.compile(r"^## Workspace and memory\n.*?(?=^## |\Z)", re.DOTALL | re.MULTILINE)
SLOT_PATTERN = re.compile(r"C[1-3]-T[1-3]-(?:baseline|memory)")
SHA256_PATTERN = re.compile(r"[0-9a-f]{64}")
GIT_TIMEOUT_SECONDS = 120
COMMIT_TIME = "1700000000 +0000"
PARTICIPANT_NAME = "Pilot Participant"
PARTICIPANT_EMAIL = "pilot@invalid.local"
# Host Git configuration, templates and hooks must not leak into a seed.
GIT_ENVIRONMENT = {
    "PATH": os.environ.get("PATH", ""), "GIT_CONFIG_GLOBAL": os.devnull,
    "GIT_CONFIG_NOSYSTEM": "1", "GIT_TERMINAL_PROMPT": "0", "LC_ALL": "C",
    "GIT_AUTHOR_NAME": PARTICIPANT_NAME, "GIT_AUTHOR_EMAIL": PARTICIPANT_EMAIL,
    "GIT_AUTHOR_DATE": COMMIT_TIME, "GIT_COMMITTER_NAME": PARTICIPANT_NAME,
    "GIT_COMMITTER_EMAIL": PARTICIPANT_EMAIL, "GIT_COMMITTER_DATE": COMMIT_TIME,
}
SOURCE_RECEIPTS = {
    "commit": "a53ba2cc7fe2803c564d54ac89f777aee61bfd56",
    "files": {
        "tests/early_pilot/copies.py":
            "ffd1f300f4cf0cd5204e4bf3dd14e823612d2d6a84e74ea3f174ef22f9783770",
        "tests/early_pilot/evidence.py":
            "2eb639cbae82783f992d9a6a3081cf45659ba443850561b4aa98d605a8c54d0f",
        "tests/early_pilot/run.py":
            "76c60a8a9d27f0a85456ea0257ed555890d781e31cdc2a1c3b570421720eb1d6",
        "tests/early_pilot/study.py":
            "9f9c40b908ab5338baf3346c97e813d11e2075a430b819031226ad3bebf04cc3",
        "tests/early_pilot/test_containers.py":
            "d5d1f9e91df73c89ec2183670b9e3098faaee8f6493eab83e70d0679f1912559",
    },
}


def prepare_seeds(repo: Path, base: str, output: Path) -> dict:
    """Create the six independent one-commit seeds from `git archive` of the full `base`.

    Every `conditions/{C1,C2,C3}-{baseline,memory}/repo` holds identical sanitized bytes
    and its own `.git` with no remotes, alternates or original objects; memory conditions
    also get an empty `memory/` corpus. The source repository is only read. Fails if
    `output/conditions` exists and removes a partial result on any error.

    Returns the original-to-seed mapping, inventory, exclusions, AGENTS diff and receipts.
    """
    study.require_full_oid(base, "base")
    conditions = Path(output) / "conditions"
    conditions.mkdir(parents=True)
    try:
        report = build_seeds(Path(repo), base, conditions)
        audit_seeds(Path(output), base, report["seed_commit"], report["starting_inventory"])
    except BaseException:
        shutil.rmtree(conditions, ignore_errors=True)
        raise
    return report


def audit_seeds(root: Path, base: str, seed_commit: str, inventory: dict) -> None:
    """Verify the six seeds against the manifest values; raise ValueError on any mismatch.

    Checks identical inventories and modes, the seed commit, one-commit history without
    the original base, and an unshared standalone Git directory per seed.
    """
    for name in list_condition_names():
        repo = Path(root) / "conditions" / name / "repo"
        if repo.is_symlink() or not repo.is_dir():
            raise ValueError(f"{name}: seed repository is missing")
        verify_layout(repo.parent, name)
        verify_inventory(repo, inventory, name)
        verify_git_directory(repo, base, seed_commit, name)


def capture_slot(root: Path, slot: str) -> dict:
    """Capture the participant's exact code, corpus and full audit inventory of one slot.

    Must run before any evaluator touches anything. Bytes go to the content-addressed
    `operator/blobs` and the record to `operator/captures/SLOT.json`; a slot is captured
    once. Fails if the participant tree changes while it is being captured.
    """
    root = Path(root)
    condition = locate_condition(root, slot)
    before = audit_inventory(root / "conditions")
    parts = {"code": (condition / "repo", CODE_EXCLUSIONS),
             "memory": (condition / "memory", MEMORY_EXCLUSIONS)}
    blobs = root / "operator/blobs"
    blobs.mkdir(parents=True, exist_ok=True)
    capture = {"slot": slot, "modes": {}}
    for label, (directory, exclusions) in parts.items():
        capture[label] = snapshot(directory, exclusions)
        capture["modes"][label] = read_modes(directory, exclusions)
        preserve_bytes(directory, blobs, capture[label]["inventory"])
    capture["audit"] = before
    require_unchanged(root, capture, "capture")
    write_capture(root, capture)
    return capture


def create_home(root: Path, slot: str) -> Path:
    """Create the new empty HOME of one slot; a slot never reuses a HOME."""
    home = locate_condition(Path(root), slot) / "homes" / slot
    home.mkdir(parents=True)
    return home


def prepare_evaluation(root: Path, slot: str) -> Path:
    """Rebuild only the captured bytes and modes in a new `operator/evaluation/SLOT`.

    The copy has its own `repo` (with a fresh `.git`), `memory`, `target` and `home` and
    shares no inode with the participant. Requires a prior `capture_slot` and an unchanged
    participant; a half-built copy is removed on failure.
    """
    root = Path(root)
    capture = read_capture(root, slot)
    target = root / "operator/evaluation" / slot
    target.mkdir(parents=True)
    try:
        build_evaluation(root, target, capture)
        require_unchanged(root, capture)
    except BaseException:
        shutil.rmtree(target, ignore_errors=True)
        raise
    return target


def require_unchanged(root: Path, before: dict, phase: str = "evaluator execution") -> None:
    """Compare every participant byte and mode, `.git`, target and HOME with `before`.

    `before` is a `capture_slot` result. The audit covers all six conditions, so a write
    into a peer condition is caught as well. Raises ValueError naming the first change.
    """
    after = audit_inventory(Path(root) / "conditions")
    if after != before["audit"]:
        differing = set(map(tuple, after)) ^ set(map(tuple, before["audit"]))
        changed = sorted({row[0] for row in differing})
        raise ValueError(f"participant state changed during {phase}: {changed[0]}")


def preserve_bytes(directory: Path, blobs: Path, inventory: list) -> None:
    """Copy the listed files into the content-addressed store, verifying each hash."""
    for relative, expected in inventory:
        path = Path(directory) / relative
        data = read_regular_file(path)
        if hashlib.sha256(data).hexdigest() != expected:
            raise ValueError("participant bytes changed during capture")
        destination = Path(blobs) / expected
        if not destination.exists():
            destination.write_bytes(data)


def build_seeds(repo: Path, base: str, conditions: Path) -> dict:
    names = list_condition_names()
    first = conditions / names[0] / "repo"
    agents = extract_base(repo, base, first)
    inventory = snapshot(first, CODE_EXCLUSIONS)
    modes = read_modes(first, CODE_EXCLUSIONS)
    for name in names[1:]:
        shutil.copytree(first, conditions / name / "repo", symlinks=True)
    seeds = [create_seed_repository(conditions / name, base) for name in names]
    return {
        "original_base": base, "seed_commit": seeds[0]["seed_commit"],
        "starting_inventory": {
            "items": inventory["items"], "sha256": inventory["sha256"],
            "modes_sha256": hash_modes(modes), "exclusions": list(CODE_EXCLUSIONS)},
        "inventory": inventory["inventory"],
        "exclusions": {"archive": list(ARCHIVE_EXCLUSIONS), "inventory": list(CODE_EXCLUSIONS)},
        "agents_diff": agents["diff"], "agents_sha256": agents["sha256"],
        "seeds": seeds, "source_receipts": SOURCE_RECEIPTS,
    }


def list_condition_names() -> list[str]:
    return [f"{chain}-{condition}" for chain in CHAINS for condition in study.CONDITIONS]


def extract_base(repo: Path, base: str, destination: Path) -> dict:
    """Unpack `git archive base` minus the shared exclusions and sanitize AGENTS.md."""
    archive = run_git(repo, "archive", "--format=tar", base).stdout
    with tarfile.open(fileobj=io.BytesIO(archive)) as tar:
        members = [member for member in tar.getmembers()
                   if not is_excluded(member.name.rstrip("/"), ARCHIVE_EXCLUSIONS)]
        for member in members:
            check_archive_member(member)
        tar.extractall(destination, members=members, filter="data")
    return sanitize_agents(destination / "AGENTS.md")


def check_archive_member(member: tarfile.TarInfo) -> None:
    path = PurePosixPath(member.name)
    if not (member.isfile() or member.isdir()) or path.is_absolute() or ".." in path.parts:
        raise ValueError(f"unsafe archive member: {member.name}")


def sanitize_agents(path: Path) -> dict:
    """Neutralize host workflow in AGENTS.md identically for every seed.

    The Engram/host "Workspace and memory" section becomes the one pilot instruction and
    the Superpowers specs/plans bullet is dropped: it names an excluded directory and
    would be a mandatory cross-slot knowledge location. Design and style rules stay.
    """
    original = path.read_text(encoding="utf-8") if path.is_file() else ""
    match = WORKSPACE_SECTION.search(original)
    if match is None:
        raise ValueError("AGENTS.md has no 'Workspace and memory' section to replace")
    gap = "\n" if match.group(0).endswith("\n\n") else ""
    section = f"## Workspace\n\n- {PILOT_INSTRUCTION}\n{gap}"
    sanitized = original[:match.start()] + section + original[match.end():]
    sanitized, removed = SUPERPOWERS_BULLET.subn("", sanitized)
    if removed != 1:
        raise ValueError("AGENTS.md has no Superpowers specs/plans instruction to remove")
    path.write_text(sanitized, encoding="utf-8")
    diff = "".join(difflib.unified_diff(
        original.splitlines(True), sanitized.splitlines(True),
        "AGENTS.md (original)", "AGENTS.md (seed)"))
    return {"diff": diff, "sha256": {"original": sha256_text(original),
                                     "sanitized": sha256_text(sanitized)}}


def create_seed_repository(condition: Path, base: str) -> dict:
    """Give one condition its own one-commit Git directory and, for memory, an empty corpus."""
    repo = condition / "repo"
    commit = initialize_repository(repo, "TK-30 isolated seed")
    if condition.name.endswith("memory"):
        (condition / "memory").mkdir()
    return {"condition": condition.name, "original_commit": base, "seed_commit": commit,
            "git": read_git_receipt(repo)}


def read_git_receipt(repo: Path) -> dict:
    return {"head": read_git(repo, "rev-parse", "HEAD").strip(),
            "tree": read_git(repo, "rev-parse", "HEAD^{tree}").strip(),
            "commit_count": int(read_git(repo, "rev-list", "--all", "--count")),
            "remotes": read_git(repo, "remote").split(),
            "alternates": (repo / ".git/objects/info/alternates").exists()}


def verify_layout(condition: Path, name: str) -> None:
    """A fresh condition holds `repo/` and, for memory only, an empty real `memory/`.

    Build outputs (`target`, `.letopis`) and per-slot `homes/` do not exist before the
    first slot, so nothing can be shared or prefilled through them.
    """
    expected = {"repo", "memory"} if name.endswith("memory") else {"repo"}
    if {entry.name for entry in condition.iterdir()} != expected:
        raise ValueError(f"{name}: unexpected entries in a fresh condition")
    memory = condition / "memory"
    if memory.exists() and (memory.is_symlink() or any(memory.iterdir())):
        raise ValueError(f"{name}: memory corpus must be an empty real directory")
    for leftover in ("target", ".letopis"):
        if os.path.lexists(condition / "repo" / leftover):
            raise ValueError(f"{name}: fresh seed already has {leftover}")


def verify_inventory(repo: Path, expected: dict, name: str) -> None:
    observed = snapshot(repo, CODE_EXCLUSIONS)
    modes = hash_modes(read_modes(repo, CODE_EXCLUSIONS))
    if (observed["items"], observed["sha256"], modes) != (
            expected["items"], expected["sha256"], expected["modes_sha256"]):
        raise ValueError(f"{name}: seed inventory differs from the manifest")


def read_modes(root: Path, exclusions: tuple[str, ...]) -> dict[str, int]:
    """Return the Unix permission bits of the files `snapshot` would list."""
    return {relative: stat.S_IMODE(path.stat().st_mode)
            for relative, path in walk_files(root, exclusions)}


def verify_git_directory(repo: Path, base: str, seed_commit: str, name: str) -> None:
    git_dir = repo / ".git"
    if git_dir.is_symlink() or not git_dir.is_dir():
        raise ValueError(f"{name}: .git must be a standalone directory")
    if (git_dir / "commondir").exists() or (git_dir / "objects/info/alternates").exists():
        raise ValueError(f"{name}: Git directory borrows another object store")
    keys = [line.split("=", 1)[0] for line in
            read_git(repo, "config", "--local", "--list").splitlines()]
    if any(key.startswith(("remote.", "include", "branch.")) or key == "core.worktree"
           for key in keys):
        raise ValueError(f"{name}: Git configuration links to another repository")
    shared = [p for p in git_dir.rglob("*") if p.is_file() and p.stat().st_nlink > 1]
    if shared:
        raise ValueError(f"{name}: hard-linked Git file {shared[0].name}")
    history = (read_git(repo, "rev-parse", "HEAD").strip(),
               read_git(repo, "rev-list", "--all", "--count").strip())
    if history != (seed_commit, "1") or has_commit(repo, base):
        raise ValueError(f"{name}: history is not the single seed commit")


def locate_condition(root: Path, slot: str) -> Path:
    """Return the participant directory of a slot, refusing paths that leave the study root."""
    if SLOT_PATTERN.fullmatch(slot) is None:
        raise ValueError(f"unknown slot: {slot!r}")
    chain, _task, condition = slot.split("-")
    conditions = root / "conditions"
    path = conditions / f"{chain}-{condition}"
    if (conditions.resolve(strict=True) != root.resolve(strict=True) / "conditions"
            or path.is_symlink() or path.resolve(strict=True).parent != conditions.resolve()):
        raise ValueError("participant directory escapes the study root")
    return path


def audit_inventory(root: Path) -> list[list]:
    """List every entry below root as `[path, kind, mode]` with file hashes and link targets."""
    rows = []
    for directory, dirnames, filenames in os.walk(root):
        for name in dirnames + filenames:
            path = Path(directory) / name
            info = path.lstat()
            rows.append([path.relative_to(root).as_posix(), describe_entry(path, info),
                         stat.S_IMODE(info.st_mode)])
    return sorted(rows)


def describe_entry(path: Path, info: os.stat_result) -> str:
    if stat.S_ISDIR(info.st_mode):
        return "dir"
    if stat.S_ISLNK(info.st_mode):
        return "link:" + os.readlink(path)
    if stat.S_ISREG(info.st_mode):
        return "file:" + hash_file(path)
    return "special"


def write_capture(root: Path, capture: dict) -> None:
    path = root / "operator/captures" / (capture["slot"] + ".json")
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("x", encoding="utf-8") as stream:
        json.dump(capture, stream)


def read_capture(root: Path, slot: str) -> dict:
    if SLOT_PATTERN.fullmatch(slot) is None:
        raise ValueError(f"unknown slot: {slot!r}")
    path = root / "operator/captures" / (slot + ".json")
    if not path.is_file():
        raise ValueError(f"{slot} is not captured; capture precedes evaluation")
    return json.loads(path.read_text(encoding="utf-8"))


def build_evaluation(root: Path, target: Path, capture: dict) -> None:
    blobs = root / "operator/blobs"
    for label in ("code", "memory"):
        directory = target / ("repo" if label == "code" else "memory")
        restore_bytes(directory, blobs, capture[label]["inventory"], capture["modes"][label])
    (target / "target").mkdir()
    (target / "home").mkdir()
    initialize_repository(target / "repo", "Disposable TK-30 evaluation copy")
    if snapshot(target / "repo", CODE_EXCLUSIONS)["sha256"] != capture["code"]["sha256"]:
        raise ValueError("evaluation copy differs from the captured code")


def snapshot(root: Path, exclusions: tuple[str, ...]) -> dict:
    """Return the P-1 `{items, sha256, inventory}` of regular files below root.

    Rows are `[posix path, sha256(bytes)]` sorted by path, serialised as canonical UTF-8
    JSON plus LF. Symlinks and special files are rejected, never followed.
    """
    rows = [[relative, hash_file(path)] for relative, path in walk_files(root, exclusions)]
    canonical = json.dumps(rows, separators=(",", ":"), ensure_ascii=False) + "\n"
    return {"items": len(rows), "sha256": hashlib.sha256(canonical.encode("utf-8")).hexdigest(),
            "inventory": rows}


def initialize_repository(repo: Path, message: str) -> str:
    """Init a standalone repository with one commit; return its commit OID.

    Fixture Markdown stays on disk as test input but is excluded from the Git index, the
    same way for every seed and evaluation copy.
    """
    run_git(repo, "init", "-q", "--template=", "--initial-branch=main")
    run_git(repo, "config", "user.name", PARTICIPANT_NAME)
    run_git(repo, "config", "user.email", PARTICIPANT_EMAIL)
    run_git(repo, "config", "commit.gpgsign", "false")
    excluded = sorted((repo / FIXTURE_MARKDOWN).rglob("*.md"))
    exclude = repo / ".git/info/exclude"
    exclude.parent.mkdir(exist_ok=True)
    exclude.write_text("".join(f"/{p.relative_to(repo).as_posix()}\n" for p in excluded))
    run_git(repo, "add", ".")
    run_git(repo, "commit", "-q", "-m", message)
    return read_git(repo, "rev-parse", "HEAD").strip()


def restore_bytes(directory: Path, blobs: Path, inventory: list, modes: dict) -> None:
    """Write captured blobs as new files (never links) with their captured modes."""
    directory.mkdir(parents=True, exist_ok=True)
    for relative, expected in inventory:
        path = PurePosixPath(relative)
        if (path.is_absolute() or ".." in path.parts or ".git" in path.parts
                or not relative or "" in path.parts):
            raise ValueError(f"unsafe captured path: {relative!r}")
        mode = modes.get(relative)
        if not isinstance(mode, int):
            raise ValueError(f"captured mode is missing: {relative!r}")
        destination = directory / path
        destination.parent.mkdir(parents=True, exist_ok=True)
        destination.write_bytes(read_blob(blobs, expected))
        destination.chmod(mode)


def read_blob(blobs: Path, digest: str) -> bytes:
    blob = blobs / digest
    if SHA256_PATTERN.fullmatch(digest) is None or not blob.is_file():
        raise ValueError("captured blob is missing")
    data = blob.read_bytes()
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("captured blob mismatch")
    return data


def walk_files(root: Path, exclusions: tuple[str, ...]) -> list[tuple[str, Path]]:
    """List `(posix path, file)` below root sorted by path; reject symlinks and specials.

    Excluded entries are not listed or entered, but they are still type-checked, so an
    excluded name such as `target` cannot hide a symlink to a shared directory.
    """
    root = Path(root)
    for item in exclusions:
        check_exclusion(item)
    if root.is_symlink():
        raise ValueError("symlink is not an inventory input: root")
    found = []
    for directory, dirnames, filenames in os.walk(root):
        kept = []
        for name in dirnames + filenames:
            path = Path(directory) / name
            relative = path.relative_to(root).as_posix()
            kind = classify_entry(path, relative)
            if is_excluded(relative, exclusions):
                continue
            (kept if kind == "dir" else found).append(name if kind == "dir"
                                                       else (relative, path))
        dirnames[:] = kept
    return sorted(found)


def classify_entry(path: Path, relative: str) -> str:
    mode = path.lstat().st_mode
    if stat.S_ISLNK(mode):
        raise ValueError(f"symlink is not an inventory input: {relative}")
    if stat.S_ISDIR(mode):
        return "dir"
    if not stat.S_ISREG(mode):
        raise ValueError(f"special file is not an inventory input: {relative}")
    return "file"


def check_exclusion(item: str) -> None:
    path = PurePosixPath(item)
    if not item or path.is_absolute() or ".." in path.parts or "\\" in item:
        raise ValueError(f"exclusion must be a confined relative path: {item!r}")


def is_excluded(relative: str, exclusions: tuple[str, ...]) -> bool:
    return any(relative == item or relative.startswith(item + "/") for item in exclusions)


def read_regular_file(path: Path) -> bytes:
    if path.is_symlink() or not path.is_file():
        raise ValueError(f"participant file is not a regular file: {path.name}")
    return path.read_bytes()


def hash_file(path: Path) -> str:
    with Path(path).open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()


def hash_modes(modes: dict[str, int]) -> str:
    canonical = json.dumps(modes, sort_keys=True, separators=(",", ":"))
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def sha256_text(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def has_commit(repo: Path, oid: str) -> bool:
    result = subprocess.run(["git", "-C", str(repo), "cat-file", "-e", f"{oid}^{{commit}}"],
                            capture_output=True, env=GIT_ENVIRONMENT,
                            timeout=GIT_TIMEOUT_SECONDS)
    return result.returncode == 0


def read_git(repo: Path, *args: str) -> str:
    return run_git(repo, *args).stdout.decode("utf-8")


def run_git(repo: Path, *args: str) -> subprocess.CompletedProcess:
    result = subprocess.run(["git", "-C", str(repo), *args], capture_output=True,
                            env=GIT_ENVIRONMENT, timeout=GIT_TIMEOUT_SECONDS)
    if result.returncode != 0:
        raise ValueError(f"git {args[0]} failed")
    return result
