"""Model-free tests for TK-30 seeds, participant capture and disposable evaluation copies."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import uuid

import copies

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
RUN = HERE / "run.py"
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
EMPTY_SNAPSHOT_SHA256 = "37517e5f3dc66819f61f5a7bb8ace1921282415f10551d2defa5c3eb0985b570"
SHA_X = "2d711642b726b04401627ca9fbac32f5c8530fb1903cc4db02258717921a4881"
SHA_Y = "a1fce4363854ff888cff4b8e7875d600c2682390412a8cf79b37d0b11148b0fa"
SHA_Z_LINE = "c865f6c5ab8d1b0bcd383a5e1e3879d22681c96bf462c269b7581d523fbe70ab"
SLOT = "C1-T1-baseline"
MEMORY_SLOT = "C1-T1-memory"
CONDITION_DIRS = ["C1-baseline", "C1-memory", "C2-baseline", "C2-memory",
                  "C3-baseline", "C3-memory"]

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
AGENTS_SANITIZED = """# Agent instructions

## Start with the project documents

- Read the task card.
- Treat the architecture as the design source.

## Rust conventions

- Keep functions short.

## Workspace

- Work only inside this repository directory. Do not use the network, other directories on this machine or any other copy of the project, and leave unrelated files as they are.
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


class PreparedSeedsTestCase(TempDirTestCase):
    def prepare(self, files=None):
        self.repo, self.base = make_source_repo(self.tmp, files)
        self.root = self.tmp / "out"
        return copies.prepare_seeds(self.repo, self.base, self.root)

    def seed_repo(self, name="C1-baseline"):
        return self.root / "conditions" / name / "repo"


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
        self.assertEqual(self.result["exclusions"]["archive"][-4:],
                         ["docs/zkit/letopis/pilot", "tests/pilot_protocol",
                          "docs/superpowers", "tests/final_pilot"])

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
        # Then
        self.assertEqual(read_tree(self.source), self.source_before)

    def test_memory_corpus_starts_empty_only_for_memory_conditions(self):
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

    def condition(self, name):
        return self.root / "conditions" / name

    def replace_memory_with_symlink(self):
        shared = self.tmp / "shared-memory"
        shared.mkdir()
        (self.condition("C3-memory") / "memory").rmdir()
        (self.condition("C3-memory") / "memory").symlink_to(shared)

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


class CaptureTestCase(PreparedSeedsTestCase):
    def setUp(self):
        super().setUp()
        self.prepare()

    def participant(self, condition="C1-baseline"):
        return self.root / "conditions" / condition


class CaptureTests(CaptureTestCase):
    def test_capture_records_code_memory_modes_and_full_audit(self):
        # Given
        repo = self.participant("C1-memory") / "repo"
        write_tree(repo, {"src/new.rs": ("x", 0o644), "scripts/new.sh": ("y", 0o755),
                          "target/debug/out": ("z", 0o644)})
        write_tree(self.participant("C1-memory") / "memory", {"note.md": ("z\n", 0o644)})

        # When
        capture = copies.capture_slot(self.root, MEMORY_SLOT)

        # Then
        paths = [row[0] for row in capture["code"]["inventory"]]
        self.assertEqual(paths, sorted(SEED_PATHS + ["src/new.rs", "scripts/new.sh"]))
        self.assertEqual(capture["code"]["items"], 8)
        self.assertEqual(capture["memory"]["inventory"], [["note.md", SHA_Z_LINE]])
        self.assertEqual(capture["memory"]["sha256"], hashlib.sha256(
            f'[["note.md","{SHA_Z_LINE}"]]\n'.encode()).hexdigest())
        self.assertEqual(capture["modes"]["code"]["scripts/new.sh"], 0o755)
        audited = {row[0] for row in capture["audit"]}
        self.assertTrue({"C1-memory/repo/target/debug/out", "C1-memory/repo/.git/HEAD",
                         "C1-baseline/repo/.git/HEAD"} <= audited)

    def test_baseline_capture_has_the_canonical_empty_memory_snapshot(self):
        # When
        capture = copies.capture_slot(self.root, SLOT)

        # Then
        self.assertEqual((capture["memory"]["items"], capture["memory"]["sha256"]),
                         (0, EMPTY_SNAPSHOT_SHA256))

    def test_symlink_parent_slot_and_foreign_root_are_rejected(self):
        # Given
        outside = self.tmp / "outside"
        (outside / "repo").mkdir(parents=True)
        (self.participant() / "repo/escape").symlink_to(outside)

        # When / Then
        with self.assertRaisesRegex(ValueError, "symlink"):
            copies.capture_slot(self.root, SLOT)
        for slot in ("../C1-T1-baseline", "C1-T1-baseline/../x", "C9-T1-baseline", "C1-T4-memory"):
            with self.subTest(slot), self.assertRaises(ValueError):
                copies.capture_slot(self.root, slot)
        shutil.move(self.participant("C1-memory"), self.tmp / "moved")
        (self.participant("C1-memory")).symlink_to(self.tmp / "moved")
        with self.assertRaisesRegex(ValueError, "study root"):
            copies.capture_slot(self.root, MEMORY_SLOT)

    def test_bytes_changed_between_inventory_and_capture_are_rejected(self):
        # Given
        directory = self.tmp / "files"
        write_tree(directory, {"a": ("x", 0o644)})
        inventory = copies.snapshot(directory, ())["inventory"]
        (directory / "a").write_text("changed")
        blobs = self.tmp / "blobs"
        blobs.mkdir()

        # When / Then
        with self.assertRaisesRegex(ValueError, "changed during capture"):
            copies.preserve_bytes(directory, blobs, inventory)
        self.assertEqual(list(blobs.iterdir()), [])

    def test_homes_are_new_for_each_slot(self):
        # When
        first = copies.create_home(self.root, "C1-T1-baseline")
        second = copies.create_home(self.root, "C1-T2-baseline")

        # Then
        self.assertNotEqual(first, second)
        self.assertEqual((list(first.iterdir()), list(second.iterdir())), ([], []))
        with self.assertRaises(FileExistsError):
            copies.create_home(self.root, "C1-T1-baseline")


