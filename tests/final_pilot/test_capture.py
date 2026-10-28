"""TK-30 test capture; model-free. Shared fixtures are private."""
import hashlib
import json
import shutil
import copies
from _copy_fixtures import (
    AGENTS_ORIGINAL,
    BASE_FILES,
    EMPTY_SNAPSHOT_SHA256,
    GIT_ENV,
    PreparedSeedsTestCase,
    SEED_PATHS,
    SHA_Y,
    SLOT,
    SUPERPOWERS_BULLET,
    TempDirTestCase,
    commit,
    git,
    make_source_repo,
    write_tree,
)
import unittest


SHA_Z_LINE = "c865f6c5ab8d1b0bcd383a5e1e3879d22681c96bf462c269b7581d523fbe70ab"


MEMORY_SLOT = "C1-T1-memory"


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

    def assert_independent_files(self, source, copy):
        for relative in ("src/lib.rs", "scripts/new.sh"):
            original, restored = (source / relative).stat(), (copy / relative).stat()
            self.assertNotEqual((original.st_dev, original.st_ino),
                                (restored.st_dev, restored.st_ino))
            self.assertEqual(restored.st_nlink, 1)


if __name__ == "__main__":
    unittest.main()
