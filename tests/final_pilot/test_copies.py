"""TK-30 test copies; model-free. Shared fixtures are private."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest
import copies
from _copy_fixtures import (
    AGENTS_ORIGINAL,
    BASE_FILES,
    EMPTY_SNAPSHOT_SHA256,
    GIT_ENV,
    HERE,
    PreparedSeedsTestCase,
    SEED_PATHS,
    SHA_Y,
    SUPERPOWERS_BULLET,
    TempDirTestCase,
    commit,
    git,
    make_source_repo,
    write_tree,
)


REPO = HERE.parent.parent


SELECTION = REPO / "docs/zkit/letopis/pilot/final/selection.json"


SHA_X = "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"


CONDITION_DIRS = ["C1-baseline", "C1-memory", "C2-baseline", "C2-memory",
                  "C3-baseline", "C3-memory"]


AGENTS_SANITIZED = """# Agent instructions

## Start with the project documents

- Read the task card.
- Treat the architecture as the design source.

## Rust conventions

- Keep functions short.

## Workspace

- Work only inside this repository directory. Do not use the network, other directories on this machine or any other copy of the project, and leave unrelated files as they are.
"""


class SnapshotTests(TempDirTestCase):
    def test_empty_snapshot_matches_literal_p1_hash(self):
        # When
        result = copies.snapshot(self.tmp, ())

        # Then
        self.assertEqual(result, {"items": 0, "sha256": EMPTY_SNAPSHOT_SHA256,
                                  "inventory": []})

    def test_rows_are_sorted_by_posix_path_with_canonical_hash(self):
        # Given
        write_tree(self.tmp, {"a/b": ("x", 0o644), "a-b": ("y", 0o644)})
        canonical = f'[["a-b","{SHA_Y}"],["a/b","{SHA_X}"]]\n'

        # When
        result = copies.snapshot(self.tmp, ())

        # Then
        self.assertEqual(result["inventory"], [["a-b", SHA_Y], ["a/b", SHA_X]])
        self.assertEqual(result["sha256"], hashlib.sha256(canonical.encode()).hexdigest())

    def test_exclusions_match_whole_path_components_only(self):
        # Given
        write_tree(self.tmp, {name: ("x", 0o644) for name in
                              ("target/a", "targets/b", ".git/c", "src/target/d")})

        # When
        result = copies.snapshot(self.tmp, (".git", "target"))

        # Then
        self.assertEqual([row[0] for row in result["inventory"]],
                         ["src/target/d", "targets/b"])

    def test_symlink_escape_is_rejected(self):
        # Given
        outside = self.tmp / "outside"
        outside.mkdir()
        (outside / "secret").write_text("x")
        for kind, link_target in (("file", outside / "secret"), ("dir", outside)):
            with self.subTest(kind):
                root = self.tmp / ("root-" + kind)
                root.mkdir()
                (root / "link").symlink_to(link_target)

                # When / Then
                with self.assertRaisesRegex(ValueError, "symlink"):
                    copies.snapshot(root, ())

    def test_excluded_symlink_or_special_entry_is_still_rejected(self):
        # Given
        outside = self.tmp / "outside"
        outside.mkdir()
        cases = {"symlink": lambda path: path.symlink_to(outside),
                 "fifo": lambda path: os.mkfifo(path)}
        for kind, create in cases.items():
            with self.subTest(kind):
                root = self.tmp / ("root-" + kind)
                root.mkdir()
                create(root / "target")

                # When / Then
                with self.assertRaises(ValueError):
                    copies.snapshot(root, ("target",))

    def test_parent_or_absolute_exclusion_is_rejected(self):
        # When / Then
        for bad in ("../x", "/abs", "a/../b", ""):
            with self.subTest(bad), self.assertRaises(ValueError):
                copies.snapshot(self.tmp, (bad,))

    def test_modes_are_reported_apart_from_the_byte_hash(self):
        # Given
        write_tree(self.tmp, {"run.sh": ("x", 0o755), "plain": ("x", 0o644)})
        before = copies.snapshot(self.tmp, ())["sha256"]

        # When
        modes = copies.read_modes(self.tmp, ())
        (self.tmp / "plain").chmod(0o755)

        # Then
        self.assertEqual(modes, {"plain": 0o644, "run.sh": 0o755})
        self.assertEqual(copies.snapshot(self.tmp, ())["sha256"], before)


class SeedTests(PreparedSeedsTestCase):
    @classmethod
    def setUpClass(cls):
        cls.shared = Path(tempfile.mkdtemp())
        cls.addClassCleanup(shutil.rmtree, cls.shared, True)
        repo, cls.base = make_source_repo(cls.shared)
        cls.source_before = read_tree(repo)
        cls.root = cls.shared / "out"
        cls.result = copies.prepare_seeds(repo, cls.base, cls.root)
        cls.source = repo

    def setUp(self):
        pass

    def test_six_seeds_share_one_inventory_and_one_set_of_modes(self):
        # When
        snapshots = [copies.snapshot(self.seed_repo(name), copies.CODE_EXCLUSIONS)
                     for name in CONDITION_DIRS]
        modes = [copies.read_modes(self.seed_repo(name), copies.CODE_EXCLUSIONS)
                 for name in CONDITION_DIRS]

        # Then
        self.assertEqual(sorted(p.name for p in (self.root / "conditions").iterdir()),
                         CONDITION_DIRS)
        self.assertEqual(len({json.dumps(s, sort_keys=True) for s in snapshots}), 1)
        self.assertEqual([row[0] for row in snapshots[0]["inventory"]], SEED_PATHS)
        self.assertEqual(len({json.dumps(m, sort_keys=True) for m in modes}), 1)
        self.assertEqual(self.result["starting_inventory"]["sha256"], snapshots[0]["sha256"])
        self.assertEqual(self.result["starting_inventory"]["items"], 6)

    def test_seed_derives_from_base_and_drops_operator_and_host_state(self):
        # When
        present = sorted(p.relative_to(self.seed_repo()).as_posix()
                         for p in self.seed_repo().rglob("*")
                         if p.is_file() and ".git" not in p.relative_to(self.seed_repo()).parts)

        # Then
        self.assertEqual(present, SEED_PATHS)
        self.assertEqual(self.result["exclusions"]["archive"][-8:],
                         ["docs/zkit/letopis/pilot", "tests/pilot_protocol",
                          "docs/superpowers", "tests/final_pilot",
                          "docs/zkit/letopis/checks/TK-30", "docs/zkit/letopis/checks/TK-31",
                          "docs/zkit/letopis/checks/TK-32", "docs/zkit/letopis/checks/TK-33"])

    def test_seed_has_no_frozen_check_or_agreement_id(self):
        # Given: every rubric identifier of the frozen selection; `regression` is a plain word
        selection = json.loads(SELECTION.read_text(encoding="utf-8"))
        tasks = [task for chain in selection["chains"] for task in chain["tasks"]]
        secrets = ({check for task in tasks for check in task["check_ids"]}
                   | set(selection["agreement_catalog"])) - {"regression"}

        # When
        leaks = sorted((path.relative_to(self.root).as_posix(), secret)
                       for path in (self.root / "conditions").rglob("*")
                       if path.is_file() and ".git" not in path.parts
                       for secret in secrets if secret in path.read_text(errors="replace"))

        # Then
        self.assertEqual(leaks, [])

    def test_executable_mode_survives_and_fixture_markdown_is_physical_but_untracked(self):
        # When
        mode = (self.seed_repo() / "scripts/check.sh").stat().st_mode & 0o777
        tracked = git(self.seed_repo(), "ls-files", "--stage").splitlines()

        # Then
        self.assertEqual(mode, 0o755)
        self.assertIn("100755", [line.split()[0] for line in tracked if "check.sh" in line])
        self.assertTrue((self.seed_repo() / "tests/fixtures/markdown/note.md").is_file())
        self.assertFalse(any("note.md" in line for line in tracked))
        self.assertEqual(git(self.seed_repo(), "status", "--porcelain"), "")

    def test_agents_replace_only_the_workspace_section_and_record_the_diff(self):
        # When
        text = (self.seed_repo() / "AGENTS.md").read_text()
        diff = self.result["agents_diff"]

        # Then
        self.assertEqual(text, AGENTS_SANITIZED)
        self.assertNotIn("superpowers", text.lower())
        self.assertIn("\n-" + SUPERPOWERS_BULLET.rstrip("\n"), diff)
        self.assertIn("\n-- Use the existing Engram store through its CLI.", diff)
        self.assertIn("\n+- Work only inside this repository directory.", diff)
        self.assertNotIn("Keep functions short", "".join(
            line for line in diff.splitlines() if line[:1] in "+-" and line[:3] not in ("+++", "---")))
        self.assertEqual(self.result["agents_sha256"]["sanitized"],
                         hashlib.sha256(AGENTS_SANITIZED.encode()).hexdigest())

    def test_git_dirs_are_independent_one_commit_stores(self):
        # When / Then
        inodes = []
        for name in CONDITION_DIRS:
            repo = self.seed_repo(name)
            git_dir = repo / ".git"
            self.assertTrue(git_dir.is_dir() and not git_dir.is_symlink())
            self.assertFalse((git_dir / "objects/info/alternates").exists())
            self.assertFalse((git_dir / "commondir").exists())
            self.assertEqual(git(repo, "remote").strip(), "")
            self.assertEqual(git(repo, "rev-list", "--all", "--count").strip(), "1")
            self.assertEqual(git(repo, "rev-parse", "HEAD").strip(), self.result["seed_commit"])
            absent = subprocess.run(["git", "-C", str(repo), "cat-file", "-e",
                                     self.base + "^{commit}"], env=GIT_ENV, capture_output=True)
            self.assertNotEqual(absent.returncode, 0)
            inodes.append({(p.stat().st_dev, p.stat().st_ino) for p in git_dir.rglob("*")
                           if p.is_file()})
        self.assertEqual(sum(len(s) for s in inodes), len(set().union(*inodes)))

    def test_mapping_records_original_to_seed_and_never_reuses_the_original_oid(self):
        # When
        mapping = self.result["seeds"]

        # Then
        self.assertEqual(self.result["original_base"], self.base)
        self.assertNotEqual(self.result["seed_commit"], self.base)
        self.assertEqual([m["condition"] for m in mapping], CONDITION_DIRS)
        self.assertEqual({m["original_commit"] for m in mapping}, {self.base})
        self.assertEqual({m["seed_commit"] for m in mapping}, {self.result["seed_commit"]})
        self.assertEqual(mapping[0]["git"]["commit_count"], 1)
        self.assertEqual(mapping[0]["git"]["remotes"], [])

    def test_source_repository_is_untouched(self):
        # Given/When: setUpClass prepares the seeds from the source repository
        # Then
        self.assertEqual(read_tree(self.source), self.source_before)

    def test_memory_corpus_starts_empty_only_for_memory_conditions(self):
        # Given/When: setUpClass prepares the seeds
        # Then
        for name in CONDITION_DIRS:
            memory = self.root / "conditions" / name / "memory"
            self.assertEqual(memory.is_dir(), name.endswith("memory"), name)
            if memory.is_dir():
                self.assertEqual(list(memory.iterdir()), [])


class SeedRejectionTests(PreparedSeedsTestCase):
    def test_short_base_missing_section_and_existing_output_are_rejected(self):
        # Given
        repo, base = make_source_repo(self.tmp)
        bare_parent = self.tmp / "bare"
        bare_parent.mkdir()
        bare, bare_base = make_source_repo(bare_parent, {"x": ("x", 0o644)})

        # When / Then
        with self.assertRaises(ValueError):
            copies.prepare_seeds(repo, base[:7], self.tmp / "o1")
        with self.assertRaisesRegex(ValueError, "Workspace and memory"):
            copies.prepare_seeds(bare, bare_base, self.tmp / "o2")
        no_bullet_parent = self.tmp / "nobullet"
        no_bullet_parent.mkdir()
        no_bullet, no_bullet_base = make_source_repo(no_bullet_parent, {
            "AGENTS.md": ("# Rules\n\n## Workspace and memory\n\n- Use Engram.\n", 0o644)})
        with self.assertRaisesRegex(ValueError, "Superpowers"):
            copies.prepare_seeds(no_bullet, no_bullet_base, self.tmp / "o4")
        existing = self.tmp / "o3"
        (existing / "conditions").mkdir(parents=True)
        with self.assertRaises(FileExistsError):
            copies.prepare_seeds(repo, base, existing)


class AuditSeedTests(PreparedSeedsTestCase):
    def test_untouched_seeds_pass(self):
        # Given
        result = self.prepare()

        # When / Then
        self.audit(result)

    def test_each_tamper_is_detected(self):
        # Given
        tampers = {
            "edited file": lambda: (self.seed_repo("C2-memory") / "src/lib.rs").write_text("z"),
            "mode change": lambda: (self.seed_repo("C3-baseline") / "src/lib.rs").chmod(0o755),
            "extra file": lambda: (self.seed_repo() / "extra").write_text("x"),
            "alternates": lambda: (self.seed_repo() / ".git/objects/info/alternates").write_text(
                str(self.tmp) + "\n"),
            "remote": lambda: git(self.seed_repo("C2-baseline"), "remote", "add", "o", "/x"),
            "shared object": self.share_object_between_two_seeds,
            "missing seed": lambda: shutil.rmtree(self.seed_repo("C3-memory")),
            "prefilled memory": lambda: write_tree(self.condition("C1-memory"),
                                                   {"memory/prefilled.md": ("x", 0o644)}),
            "memory in baseline": lambda: (self.condition("C1-baseline") / "memory").mkdir(),
            "memory is a symlink": self.replace_memory_with_symlink,
            "missing memory": lambda: (self.condition("C2-memory") / "memory").rmdir(),
            "unexpected homes": lambda: (self.condition("C3-baseline") / "homes").mkdir(),
            "real target directory": lambda: (self.seed_repo("C2-baseline") / "target").mkdir(),
            "real letopis directory": lambda: (self.seed_repo("C2-memory") / ".letopis").mkdir(),
            "symlinked target": lambda: self.link_into_shared(["C1-baseline"], "target"),
            "symlinked letopis": lambda: self.link_into_shared(["C1-baseline"], ".letopis"),
            "target shared by two conditions": lambda: self.link_into_shared(
                ["C1-baseline", "C1-memory"], "target"),
        }

        # When / Then
        for name, tamper in tampers.items():
            with self.subTest(name):
                result = self.prepare()
                tamper()
                try:
                    with self.assertRaises(ValueError):
                        self.audit(result)
                finally:
                    shutil.rmtree(self.root, ignore_errors=True)
                    shutil.rmtree(self.repo, ignore_errors=True)

    def audit(self, result):
        copies.audit_seeds(self.root, result["original_base"], result["seed_commit"],
                           result["starting_inventory"])

    def replace_memory_with_symlink(self):
        shared = self.tmp / "shared-memory"
        shared.mkdir()
        (self.condition("C3-memory") / "memory").rmdir()
        (self.condition("C3-memory") / "memory").symlink_to(shared)

    def condition(self, name):
        return self.root / "conditions" / name

    def link_into_shared(self, names, entry):
        shared = self.tmp / "shared-dir"
        shared.mkdir(exist_ok=True)
        for name in names:
            (self.seed_repo(name) / entry).symlink_to(shared)

    def share_object_between_two_seeds(self):
        first = self.seed_repo("C1-baseline") / ".git/objects"
        second = self.seed_repo("C1-memory") / ".git/objects"
        victim = next(p for p in sorted(first.glob("??/*")))
        twin = second / victim.relative_to(first)
        twin.unlink()
        os.link(victim, twin)


class SourceReceiptTests(unittest.TestCase):
    def test_receipts_match_the_pinned_tk17_blobs(self):
        # Given
        commit = copies.SOURCE_RECEIPTS["commit"]
        present = subprocess.run(["git", "-C", str(REPO), "cat-file", "-e", commit + "^{commit}"],
                                 env=GIT_ENV)
        if present.returncode != 0:
            self.skipTest("pinned TK-17 commit is not in this repository")

        # When / Then
        self.assertEqual(sorted(copies.SOURCE_RECEIPTS["files"]),
                         ["tests/early_pilot/copies.py", "tests/early_pilot/evidence.py",
                          "tests/early_pilot/run.py", "tests/early_pilot/study.py",
                          "tests/early_pilot/test_containers.py"])
        for path, expected in copies.SOURCE_RECEIPTS["files"].items():
            blob = subprocess.run(["git", "-C", str(REPO), "show", f"{commit}:{path}"],
                                  capture_output=True, check=True, env=GIT_ENV).stdout
            self.assertEqual(hashlib.sha256(blob).hexdigest(), expected, path)
        self.assertEqual(len(commit), 40)


def read_tree(root):
    """Independent byte/mode listing of a directory, used to prove nothing was touched."""
    listing = {}
    for directory, _dirs, names in os.walk(root):
        for name in names:
            path = Path(directory) / name
            listing[str(path.relative_to(root))] = (path.read_bytes(), path.stat().st_mode)
    return listing


if __name__ == "__main__":
    unittest.main()
