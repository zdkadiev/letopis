"""Model-free tests for the TK-30 selection, preparation manifest and readiness."""
import copy
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import study

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent
FINAL = REPO / "docs/zkit/letopis/pilot/final"
SELECTION = FINAL / "selection.json"
RUN = HERE / "run.py"
BASE = "0123456789abcdef0123456789abcdef01234567"
OTHER_BASE = "fedcba9876543210fedcba9876543210fedcba98"

EXPECTED_SLOTS = [
    "C1-T1-baseline", "C1-T2-baseline", "C1-T3-baseline",
    "C1-T1-memory", "C1-T2-memory", "C1-T3-memory",
    "C2-T1-memory", "C2-T2-memory", "C2-T3-memory",
    "C2-T1-baseline", "C2-T2-baseline", "C2-T3-baseline",
    "C3-T1-baseline", "C3-T2-baseline", "C3-T3-baseline",
    "C3-T1-memory", "C3-T2-memory", "C3-T3-memory",
]
EXPECTED_CHECKS = {
    "C1-T1": ["version_value", "version_no_stdin_or_state", "regression"],
    "C1-T2": ["help_commands_and_examples", "help_no_stdin_or_state", "regression"],
    "C1-T3": ["command_help_forms", "help_rejects_invalid_argv", "regression"],
    "C2-T1": ["bundle_layout", "manifest_integrity", "bundle_rejects_invalid_input",
              "regression"],
    "C2-T2": ["install_layout_and_repeat", "install_failure_preserves_previous",
              "regression"],
    "C2-T3": ["installed_workflow", "installed_missing_or_damaged",
              "installed_independence", "regression"],
    "C3-T1": ["two_requests_before_release", "bounded_buffer", "one_owner_short_writes",
              "regression"],
    "C3-T2": ["shared_hash_durable_attempts", "out_of_order_identity",
              "invalid_retry_bound", "regression"],
    "C3-T3": ["detached_overlap", "caller_owner_lifecycle", "search_with_drain",
              "regression"],
}
ALL_PENDING = ["seeds", "product", "memory_versions", "embedding", "assets", "model",
               "agent_config", "g2", "prompt_freeze", "base_drift_recheck"]


def set_in(section, key):
    return lambda manifest, value: manifest[section].__setitem__(key, value)


PENDING_SETTERS = {
    "seeds": lambda m, v: m.__setitem__("seed_commit", v),
    "product": set_in("product", "final_base"),
    "memory_versions": set_in("product", "skill"),
    "embedding": set_in("product", "embedding_config"),
    "assets": set_in("environment", "image"),
    "model": set_in("agent", "model_revision"),
    "agent_config": set_in("agent", "config_sha256"),
    "g2": set_in("gates", "g2"),
    "prompt_freeze": set_in("gates", "prompt_freeze"),
    "base_drift_recheck": set_in("gates", "base_recheck"),
}


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"))


def build_unready_manifest(local_tools_ok=False):
    selection = study.load_selection(SELECTION)
    inputs = {"original_base": BASE, "local_tools_ok": local_tools_ok, "file_sha256": {}}
    return selection, study.build_manifest(selection, inputs)


def fill_every_input(manifest):
    """Return a copy of the manifest with every freeze input explicitly supplied."""
    filled = copy.deepcopy(manifest)
    filled["seed_commit"] = "1" * 40
    filled["starting_inventory"] = {"sha256": "2" * 64}
    filled["product"] = {"final_base": "3" * 40, "memory_binary": "memory-1",
                         "skill": "skill-1", "embedding_config": "embedding-1"}
    filled["environment"].update({"image": "image-1", "pinned_assets": "assets-1"})
    filled["agent"].update({"model": "model-1", "model_revision": "unknown",
                            "budget": "budget-1", "config_sha256": "4" * 64})
    filled["gates"] = {"g2": {"evidence": "g2/report.md#L1"},
                       "prompt_freeze": {"evidence": "freeze/approval.md#L1"},
                       "base_recheck": {"base_commit": filled["original_base"],
                                        "selection_sha256":
                                        filled["source_receipts"]["selection_sha256"]}}
    return filled


class SelectionLoadingTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def reject_after(self, change):
        selection = load_json(SELECTION)
        change(selection)
        path = self.tmp / "selection.json"
        path.write_text(json.dumps(selection), encoding="utf-8")
        with self.assertRaises(study.SelectionError):
            study.load_selection(path)

    def test_frozen_selection_has_literal_ids_checks_and_reuse(self):
        # When
        selection = study.load_selection(SELECTION)

        # Then
        tasks = {t["id"]: t for c in selection["chains"] for t in c["tasks"]}
        self.assertEqual([c["id"] for c in selection["chains"]], ["C1", "C2", "C3"])
        self.assertEqual({k: v["check_ids"] for k, v in tasks.items()}, EXPECTED_CHECKS)
        self.assertEqual(tasks["C2-T1"]["reuse_from"], [])
        self.assertEqual(tasks["C2-T2"]["reuse_from"], ["C2-T1"])
        self.assertEqual(tasks["C3-T3"]["reuse_from"], ["C3-T1", "C3-T2"])
        for task in tasks.values():
            self.assertEqual(task["agreement_ids"], ["hexagonal_sync", "typed_safe_errors"])
        self.assertEqual([c["issue"]["url"] for c in selection["chains"]],
                         ["https://github.com/zaurkadiev/letopis/issues/53",
                          "https://github.com/zaurkadiev/letopis/issues/43",
                          "https://github.com/zaurkadiev/letopis/issues/49"])

    def test_missing_third_task_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][1]["tasks"].pop())

    def test_repeated_task_id_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][2]["tasks"][2].update(id="C3-T2"))

    def test_task_id_repeated_across_chains_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][1]["tasks"][0].update(id="C1-T1"))

    def test_forward_reuse_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][0]["tasks"][1].update(reuse_from=["C1-T3"]))

    def test_first_task_reuse_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][0]["tasks"][0].update(reuse_from=["C1-T1"]))

    def test_cross_chain_reuse_is_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][1]["tasks"][1].update(reuse_from=["C1-T1"]))

    def test_unknown_agreement_and_empty_prompt_are_rejected(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][0]["tasks"][0].update(agreement_ids=["x"]))
        self.reject_after(lambda s: s["chains"][0]["tasks"][0].update(prompt=" "))

    def test_condition_order_must_cover_both_conditions_once(self):
        # When / Then
        self.reject_after(lambda s: s["chains"][0].update(condition_order=["memory", "memory"]))


class ParticipantTextTests(unittest.TestCase):
    def test_prompts_and_instructions_hide_oracle_and_rubric(self):
        # Given
        selection = load_json(SELECTION)
        visible = [t["prompt"] for c in selection["chains"] for t in c["tasks"]]
        visible.append((FINAL / "instructions.md").read_text(encoding="utf-8"))
        hidden = ["regression", "hexagonal_sync", "typed_safe_errors", "version_value",
                  "bounded_buffer", "detached_overlap", "installed_workflow", "check_id",
                  "rubric", "oracle"]
        hidden += [f["id"] for c in selection["chains"] for f in c["recurring_facts"]]

        # When / Then
        for text in visible:
            for word in hidden:
                self.assertNotIn(word, text)

    def test_prompts_state_frozen_executable_contracts(self):
        # Given
        selection = load_json(SELECTION)
        prompts = {t["id"]: t["prompt"] for c in selection["chains"] for t in c["tasks"]}

        # When / Then
        self.assertIn("`letopis 0.0.0`", prompts["C1-T1"])
        for command in ("python3 scripts/package.py --binary PATH --skill PATH --output NEW_DIR",
                        "bin/letopis", "share/letopis/skills/letopis/SKILL.md",
                        "manifest.json", "source_commit", "target", "sorted"):
            self.assertIn(command, prompts["C2-T1"])
        self.assertIn("python3 scripts/install.py --bundle DIR --prefix DIR", prompts["C2-T2"])
        self.assertIn("python3 scripts/check_install.py --prefix DIR", prompts["C2-T3"])
        for task_id in ("C3-T1", "C3-T2", "C3-T3"):
            for phrase in ("fixed at 2", "std", "blocking I/O", "no async", "no batching",
                           "no new configuration"):
                self.assertIn(phrase, prompts[task_id])

    def test_every_chain_has_concrete_recurring_facts(self):
        # When
        selection = load_json(SELECTION)

        # Then
        for chain in selection["chains"]:
            self.assertGreaterEqual(len(chain["recurring_facts"]), 3)
            for fact in chain["recurring_facts"]:
                self.assertTrue(fact["fact"].strip())
                self.assertNotIn(fact["recurs_in"], ([], None))

    def test_readiness_document_lists_prompt_wording_as_pending_human_freeze(self):
        # When
        text = (FINAL / "readiness.md").read_text(encoding="utf-8")

        # Then
        self.assertIn("prompt wording", text)
        self.assertIn("unready", text)
        self.assertIn("ручным шагом", text)
        for item in ALL_PENDING + ["input_drift"]:
            self.assertIn(f"`{item}`", text)


