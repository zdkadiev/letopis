import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from fixtures import make_dataset, write_dataset
from validation import validate
from check import summarize

HERE = Path(__file__).resolve().parent


def mark_failed_with_blocked_followup(sessions):
    first, following = sessions[0], sessions[1]
    first["outcome"] = "failed"
    first["checks"][0]["passed"] = False
    following.update(outcome="not_run", session_id=None, reason="unsuccessful predecessor",
                     code_after_sha256=following["code_before_sha256"],
                     memory_after=copy.deepcopy(following["memory_before"]), requests=[], research=[],
                     workload_profile=[])
    following["request_inventory"]["ids"] = []
    for check in following["checks"]:
        check.update(passed=None, evidence=None)
    for agreement in following["agreements"]:
        agreement.update(violated=None, evidence=None)


class ReportTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.study, self.sessions = make_dataset(self.root)

    def report(self):
        validate(self.study, self.sessions, self.root)
        return summarize(self.study, self.sessions)

    def pilot_labels(self):
        # Exercises supplied labels only; still no actual pilot/model execution.
        self.study["kind"] = "pilot"
        for chain in self.study["chains"]:
            chain["real_tasks"] = True

    def test_literal_totals_include_auxiliary_usage_without_subset_double_counting(self):
        result = self.report()
        baseline, memory = (result["conditions"][x] for x in ("baseline", "memory"))
        self.assertEqual(baseline["tokens"]["total"], 480)
        self.assertEqual(memory["tokens"]["total"], 462)
        self.assertEqual(baseline["tokens"]["input_tokens"], 400)
        self.assertEqual(baseline["tokens"]["cached_input_tokens"], 16)
        self.assertEqual(baseline["tokens"]["reasoning_output_tokens"], 8)
        self.assertEqual(memory["tokens"]["by_model"]["fake-embedding"]["total"], 12)
        self.assertEqual(memory["tokens"]["by_purpose"]["embedding"]["total"], 12)
        self.assertEqual(memory["tokens"]["request_count"], 8)
        self.assertEqual(result["status"], "demonstration_only")
        self.assertEqual(result["agent_executions"], 0)
        self.assertNotIn("winner", result)
        self.assertTrue(result["correctness_preserved"])
        self.assertTrue(result["data_complete"])
        self.assertEqual(len(result["paired_correct_slots"]), 4)

    def test_research_agreement_denominators_and_workload_rows_are_visible(self):
        result = self.report()["conditions"]
        self.assertEqual(result["baseline"]["research"],
                         {"first": 2, "repeat": 2, "revalidation": 0, "unresolved": 0, "repeat_count": 2})
        self.assertEqual(result["memory"]["research"]["revalidation"], 2)
        agreements = result["baseline"]["agreements"]
        self.assertEqual((agreements["planned"], agreements["assessed"], agreements["violations"]), (4, 4, 1))
        self.assertEqual(agreements["violation_rate"], 0.25)
        profile = result["memory"]["workload_profile"]
        self.assertEqual(profile["events"], 6)
        self.assertEqual(profile["by_operation"], {"create": 4, "read": 2})
        self.assertEqual(profile["known_bytes"], 520)
        self.assertEqual(len(profile["records"]), 6)

    def test_unknown_input_retains_known_subtotal_and_blocks_complete_comparison(self):
        self.pilot_labels()
        self.sessions[0]["requests"][0]["input_tokens"] = None
        result = self.report()
        tokens = result["conditions"]["baseline"]["tokens"]
        self.assertIsNone(tokens["total"])
        self.assertIsNone(tokens["input_tokens"])
        self.assertEqual(tokens["known_total"], 380)
        self.assertEqual(tokens["unknown_request_ids"], [self.sessions[0]["requests"][0]["id"]])
        self.assertFalse(result["data_complete"])
        self.assertEqual(result["status"], "incomplete_data")

    def test_unknown_cache_breakdown_does_not_erase_known_usage(self):
        self.sessions[0]["requests"][0]["cached_input_tokens"] = None
        result = self.report()
        self.assertEqual(result["conditions"]["baseline"]["tokens"]["total"], 480)
        self.assertIsNone(result["conditions"]["baseline"]["tokens"]["cached_input_tokens"])
        self.assertTrue(result["data_complete"])

    def test_failed_and_not_run_tasks_remain_in_correctness_and_costs(self):
        self.pilot_labels()
        mark_failed_with_blocked_followup(self.sessions)
        result = self.report()
        self.assertEqual(result["conditions"]["baseline"]["outcomes"],
                         {"correct": 2, "failed": 1, "incomplete": 0, "not_run": 1})
        self.assertEqual(result["conditions"]["baseline"]["tokens"]["total"], 360)
        self.assertEqual(result["status"], "correctness_not_preserved")
        self.assertFalse(result["correctness_preserved"])
        self.assertEqual(result["paired_correct_slots"], ["C2-T1", "C2-T2"])
        self.assertEqual(result["conditions"]["baseline"]["agreements"]["not_run"], 1)

    def test_unresolved_annotations_do_not_look_like_zero_violations_or_repeats(self):
        self.pilot_labels()
        self.sessions[1]["research"][0]["kind"] = "unresolved"
        self.sessions[0]["agreements"][0]["violated"] = None
        result = self.report()
        baseline = result["conditions"]["baseline"]
        self.assertIsNone(baseline["research"]["repeat_count"])
        self.assertEqual(baseline["research"]["repeat"], 1)
        self.assertEqual(baseline["research"]["unresolved"], 1)
        self.assertIsNone(baseline["agreements"]["violation_rate"])
        self.assertFalse(result["data_complete"])

    def test_absent_agreement_opportunities_are_unmeasured_not_perfect(self):
        self.pilot_labels()
        for chain in self.study["chains"]:
            for task in chain["tasks"]:
                task["agreement_ids"] = []
        for record in self.sessions:
            record["agreements"] = []
        result = self.report()
        self.assertIsNone(result["conditions"]["memory"]["agreements"]["violation_rate"])
        self.assertFalse(result["data_complete"])

    def test_unknown_workload_measurement_is_preserved_for_resource_handoff(self):
        self.sessions[2]["workload_profile"][0]["bytes"] = None
        result = self.report()
        profile = result["conditions"]["memory"]["workload_profile"]
        self.assertFalse(profile["complete"])
        self.assertEqual(profile["known_bytes"], 440)
        self.assertIsNone(profile["records"][0]["bytes"])

    def test_supplied_complete_pilot_is_only_descriptive(self):
        self.pilot_labels()
        result = self.report()
        self.assertEqual(result["status"], "descriptive_only")
        self.assertEqual(result["agent_executions"], 0)
        self.assertIn("supplied", result["scope"])

    def test_final_sample_is_three_chains_of_three_with_all_eighteen_records(self):
        self.study, self.sessions = make_dataset(self.root, "final")
        result = self.report()
        self.assertEqual(result["conditions"]["baseline"]["outcomes"]["correct"], 9)
        self.assertEqual(result["conditions"]["memory"]["outcomes"]["correct"], 9)

    def test_failed_check_does_not_hide_other_unassessed_acceptance_evidence(self):
        self.pilot_labels()
        mark_failed_with_blocked_followup(self.sessions)
        self.sessions[0]["checks"][1].update(passed=None, evidence=None)
        result = self.report()
        self.assertFalse(result["data_complete"])
        self.assertFalse(result["correctness_preserved"])
        checks = result["conditions"]["baseline"]["correctness_checks"]
        self.assertEqual(checks["planned"], 8)
        self.assertEqual(checks["assessed"], 5)
        self.assertEqual(checks["not_run"], 2)
        self.assertEqual(checks["unresolved_check_ids"], ["C1-T1/regression"])
        self.assertEqual(result["conditions"]["baseline"]["outcomes"]["failed"], 1)
        self.assertEqual(result["status"], "incomplete_data")


