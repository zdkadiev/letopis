"""Comparison contracts with literal statistics and real evidence/fixture files."""

import json
import subprocess
import sys
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

from compare import (freeze_build, run_comparison, summarize_comparison,
                     verify_equivalent_cases, verify_frozen_build)
from scenarios import (hash_file, prepare_fixture, prepare_scenario, read_product_inputs,
                       run_git, write_json)
from report import validate_result


class ComparisonTests(unittest.TestCase):
    def test_successful_medians_exclude_censored_and_invalid_attempts(self):
        # Given: known successful times and independently invalid attempts.
        before = [_record(value) for value in (2, 4, 6)] + [_record(120, "timeout")]
        after = [_record(value) for value in (1, 2, 3)] + [_record(600, "nonzero")]
        after.append(dict(_record(400), validation={"valid": False}))
        # When: comparing successful distributions.
        summary = summarize_comparison(before, after)
        # Then: independently known medians and speedup, never censored durations.
        self.assertEqual(summary["before_median_seconds"], 4.0)
        self.assertEqual(summary["after_median_seconds"], 2.0)
        self.assertEqual(summary["speedup"], 2.0)
        self.assertEqual(summary["reduction_percent"], 50.0)
        self.assertEqual(summary["before"]["failures"], 1)
        self.assertEqual(summary["after"]["failures"], 2)

    def test_all_failures_have_no_speedup(self):
        # Given: only a censored attempt on one side.
        before, after = [_record(120, "timeout")], [_record(1)]
        # When: summarizing incomplete coverage.
        summary = summarize_comparison(before, after)
        # Then: no invented median or ratio is reported.
        self.assertIsNone(summary["before_median_seconds"])
        self.assertIsNone(summary["speedup"])

    def test_freeze_survives_checkout_change_but_rejects_archived_source_tamper(self):
        # Given: a frozen clean product built through the release builder.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            source, pin = _freeze(root)
            # When: the working checkout changes after the freeze.
            (source / "src/main.rs").write_text("changed checkout")
            verify_frozen_build(pin)
            archived = Path(pin["source_archive"]) / "src/main.rs"
            archived.chmod(0o600)
            archived.write_text("altered archive")
            # Then: old checkout drift is allowed, old archived input drift is not.
            with self.assertRaises(ValueError):
                verify_frozen_build(pin)

    def test_modified_binary_and_original_receipt_are_rejected(self):
        # Given: an authentic freeze receipt and executable.
        with tempfile.TemporaryDirectory() as directory:
            _, pin = _freeze(Path(directory).resolve())
            binary = Path(pin["executable"])
            original = binary.read_bytes()
            # When / Then: binary and original release receipt drift fail separately.
            binary.chmod(0o755)
            binary.write_bytes(b"changed")
            with self.assertRaises(ValueError):
                verify_frozen_build(pin)
            binary.write_bytes(original)
            receipt = Path(pin["release_receipt"])
            receipt.chmod(0o600)
            receipt.write_text("{}")
            with self.assertRaises(ValueError):
                verify_frozen_build(pin)

    def test_handwritten_executable_hash_is_not_a_frozen_build(self):
        # Given: a hand-written pin without archived sources/release provenance.
        with tempfile.TemporaryDirectory() as directory:
            binary = Path(directory) / "binary"
            binary.write_bytes(b"arbitrary")
            # When / Then: an arbitrary executable cannot enter the campaign.
            with self.assertRaises(ValueError):
                verify_frozen_build({"executable": str(binary), "sha256": hash_file(binary)})

    def test_fixture_content_and_oracle_drift_are_rejected(self):
        # Given: equal generated fixtures, copied independently.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fixture = prepare_fixture(root / "fixture", 60)
            first = prepare_scenario(fixture, "inspect", root / "first")
            second = prepare_scenario(fixture, "inspect", root / "second")
            verify_equivalent_cases(first, second)
            # When / Then: actual bytes and independently frozen expectations matter.
            path = second["repository"] / "knowledge/part-0000.md"
            original = path.read_bytes()
            path.write_bytes(original + b"drift")
            with self.assertRaises(ValueError):
                verify_equivalent_cases(first, second)
            path.write_bytes(original)
            second["expected"]["inspection"]["lines"] = [1, 1]
            with self.assertRaises(ValueError):
                verify_equivalent_cases(first, second)

    def test_independent_inspect_oracle_rejects_self_consistent_wrong_fragment(self):
        # Given: a real generated anchor and its oracle, frozen before invocation.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            fixture = prepare_fixture(root / "fixture", 60)
            case = prepare_scenario(fixture, "inspect", root / "case")
            wrong = dict(case["expected"]["inspection"], lines=[1, 1])
            response = {"ok": True, "result": {"snapshot": fixture["source_tree"],
                        "elements": {case["expected"]["id"]: wrong}}}
            # When: validating a successful transport with the wrong fragment.
            result = validate_result(case, _measurement(response))
            # Then: transport success cannot substitute for addressed-byte correctness.
            self.assertFalse(result["valid"])

    def test_orchestration_alternates_order_and_flushes_each_attempt(self):
        # Given: two frozen copies of a tiny CLI with independently counted projection.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, pin = _freeze(root)
            args = _args(root, pin)
            # When: two warmups and two measured pairs execute as fresh processes.
            result = run_comparison(args)
            rows = [json.loads(line) for line in (args.evidence / "samples.jsonl").read_text().splitlines()]
            # Then: matched pairs alternate order; warmups never inflate coverage.
            self.assertEqual([row["side"] for row in rows if row["phase"] == "measured"],
                             ["before", "after", "after", "before"])
            self.assertEqual(result["series"][0]["before"]["n"], 2)
            self.assertEqual(result["series"][0]["after"]["n"], 2)
            self.assertEqual({row["projection_revision"] for row in rows if row["phase"] == "measured"}, {1})
            with self.assertRaises(ValueError):
                run_comparison(args)

    def test_output_drift_is_retained_and_excluded(self):
        # Given: a CLI returning check findings, with valid build provenance.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, pin = _freeze(root, bad=True)
            args = _args(root, pin)
            # When: first-publication output fails its independent oracle.
            result = run_comparison(args)
            rows = [json.loads(line) for line in (args.evidence / "samples.jsonl").read_text().splitlines()]
            # Then: failed initialization remains evidence and prevents timing claims.
            self.assertEqual(result["outcome"], "incomplete")
            self.assertTrue(rows)
            self.assertFalse(rows[0]["validation"]["valid"])

    def test_freeze_refuses_existing_evidence(self):
        # Given: an existing output with valuable evidence.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "sentinel").write_text("keep")
            # When / Then: output reuse is rejected before any build or replacement.
            with self.assertRaises(ValueError):
                freeze_build(root, root)
            self.assertEqual((root / "sentinel").read_text(), "keep")

    def test_initialization_stage_keeps_one_publication_and_three_fresh_audits(self):
        # Given: a valid frozen fixture CLI and the separate initialization stage.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, pin = _freeze(root)
            args = _args(root, pin)
            args.stage = "initialization"
            # When: publication and audit probes execute outside ordinary series.
            result = run_comparison(args)
            audit, publication = result["series"]
            # Then: each side has one publication and three independently reset audits.
            self.assertEqual(audit["before"]["n"], 3)
            self.assertEqual(audit["after"]["n"], 3)
            self.assertEqual(publication["before"]["n"], 1)
            self.assertEqual(publication["after"]["n"], 1)
            self.assertEqual(len(list(args.evidence.glob("case-check-*-before"))), 3)

    def test_timed_out_publication_is_flushed_and_never_a_success(self):
        # Given: a publication budget below process startup time.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, pin = _freeze(root)
            args = _args(root, pin)
            args.initial_timeout = 0.000001
            # When: owned calls exhaust their explicit budgets.
            result = run_comparison(args)
            rows = [json.loads(line) for line in (args.evidence / "samples.jsonl").read_text().splitlines()]
            # Then: both timeout attempts remain, without a median or later timings.
            self.assertEqual([row["outcome"] for row in rows], ["timeout", "timeout"])
            self.assertEqual(result["outcome"], "incomplete")
            self.assertEqual(result["series"][0]["before"]["n"], 0)
            self.assertIsNone(result["series"][0]["speedup"])

    def test_cli_rejects_pairs_for_non_timing_stages(self):
        # Given: an explicit pairs override for each separately bounded probe stage.
        runner = Path(__file__).with_name("compare.py")
        for stage in ("initialization", "contention", "profiles"):
            arguments = [sys.executable, str(runner), "--stage", stage, "--pairs", "10",
                         "--before", "missing", "--after", "missing", "--evidence", "/tmp/not-created"]
            # When: parsing the unsupported override without opening receipts.
            result = subprocess.run(arguments, capture_output=True, text=True, check=False)
            # Then: it fails as an input error instead of silently ignoring the request.
            self.assertEqual(result.returncode, 2)
            self.assertIn("timings-only", result.stderr)

    def test_different_toolchains_cannot_enter_the_same_campaign(self):
        # Given: independently frozen binaries built with different compilers.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, before = _freeze(root / "before")
            _, after = _freeze(root / "after", rustc="rustc other fixture")
            args = _args(root, before)
            args.after = Path(after["receipt"])
            # When / Then: incomparable compilers fail before any fixture or call.
            with self.assertRaisesRegex(ValueError, "toolchain"):
                run_comparison(args)
            self.assertFalse(args.evidence.exists())

    def test_stage_watchdog_preserves_the_inflight_censored_attempt(self):
        # Given: a publication that exceeds the whole-stage budget.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            _, pin = _freeze(root, delay=2)
            args = _args(root, pin)
            args.stage_seconds = 0.5
            # When: the watchdog interrupts an owned process rather than setup.
            result = run_comparison(args)
            rows = [json.loads(line) for line in (args.evidence / "samples.jsonl").read_text().splitlines()]
            # Then: the interrupted attempt is retained and no second side starts.
            self.assertEqual(result["outcome"], "incomplete")
            self.assertEqual(len(rows), 1)
            self.assertEqual(rows[0]["outcome"], "interrupted")
            self.assertEqual(rows[0]["censor_reason"], "stage watchdog")


