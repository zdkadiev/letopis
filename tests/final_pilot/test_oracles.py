"""TK-30 test oracles; model-free. Shared fixtures are private."""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import sys
import types
import unittest
from contextlib import redirect_stdout
from unittest import mock
import copies
import evaluation
import run
from _copy_fixtures import make_source_repo
from _oracle_fixtures import (
    TempDirTestCase,
)


HERE = Path(__file__).resolve().parent


SELECTION = HERE.parent.parent / "docs/zkit/letopis/pilot/final/selection.json"


SLOT = "C1-T1-baseline"


FAKE_ORACLE = "tk30_fake_oracle"


C1_T1_IDS = ["version_value", "version_no_stdin_or_state", "regression"]


class EvaluationDispatchTests(TempDirTestCase):
    def setUp(self):
        super().setUp()
        repo, base = make_source_repo(self.tmp)
        self.root = self.tmp / "study"
        copies.prepare_seeds(repo, base, self.root)
        (self.root / "inputs").mkdir()
        shutil.copyfile(SELECTION, self.root / "inputs/selection.json")
        self.audit = copies.audit_inventory(self.root / "conditions")

    def test_unknown_slot_or_task_is_rejected_before_capture(self):
        for slot in ("C4-T1-baseline", "C1-T4-memory", "../C1-T1-baseline"):
            with self.subTest(slot):
                # When / Then
                with self.assertRaisesRegex(ValueError, "unknown slot"):
                    evaluation.evaluate(self.root, slot)
        with self.assertRaisesRegex(ValueError, "no oracle"):
            evaluation.load_oracle("C9-T1")
        self.assertFalse((self.root / "operator").exists())

    def test_check_id_mismatch_is_rejected(self):
        # Given
        self.install_oracle(lambda copy, task, evidence: [
            {"id": "version_value", "passed": True, "evidence": "x"},
            {"id": "regression", "passed": True, "evidence": "x"}])

        # When / Then
        with self.assertRaisesRegex(ValueError, "differ from the frozen"):
            evaluation.evaluate(self.root, SLOT)

    def test_infra_failure_is_preserved_as_null_checks(self):
        # Given
        def fail(copy, task, evidence):
            raise evaluation.InfraError("cargo is not on PATH")
        self.install_oracle(fail)

        # When
        result = evaluation.evaluate(self.root, SLOT)

        # Then
        self.assertEqual(result["status"], "infra_error")
        self.assertEqual([(c["id"], c["passed"]) for c in result["checks"]],
                         [(check_id, None) for check_id in C1_T1_IDS])
        self.assertIn("cargo is not on PATH", result["checks"][0]["evidence"])
        self.assertFalse((self.root / "operator/evaluation" / SLOT).exists())

    def test_write_back_is_detected_after_oracle_failure(self):
        # Given: an oracle that writes into the participant and then crashes
        def write_back(copy, task, evidence):
            (copy.parents[2] / "conditions/C1-baseline/repo/oracle-marker").write_text("x")
            raise RuntimeError("oracle crashed")
        self.install_oracle(write_back)

        # When / Then
        with self.assertRaisesRegex(ValueError, "participant state changed"):
            evaluation.evaluate(self.root, SLOT)

    def test_assessed_checks_keep_failures_and_exit_zero(self):
        # Given
        checks = [{"id": "version_value", "passed": False, "evidence": "exit 1"},
                  {"id": "version_no_stdin_or_state", "passed": None, "evidence": "-"},
                  {"id": "regression", "passed": True, "evidence": "ok"}]
        self.install_oracle(lambda copy, task, evidence: checks)
        output = io.StringIO()

        # When
        with redirect_stdout(output):
            code = run.main(["evaluate", "--root", str(self.root), "--slot", SLOT])

        # Then
        result = json.loads(output.getvalue())
        self.assertEqual((code, result["task_id"], result["status"]), (0, "C1-T1", "assessed"))
        self.assertEqual(result["checks"], checks)
        self.assertEqual(copies.audit_inventory(self.root / "conditions"), self.audit)
        self.assertTrue((self.root / "operator/evidence" / SLOT / "run-1/result.json").is_file())

    def test_relative_root_reaches_the_oracle_as_absolute_paths(self):
        # Given: the operator passes the study root relative to its working directory
        seen = []

        def record(copy, task, evidence):
            seen.append((copy, evidence))
            return [{"id": check_id, "passed": None, "evidence": "-"} for check_id in C1_T1_IDS]
        self.install_oracle(record)
        self.enterContext(contextlib.chdir(self.tmp))

        # When
        result = evaluation.evaluate(Path("study"), SLOT)

        # Then
        self.assertEqual(result["status"], "assessed")
        copy, evidence = seen[0]
        self.assertEqual((copy.is_absolute(), evidence.is_absolute()), (True, True))
        self.assertEqual(evaluation.build_environment(copy)["CARGO_TARGET_DIR"],
                         str(self.tmp.resolve() / "study/operator/evaluation" / SLOT / "target"))

    def test_cli_path_arguments_are_absolute(self):
        # Given
        self.enterContext(contextlib.chdir(self.tmp))

        # When
        parser = run.build_parser()
        audit = parser.parse_args(["audit", "--root", "study"])
        evaluate = parser.parse_args(["evaluate", "--root", "study", "--slot", SLOT])
        prepare = parser.parse_args(["prepare", "--repo", "r", "--base", "b", "--output", "o"])

        # Then
        self.assertEqual([audit.root, evaluate.root, prepare.repo, prepare.output],
                         [self.tmp.resolve() / name for name in ("study", "study", "r", "o")])

    def test_operator_state_tamper_is_detected_after_the_oracle(self):
        # Given: oracle sources outside the participant, and three tamper targets
        sources = self.tmp / "operator-sources"
        sources.mkdir()
        (sources / "cli.py").write_text("ORACLE = 1\n")
        self.enterContext(mock.patch.object(evaluation, "OPERATOR_SOURCES", (sources,)))
        targets = {"oracle source": sources / "cli.py",
                   "selection input": self.root / "inputs/selection.json",
                   "capture blob": self.root / "operator/blobs/planted"}
        for name, target in targets.items():
            with self.subTest(name):
                original = target.read_bytes() if target.exists() else None
                def tamper(copy, task, evidence, target=target):
                    target.write_text("tampered")
                    return [{"id": i, "passed": True, "evidence": "-"} for i in C1_T1_IDS]
                self.install_oracle(tamper)

                # When / Then
                with self.assertRaisesRegex(ValueError, "operator state changed"):
                    evaluation.evaluate(self.root, SLOT)
                if original is None:
                    target.unlink()
                else:
                    target.write_bytes(original)

    def test_invalid_slot_exits_two(self):
        # Given
        output = io.StringIO()

        # When
        with redirect_stdout(output):
            code = run.main(["evaluate", "--root", str(self.root), "--slot", "C1-T1"])

        # Then
        self.assertEqual(code, 2)
        self.assertIn("unknown slot", json.loads(output.getvalue())["error"])

    def test_build_without_toolchain_is_infra_error(self):
        # Given
        copy = self.tmp / "copy"
        (copy / "repo").mkdir(parents=True)

        # When / Then
        with mock.patch.dict(os.environ, {"PATH": str(self.tmp / "no-tools")}):
            with self.assertRaisesRegex(evaluation.InfraError, "cannot start cargo"):
                evaluation.build_binary(copy, self.evidence)

    def install_oracle(self, action):
        module = types.ModuleType(FAKE_ORACLE)
        module.evaluate = action
        self.enterContext(mock.patch.dict(sys.modules, {FAKE_ORACLE: module}))
        self.enterContext(mock.patch.dict(evaluation.ORACLE_MODULES, {"C1": FAKE_ORACLE}))


