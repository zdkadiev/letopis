"""Causal variants preserve bytes, knowledge count and author source tree."""

from pathlib import Path
import tempfile
import unittest

from controls import mask_external_edges
from scenarios import prepare_fixture


class ControlTests(unittest.TestCase):
    def test_only_external_declarations_change_and_sizes_remain_fixed(self):
        # Given: sixty known fragments with one external edge each.
        with tempfile.TemporaryDirectory() as directory:
            fixture = prepare_fixture(Path(directory).resolve() / "fixture", 60)
            repository = fixture["repository"]
            before = (repository / "knowledge/part-0000.md").read_bytes()
            tree = fixture["source_tree"]
            # When: masking external declaration lines with same-length plain spaces.
            control = mask_external_edges(fixture)
            after = (repository / "knowledge/part-0000.md").read_bytes()
            # Then: source files, tree, packing and fragment byte lengths are unchanged.
            self.assertEqual(control["counts"]["knowledge"], 60)
            self.assertEqual(control["counts"]["source"], 0)
            self.assertEqual(control["counts"]["edges"], 240)
            self.assertEqual(len(before), len(after))
            self.assertEqual(control["source_tree"], tree)
            self.assertEqual(before.count(b"source_snapshot"), after.count(b"source_snapshot"))
            self.assertNotIn(b'letopis:type=external', after)
            self.assertEqual(before.count(b"letopis:type=includes"), after.count(b"letopis:type=includes"))


if __name__ == "__main__":
    unittest.main()
