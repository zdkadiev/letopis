"""Model-free tests for the TK-30 operator command boundary."""
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_study import ALL_PENDING, BASE, FINAL, RUN, fill_every_input, load_json


class RunCommandTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.repo = self.tmp / "product"
        self.repo.mkdir()
        self.git("init", "-q")
        (self.repo / "a.txt").write_text("a\n")
        (self.repo / "AGENTS.md").write_text(
            "# Rules\n\n- Keep agreed specifications and implementation plans in "
            "`docs/superpowers/specs/`. When the user requests Superpowers, follow it.\n\n"
            "## Workspace and memory\n\n- Use Engram.\n")
        self.git("add", "a.txt", "AGENTS.md")
        self.git("-c", "user.name=t", "-c", "user.email=t@example.com",
                 "commit", "-q", "-m", "init")
        self.base = self.git("rev-parse", "HEAD").strip()

    def test_prepare_writes_unready_manifest_and_inputs_only(self):
        # Given
        output = self.tmp / "out"

        # When
        code, report = self.prepare(output)

        # Then
        self.assertEqual(code, 0)
        self.assertEqual(report["state"], "unready")
        self.assertEqual(sorted(p.name for p in output.iterdir()),
                         ["conditions", "inputs", "manifest.json", "seeds.json"])
        self.assertNotIn("seeds", report["pending"])
        self.assertEqual(sorted(p.name for p in (output / "inputs").iterdir()),
                         ["instructions.md", "readiness.md", "selection.json"])
        manifest = load_json(output / "manifest.json")
        self.assertEqual(manifest["original_base"], self.base)
        self.assertEqual(len(manifest["order"]), 18)
        self.assertEqual(manifest["model_executions"], 0)
        self.assertEqual(
            manifest["source_receipts"]["tk17_sources"]["files"]
            ["tests/early_pilot/prepare_image.py"],
            "da9013ac13542b525c6640b3ded17a9072eb7beebe88e72ec572d49dd6bdc3f7")

    def test_prepare_rejects_existing_output_unknown_or_short_base(self):
        # Given
        existing = self.tmp / "exists"
        existing.mkdir()

        # When / Then
        self.assertEqual(self.prepare(existing)[0], 2)
        self.assertEqual(list(existing.iterdir()), [])
        self.assertEqual(self.prepare(self.tmp / "o2", base=BASE)[0], 2)
        self.assertEqual(self.prepare(self.tmp / "o3", base=self.base[:7])[0], 2)
        self.assertFalse((self.tmp / "o2").exists() or (self.tmp / "o3").exists())

    def test_audit_reports_pending_as_unready_with_exit_two(self):
        # Given
        output = self.tmp / "out"
        self.prepare(output)

        # When
        code, report = self.run_cli("audit", "--root", output)

        # Then
        self.assertEqual(code, 2)
        self.assertEqual(report["state"], "unready")
        self.assertEqual(report["pending"], [p for p in ALL_PENDING if p != "seeds"])

    def test_audit_without_manifest_is_an_input_error(self):
        # When
        code, report = self.run_cli("audit", "--root", self.tmp)

        # Then
        self.assertEqual(code, 2)
        self.assertIn("error", report)

    def test_audit_rejects_a_manifest_that_is_not_an_object(self):
        # Given
        path = self.tmp / "manifest.json"
        path.write_text("[]\n")
        shutil.copytree(FINAL, self.tmp / "inputs")

        # When
        result = subprocess.run([sys.executable, "-B", str(RUN), "audit", "--root",
                                 str(self.tmp)], capture_output=True, text=True)

        # Then
        self.assertEqual(result.returncode, 2)
        self.assertEqual(json.loads(result.stdout), {"error": "manifest must be an object"})
        self.assertEqual(result.stderr, "")

    def test_audit_recomputes_state_instead_of_trusting_manifest_claim(self):
        # Given
        output = self.tmp / "out"
        self.prepare(output)
        manifest = load_json(output / "manifest.json")
        manifest["state"] = "ready"
        manifest["pending"] = []
        (output / "manifest.json").write_text(json.dumps(manifest))

        # When
        code, report = self.run_cli("audit", "--root", output)

        # Then
        self.assertEqual((code, report["state"]), (2, "unready"))
        self.assertIn("g2", report["pending"])

    def test_audit_accepts_fully_supplied_untouched_inputs(self):
        # Given
        output = self.tmp / "out"
        self.freeze_prepared_root(output)

        # When
        code, report = self.run_cli("audit", "--root", output)

        # Then
        self.assertEqual((code, report), (0, {"state": "ready", "pending": []}))

    def test_audit_detects_inputs_edited_after_recheck(self):
        # Given
        edits = {
            "inputs/selection.json": lambda t: t.replace("Add a `--version`",
                                                         "Please add a `--version`"),
            "inputs/instructions.md": lambda t: t + "extra\n",
            "inputs/readiness.md": lambda t: t + "extra\n",
        }

        # When / Then
        for name, edit in edits.items():
            with self.subTest(name):
                output = self.tmp / name.replace("/", "_")
                self.freeze_prepared_root(output)
                target = output / name
                target.write_text(edit(target.read_text(encoding="utf-8")), encoding="utf-8")
                code, report = self.run_cli("audit", "--root", output)
                self.assertEqual(code, 2)
                self.assertEqual(report["state"], "unready")
                self.assertIn("input_drift", report["pending"])

    def test_audit_rejects_stored_base_that_is_not_a_full_oid(self):
        # Given
        output = self.tmp / "out"
        self.freeze_prepared_root(output)
        manifest = load_json(output / "manifest.json")
        manifest["original_base"] = self.base[:7]
        (output / "manifest.json").write_text(json.dumps(manifest))

        # When
        code, report = self.run_cli("audit", "--root", output)

        # Then
        self.assertEqual((code, report["state"]), (2, "unready"))
        self.assertIn("input_drift", report["pending"])

    def test_evaluate_without_prepared_inputs_never_reports_success(self):
        # When
        code, report = self.run_cli("evaluate", "--root", self.tmp, "--slot", "C1-T1-baseline")

        # Then
        self.assertEqual(code, 2)
        self.assertEqual(set(report), {"error"})

    def test_session_subcommand_exits_before_auth_or_executable(self):
        # Given: a trap auth path (a FIFO blocks any read) and a trap executable
        codex_home = self.tmp / "codex_home"
        codex_home.mkdir()
        auth = codex_home / "auth.json"
        os.mkfifo(auth)
        bin_dir = self.tmp / "bin"
        bin_dir.mkdir()
        marker = self.tmp / "codex-started"
        shim = bin_dir / "codex"
        shim.write_text(f"#!/bin/sh\ntouch '{marker}'\n")
        shim.chmod(0o755)
        env = dict(os.environ, CODEX_HOME=str(codex_home), HOME=str(self.tmp),
                   PATH=f"{bin_dir}{os.pathsep}{os.environ['PATH']}")

        # When
        code, report = self.run_cli("session", "--auth", auth, "--slot", "C1-T1-baseline",
                                    env=env, timeout=10)

        # Then
        self.assertEqual(code, 2)
        self.assertIn("error", report)
        self.assertFalse(marker.exists())

    def test_old_python_exits_two_with_json_error(self):
        # Given
        old = shutil.which("python3")
        probe = subprocess.run([old, "-c", "import sys;print(sys.version_info >= (3, 11, 4))"],
                               capture_output=True, text=True) if old else None
        if probe is None or probe.stdout.strip() != "False":
            self.skipTest("default python3 is not older than 3.11.4")

        # When
        code, report = self.run_cli("audit", "--root", self.tmp, python=old)

        # Then
        self.assertEqual(code, 2)
        self.assertIn("Python 3.11.4", report["error"])

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True,
                              capture_output=True, text=True).stdout

    def freeze_prepared_root(self, output):
        """Fill every freeze input and a matching recheck, as an operator would."""
        self.prepare(output)
        path = output / "manifest.json"
        prepared = load_json(path)
        filled = fill_every_input(prepared)
        filled["seed_commit"] = prepared["seed_commit"]
        filled["starting_inventory"] = prepared["starting_inventory"]
        path.write_text(json.dumps(filled))

    def prepare(self, output, base=None):
        return self.run_cli("prepare", "--repo", self.repo, "--base", base or self.base,
                            "--output", output)

    def run_cli(self, *args, python=sys.executable, env=None, timeout=20):
        result = subprocess.run([python, "-B", str(RUN), *map(str, args)], cwd=self.tmp,
                                capture_output=True, text=True, env=env, timeout=timeout)
        return result.returncode, json.loads(result.stdout)


if __name__ == "__main__":
    unittest.main()