class EvaluationTests(CaptureTestCase):
    def test_copy_has_only_captured_bytes_modes_and_its_own_state(self):
        # Given
        repo = self.participant() / "repo"
        write_tree(repo, {"scripts/new.sh": ("y", 0o755), "target/debug/out": ("z", 0o644)})
        write_tree(self.participant() / "homes" / SLOT, {"auth.json": ("secret", 0o600)})
        capture = copies.capture_slot(self.root, SLOT)

        # When
        evaluation = copies.prepare_evaluation(self.root, SLOT)

        # Then
        self.assertEqual(evaluation, self.root / "operator/evaluation" / SLOT)
        copy = evaluation / "repo"
        restored = copies.snapshot(copy, copies.CODE_EXCLUSIONS)
        self.assertEqual(restored["sha256"], capture["code"]["sha256"])
        self.assertEqual((copy / "scripts/new.sh").stat().st_mode & 0o777, 0o755)
        self.assertEqual(sorted(p.name for p in evaluation.iterdir()),
                         ["home", "memory", "repo", "target"])
        self.assertEqual((list((evaluation / "home").iterdir()),
                          list((evaluation / "target").iterdir())), ([], []))
        self.assertFalse((copy / "target").exists())
        self.assert_independent_files(repo, copy)
        self.assertEqual(git(copy, "remote").strip(), "")
        self.assertFalse((copy / ".git/objects/info/alternates").exists())
        self.assertNotEqual(git(copy, "rev-parse", "HEAD").strip(), "")

    def assert_independent_files(self, source, copy):
        for relative in ("src/lib.rs", "scripts/new.sh"):
            original, restored = (source / relative).stat(), (copy / relative).stat()
            self.assertNotEqual((original.st_dev, original.st_ino),
                                (restored.st_dev, restored.st_ino))
            self.assertEqual(restored.st_nlink, 1)

    def test_capture_precedes_evaluation(self):
        # Given
        with self.assertRaisesRegex(ValueError, "not captured"):
            copies.prepare_evaluation(self.root, SLOT)
        t1 = copies.capture_slot(self.root, SLOT)
        evaluation = copies.prepare_evaluation(self.root, SLOT)

        # When: the evaluator edits its copy and a peer condition stays untouched.
        (evaluation / "repo/src/lib.rs").write_text("evaluator edit")
        write_tree(evaluation, {"repo/target/leak": ("x", 0o644), "home/h": ("x", 0o644)})
        t2 = copies.capture_slot(self.root, "C1-T2-baseline")

        # Then: T2 starts from the participant's own captured T1, not the copy.
        self.assertEqual(t2["code"], t1["code"])
        self.assertEqual((self.participant() / "repo/src/lib.rs").read_text(), "x")
        copies.require_unchanged(self.root, t1)

    def test_evaluator_cannot_write_back_and_every_unsafe_write_is_caught(self):
        # Given
        capture = copies.capture_slot(self.root, SLOT)
        repo = self.participant() / "repo"
        unsafe = {
            "worktree file": lambda: (repo / "src/lib.rs").write_text("leak"),
            "new file": lambda: (repo / "leak").write_text("x"),
            "target file": lambda: write_tree(repo, {"target/leak": ("x", 0o644)}),
            "home file": lambda: write_tree(self.participant(), {"homes/h/leak": ("x", 0o644)}),
            "git state": lambda: (repo / ".git/HEAD").write_text("ref: refs/heads/leak\n"),
            "mode only": lambda: (repo / "scripts/check.sh").chmod(0o644),
            "deleted audit file": lambda: (repo / "Cargo.toml").unlink(),
            "empty directory": lambda: (repo / "emptydir").mkdir(),
            "peer condition": lambda: write_tree(self.participant("C2-memory"),
                                                 {"repo/leak": ("x", 0o644)}),
            "symlink": lambda: (repo / "link").symlink_to("/"),
        }

        # When: a correctly isolated evaluation changes only its own copy.
        evaluation = copies.prepare_evaluation(self.root, SLOT)
        (evaluation / "repo/src/lib.rs").write_text("evaluator edit")
        copies.require_unchanged(self.root, capture)

        # Then: each shared-mount style write is detected.
        for name, write in unsafe.items():
            with self.subTest(name):
                snapshot = self.tmp / ("backup-" + name.replace(" ", "-"))
                shutil.copytree(self.root / "conditions", snapshot, symlinks=True)
                write()
                with self.assertRaises(ValueError):
                    copies.require_unchanged(self.root, capture)
                shutil.rmtree(self.root / "conditions")
                shutil.move(snapshot, self.root / "conditions")
                copies.require_unchanged(self.root, capture)

    def test_tampered_capture_is_rejected_by_the_matching_check(self):
        # Given
        copies.capture_slot(self.root, SLOT)
        record_path = self.root / "operator/captures" / (SLOT + ".json")
        original = record_path.read_text()
        tampers = {
            "parent path": (lambda r: add_row(r, "../escape"), "unsafe captured path"),
            "absolute path": (lambda r: add_row(r, "/abs"), "unsafe captured path"),
            "git path": (lambda r: add_row(r, ".git/config"), "unsafe captured path"),
            "blob mismatch": (lambda r: r["code"]["inventory"].__setitem__(
                0, [r["code"]["inventory"][0][0], SHA_Y]), "evaluation copy differs"),
        }

        def add_row(record, path):
            """Reference a real blob with a valid mode so only the path check can object."""
            record["code"]["inventory"].append([path, record["code"]["inventory"][0][1]])
            record["modes"]["code"][path] = 0o644

        # When / Then
        for name, (tamper, message) in tampers.items():
            with self.subTest(name):
                tampered = json.loads(original)
                tamper(tampered)
                record_path.write_text(json.dumps(tampered))
                with self.assertRaisesRegex(ValueError, message):
                    copies.prepare_evaluation(self.root, SLOT)
                self.assertFalse((self.root / "operator/evaluation" / SLOT).exists())

    def test_restore_never_writes_outside_the_target_directory(self):
        # Given: a real blob, so only the path check stands between the row and the write.
        source = self.tmp / "source-files"
        write_tree(source, {"a": ("x", 0o644)})
        blobs = self.tmp / "blobs"
        blobs.mkdir()
        inventory = copies.snapshot(source, ())["inventory"]
        copies.preserve_bytes(source, blobs, inventory)
        outside_absolute = self.tmp / "absolute-target"
        escapes = {"parent path": ("../escape", self.tmp / "work/escape"),
                   "absolute path": (str(outside_absolute), outside_absolute)}

        # When / Then
        for name, (row_path, landing) in escapes.items():
            with self.subTest(name):
                row = [[row_path, inventory[0][1]]]
                with self.assertRaisesRegex(ValueError, "unsafe captured path"):
                    copies.restore_bytes(self.tmp / "work/repo", blobs, row, {row_path: 0o644})
                self.assertFalse(landing.exists(), landing)

    def test_participant_changed_after_capture_is_rejected_by_the_copy(self):
        # Given
        copies.capture_slot(self.root, SLOT)
        (self.participant() / "repo/src/lib.rs").write_text("changed later")

        # When / Then
        with self.assertRaisesRegex(ValueError, "participant state changed"):
            copies.prepare_evaluation(self.root, SLOT)

    def test_a_slot_copy_cannot_be_prepared_twice(self):
        # Given
        copies.capture_slot(self.root, SLOT)
        copies.prepare_evaluation(self.root, SLOT)

        # When / Then
        with self.assertRaises(FileExistsError):
            copies.prepare_evaluation(self.root, SLOT)


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


