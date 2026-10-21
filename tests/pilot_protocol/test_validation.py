import copy
import json
from pathlib import Path
import tempfile
import unittest

from fixtures import EMPTY_MEMORY_SHA256, make_dataset
from validation import validate


class ValidationTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.study, self.sessions = make_dataset(self.root)

    def rejects(self, mutate, expected):
        study, sessions = copy.deepcopy(self.study), copy.deepcopy(self.sessions)
        mutate(study, sessions)
        with self.assertRaisesRegex(ValueError, expected):
            validate(study, sessions, self.root)

    def test_early_and_final_shapes_accept_independent_trajectories(self):
        validate(self.study, self.sessions, self.root)
        study, sessions = make_dataset(self.root, "final")
        validate(study, list(reversed(sessions)), self.root)

    def test_study_rejects_wrong_samples_or_unfrozen_configuration(self):
        cases = [
            (lambda s, r: s.update(protocol_version=2), "protocol_version"),
            (lambda s, r: s.update(stage="unknown"), "stage"),
            (lambda s, r: s["chains"].pop(), "chains"),
            (lambda s, r: s["chains"][0]["tasks"].pop(), "tasks"),
            (lambda s, r: s["agent"].update(config_sha256="unknown"), "config_sha256"),
            (lambda s, r: s.update(kind="pilot"), "real_tasks"),
            (lambda s, r: s["conditions"]["memory"].update(workspace_id="workspace-baseline"), "workspace_id"),
            (lambda s, r: s["conditions"]["memory"].update(home_id="home-baseline"), "home_id"),
            (lambda s, r: s["conditions"]["memory"].update(external_memory_disabled=False), "external_memory"),
            (lambda s, r: s["conditions"]["baseline"].update(memory_store_id="other"), "memory_store_id"),
            (lambda s, r: s["chains"][0]["tasks"][1].update(reuse_from=[]), "reuse_from"),
            (lambda s, r: s["chains"][0]["tasks"][1].update(reuse_from=["C2-T1"]), "reuse_from"),
        ]
        for mutate, expected in cases:
            with self.subTest(expected=expected):
                self.rejects(mutate, expected)

    def test_all_planned_slots_and_unique_actual_sessions_are_required(self):
        self.rejects(lambda s, r: r.pop(), "missing.*slot")
        self.rejects(lambda s, r: r.append(copy.deepcopy(r[0])), "duplicate.*slot")
        self.rejects(lambda s, r: r[1].update(session_id=r[0]["session_id"]), "session_id")
        self.rejects(lambda s, r: r[0].update(condition="other"), "condition")
        self.rejects(lambda s, r: r[0].update(task_id="C99-T1"), "slot")

    def test_shared_inputs_and_code_continuity_are_enforced(self):
        self.rejects(lambda s, r: r[0].update(agent_config_sha256="b" * 64), "agent_config_sha256")
        self.rejects(lambda s, r: r[0].update(prompt_sha256="b" * 64), "prompt_sha256")
        self.rejects(lambda s, r: r[0].update(code_before_sha256="b" * 64), "code_before")
        self.rejects(lambda s, r: r[1].update(code_before_sha256="b" * 64), "code_before")

    def test_empty_memory_and_within_chain_memory_continuity_are_required(self):
        self.assertEqual(self.sessions[2]["memory_before"], {"items": 0, "sha256": EMPTY_MEMORY_SHA256})
        self.rejects(lambda s, r: r[2]["memory_before"].update(items=1, sha256="b" * 64), "memory_before")
        self.rejects(lambda s, r: r[3]["memory_before"].update(items=0, sha256=EMPTY_MEMORY_SHA256), "memory_before")
        self.rejects(lambda s, r: r[0]["memory_after"].update(items=1, sha256="b" * 64), "memory_after")
        self.rejects(lambda s, r: r[2]["memory_after"].update(items=True), "items")

    def test_correctness_and_common_check_coverage_cannot_be_relabelled(self):
        self.rejects(lambda s, r: r[0]["checks"][0].update(passed=False), "outcome")
        self.rejects(lambda s, r: r[0]["checks"].pop(), "checks")
        self.rejects(lambda s, r: r[0]["checks"][0].update(id="different"), "checks")
        self.rejects(lambda s, r: r[0].update(outcome="failed"), "outcome")

    def test_complete_request_inventory_and_nonnegative_normalized_usage(self):
        self.rejects(lambda s, r: r[0]["request_inventory"]["ids"].append("hidden-aux-call"), "request_inventory")
        self.rejects(lambda s, r: r[0]["requests"].append(copy.deepcopy(r[0]["requests"][0])), "request")
        self.rejects(lambda s, r: r[0]["requests"][0].update(input_tokens=-1), "input_tokens")
        self.rejects(lambda s, r: r[0]["requests"][0].update(input_tokens=True), "input_tokens")
        self.rejects(lambda s, r: r[0]["requests"][0].update(cached_input_tokens=101), "cached_input")
        self.rejects(lambda s, r: r[0]["requests"][0].update(reasoning_output_tokens=21), "reasoning_output")
        self.rejects(lambda s, r: r[0]["requests"][0].update(model="unfrozen-model"), "model")
        self.sessions[0]["requests"][0]["input_tokens"] = None
        validate(self.study, self.sessions, self.root)

    def test_research_and_agreement_annotations_require_prior_facts_and_opportunities(self):
        self.rejects(lambda s, r: r[0]["research"][0].update(kind="repeat"), "research")
        self.rejects(lambda s, r: r[1]["research"][0].update(kind="first"), "research")
        self.rejects(lambda s, r: r[0]["agreements"].clear(), "agreements")
        self.rejects(lambda s, r: r[0]["agreements"][0].update(violated="no"), "violated")
        self.rejects(lambda s, r: r[2]["workload_profile"][0].update(bytes=-1), "bytes")
        self.sessions[1]["research"][0]["kind"] = "unresolved"
        self.sessions[0]["agreements"][0]["violated"] = None
        validate(self.study, self.sessions, self.root)

    def test_evidence_is_hashed_confined_regular_and_mandatory(self):
        self.rejects(lambda s, r: r[0]["trace"].update(sha256="0" * 64), "sha256")
        self.rejects(lambda s, r: r[0]["trace"].update(path="missing.txt"), "evidence")
        self.rejects(lambda s, r: r[0]["trace"].update(path="../outside.txt"), "evidence")
        self.rejects(lambda s, r: r[0]["trace"].update(path=str(self.root / "evidence/demonstration.txt")), "evidence")
        self.rejects(lambda s, r: r[0]["trace"].update(path="evidence"), "regular")
        outside = self.root.parent / (self.root.name + "-outside.txt")
        outside.write_text("outside root")
        self.addCleanup(outside.unlink)
        (self.root / "escape.txt").symlink_to(outside)
        self.rejects(lambda s, r: r[0]["trace"].update(path="escape.txt"), "evidence")

    def test_failed_predecessor_requires_explicit_not_run_followups(self):
        self.sessions[0]["outcome"] = "failed"
        self.sessions[0]["checks"][0]["passed"] = False
        with self.assertRaisesRegex(ValueError, "predecessor"):
            validate(self.study, self.sessions, self.root)

    def test_model_names_are_opaque_and_malformed_types_raise_value_error(self):
        self.study["agent"]["auxiliary_models"] = ["vendor/model@revision"]
        for record in self.sessions:
            for request in record["requests"]:
                if request["purpose"] == "embedding":
                    request["model"] = "vendor/model@revision"
        validate(self.study, self.sessions, self.root)
        self.sessions[0]["requests"][0]["model"] = {"invalid": "object"}
        with self.assertRaisesRegex(ValueError, "model"):
            validate(self.study, self.sessions, self.root)

    def test_symlink_loop_in_evidence_is_an_actionable_record_error(self):
        (self.root / "loop.txt").symlink_to("loop.txt")
        self.rejects(lambda s, r: r[0]["trace"].update(path="loop.txt"), "evidence")

    def test_profile_times_still_reject_nonfinite_floats_and_booleans(self):
        for key in ("timestamp_ms", "elapsed_ms"):
            for value in (float("inf"), float("nan"), True, -1):
                with self.subTest(key=key, value=value):
                    self.rejects(lambda s, r: r[2]["workload_profile"][0].update({key: value}), key)

    def test_operator_forms_replace_experiment_specific_fixture_values_with_placeholders(self):
        folder = Path(__file__).resolve().parents[2] / "docs/zkit/letopis/pilot/templates"
        study = json.loads((folder / "study.json").read_text())
        session = json.loads((folder / "session.json").read_text())
        for key in ("name", "version", "model", "model_revision", "config_sha256"):
            self.assertTrue(study["agent"][key].startswith("<fill"), key)
        self.assertTrue(study["conditions"]["memory"]["memory_store_id"].startswith("<fill"))
        for chain in study["chains"]:
            self.assertTrue(chain["base_commit"].startswith("<fill"))
            self.assertTrue(chain["starting_code_sha256"].startswith("<fill"))
            self.assertIsNone(chain["real_tasks"])
        for key in ("prompt_sha256", "code_before_sha256", "code_after_sha256"):
            self.assertTrue(session[key].startswith("<fill"), key)
        for key in ("memory_before", "memory_after"):
            self.assertIsNone(session[key]["items"])
            self.assertTrue(session[key]["sha256"].startswith("<fill"))


if __name__ == "__main__":
    unittest.main()
