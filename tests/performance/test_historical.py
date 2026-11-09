"""Historical fixture bytes and durable campaign scheduling contracts."""

import contextlib
import io
import json
from pathlib import Path
import re
import sqlite3
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import historical
from scenarios import run_git


class HistoricalTests(unittest.TestCase):
    def test_shared_fixture_contains_distinct_ranges(self):
        # Given: independent directories with the same fixed seed.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            # When: rendering the shared historical workload twice.
            left = historical.prepare_historical_fixture(root / "left", 90, "shared")
            right = historical.prepare_historical_fixture(root / "right", 90, "shared")
            data = (left["repository"] / "knowledge/part-0000.md").read_bytes()
            # Then: tree, bytes, metadata, navigation and fragment sizes agree.
            self.assertEqual(left["source_tree"], right["source_tree"])
            self.assertEqual(left["input_sha256"], right["input_sha256"])
            self.assertEqual(left["counts"], {"knowledge": 90, "source": 61, "edges": 90, "includes": 0})
            self.assertEqual(len(list((left["repository"] / "sources").iterdir())), 1)
            self.assertIn(b'[source](../sources/shared.txt "letopis:type=external;path=sources/shared.txt")', data)
            self.assertIn(b'../sources/shared.txt#L2-L2', data)
            self.assertIn(b'../sources/shared.txt#L30-L30', data)
            self.assertEqual(len(data), 60000)
            self.assertEqual(data.count(left["source_tree"].encode()), 30)
            source = (left["repository"] / "sources/shared.txt").read_bytes()
            self.assertEqual(source.count(b"\n"), 100)
            self.assertEqual(run_git(left["repository"], "show", left["source_tree"] + ":sources/shared.txt"), source)

    def test_distinct_fixture_uses_one_source_path_per_knowledge(self):
        # Given: a small distinct-path workload.
        with tempfile.TemporaryDirectory() as directory:
            # When: rendering the complete historical tree.
            fixture = historical.prepare_historical_fixture(Path(directory).resolve() / "fixture", 30, "distinct")
            repository = fixture["repository"]
            data = (repository / "knowledge/part-0000.md").read_text()
            # Then: each link owns a whole file with identical bytes.
            self.assertEqual(fixture["counts"], {"knowledge": 30, "source": 30, "edges": 30, "includes": 0})
            self.assertEqual(len(set(re.findall(r'path=(sources/source-\d+\.txt)', data))), 30)
            self.assertNotIn("#L", data)
            contents = [(repository / f"sources/source-{index}.txt").read_bytes() for index in range(30)]
            self.assertTrue(all(content == contents[0] for content in contents))
            self.assertEqual(run_git(repository, "ls-tree", "-r", "--name-only", fixture["source_tree"]).count(b"\n"), 30)
            self.assertFalse(json.loads((repository / ".letopis/config.json").read_text())["embedding_model"]["enabled"])

    def test_three_pairs_alternate_sides(self):
        # Given: instrumented inexpensive calls and real standalone fixtures.
        with tempfile.TemporaryDirectory() as directory:
            arguments = _arguments(Path(directory).resolve())
            # When: the three-pair schedule runs.
            result, rows = _campaign(arguments)
            measured = [row for row in rows if row["phase"] == "measured"]
            # Then: six ordinary calls alternate direction with identical manifests.
            self.assertEqual([row["side"] for row in measured], ["before", "after", "after", "before", "before", "after"])
            self.assertEqual(len([row for row in rows if row["phase"] == "conditioning"]), 4)
            self.assertEqual(result["outcome"], "complete")
            manifest = json.loads((arguments.evidence / "manifest.json").read_text())
            self.assertEqual(manifest["fixtures"]["before"], manifest["fixtures"]["after"])
            self.assertEqual(result["series"][0]["before"]["n"], 3)
            self.assertEqual(result["series"][0]["after"]["n"], 3)
            # When / Then: immutable evidence rejects reuse.
            with self.assertRaises(ValueError):
                historical.run_campaign(arguments)

    def test_timeout_stops_series_and_preserves_attempt(self):
        # Given: before's first ordinary call times out.
        with tempfile.TemporaryDirectory() as directory:
            # When: the failure reaches orchestration.
            result, rows = _campaign(_arguments(Path(directory).resolve()), failure="timeout")
            measured = [row for row in rows if row["phase"] == "measured"]
            # Then: failed side stops while the feasible side finishes.
            self.assertEqual([row["side"] for row in measured], ["before", "after", "after", "after"])
            self.assertEqual(measured[0]["outcome"], "timeout")
            self.assertEqual(result["outcome"], "incomplete")
            self.assertEqual(result["series"][0]["before"]["n"], 0)
            self.assertEqual(result["series"][0]["before"]["attempts"], 1)

    def test_error_stops_series_without_retry(self):
        # Given: an ordinary invocation exits unsuccessfully.
        with tempfile.TemporaryDirectory() as directory:
            # When: the error reaches orchestration.
            result, rows = _campaign(_arguments(Path(directory).resolve()), failure="nonzero")
            # Then: that side is not retried and its failure stays visible.
            measured = [row for row in rows if row["phase"] == "measured" and row["side"] == "before"]
            self.assertEqual(len(measured), 1)
            self.assertEqual(measured[0]["outcome"], "nonzero")
            self.assertEqual(result["outcome"], "incomplete")

    def test_interruption_stops_new_series(self):
        # Given: cancellation in the first measured command.
        with tempfile.TemporaryDirectory() as directory:
            arguments = _arguments(Path(directory).resolve())
            arguments.scenarios = ["check", "status"]
            # When: an owned measurement reports interruption.
            result, rows = _campaign(arguments, failure="interrupted")
            # Then: cancellation persists and no next side/scenario starts.
            self.assertEqual(result["outcome"], "interrupted")
            self.assertEqual(rows[-1]["outcome"], "interrupted")
            self.assertEqual(len([row for row in rows if row["phase"] == "measured"]), 1)
            self.assertFalse(any(row["name"] == "status" for row in rows))

    def test_profiles_do_not_enter_ordinary_samples(self):
        # Given: profile durations deliberately differ from ordinary durations.
        with tempfile.TemporaryDirectory() as directory:
            arguments = _arguments(Path(directory).resolve())
            arguments.stage = "profiles"
            # When: three alternating plain/profile controls run per side.
            result, rows = _campaign(arguments)
            # Then: ordinary controls retain their own six samples and median.
            self.assertEqual(len([row for row in rows if row["phase"] == "measured" and row["profile_mode"] == "none"]), 6)
            self.assertEqual(len([row for row in rows if row["profile_mode"] == "samply"]), 6)
            self.assertEqual([row["profile_mode"] for row in rows if row["phase"] == "measured"],
                             ["none", "samply", "none", "samply", "samply", "none", "samply", "none", "none", "samply", "none", "samply"])
            self.assertEqual(result["series"][0]["before"]["median"], 1)
            self.assertEqual(result["series"][0]["after"]["median"], 1)
            self.assertTrue(all(row["duration_seconds"] == 90 for row in rows if row["profile_mode"] == "samply"))

    def test_baseline_only_has_no_candidate_comparison(self):
        # Given: only a before receipt exists.
        with tempfile.TemporaryDirectory() as directory:
            arguments = _arguments(Path(directory).resolve())
            arguments.after = None
            # When: gathering a pre-change baseline.
            result, rows = _campaign(arguments)
            # Then: no fabricated after samples or speedup appear.
            self.assertEqual({row["side"] for row in rows}, {"before"})
            self.assertEqual(result["mode"], "baseline_only")
            self.assertNotIn("speedup", result["series"][0])
            self.assertNotIn("after", result["series"][0])