class SeedCommandTests(TempDirTestCase):
    def setUp(self):
        super().setUp()
        self.repo, self.base = make_source_repo(self.tmp)

    def run_cli(self, *args):
        result = subprocess.run([sys.executable, "-B", str(RUN), *map(str, args)],
                                cwd=self.tmp, capture_output=True, text=True, timeout=120)
        return result.returncode, json.loads(result.stdout)

    def prepare(self, output):
        return self.run_cli("prepare", "--repo", self.repo, "--base", self.base,
                            "--output", output)

    def test_prepare_fills_the_seed_inputs_with_real_values(self):
        # Given
        output = self.tmp / "out"

        # When
        code, report = self.prepare(output)

        # Then
        manifest = json.loads((output / "manifest.json").read_text())
        seeds = json.loads((output / "seeds.json").read_text())
        head = git(output / "conditions/C2-memory/repo", "rev-parse", "HEAD").strip()
        self.assertEqual(code, 0)
        self.assertNotIn("seeds", report["pending"])
        self.assertEqual(manifest["seed_commit"], head)
        self.assertNotEqual(manifest["seed_commit"], self.base)
        self.assertEqual(manifest["starting_inventory"]["items"], 6)
        self.assertEqual(manifest["starting_inventory"]["sha256"], seeds["starting_inventory"]["sha256"])
        self.assertEqual(manifest["source_receipts"]["tk17_sources"]["commit"],
                         copies.SOURCE_RECEIPTS["commit"])
        self.assertEqual(manifest["model_executions"], 0)

    def test_audit_reports_seed_drift_and_missing_seeds_as_unready(self):
        # Given
        edits = {
            "edited file": lambda root: (root / "conditions/C3-baseline/repo/src/lib.rs").write_text("z"),
            "alternates": lambda root: (root / "conditions/C1-memory/repo/.git/objects/info/alternates").write_text("/x\n"),
            "missing seeds": lambda root: shutil.rmtree(root / "conditions"),
        }

        # When / Then
        for name, edit in edits.items():
            with self.subTest(name):
                output = self.tmp / name.replace(" ", "-")
                self.prepare(output)
                edit(output)
                code, report = self.run_cli("audit", "--root", output)
                self.assertEqual(code, 2)
                self.assertIn("seed_drift", report["pending"])
                self.assertEqual(report["state"], "unready")

    def test_audit_without_drift_does_not_report_seed_drift(self):
        # Given
        output = self.tmp / "out"
        self.prepare(output)

        # When
        code, report = self.run_cli("audit", "--root", output)

        # Then
        self.assertEqual(code, 2)
        self.assertNotIn("seed_drift", report["pending"])
        self.assertNotIn("seeds", report["pending"])


