"""Model-free tests for TK-30 telemetry normalization, preparation slots and the P-1 demo.

Every token number below is an independently authored literal, never a value computed by
the code under test.
"""
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

import records
import telemetry
from test_environment import build_manifest

HERE = Path(__file__).resolve().parent
CHECKER = HERE.parent / "pilot_protocol/check.py"
CONTEXT = {"slot": "C1-T1-baseline", "model": "agent-model", "purpose": "agent",
           "correlation_attribute": "request.id"}
TOKEN_KEYS = ("input_tokens", "output_tokens", "cached_input_tokens",
              "reasoning_output_tokens")


class TelemetryTests(unittest.TestCase):
    def test_cached_and_reasoning_are_subsets_of_input_and_output(self):
        # Given
        events = [request("r1"), completion("r1")]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then: independently authored input=100, output=20, cached=80, reasoning=5.
        row = result["requests"][0]
        self.assertEqual(row["input_tokens"] + row["output_tokens"], 120)
        self.assertEqual(row["cached_input_tokens"], 80)
        self.assertEqual(row["reasoning_output_tokens"], 5)
        self.assertEqual((row["id"], row["model"], row["purpose"]),
                         ("C1-T1-baseline:r1", "agent-model", "agent"))
        self.assertEqual(result["request_inventory"], {"ids": ["C1-T1-baseline:r1"]})
        self.assertTrue(result["complete"])
        self.assertEqual(result["unresolved"], [])
        self.assertEqual(result["correlation_attribute"], "request.id")

    def test_synthetic_unknown_ids_never_pair_with_real_request_ids(self):
        # Given: a real correlation ID looks exactly like the synthetic unknown label.
        events = [request("uncorrelated-1"), request(), completion("uncorrelated-1")]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertEqual(len(set(result["request_inventory"]["ids"])), 2)
        self.assertEqual(result["requests"][0]["input_tokens"], 100)
        self.assertIsNone(result["requests"][1]["input_tokens"])
        self.assertIsNone(result["requests"][1]["output_tokens"])
        self.assertFalse(result["complete"])

    def test_equal_counts_without_correlation_remain_unknown(self):
        # Given: two requests and two completions, but no correlation identity
        events = [request(), request(), completion(), completion(None, 300, 40, 0, 0)]

        # When
        unmatched = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertFalse(unmatched["complete"])
        self.assertEqual(len(unmatched["requests"]), 2)
        for row in unmatched["requests"]:
            self.assertEqual([row[key] for key in TOKEN_KEYS], [None] * 4)
        self.assertEqual([item["reason"] for item in unmatched["unresolved"]],
                         ["completion_without_correlation"] * 2)

    def test_duplicate_collector_event_counts_once(self):
        # Given
        events = [request("r1"), request("r1"), completion("r1"), completion("r1")]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertEqual(len(result["requests"]), 1)
        self.assertEqual(result["requests"][0]["input_tokens"], 100)
        self.assertTrue(result["complete"])

    def test_retried_request_and_control_completion_remain_separate_rows(self):
        # Given: r1 failed without completion and was retried as r2; r3 is a control call
        events = [request("r1"), request("r2"), completion("r2", 50, 7, 10, 1),
                  request("r3"), completion("r3", 9, 2, 0, 0)]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        rows = {row["id"].split(":")[1]: row for row in result["requests"]}
        self.assertEqual(sorted(rows), ["r1", "r2", "r3"])
        self.assertIsNone(rows["r1"]["input_tokens"])
        self.assertEqual((rows["r2"]["input_tokens"], rows["r2"]["output_tokens"]), (50, 7))
        self.assertEqual((rows["r3"]["input_tokens"], rows["r3"]["output_tokens"]), (9, 2))
        self.assertFalse(result["complete"])

    def test_conflicting_duplicates_stay_unresolved(self):
        # Given
        events = [request("r1"), completion("r1", 100, 20), completion("r1", 100, 30)]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertIsNone(result["requests"][0]["output_tokens"])
        self.assertEqual([(item["reason"], item["id"]) for item in result["unresolved"]],
                         [("conflicting_duplicate", "r1")])
        self.assertFalse(result["complete"])

    def test_unmatched_completion_is_kept_separately(self):
        # Given
        events = [request("r1"), completion("r1"), completion("r9", 70, 3, 0, 0)]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertEqual(result["request_inventory"]["ids"], ["C1-T1-baseline:r1"])
        self.assertEqual(len(result["unresolved"]), 1)
        kept = result["unresolved"][0]
        self.assertEqual((kept["reason"], kept["id"]), ("completion_without_request", "r9"))
        self.assertEqual(kept["usage"]["input_tokens"], 70)
        self.assertFalse(result["complete"])

    def test_missing_counter_stays_null_not_zero(self):
        # Given
        events = [request("r1"), completion("r1", reasoning=None)]

        # When
        result = telemetry.normalize_events(events, CONTEXT)

        # Then
        self.assertIsNone(result["requests"][0]["reasoning_output_tokens"])
        self.assertEqual(result["requests"][0]["output_tokens"], 20)

    def test_no_events_are_incomplete_not_zero_usage(self):
        # When
        result = telemetry.normalize_events([], CONTEXT)

        # Then
        self.assertFalse(result["complete"])
        self.assertEqual(result["requests"], [])
        self.assertEqual([item["reason"] for item in result["unresolved"]],
                         ["no_request_events"])

    def test_missing_collector_file_is_an_error_not_empty_telemetry(self):
        # Given
        with tempfile.TemporaryDirectory() as folder:
            missing = Path(folder) / "never-written.jsonl"

            # When / Then
            with self.assertRaises(FileNotFoundError):
                telemetry.read_collector(missing)

    def test_correlation_attribute_is_chosen_by_context(self):
        # Given: the request carries two identities, the completion only one
        events = [dict(request("r1"), **{"trace.id": "t1"}), completion("r1")]
        by_trace = dict(CONTEXT, correlation_attribute="trace.id")

        # When
        paired = telemetry.normalize_events(events, CONTEXT)
        unpaired = telemetry.normalize_events(events, by_trace)

        # Then
        self.assertTrue(paired["complete"])
        self.assertEqual(paired["requests"][0]["input_tokens"], 100)
        self.assertEqual(unpaired["correlation_attribute"], "trace.id")
        self.assertEqual(unpaired["request_inventory"]["ids"], ["C1-T1-baseline:t1"])
        self.assertIsNone(unpaired["requests"][0]["input_tokens"])
        self.assertEqual([item["reason"] for item in unpaired["unresolved"]],
                         ["completion_without_correlation"])
        self.assertFalse(unpaired["complete"])

    def test_unknown_correlation_attribute_pairs_nothing(self):
        # Given
        context = {key: value for key, value in CONTEXT.items()
                   if key != "correlation_attribute"}

        # When
        result = telemetry.normalize_events([request("r1"), completion("r1")], context)

        # Then
        self.assertIsNone(result["correlation_attribute"])
        self.assertEqual(result["request_inventory"]["ids"], ["C1-T1-baseline:uncorrelated-1"])
        self.assertIsNone(result["requests"][0]["input_tokens"])
        self.assertIn("correlation_attribute_unknown",
                      [item["reason"] for item in result["unresolved"]])
        self.assertFalse(result["complete"])

    def test_context_must_fix_slot_model_and_purpose(self):
        # When / Then
        for missing in ("slot", "model", "purpose"):
            with self.subTest(missing):
                context = {key: value for key, value in CONTEXT.items() if key != missing}
                with self.assertRaises(ValueError):
                    telemetry.normalize_events([], context)

    def test_collector_file_is_flattened_with_integer_counters(self):
        # Given: one OTLP/HTTP JSON batch as written by the TK-17 collector
        with tempfile.TemporaryDirectory() as folder:
            path = Path(folder) / "slot.jsonl"
            attributes = [{"key": "event.kind", "value": {"stringValue": "response.completed"}},
                          {"key": "input_token_count", "value": {"intValue": "100"}}]
            batch = {"payload": {"resourceLogs": [{"scopeLogs": [
                {"logRecords": [{"attributes": attributes}]}]}]}}
            path.write_text(json.dumps(batch) + "\n")

            # When
            events = telemetry.read_collector(path)

        # Then
        self.assertEqual(events, [{"event.kind": "response.completed",
                                   "input_token_count": 100}])


class RecordTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_templates_are_eighteen_unexecuted_preparation_slots(self):
        # Given
        manifest = build_manifest(image=None)

        # When
        result = records.build_templates(manifest, self.tmp / "records")

        # Then
        files = sorted((self.tmp / "records").iterdir())
        self.assertEqual((result["slots"], result["state"]), (18, "unready"))
        self.assertEqual(len(files), 18)
        first = json.loads((self.tmp / "records/C1-T1-baseline.json").read_text())
        self.assertEqual(first, {
            "kind": "preparation", "state": "unready", "slot": "C1-T1-baseline",
            "chain_id": "C1", "task_id": "C1-T1", "condition": "baseline", "position": 1,
            "session_id": None, "outcome": None, "checks": None,
            "request_inventory": None, "requests": []})
        for path in files:
            saved = json.loads(path.read_text())
            self.assertEqual((saved["kind"], saved["session_id"], saved["checks"]),
                             ("preparation", None, None))

    def test_templates_refuse_existing_output_or_partial_order(self):
        # Given
        manifest = build_manifest(image=None)
        (self.tmp / "exists").mkdir()
        short = dict(manifest, order=manifest["order"][:4])

        # When / Then
        with self.assertRaises(FileExistsError):
            records.build_templates(manifest, self.tmp / "exists")
        with self.assertRaises(ValueError):
            records.build_templates(short, self.tmp / "short")
        self.assertFalse((self.tmp / "short").exists())

    def test_demo_passes_p1_checker_as_demonstration_only(self):
        # Given
        output = self.tmp / "demo"

        # When
        written = records.write_demo(output)
        checked = subprocess.run([sys.executable, "-B", str(CHECKER), str(written / "study.json"),
                                  str(written / "sessions"), "--evidence-root", str(written)],
                                 capture_output=True, text=True, timeout=60)

        # Then
        self.assertEqual(checked.returncode, 0, checked.stdout)
        report = json.loads(checked.stdout)
        self.assertEqual((report["status"], report["kind"], report["stage"]),
                         ("demonstration_only", "demonstration", "final"))
        self.assertEqual(report["agent_executions"], 0)
        self.assertEqual(len(list((written / "sessions").glob("*.json"))), 18)
        for condition in ("baseline", "memory"):
            self.assertEqual(sum(report["conditions"][condition]["outcomes"].values()), 9)
        study = json.loads((written / "study.json").read_text())
        self.assertNotEqual(study["kind"], "pilot")

    def test_demo_refuses_existing_output(self):
        # Given
        (self.tmp / "demo").mkdir()

        # When / Then
        with self.assertRaises(FileExistsError):
            records.write_demo(self.tmp / "demo")


def request(request_id=None):
    event = {"event.name": "codex.api_request", "endpoint": "/v1/responses"}
    if request_id is not None:
        event["request.id"] = request_id
    return event


def completion(request_id=None, input_tokens=100, output_tokens=20, cached=80, reasoning=5):
    event = {"event.kind": "response.completed", "input_token_count": input_tokens,
             "output_token_count": output_tokens, "cached_token_count": cached,
             "reasoning_token_count": reasoning}
    if request_id is not None:
        event["request.id"] = request_id
    return event


if __name__ == "__main__":
    unittest.main()