class InfraCauseTests(TempDirTestCase):
    def test_participant_build_failure_keeps_regression_verdict(self):
        # Given: make check already failed, then the build of the same patch fails
        regression = {"id": "regression", "passed": False, "evidence": "make check exit 2"}
        observe = mock.Mock(side_effect=evaluation.InfraError("cargo build failed",
                                                              evaluation.CAUSE_BUILD))

        # When
        with mock.patch.object(evaluation, "run_regression", return_value=regression):
            with self.assertRaises(evaluation.InfraError) as raised:
                evaluation.observe_after_regression(self.tmp, self.evidence, observe)
        status, checks, cause = evaluation.grade_failure(raised.exception, C1_T1_IDS)

        # Then
        self.assertEqual((status, cause), ("infra_error", "participant_build"))
        self.assertEqual([(c["id"], c["passed"]) for c in checks],
                         [("version_value", None), ("version_no_stdin_or_state", None),
                          ("regression", False)])

    def test_build_failure_causes_are_classified(self):
        # Given: a copy whose source has no Cargo manifest, and a PATH without cargo
        copy = self.tmp / "copy"
        (copy / "repo").mkdir(parents=True)

        # When
        with self.assertRaises(evaluation.InfraError) as participant:
            evaluation.build_binary(copy, self.evidence)
        with mock.patch.dict(os.environ, {"PATH": str(self.tmp / "no-tools")}):
            with self.assertRaises(evaluation.InfraError) as environment:
                evaluation.build_binary(copy, self.evidence)

        # Then
        self.assertEqual(participant.exception.cause, "participant_build")
        self.assertEqual(environment.exception.cause, "environment")


class AcceptanceReferenceTests(unittest.TestCase):
    def test_selection_names_the_dispatched_oracle_of_every_task(self):
        # Given
        selection = json.loads(SELECTION.read_text())
        expected = {"C1": "tests/final_pilot/oracles/cli.py",
                    "C2": "tests/final_pilot/oracles/delivery.py",
                    "C3": "tests/final_pilot/oracles/drain.py"}

        # When
        references = {task["id"]: task["acceptance"] for chain in selection["chains"]
                      for task in chain["tasks"]}

        # Then
        self.assertEqual(len(references), 9)
        for task_id, acceptance in references.items():
            self.assertEqual(acceptance["oracle"], expected[task_id[:2]])
            self.assertEqual(evaluation.ORACLE_MODULES[task_id[:2]],
                             "oracles." + Path(acceptance["oracle"]).stem)
            self.assertTrue((HERE.parent.parent / acceptance["oracle"]).is_file())
        self.assertEqual(references["C3-T1"]["manual_checks"], ["bounded_buffer"])


if __name__ == "__main__":
    unittest.main()