class ManifestTests(unittest.TestCase):
    def test_order_is_the_exact_p1_sequence_of_eighteen_slots(self):
        # When
        _, manifest = build_unready_manifest()

        # Then
        self.assertEqual([s["slot"] for s in manifest["order"]], EXPECTED_SLOTS)
        self.assertEqual(len(manifest["order"]), 18)
        self.assertEqual(manifest["order"][6],
                         {"slot": "C2-T1-memory", "chain": "C2", "task_id": "C2-T1",
                          "condition": "memory", "position": 7})

    def test_manifest_is_a_private_unready_draft_without_model_runs(self):
        # When
        _, manifest = build_unready_manifest()

        # Then
        self.assertEqual(manifest["preparation_version"], 1)
        self.assertEqual(manifest["state"], "unready")
        self.assertEqual(manifest["model_executions"], 0)
        self.assertEqual(manifest["original_base"], BASE)
        self.assertIsNone(manifest["seed_commit"])
        self.assertIsNone(manifest["starting_inventory"])
        self.assertEqual(manifest["pending"], ALL_PENDING)

    def test_missing_product_model_or_gate_stays_unready(self):
        # Given
        _, manifest = build_unready_manifest(local_tools_ok=True)

        # When
        readiness = study.assess_readiness(manifest)

        # Then
        self.assertTrue(manifest["environment"]["local_tools_ok"])
        self.assertEqual(readiness["state"], "unready")
        self.assertEqual(readiness["pending"], ALL_PENDING)
        self.assertIn("g2", readiness["pending"])
        self.assertEqual(manifest["model_executions"], 0)

    def test_each_missing_input_alone_is_reported_pending(self):
        # Given
        _, manifest = build_unready_manifest()
        filled = fill_every_input(manifest)

        # When / Then
        self.assertEqual(study.assess_readiness(filled), {"state": "ready", "pending": []})
        for name, setter in PENDING_SETTERS.items():
            with self.subTest(name):
                broken = copy.deepcopy(filled)
                setter(broken, None)
                self.assertEqual(study.assess_readiness(broken),
                                 {"state": "unready", "pending": [name]})

    def test_placeholder_values_never_clear_a_pending_item(self):
        # Given
        _, manifest = build_unready_manifest()
        filled = fill_every_input(manifest)
        placeholders = ["", "  ", {}, [], False, 0]
        gate_only = [True, "passed", "failed", {"verdict": "pass"}, {"evidence": ""},
                     {"evidence": {}}]

        # When / Then
        for name, setter in PENDING_SETTERS.items():
            gate = name in ("g2", "prompt_freeze")
            for value in placeholders + (gate_only if gate else []):
                with self.subTest(name=name, value=value):
                    broken = copy.deepcopy(filled)
                    setter(broken, value)
                    self.assertEqual(study.assess_readiness(broken),
                                     {"state": "unready", "pending": [name]})

    def test_missing_manifest_section_is_pending_not_an_exception(self):
        # Given
        _, manifest = build_unready_manifest()
        filled = fill_every_input(manifest)
        del filled["product"]
        del filled["gates"]["g2"]

        # When
        readiness = study.assess_readiness(filled)

        # Then
        self.assertEqual(readiness["pending"], ["product", "memory_versions", "embedding", "g2"])

    def test_base_drift_keeps_unready(self):
        # Given
        _, manifest = build_unready_manifest()
        frozen = fill_every_input(manifest)
        self.assertEqual(study.assess_readiness(frozen)["state"], "ready")

        # When
        moved_base = copy.deepcopy(frozen)
        moved_base["original_base"] = OTHER_BASE
        moved_selection = copy.deepcopy(frozen)
        moved_selection["source_receipts"]["selection_sha256"] = "5" * 64

        # Then
        self.assertEqual(study.assess_readiness(moved_base),
                         {"state": "unready", "pending": ["input_drift", "base_drift_recheck"]})
        self.assertEqual(study.assess_readiness(moved_selection),
                         {"state": "unready", "pending": ["base_drift_recheck"]})

    def test_observed_inputs_that_differ_from_receipts_are_drift(self):
        # Given
        _, manifest = build_unready_manifest()
        frozen = fill_every_input(manifest)
        receipts = frozen["source_receipts"]
        matching = {"selection_sha256": receipts["selection_sha256"], "file_sha256": {}}

        # When
        edited_selection = dict(matching, selection_sha256="6" * 64)
        edited_file = dict(matching, file_sha256={"instructions.md": "7" * 64})

        # Then
        self.assertEqual(study.assess_readiness(frozen, matching)["state"], "ready")
        self.assertEqual(study.assess_readiness(frozen, edited_selection)["pending"],
                         ["input_drift", "base_drift_recheck"])
        self.assertEqual(study.assess_readiness(frozen, edited_file)["pending"],
                         ["input_drift"])

    def test_stored_base_must_be_a_full_object_id(self):
        # Given
        _, manifest = build_unready_manifest()
        frozen = fill_every_input(manifest)

        # When / Then
        for bad in ("d260348", BASE.upper(), "", None, 7):
            with self.subTest(bad=bad):
                broken = copy.deepcopy(frozen)
                broken["original_base"] = bad
                broken["source_receipts"]["base_commit"] = bad
                broken["gates"]["base_recheck"]["base_commit"] = bad
                self.assertEqual(study.assess_readiness(broken)["state"], "unready")
                self.assertIn("input_drift", study.assess_readiness(broken)["pending"])

    def test_selection_hash_changes_with_any_prompt_edit(self):
        # Given
        selection = study.load_selection(SELECTION)
        edited = copy.deepcopy(selection)
        edited["chains"][0]["tasks"][0]["prompt"] += " "

        # When
        first = study.build_manifest(selection, {"original_base": BASE})
        second = study.build_manifest(edited, {"original_base": BASE})

        # Then
        self.assertNotEqual(first["source_receipts"]["selection_sha256"],
                            second["source_receipts"]["selection_sha256"])
        self.assertEqual(len(first["source_receipts"]["selection_sha256"]), 64)

    def test_non_full_object_id_is_rejected(self):
        # Given
        selection = study.load_selection(SELECTION)

        # When / Then
        for bad in ("d260348", BASE.upper(), BASE[:-1], "", None):
            with self.assertRaises(ValueError):
                study.build_manifest(selection, {"original_base": bad})


class RunCommandTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.repo = self.tmp / "product"
        self.repo.mkdir()
        self.git("init", "-q")
        (self.repo / "a.txt").write_text("a\n")
        self.git("add", "a.txt")
        self.git("-c", "user.name=t", "-c", "user.email=t@example.com",
                 "commit", "-q", "-m", "init")
        self.base = self.git("rev-parse", "HEAD").strip()

    def git(self, *args):
        return subprocess.run(["git", "-C", str(self.repo), *args], check=True,
                              capture_output=True, text=True).stdout

    def run_cli(self, *args, python=sys.executable, env=None, timeout=20):
        result = subprocess.run([python, "-B", str(RUN), *map(str, args)], cwd=self.tmp,
                                capture_output=True, text=True, env=env, timeout=timeout)
        return result.returncode, json.loads(result.stdout)

    def prepare(self, output, base=None):
        return self.run_cli("prepare", "--repo", self.repo, "--base", base or self.base,
                            "--output", output)

    def test_prepare_writes_unready_manifest_and_inputs_only(self):
        # Given
        output = self.tmp / "out"

        # When
        code, report = self.prepare(output)

        # Then
        self.assertEqual(code, 0)
        self.assertEqual(report["state"], "unready")
        self.assertEqual(sorted(p.name for p in output.iterdir()), ["inputs", "manifest.json"])
        self.assertEqual(sorted(p.name for p in (output / "inputs").iterdir()),
                         ["instructions.md", "readiness.md", "selection.json"])
        manifest = load_json(output / "manifest.json")
        self.assertEqual(manifest["original_base"], self.base)
        self.assertEqual(len(manifest["order"]), 18)
        self.assertEqual(manifest["model_executions"], 0)

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
        self.assertEqual(report["pending"], ALL_PENDING)

    def test_audit_without_manifest_is_an_input_error(self):
        # When
        code, report = self.run_cli("audit", "--root", self.tmp)

        # Then
        self.assertEqual(code, 2)
        self.assertIn("error", report)

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

    def freeze_prepared_root(self, output):
        """Fill every freeze input and a matching recheck, as an operator would."""
        self.prepare(output)
        path = output / "manifest.json"
        path.write_text(json.dumps(fill_every_input(load_json(path))))

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

    def test_evaluate_is_not_wired_and_never_reports_success(self):
        # When
        code, report = self.run_cli("evaluate", "--root", self.tmp, "--slot", "C1-T1-baseline")

        # Then
        self.assertEqual(code, 2)
        self.assertEqual(report, {"error": "evaluate is not wired until Task 3"})

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
        probe = subprocess.run([old, "-c", "import sys;print(sys.version_info >= (3, 11))"],
                               capture_output=True, text=True) if old else None
        if probe is None or probe.stdout.strip() != "False":
            self.skipTest("default python3 is not older than 3.11")

        # When
        code, report = self.run_cli("audit", "--root", self.tmp, python=old)

        # Then
        self.assertEqual(code, 2)
        self.assertIn("Python 3.11", report["error"])


if __name__ == "__main__":
    unittest.main()
