"""Literal statistics and command-specific correctness oracles."""

import json
import hashlib
from pathlib import Path
import unittest
from unittest.mock import patch

from report import summarize_samples, validate_result


def make_record(value: float, outcome: str = "success", mode: str = "fts_disabled") -> dict:
    return {"duration_seconds": value, "outcome": outcome, "validation": {"valid": True},
            "phase": "measured", "embedding_mode": mode, "name": "search", "nodes": 60,
            "profile_mode": "none"}


def make_measurement(response) -> dict:
    return {"outcome": "success", "exit_code": 0,
            "stdout": {"truncated": False, "capture_hex": json.dumps(response).encode().hex()}}


class ReportTests(unittest.TestCase):
    def test_statistics_exclude_timeouts_and_hide_unstable_quantiles(self):
        # Given: four known timings and one timed-out attempt.
        records = [make_record(value) for value in (1, 2, 3, 4)] + [make_record(100, "timeout")]
        # When: computing descriptive statistics.
        summary = summarize_samples(records)
        # Then: censored attempts do not turn into latency observations.
        self.assertEqual(summary["n"], 4)
        self.assertEqual(summary["attempts"], 5)
        self.assertEqual(summary["median"], 2.5)
        self.assertEqual((summary["min"], summary["max"]), (1, 4))
        self.assertIsNone(summary["p95"])
        self.assertIsNone(summary["p99"])
        self.assertEqual(summarize_samples([make_record(i) for i in range(1, 101)])["p95"], 95)

    def test_rejects_pooling_distinct_phases_or_embedding_modes(self):
        # Given: timings from incomparable execution conditions.
        first = make_record(1)
        alternatives = [dict(first, phase="conditioning"), make_record(2, mode="fake_vectors")]
        # When / Then: each mixed group is rejected before quantiles.
        for second in alternatives:
            with self.assertRaises(ValueError):
                summarize_samples([first, second])

    def test_rejects_wrong_search_ids_and_cli_failure(self):
        # Given: a request with a uniquely expected result.
        scenario = {"repository": Path.cwd(), "expected": {"command": "search", "id": "a.md"}}
        # When / Then: exit zero does not validate the wrong ID or ok:false.
        for response in ({"ok": True, "result": [{"element_id": "wrong.md"}]},
                         {"ok": False, "error": {}}, {"ok": True, "result": []}):
            self.assertFalse(validate_result(scenario, make_measurement(response))["valid"])

    def test_rejects_malformed_json_and_invalid_check_results(self):
        # Given: successful process exits with invalid transport or findings.
        scenario = {"repository": Path.cwd(), "expected": {"command": "check"}}
        malformed = make_measurement({})
        malformed["stdout"]["capture_hex"] = b"not json".hex()
        # When / Then: these are correctness failures, not fast successes.
        self.assertFalse(validate_result(scenario, malformed)["valid"])
        response = {"ok": True, "result": {"errors": [{"code": "bad"}]}}
        self.assertFalse(validate_result(scenario, make_measurement(response))["valid"])

    def test_rejects_incomplete_traversal_and_unverifiable_inspection(self):
        # Given: a known graph edge and an inspect request with no valid tree/hash.
        expected = {"command": "traverse", "edges": [["a.md", "b.md", "includes"]]}
        scenario = {"repository": Path.cwd(), "expected": expected}
        # When / Then: missing edges and absent captured bytes cannot pass.
        self.assertFalse(validate_result(scenario, make_measurement(
            {"ok": True, "result": {"elements": [], "edges": []}}))["valid"])
        scenario["expected"] = {"command": "inspect", "id": "a.md"}
        self.assertFalse(validate_result(scenario, make_measurement(
            {"ok": True, "result": {"snapshot": "invalid", "elements": {
                "a.md": {"path": "a.md", "hash": "wrong"}}}}))["valid"])

    def test_inspect_hash_uses_captured_range_bytes(self):
        # Given: two lines in a captured tree, with only line two addressed.
        scenario = {"repository": Path.cwd(), "expected": {"command": "inspect", "id": "a.md"}}
        element = {"path": "a.md", "lines": [2, 2], "hash": hashlib.sha256(b"two\n").hexdigest()}
        response = {"ok": True, "result": {"snapshot": "1" * 40, "elements": {"a.md": element}}}
        # When: checking the claimed bytes, independently of live file contents.
        with patch("report.run_git", return_value=b"one\ntwo\n"):
            valid = validate_result(scenario, make_measurement(response))
            element["hash"] = "0" * 64
            invalid = validate_result(scenario, make_measurement(response))
        # Then: exact captured bytes pass while an incorrect hash fails.
        self.assertTrue(valid["valid"])
        self.assertFalse(invalid["valid"])


if __name__ == "__main__":
    unittest.main()
