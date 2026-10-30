"""Independent fixture and binary identity assertions."""

import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch
from types import SimpleNamespace
import json

from scenarios import (ROOT, build_release, build_traversal_oracle, prepare_existing_fixture, prepare_fixture, prepare_scenario,
                       read_product_inputs, run_git, verify_build)


class ScenarioTests(unittest.TestCase):
    def test_packing_control_preserves_target_bytes_and_source_tree(self):
        # Given: identical seed/count/fragment bytes with different file packing.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            original = prepare_fixture(root / "thirty", 60)
            # When: varying only the generator's elements-per-file setting.
            packed = prepare_fixture(root / "ten", 60, elements_per_file=10)
            # Then: the addressed first fragment and author tree remain identical.
            self.assertEqual(packed["packing"]["file_count"], 6)
            self.assertEqual(packed["counts"]["edges"], 300)
            self.assertEqual(packed["source_tree"], original["source_tree"])
            self.assertEqual((original["repository"] / "knowledge/part-0000.md").read_bytes()[:2000],
                             (packed["repository"] / "knowledge/part-0000.md").read_bytes()[:2000])

    def test_reuses_only_verified_disabled_synthetic_fixture_copy(self):
        # Given: a generated model-free fixture with independently verifiable bytes.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            original = prepare_fixture(root / "original", 60)
            # When: copying an initialized or generated fixture for a later stage.
            copied = prepare_existing_fixture(root / "original/corpus.json", root / "copy")
            # Then: canonical bytes match while the original remains untouched.
            self.assertEqual(copied["input_sha256"], original["input_sha256"])
            self.assertNotEqual(copied["repository"], original["repository"])
            config = original["repository"] / ".letopis/config.json"
            value = json.loads(config.read_text()); value["embedding_model"]["enabled"] = True
            config.write_text(json.dumps(value))
            with self.assertRaises(ValueError):
                prepare_existing_fixture(root / "original/corpus.json", root / "rejected")
    def test_build_receipt_owns_immutable_executable_copy(self):
        # Given: Cargo's target binary can later be replaced by another build.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            artifact = root / "target-binary"
            artifact.write_bytes(b"release original")
            message = {"reason": "compiler-artifact", "target": {"name": "letopis"}, "executable": str(artifact)}
            # When: pinning a completed release artifact then replacing Cargo's output.
            with patch("scenarios.read_product_inputs", return_value={"src/pinned": "x"}), \
                    patch("scenarios.run_git", return_value=b"1" * 40), \
                    patch("scenarios.subprocess.check_output", return_value="rustc fixture"), \
                    patch("scenarios.subprocess.run", return_value=SimpleNamespace(stdout=json.dumps(message).encode())):
                pin = build_release(ROOT, root / "target", root / "evidence")
                artifact.write_bytes(b"different build")
                # Then: later builds cannot invalidate an earlier measurement's executable.
                self.assertNotEqual(Path(pin["executable"]), artifact)
                self.assertEqual(Path(pin["executable"]).read_bytes(), b"release original")
                verify_build(pin)
    def test_missing_build_provenance_cannot_be_measured(self):
        # Given: an executable hash without any evidence of a release build.
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "binary"
            binary.write_bytes(b"original")
            # When / Then: hashing alone does not establish source/build pairing.
            with self.assertRaises(ValueError):
                verify_build({"executable": str(binary),
                              "sha256": hashlib.sha256(b"original").hexdigest()})
    def test_fixed_neighborhood_includes_the_next_element(self):
        # Given: root zero includes node one, duplicating a relation target.
        root = "knowledge/part-0000.md#node-000000"
        next_node = "knowledge/part-0000.md#node-000001"
        # When: freezing the depth-two neighborhood of this literal fixture.
        oracle = build_traversal_oracle()
        # Then: four expanded knowledge nodes produce twenty edges and eleven IDs.
        self.assertEqual(len(oracle["edges"]), 20)
        self.assertEqual(len(oracle["element_ids"]), 11)
        self.assertIn([root, next_node, "includes"], oracle["edges"])

    def test_generates_literal_smoke_counts(self):
        # Given: a new output directory.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory).resolve() / "fixture"
            # When: generating sixty knowledge elements.
            fixture = prepare_fixture(output, 60)
            # Then: independently known packing and graph counts match.
            self.assertEqual(fixture["counts"]["knowledge"], 60)
            self.assertEqual(fixture["counts"]["source"], 60)
            self.assertEqual(fixture["counts"]["edges"], 300)
            self.assertEqual(fixture["packing"]["file_count"], 2)
            self.assertEqual(fixture["fragment_bytes"]["min"], 2000)

    def test_mutation_samples_restore_equivalent_inputs(self):
        # Given: a deterministic fixture, copied twice for one-file edits.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fixture = prepare_fixture(root / "fixture", 60)
            # When: preparing independent mutation samples.
            first = prepare_scenario(fixture, "refresh_one", root / "first")
            second = prepare_scenario(fixture, "refresh_one", root / "second")
            # Then: both begin with the same input hashes, and change one file.
            self.assertEqual(first["state_hashes"], second["state_hashes"])
            self.assertEqual(first["changed_paths"], ["knowledge/part-0000.md"])
            self.assertEqual(first["expected"], second["expected"])
            self.assertNotEqual(first["repository"], second["repository"])

    def test_mutation_cardinalities_and_collision_rejection(self):
        # Given: two Markdown files with independent ordinary body text.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fixture = prepare_fixture(root / "fixture", 60)
            # When: preparing several-file and bulk edits.
            for name in ("refresh_several", "refresh_bulk"):
                scenario = prepare_scenario(fixture, name, root / name)
                # Then: both files change, with metadata and links preserved.
                self.assertEqual(len(scenario["changed_paths"]), 2)
                for relative in scenario["changed_paths"]:
                    original = (fixture["repository"] / relative).read_bytes()
                    actual = (scenario["repository"] / relative).read_bytes()
                    self.assertEqual(actual, original.replace(b"Synthetic case", b"Altered___case"))
            with self.assertRaises(ValueError):
                prepare_fixture(root / "fixture", 60)
            with self.assertRaises(ValueError):
                prepare_scenario(fixture, "search", fixture["repository"] / "nested")

    def test_rejects_symlink_and_modified_binary(self):
        # Given: a symlink output and a pinned executable.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "link").symlink_to(root / "missing")
            binary = root / "binary"
            binary.write_bytes(b"original")
            pin = make_pin(binary, b"original")
            # When / Then: symlink destinations and changed binaries are rejected.
            with self.assertRaises(ValueError):
                prepare_fixture(root / "link", 60)
            verify_build(pin)
            binary.write_bytes(b"changed")
            with self.assertRaises(ValueError):
                verify_build(pin)

    def test_rejects_changed_product_source_before_measurement(self):
        # Given: the binary matches its receipt, but product sources do not.
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "binary"
            binary.write_bytes(b"pinned")
            pin = dict(make_pin(binary, b"pinned"), inputs={"src/main.rs": "original"})
            # When / Then: a source mismatch invalidates this executable pairing.
            with patch("scenarios.read_product_inputs", return_value={"src/main.rs": "changed"}):
                with self.assertRaises(ValueError):
                    verify_build(pin)


def make_pin(binary: Path, data: bytes) -> dict:
    return {"executable": str(binary), "sha256": hashlib.sha256(data).hexdigest(),
            "inputs": read_product_inputs(ROOT), "product_commit": run_git(ROOT, "rev-parse", "HEAD").decode().strip(),
            "rustc": "rustc fixture", "command": ["cargo", "build", "--release", "--locked", "--offline"],
            "release_debug": "1", "target": "fixture"}


if __name__ == "__main__":
    unittest.main()
