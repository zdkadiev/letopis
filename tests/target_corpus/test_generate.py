"""Independent behavior checks for the deterministic target-corpus fixture."""

import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import tempfile
import unittest
from unittest.mock import patch

from generate import CorpusOptions, generate_corpus, plan_packing
from validate import validate_corpus


class GeneratorTests(unittest.TestCase):
    def test_smoke_has_literal_counts_targets_and_exact_utf8_sizes(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "smoke"
            manifest = generate_corpus(output, CorpusOptions(seed=15, knowledge_count=60))
            checked = validate_corpus(output / "corpus.json")
            first = (output / "repository/knowledge/part-0000.md").read_bytes()
            last = (output / "repository/knowledge/part-0001.md").read_bytes()

            self.assertEqual(manifest, checked)
            self.assertEqual(manifest["counts"],
                             {"knowledge": 60, "source": 60, "edges": 300, "includes": 44})
            self.assertEqual(manifest["packing"]["distribution"], [30, 30])
            self.assertEqual(manifest["fragment_bytes"],
                             {"count": 60, "sum": 120000, "mean": 2000.0,
                              "min": 2000, "max": 2000})
            self.assertIn(b"sources/shard-0000.txt#L1-L1", first)
            self.assertIn(b"sources/shard-0000.txt#L60-L60", last)
            self.assertIn(b"knowledge/part-0000.md#node-000000", last)
            fragments = re.split(rb"(?=^## )", first, flags=re.MULTILINE)[1:]
            self.assertEqual(len(fragments), 30)
            self.assertEqual({len(fragment) for fragment in fragments}, {2000})
            self.assertGreater(len(first), len(first.decode("utf-8")))

    def test_bytes_are_stable_across_paths_and_hostile_git_configuration(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            options = CorpusOptions(seed=17, knowledge_count=60)
            first = generate_corpus(root / "one", options)
            config = root / "hostile.gitconfig"
            config.write_text("[core]\n autocrlf = true\n[commit]\n gpgSign = true\n"
                              "[init]\n defaultBranch = hostile\n", encoding="utf-8")
            environment = {"GIT_CONFIG_GLOBAL": str(config), "GIT_DEFAULT_HASH": "sha256",
                           "GIT_AUTHOR_NAME": "Hostile", "GIT_INDEX_FILE": str(root / "wrong")}
            with patch.dict(os.environ, environment):
                second = generate_corpus(root / "different-parent/two", options)
                validate_corpus(root / "different-parent/two/corpus.json")

            self.assertEqual(first, second)
            for relative in first["input_sha256"]:
                self.assertEqual((root / "one/repository" / relative).read_bytes(),
                                 (root / "different-parent/two/repository" / relative).read_bytes())

    def test_includes_have_depth_at_most_three_and_sources_cross_shards(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "shards"
            generate_corpus(output, CorpusOptions(seed=15, knowledge_count=110))
            repository = output / "repository"
            graph = {}
            for path in sorted((repository / "knowledge").glob("*.md")):
                relative = path.relative_to(repository).as_posix()
                fragments = re.split(r"(?=^## )", path.read_text(), flags=re.MULTILINE)[1:]
                for fragment in fragments:
                    anchor = re.search(r"\{#([^}]+)\}", fragment)[1]
                    graph[f"{relative}#{anchor}"] = re.findall(
                        r'\[relation 4\]\(([^ ]+) "letopis:type=includes"\)', fragment)
            for root in graph:
                pending = [(root, ())]
                while pending:
                    node, ancestors = pending.pop()
                    self.assertNotIn(node, ancestors)
                    self.assertLessEqual(len(ancestors), 3)
                    pending.extend((target, (*ancestors, node)) for target in graph[node])
            last = (repository / "knowledge/part-0003.md").read_text()
            self.assertIn('sources/shard-0001.txt#L10-L10', last)
            self.assertEqual(validate_corpus(output / "corpus.json")["counts"]["source"], 110)

    def test_full_packing_needs_no_generated_payload(self):
        packing = plan_packing(100000, 30)
        self.assertEqual(len(packing), 3334)
        self.assertEqual(packing[:-1], [30] * 3333)
        self.assertEqual(packing[-1], 10)

    def test_small_remainder_is_redistributed(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "remainder"
            manifest = generate_corpus(output, CorpusOptions(seed=1, knowledge_count=31))
            self.assertEqual(manifest["packing"]["distribution"], [21, 10])
            self.assertEqual(validate_corpus(output / "corpus.json")["counts"]["edges"], 155)

    def test_invalid_options_do_not_create_output(self):
        invalid = [CorpusOptions(seed=1, knowledge_count=0),
                   CorpusOptions(seed=1, knowledge_count=9),
                   CorpusOptions(seed=1, knowledge_count=11, elements_per_file=10),
                   CorpusOptions(seed=1, knowledge_count=60, elements_per_file=51),
                   CorpusOptions(seed=1, knowledge_count=60, fragment_bytes=50),
                   CorpusOptions(seed=True, knowledge_count=60),
                   CorpusOptions(seed=1, knowledge_count=60.0)]
        with tempfile.TemporaryDirectory() as temporary:
            for index, options in enumerate(invalid):
                with self.subTest(options=options):
                    output = Path(temporary).resolve() / str(index)
                    with self.assertRaises(ValueError):
                        generate_corpus(output, options)
                    self.assertFalse(output.exists())

    def test_existing_outputs_and_symlink_parents_are_rejected(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            existing = root / "existing"
            existing.mkdir()
            (existing / "keep").write_text("preserve", encoding="utf-8")
            link = root / "link"
            link.symlink_to(existing, target_is_directory=True)
            for output in (existing, link, link / "child"):
                with self.subTest(output=output), self.assertRaises(ValueError):
                    generate_corpus(output, CorpusOptions(seed=1, knowledge_count=10))
            self.assertEqual((existing / "keep").read_text(), "preserve")

    def test_cli_defaults_and_flags(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "cli"
            result = subprocess.run(["python3", "-B", str(Path(__file__).with_name("generate.py")),
                                     "--output", str(output), "--nodes", "10", "--seed", "4",
                                     "--fragment-bytes", "2101", "--elements-per-file", "10"],
                                    capture_output=True, text=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            manifest = validate_corpus(output / "corpus.json")
            self.assertEqual(manifest["options"]["seed"], 4)
            self.assertEqual(manifest["fragment_bytes"]["sum"], 21010)


class ValidatorTests(unittest.TestCase):
    def test_rejects_cycle_even_with_updated_input_hashes(self):
        def mutate(text):
            return text.replace(
                '[relation 4](knowledge/part-0000.md#node-000007 "letopis:type=depends_on")',
                '[relation 4](knowledge/part-0000.md#node-000000 "letopis:type=includes")', 1)
        self._assert_rejected(mutate, "cycle")

    def test_rejects_duplicate_triples_even_with_updated_input_hashes(self):
        def mutate(text):
            return text.replace(
                '[relation 2](knowledge/part-0000.md#node-000002 "letopis:type=depends_on")',
                '[relation 2](knowledge/part-0000.md#node-000001 "letopis:type=depends_on")', 1)
        self._assert_rejected(mutate, "duplicate")

    def test_rejects_out_of_range_source_even_with_updated_input_hashes(self):
        self._assert_rejected(lambda text: text.replace("#L1-L1", "#L101-L101", 1), "source")

    def test_rejects_dangling_target_even_with_updated_input_hashes(self):
        self._assert_rejected(lambda text: text.replace("md#node-000001", "md#node-999999", 1),
                              "target")

    def test_rejects_self_link_even_with_updated_input_hashes(self):
        self._assert_rejected(lambda text: text.replace("md#node-000001", "md#node-000000", 1),
                              "self")

    def test_rejects_removed_edge_even_with_updated_input_hashes(self):
        self._assert_rejected(lambda text: re.sub(r'^\[relation 2\].*\n', '', text,
                                                  count=1, flags=re.MULTILINE), "edge")

    def test_rejects_shortened_payload_even_with_updated_input_hashes(self):
        self._assert_rejected(lambda text: text.replace("Synthetic case", "S case", 1), "fragment")

    def test_rejects_unmarked_heading_that_changes_the_product_fragment_range(self):
        self._assert_rejected(lambda text: text.replace("Evidence remains", "## New section  ", 1),
                              "marked")

    def test_rejects_extra_metadata_in_a_fragment(self):
        self._assert_rejected(lambda text: text.replace("Evidence remains stable", "<!-- letopis: {} -->   ", 1),
                              "metadata")

    def test_rejects_modified_fragment_hash(self):
        self._assert_rejected(lambda text: text.replace("Synthetic", "Different", 1),
                              "hash", update_hash=False)

    def test_rejects_source_tree_mutation_and_symlink_input(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "fixture"
            generate_corpus(output, CorpusOptions(seed=15, knowledge_count=10))
            source = output / "repository/sources/shard-0000.txt"
            original = source.read_bytes()
            source.write_bytes(original.replace(b"record", b"broken", 1))
            self._update_hash(output, "sources/shard-0000.txt")
            with self.assertRaisesRegex(ValueError, "source tree"):
                validate_corpus(output / "corpus.json")
            source.write_bytes(original)
            self._update_hash(output, "sources/shard-0000.txt")
            source.unlink()
            source.symlink_to(output / "corpus.json")
            with self.assertRaisesRegex(ValueError, "symlink"):
                validate_corpus(output / "corpus.json")

    def test_rejects_false_manifest_statistics_and_unlisted_inputs(self):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "fixture"
            generate_corpus(output, CorpusOptions(seed=15, knowledge_count=10))
            path = output / "corpus.json"
            original = path.read_text()
            manifest = json.loads(original)
            manifest["counts"]["edges"] = 49
            path.write_text(json.dumps(manifest), encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "counts"):
                validate_corpus(path)
            path.write_text(original, encoding="utf-8")
            (output / "repository/knowledge/extra.md").write_text("unexpected", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, "input"):
                validate_corpus(path)

    def _assert_rejected(self, mutate, error, update_hash=True):
        with tempfile.TemporaryDirectory() as temporary:
            output = Path(temporary).resolve() / "fixture"
            generate_corpus(output, CorpusOptions(seed=15, knowledge_count=10))
            path = output / "repository/knowledge/part-0000.md"
            path.write_text(mutate(path.read_text(encoding="utf-8")), encoding="utf-8")
            if update_hash:
                self._update_hash(output, "knowledge/part-0000.md")
            with self.assertRaisesRegex(ValueError, error):
                validate_corpus(output / "corpus.json")

    def _update_hash(self, output, relative):
        path = output / "corpus.json"
        manifest = json.loads(path.read_text())
        manifest["input_sha256"][relative] = hashlib.sha256(
            (output / "repository" / relative).read_bytes()).hexdigest()
        path.write_text(json.dumps(manifest), encoding="utf-8")


if __name__ == "__main__":
    unittest.main()
