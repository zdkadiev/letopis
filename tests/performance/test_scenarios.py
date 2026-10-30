"""Independent fixture and binary identity assertions."""

import hashlib
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

from scenarios import prepare_fixture, prepare_scenario, verify_build


class ScenarioTests(unittest.TestCase):
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
            pin = {"executable": str(binary), "sha256": hashlib.sha256(b"original").hexdigest()}
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
            pin = {"executable": str(binary), "sha256": hashlib.sha256(b"pinned").hexdigest(),
                   "inputs": {"src/main.rs": "original"}}
            # When / Then: a source mismatch invalidates this executable pairing.
            with patch("scenarios.read_product_inputs", return_value={"src/main.rs": "changed"}):
                with self.assertRaises(ValueError):
                    verify_build(pin)


if __name__ == "__main__":
    unittest.main()