class CliTests(unittest.TestCase):
    def setUp(self):
        self.scratch = tempfile.TemporaryDirectory()
        self.addCleanup(self.scratch.cleanup)
        self.root = Path(self.scratch.name)
        self.study, self.sessions = make_dataset(self.root)
        write_dataset(self.root, self.study, self.sessions)

    def command(self):
        return subprocess.run([sys.executable, "-B", str(HERE / "check.py"),
                               str(self.root / "study.json"), str(self.root / "sessions"),
                               "--evidence-root", str(self.root)], capture_output=True, text=True)

    def test_real_cli_reports_demo_and_never_changes_input_bytes(self):
        before = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                  for p in self.root.rglob("*") if p.is_file()}
        result = self.command()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(json.loads(result.stdout)["status"], "demonstration_only")
        after = {str(p): hashlib.sha256(p.read_bytes()).hexdigest()
                 for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def test_cli_rejects_corruption_without_repair(self):
        target = self.root / "evidence/demonstration.txt"
        target.write_text("changed")
        result = self.command()
        self.assertEqual(result.returncode, 2)
        self.assertIn("checksum", json.loads(result.stdout)["error"])
        self.assertEqual(target.read_text(), "changed")

    def test_duplicate_json_keys_nonfinite_numbers_and_wrong_root_are_errors(self):
        for content, message in [('{"protocol_version":1,"protocol_version":1}', "duplicate JSON key"),
                                 ('{"value":NaN}', "non-finite JSON number"),
                                 ('[]', "study")]:
            with self.subTest(content=content):
                (self.root / "study.json").write_text(content)
                result = self.command()
                self.assertEqual(result.returncode, 2)
                self.assertIn(message, json.loads(result.stdout)["error"])
                self.assertNotIn("Traceback", result.stderr)

    def test_session_input_symlink_cannot_read_outside_session_directory(self):
        source = self.root / "sessions/baseline-C1-T1.json"
        external = self.root / "outside-session.json"
        source.rename(external)
        source.symlink_to(external)
        result = self.command()
        self.assertEqual(result.returncode, 2)
        self.assertIn("session", json.loads(result.stdout)["error"])

    def test_session_symlink_loop_reports_json_error_without_traceback(self):
        source = self.root / "sessions/baseline-C1-T1.json"
        source.unlink()
        source.symlink_to(source.name)
        result = self.command()
        self.assertEqual(result.returncode, 2)
        self.assertFalse(json.loads(result.stdout)["protocol_valid"])
        self.assertNotIn("Traceback", result.stderr)

    def test_large_integer_profile_times_do_not_overflow_float_conversion(self):
        for key in ("timestamp_ms", "elapsed_ms"):
            with self.subTest(key=key):
                study, sessions = make_dataset(self.root)
                sessions[2]["workload_profile"][0][key] = 10 ** 400
                write_dataset(self.root, study, sessions)
                result = self.command()
                self.assertEqual(result.returncode, 0, result.stderr)
                row = json.loads(result.stdout)["conditions"]["memory"]["workload_profile"]["records"][0]
                self.assertEqual(row[key], 10 ** 400)
                self.assertNotIn("Traceback", result.stderr)


if __name__ == "__main__":
    unittest.main()