DOCKER_OUTPUT = os.environ.get("TK30_CONTAINER_OUTPUT")
DOCKER_IMAGE = os.environ.get("TK30_TEST_IMAGE", "letopis-tk17-codex:v2-0.164.0")
TOY_FILES = {
    "AGENTS.md": (AGENTS_ORIGINAL, 0o644),
    "Cargo.toml": ('[package]\nname="tk30_copy_probe"\nversion="0.0.0"\nedition="2024"\n', 0o644),
    "src/lib.rs": ("pub fn get_number() -> u8 { 7 }\n", 0o644),
    "build.rs": ('fn main() { let data = std::fs::read("/oracle/marker").unwrap();\n'
                 'let target = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR")'
                 '.unwrap()).join("target"); std::fs::create_dir_all(&target).unwrap();\n'
                 'std::fs::write(target.join("leak-attempt"), data).unwrap(); }\n', 0o644),
}
MARKER = "TK30_PRIVATE_ORACLE_MARKER"
DOCKER_BASE = ["docker", "run", "--rm", "--network", "none", "--cap-drop", "ALL",
               "--security-opt", "no-new-privileges"]


@unittest.skipUnless(DOCKER_OUTPUT, "set TK30_CONTAINER_OUTPUT to a new private evidence "
                                    "directory to run the model-free Docker boundary proof")
