"""TK-30 test seed commands; model-free. Shared fixtures are private."""
import json
import shutil
import subprocess
import sys
import copies
from _copy_fixtures import (
    AGENTS_ORIGINAL,
    BASE_FILES,
    GIT_ENV,
    HERE,
    SUPERPOWERS_BULLET,
    TempDirTestCase,
    commit,
    git,
    make_source_repo,
    write_tree,
)
import unittest


RUN = HERE / "run.py"


class SeedCommandTests(TempDirTestCase):
    def setUp(self):
        super().setUp()
        self.repo, self.base = make_source_repo(self.tmp)

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

    def prepare(self, output):
        return self.run_cli("prepare", "--repo", self.repo, "--base", self.base,
                            "--output", output)

    def run_cli(self, *args):
        result = subprocess.run([sys.executable, "-B", str(RUN), *map(str, args)],
                                cwd=self.tmp, capture_output=True, text=True, timeout=120)
        return result.returncode, json.loads(result.stdout)


if __name__ == "__main__":
    unittest.main()
