"""TK-30  copy fixtures; model-free. Shared fixtures are private."""
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import copies


HERE = Path(__file__).resolve().parent


GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}


EMPTY_SNAPSHOT_SHA256 = "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570"


SHA_Y = "a1fce4363854ff888cff4b8e7875d600c2682390412a8cf79b37d0b11148b0fa"


SLOT = "C1-T1-baseline"


SUPERPOWERS_BULLET = (
    "- Keep agreed specifications and implementation plans in `docs/superpowers/specs/` and "
    "`docs/superpowers/plans/`. When the user requests Superpowers, follow its applicable "
    "workflow and preserve the resulting decisions in those documents.\n")


AGENTS_ORIGINAL = f"""# Agent instructions

## Start with the project documents

- Read the task card.
{SUPERPOWERS_BULLET}- Treat the architecture as the design source.

## Rust conventions

- Keep functions short.

## Workspace and memory

- Inspect the current branch.
- Use the existing Engram store through its CLI.
"""


BASE_FILES = {
    "AGENTS.md": (AGENTS_ORIGINAL, 0o644),
    "Cargo.toml": ("x", 0o644),
    "src/lib.rs": ("x", 0o644),
    "scripts/check.sh": ("#!/bin/sh\n", 0o755),
    "tests/fixtures/markdown/note.md": ("y", 0o644),
    "docs/zkit/letopis/design.md": ("x", 0o644),
    "docs/zkit/letopis/pilot/protocol.md": ("operator", 0o644),
    "tests/pilot_protocol/check.py": ("operator", 0o644),
    "docs/superpowers/specs/s.md": ("operator", 0o644),
    "tests/final_pilot/x.py": ("operator", 0o644),
    "docs/zkit/letopis/checks/TK-30/report.md": (
        "version_value two_requests_before_release hexagonal_sync", 0o644),
    "docs/zkit/letopis/checks/TK-33/verification.json": ('{"bounded_buffer": 1}', 0o644),
    ".superpowers/n": ("host", 0o644),
    ".artifacts/n": ("host", 0o644),
    ".engram/db": ("host", 0o644),
    ".agents/a": ("host", 0o644),
    "target/t": ("build", 0o644),
}


SEED_PATHS = ["AGENTS.md", "Cargo.toml", "docs/zkit/letopis/design.md", "scripts/check.sh",
              "src/lib.rs", "tests/fixtures/markdown/note.md"]


class TempDirTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)


class PreparedSeedsTestCase(TempDirTestCase):
    def prepare(self, files=None):
        self.repo, self.base = make_source_repo(self.tmp, files)
        self.root = self.tmp / "out"
        return copies.prepare_seeds(self.repo, self.base, self.root)

    def seed_repo(self, name="C1-baseline"):
        return self.root / "conditions" / name / "repo"


def make_source_repo(parent, files=None):
    """Create a repository with a base commit and a later HEAD commit; return both OIDs."""
    repo = parent / "source"
    repo.mkdir()
    git(repo, "init", "-q")
    write_tree(repo, BASE_FILES if files is None else files)
    git(repo, "add", "-f", ".")
    commit(repo, "base")
    base = git(repo, "rev-parse", "HEAD").strip()
    (repo / "LATER.txt").write_text("later")
    git(repo, "add", "LATER.txt")
    commit(repo, "later")
    return repo, base


def commit(repo, message):
    git(repo, "-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-q",
        "-m", message)


def git(repo, *args):
    return subprocess.run(["git", "-C", str(repo), *args], check=True, capture_output=True,
                          text=True, env=GIT_ENV).stdout


def write_tree(root, files):
    for relative, (text, mode) in files.items():
        path = root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text)
        path.chmod(mode)