class ContainerBoundaryTests(unittest.TestCase):
    def test_build_script_writes_only_the_disposable_copy(self):
        # Given
        root = Path(DOCKER_OUTPUT).resolve()
        root.mkdir(mode=0o700, parents=True, exist_ok=False)
        (root / "source-parent").mkdir()
        source, base = make_source_repo(root / "source-parent", TOY_FILES)
        study = root / "study"
        copies.prepare_seeds(source, base, study)
        capture = copies.capture_slot(study, SLOT)
        evaluation = copies.prepare_evaluation(study, SLOT)
        oracle = root / "oracle"
        oracle.mkdir()
        (oracle / "marker").write_text(MARKER)
        run_id = uuid.uuid4().hex[:12]

        # When
        build = run_evaluator_build(run_id, evaluation, oracle, root / "build.log")
        denial = run_future_slot_probe(run_id, study, source, root / "t2-denial.log")
        unsafe = run_shared_mount_write(run_id, study, oracle, root / "unsafe.log")

        # Then
        self.assertEqual(build.returncode, 0, (root / "build.log").read_text())
        self.assertEqual((evaluation / "repo/target/leak-attempt").read_text(), MARKER)
        self.assertEqual(denial.returncode, 0, (root / "t2-denial.log").read_text())
        self.assertEqual(unsafe.returncode, 0)
        with self.assertRaises(ValueError):
            copies.require_unchanged(study, capture)
        self.assert_owned_timeout_leaves_no_container(root)
        (root / "receipt.json").write_text(json.dumps({
            "status": "PASS", "model_executions": 0, "image": DOCKER_IMAGE,
            "tk17_sources_commit": copies.SOURCE_RECEIPTS["commit"],
            "build_script_writeback_only_disposable": True,
            "future_t2_marker_denied": True, "unsafe_shared_mount_detected": True,
            "owned_timeout_removed": True}, indent=2) + "\n")

    def assert_owned_timeout_leaves_no_container(self, root):
        run_id = uuid.uuid4().hex[:12]
        listing = ["docker", "ps", "-a", "--filter", f"label=tk30-run={run_id}", "--format",
                   "{{.Names}}"]
        with self.assertRaises(subprocess.TimeoutExpired):
            run_owned_container(run_id, ["--entrypoint", "sh", DOCKER_IMAGE, "-c", "sleep 30"],
                                root / "timeout.log", 2)
        self.assertEqual(subprocess.check_output(listing, text=True), "")


def run_evaluator_build(run_id, evaluation, oracle, log):
    """Build the evaluation copy with the private oracle mounted and a fresh cargo home."""
    return run_owned_container(run_id, [
        "-v", f"{evaluation / 'repo'}:/workspace", "-v", f"{evaluation / 'target'}:/operator-target",
        "-v", f"{evaluation / 'home'}:/home/eval", "-v", f"{oracle}:/oracle:ro",
        "--workdir", "/workspace", "--env", "CARGO_HOME=/tmp/fresh-cargo",
        "--env", "CARGO_TARGET_DIR=/operator-target", "--entrypoint", "sh", DOCKER_IMAGE,
        "-c", "cargo build --offline --quiet"], log, 180)


def run_future_slot_probe(run_id, study, source, log):
    """Mount only what the next slot gets and assert no oracle, target or source is visible."""
    home = copies.create_home(study, "C1-T2-baseline")
    probe = ("test ! -e /oracle && test ! -e /operator-target "
             "&& test ! -e /workspace/target/leak-attempt "
             f"&& test ! -e {source} && ! grep -rq {MARKER} /workspace /home/pilot")
    return run_owned_container(run_id, [
        "-v", f"{study / 'conditions/C1-baseline/repo'}:/workspace", "-v", f"{home}:/home/pilot",
        "--workdir", "/workspace", "--entrypoint", "sh", DOCKER_IMAGE, "-c", probe], log, 60)


def run_shared_mount_write(run_id, study, oracle, log):
    """Unsafe control: the oracle is copied into the participant repository itself."""
    return run_owned_container(run_id, [
        "-v", f"{study / 'conditions/C1-baseline/repo'}:/workspace", "-v", f"{oracle}:/oracle:ro",
        "--entrypoint", "sh", DOCKER_IMAGE, "-c",
        "mkdir -p /workspace/target && cp /oracle/marker /workspace/target/leak-attempt"], log, 60)


def run_owned_container(run_id, arguments, log, timeout):
    """Run one labelled container; on timeout stop and remove only that exact name."""
    name = f"tk30-eval-{run_id}-{uuid.uuid4().hex[:6]}"
    command = [*DOCKER_BASE, "--name", name, "--label", f"tk30-run={run_id}", *arguments]
    with log.open("wb") as output:
        try:
            return subprocess.run(command, stdout=output, stderr=output, timeout=timeout)
        except subprocess.TimeoutExpired:
            subprocess.run(["docker", "rm", "-f", name], capture_output=True, timeout=30)
            raise


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