def _record(seconds, outcome="success"):
    return {"duration_seconds": seconds, "outcome": outcome, "validation": {"valid": True},
            "name": "check", "nodes": 60, "phase": "measured",
            "profile_mode": "none", "embedding_mode": "fts_disabled"}


def _measurement(response):
    return {"outcome": "success", "stdout": {"truncated": False,
            "capture_hex": json.dumps(response).encode().hex()}}


def _args(root, pin):
    return SimpleNamespace(stage="timings", before=Path(pin["receipt"]), after=Path(pin["receipt"]),
                           nodes=60, scenarios=["check"], pairs=2, timeout=5,
                           initial_timeout=5, stage_seconds=60, evidence=root / "comparison")


def _freeze(root, bad=False, rustc="rustc test fixture", delay=0):
    source = root / "source"
    (source / "src").mkdir(parents=True)
    (source / "src/main.rs").write_text("fn main() {}\n")
    run_git(source, "init", "--quiet", "--template=", "--initial-branch=main")
    run_git(source, "add", ".")
    run_git(source, "-c", "user.name=Fixture", "-c", "user.email=fixture@example.invalid",
            "commit", "-qm", "test source")
    def build(repository, target, evidence):
        evidence.mkdir(parents=True)
        binary = evidence / "product/letopis"
        binary.parent.mkdir()
        script = f"#!{sys.executable}\nimport time; time.sleep({delay})\n" + _CLI
        binary.write_text(script.replace("ERRORS", '[{"code":"drift"}]' if bad else "[]"))
        binary.chmod(0o755)
        pin = {"executable": str(binary), "sha256": hash_file(binary),
               "inputs": read_product_inputs(repository),
               "product_commit": run_git(repository, "rev-parse", "HEAD").decode().strip(),
               "rustc": rustc, "command": ["cargo", "build", "--release", "--locked", "--offline"],
               "release_debug": "1", "target": str(target)}
        write_json(evidence / "build.json", pin)
        return pin
    with patch("compare.build_release", side_effect=build):
        return source, freeze_build(source, root / "frozen")


_CLI = '''import json, pathlib, sqlite3, sys
service = pathlib.Path('.letopis')
connection = sqlite3.connect(service / 'cache.db')
connection.executescript("CREATE TABLE IF NOT EXISTS nodes(kind TEXT); CREATE TABLE IF NOT EXISTS edges(id INTEGER); CREATE TABLE IF NOT EXISTS project(id INTEGER PRIMARY KEY, revision INTEGER); INSERT OR IGNORE INTO project VALUES(1,1);")
if connection.execute('SELECT count(*) FROM nodes').fetchone()[0] == 0:
    connection.executemany('INSERT INTO nodes VALUES(?)', [('vertex',)] * 60 + [('source',)] * 60)
    connection.executemany('INSERT INTO edges VALUES(?)', [(i,) for i in range(300)])
    connection.commit()
connection.close()
request = json.load(sys.stdin)
result = {'errors': ERRORS} if sys.argv[1] == 'check' else ([{'element_id': 'knowledge/part-0000.md#node-000000', 'flags': []}] if request.get('query') else [])
print(json.dumps({'ok': not bool(ERRORS), 'result': result}))
'''


if __name__ == "__main__":
    unittest.main()