def _arguments(root):
    receipt = root / "build.json"
    receipt.write_text(json.dumps({"executable": "fixture-letopis", "rustc": "test compiler"}))
    return SimpleNamespace(before=receipt, after=receipt, nodes=30, variant="shared", stage="timings",
                           scenarios=["check"], timeout=120, initial_timeout=600,
                           stage_seconds=1200, evidence=root / "evidence")


def _campaign(arguments, failure=None):
    def measure(argv, repository, data, options):
        connection = sqlite3.connect(repository / ".letopis/cache.db")
        connection.executescript("create table if not exists nodes(kind text); create table if not exists edges(id integer);")
        if not connection.execute("select count(*) from nodes").fetchone()[0]:
            connection.executemany("insert into nodes values(?)", [("vertex",)] * 30 + [("source",)] * 29)
            connection.executemany("insert into edges values(?)", [(index,) for index in range(30)])
            connection.commit()
        connection.close()
        previous = [json.loads(line) for line in (arguments.evidence / "samples.jsonl").read_text().splitlines()]
        is_failure = failure and sum(row["phase"] == "conditioning" for row in previous) == 4 and not any(row["phase"] == "measured" for row in previous)
        return _measurement(failure if is_failure else "success", 1)
    def profile(case, build, evidence):
        evidence.mkdir()
        receipt = {"outcome": "recorded", "measurement": _measurement("success", 90)}
        (evidence / "receipt.json").write_text(json.dumps(receipt))
        return receipt
    with contextlib.redirect_stdout(io.StringIO()), patch("historical.verify_frozen_build"), \
            patch("historical.measure_process", side_effect=measure), patch("historical.record_profile", side_effect=profile):
        result = historical.run_campaign(arguments)
    rows = [json.loads(line) for line in (arguments.evidence / "samples.jsonl").read_text().splitlines()]
    return result, rows


def _measurement(outcome, seconds):
    return {"outcome": outcome, "duration_seconds": seconds, "exit_code": 0,
            "pid": 123, "resources": {}, "stdout": {"capture_hex": b'{"ok":true,"result":{"errors":[]}}'.hex(),
            "truncated": False}, "stderr": {"capture_hex": "", "truncated": False}}


if __name__ == "__main__":
    unittest.main()
