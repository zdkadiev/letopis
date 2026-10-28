"""Model-free tests of TK-30 evaluation dispatch and the independent task oracles.

Inputs are literal filesystem, HTTP and JSON fixtures and tiny toy scripts that mimic
observable behaviour; none is a C1-C3 implementation or a pilot record.
"""
import contextlib
import io
import json
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import types
import unittest
from contextlib import redirect_stdout
from unittest import mock

import copies
import evaluation
import run
from oracles import cli, delivery, drain
from test_copies import make_source_repo

HERE = Path(__file__).resolve().parent
SELECTION = HERE.parent.parent / "docs/zkit/letopis/pilot/final/selection.json"
SLOT = "C1-T1-baseline"
FAKE_ORACLE = "tk30_fake_oracle"
C1_T1_IDS = ["version_value", "version_no_stdin_or_state", "regression"]
GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}
INVALID = ('{"ok":false,"error":{"code":"invalid_request","message":"invalid command input",'
           '"details":{}}}')
GENERAL_HELP = """Usage: letopis COMMAND < request.json
Every command reads one JSON object on standard input.
Examples:
  echo '{"paths":["a.md"]}' | letopis check
  echo '{"element_ids":["a.md"]}' | letopis inspect
  echo '{"query":"q","limit":5}' | letopis search
  echo '{"element_ids":["a.md"],"mode":"relations","direction":"outgoing","depth":1,"limit":5}' | letopis traverse
  echo '{}' | letopis reindex
  echo '{}' | letopis status
"""
COMMAND_HELP = {
    "check": "paths (optional): repository paths\n",
    "inspect": "element_ids (required): ids\n",
    "search": "query (required)\nlimit (required)\nfilters (optional)\n",
    "traverse": ("element_ids (required)\nmode (required)\ndirection (required)\n"
                 "depth (required)\nlimit (required)\nedge_types (optional)\n"
                 "edge_limit (optional)\n"),
    "reindex": "input: {}\n",
    "status": "input: {}\n",
}
BUFFER_SOURCE = "fn drain_ready() {\n    let window = 2;\n}\n"


class TempDirTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)
        self.evidence = self.tmp / "evidence"
        self.evidence.mkdir()


class EvaluationDispatchTests(TempDirTestCase):
    def setUp(self):
        super().setUp()
        repo, base = make_source_repo(self.tmp)
        self.root = self.tmp / "study"
        copies.prepare_seeds(repo, base, self.root)
        (self.root / "inputs").mkdir()
        shutil.copyfile(SELECTION, self.root / "inputs/selection.json")
        self.audit = copies.audit_inventory(self.root / "conditions")

    def install_oracle(self, action):
        module = types.ModuleType(FAKE_ORACLE)
        module.evaluate = action
        self.enterContext(mock.patch.dict(sys.modules, {FAKE_ORACLE: module}))
        self.enterContext(mock.patch.dict(evaluation.ORACLE_MODULES, {"C1": FAKE_ORACLE}))

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


class VersionOracleTests(TempDirTestCase):
    def write_binary(self, body):
        binary = self.tmp / "letopis"
        binary.write_text(f"#!/bin/sh\n{body}\nprintf '%s\\n' '{INVALID}'\nexit 1\n")
        binary.chmod(0o755)
        return binary

    def observe(self, body):
        checks = cli.observe_version(self.write_binary(body), self.evidence)
        return {check["id"]: check["passed"] for check in checks}

    def test_exact_version_without_state_passes(self):
        # When
        result = self.observe('[ "$*" = "--version" ] && printf "letopis 0.0.0\\n" && exit 0')

        # Then
        self.assertEqual(result, {"version_value": True, "version_no_stdin_or_state": True})

    def test_wrong_version_or_json_failure_fails(self):
        for body in ('[ "$*" = "--version" ] && printf "letopis 0.0.1\\n" && exit 0', ":"):
            with self.subTest(body):
                # When
                result = self.observe(body)

                # Then
                self.assertFalse(result["version_value"])

    def test_reading_stdin_or_writing_state_fails(self):
        # Given
        reader = 'cat >/dev/null; printf "letopis 0.0.0\\n"; exit 0'
        writer = 'touch "$HOME/.state" .letopis; printf "letopis 0.0.0\\n"; exit 0'

        # When
        with mock.patch.object(cli, "FLAG_WATCHDOG_SECONDS", 1):
            hung = self.observe(reader)
        stateful = self.observe(writer)

        # Then
        self.assertEqual(hung, {"version_value": False, "version_no_stdin_or_state": False})
        self.assertEqual(stateful, {"version_value": True, "version_no_stdin_or_state": False})


class HelpOracleTests(TempDirTestCase):
    def write_binary(self, help_text, extra=""):
        binary = self.tmp / "letopis"
        lines = ["#!/bin/sh", extra,
                 'if [ "$*" = "--version" ]; then printf "letopis 0.0.0\\n"; exit 0; fi',
                 f"if [ \"$*\" = \"--help\" ]; then cat <<'EOF'\n{help_text}EOF\nexit 0; fi"]
        for command, text in COMMAND_HELP.items():
            lines.append(f"if [ \"$*\" = \"{command} --help\" ]; then cat <<'EOF'\n"
                         f"{text}EOF\nexit 0; fi")
        lines += ['if [ "$*" = "embed --drain --help" ]; then echo "internal drain"; exit 0; fi',
                  f"printf '%s\\n' '{INVALID}'", "exit 1"]
        binary.write_text("\n".join(lines) + "\n")
        binary.chmod(0o755)
        return binary

    def test_general_help_needs_json_stdin_and_every_example(self):
        # Given
        without_traverse = GENERAL_HELP.replace("| letopis traverse", "")

        # When
        complete = cli.observe_help(self.write_binary(GENERAL_HELP), self.evidence)
        missing = cli.observe_help(self.write_binary(without_traverse), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in complete], [True, True])
        self.assertEqual([c["passed"] for c in missing], [False, True])
        self.assertIn("traverse", missing[0]["evidence"])

    def test_command_help_explains_forms_and_keeps_invalid_argv(self):
        # When
        good = cli.observe_command_help(self.write_binary(GENERAL_HELP), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in good], [True, True])

    def test_help_for_unknown_command_or_wrong_presence_fails(self):
        # Given
        lenient = 'if [ "$2" = "--help" ] && [ "$1" = "bogus" ]; then echo help; exit 0; fi'
        wrong = dict(COMMAND_HELP, search="query (required)\nlimit (optional)\nfilters (optional)\n")

        # When
        accepted = cli.observe_command_help(self.write_binary(GENERAL_HELP, lenient),
                                            self.evidence)
        with mock.patch.dict(COMMAND_HELP, wrong):
            misdescribed = cli.observe_command_help(self.write_binary(GENERAL_HELP),
                                                    self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in accepted], [True, False])
        self.assertEqual([c["passed"] for c in misdescribed], [False, True])
        self.assertIn("search --help does not mark limit as required", misdescribed[0]["evidence"])


TOY_PACKAGE = r'''
import argparse, hashlib, json, os, subprocess, sys
from pathlib import Path
parser = argparse.ArgumentParser()
for name in ("--binary", "--skill", "--output"):
    parser.add_argument(name, type=Path, required=True)
args = parser.parse_args()
for item in (args.binary, args.skill):
    if not item.is_file() or item.stat().st_size == 0:
        sys.exit(f"invalid input {item}")
if args.output.exists():
    sys.exit("output exists")
COMMIT = subprocess.run(["git", "rev-parse", "HEAD"], capture_output=True, text=True).stdout
HOST = [l[6:] for l in subprocess.run(["rustc", "-vV"], capture_output=True,
        text=True).stdout.splitlines() if l.startswith("host: ")][0]
files = {"bin/letopis": args.binary, "share/letopis/skills/letopis/SKILL.md": args.skill}
for relative, source in files.items():
    path = args.output / relative
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(source.read_bytes())
    path.chmod(0o755 if relative.startswith("bin") else 0o644)
entries = [{"path": p, "sha256": hashlib.sha256(s.read_bytes()).hexdigest()}
           for p, s in sorted(files.items())]
(args.output / "manifest.json").write_text(json.dumps({"version": "0.0.0",
    "source_commit": COMMIT.strip(), "target": HOST, "files": entries}))
'''
TOY_INSTALL = r'''
import argparse, hashlib, json, os, shutil, sys
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--bundle", type=Path, required=True)
parser.add_argument("--prefix", type=Path, required=True)
args = parser.parse_args()
VERIFY = True
manifest = json.loads((args.bundle / "manifest.json").read_text())
for entry in manifest["files"]:
    source = args.bundle / entry["path"]
    if VERIFY and (not source.is_file() or hashlib.sha256(source.read_bytes()).hexdigest()
                   != entry["sha256"]):
        sys.exit(f"damaged bundle: {entry['path']}")
    target = args.prefix / entry["path"]
    if VERIFY and target.exists() and not target.is_file():
        sys.exit(f"collision at {entry['path']}")
for entry in manifest["files"]:
    target = args.prefix / entry["path"]
    target.parent.mkdir(parents=True, exist_ok=True)
    staged = target.with_name(target.name + ".new")
    shutil.copy2(args.bundle / entry["path"], staged)
    os.replace(staged, target)
'''
TOY_CHECK = r'''
import argparse, json, os, subprocess, sys, tempfile
from pathlib import Path
parser = argparse.ArgumentParser()
parser.add_argument("--prefix", type=Path, required=True)
args = parser.parse_args()
binary = args.prefix.resolve() / "bin/letopis"
skill = args.prefix.resolve() / "share/letopis/skills/letopis/SKILL.md"
if not binary.is_file() or binary.stat().st_size == 0 or not os.access(binary, os.X_OK):
    sys.exit(f"bin/letopis is missing, empty or not executable")
if CHECK_SKILL and (not skill.is_file() or skill.stat().st_size == 0):
    sys.exit("SKILL.md is missing or empty")
requests = {"check": {}, "search": {"query": "note", "limit": 5},
            "inspect": {"element_ids": ["note.md"]}, "reindex": {}}
with tempfile.TemporaryDirectory() as repo:
    subprocess.run(["git", "init", "-q", repo], check=True)
    for command, request in requests.items():
        try:
            result = subprocess.run([str(binary), command], cwd=repo, capture_output=True,
                                    input=json.dumps(request).encode())
        except OSError:
            sys.exit("bin/letopis cannot be executed")
        if result.returncode != 0 or not json.loads(result.stdout)["ok"]:
            sys.exit(f"bin/letopis {command} failed")
'''
TOY_PRODUCT = b"#!/bin/sh\ncat >/dev/null\nprintf '{\"ok\":true,\"result\":{}}\\n'\n"


class DeliveryOracleTests(TempDirTestCase):
    def make_copy(self, name, script):
        copy = self.tmp / "copy"
        for directory in ("repo/scripts", "repo/skills/letopis", "home", "target"):
            (copy / directory).mkdir(parents=True)
        (copy / "repo/skills/letopis/SKILL.md").write_bytes(delivery.TOY_SKILL)
        (copy / "repo/scripts" / f"{name}.py").write_text(script)
        for argv in (["init", "-q"], ["add", "."],
                     ["-c", "user.name=t", "-c", "user.email=t@x", "commit", "-qm", "c"]):
            subprocess.run(["git", "-C", str(copy / "repo"), *argv], check=True,
                           capture_output=True, env=GIT_ENV)
        work = copy / "delivery"
        work.mkdir()
        return copy, work

    def test_package_toy_passes_and_lenient_output_fails(self):
        # Given
        lenient = TOY_PACKAGE.replace('    sys.exit("output exists")', '    pass')

        # When
        good = delivery.observe_package(*self.make_copy("package", TOY_PACKAGE), self.evidence)
        shutil.rmtree(self.tmp / "copy")
        loose = delivery.observe_package(*self.make_copy("package", lenient), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in good], [True, True, True])
        self.assertEqual([c["passed"] for c in loose], [True, True, False])
        self.assertIn("existing output", loose[2]["evidence"])

    def test_package_manifest_uses_independent_literal_hashes(self):
        # Given
        copy, _work = self.make_copy("package", TOY_PACKAGE)
        commit = subprocess.run(["git", "-C", str(copy / "repo"), "rev-parse", "HEAD"],
                                capture_output=True, text=True, check=True).stdout.strip()

        # When
        manifest = delivery.build_manifest(copy)

        # Then
        self.assertEqual(manifest["files"], [
            {"path": "bin/letopis",
             "sha256": "2ff8b2ef8d209611d6bb0e86f0fa7fac831528e57d32c2755783bb3f3f9422ac"},
            {"path": "share/letopis/skills/letopis/SKILL.md",
             "sha256": "62cafedd20a4ab280da34391ea756977837a4d10add17f7e6169c881efcd0a71"}])
        self.assertEqual((manifest["version"], manifest["source_commit"]), ("0.0.0", commit))

    def test_install_toy_passes_and_unverified_install_fails(self):
        # Given
        unverified = TOY_INSTALL.replace("VERIFY = True", "VERIFY = False")

        # When
        good = delivery.observe_install(*self.make_copy("install", TOY_INSTALL), self.evidence)
        shutil.rmtree(self.tmp / "copy")
        loose = delivery.observe_install(*self.make_copy("install", unverified), self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in good], [True, True])
        self.assertEqual([c["passed"] for c in loose], [True, False])
        for case in ("changed binary", "directory collision"):
            self.assertIn(case, loose[1]["evidence"])

    def test_installed_check_toy_passes_and_skill_blind_check_fails(self):
        # Given
        real = self.tmp / "product"
        real.write_bytes(TOY_PRODUCT)
        real.chmod(0o755)
        blind = TOY_CHECK.replace("CHECK_SKILL and", "False and")
        careful = TOY_CHECK.replace("CHECK_SKILL and", "True and")

        # When
        good = delivery.observe_installed_check(*self.make_copy("check_install", careful),
                                                real, self.evidence)
        shutil.rmtree(self.tmp / "copy")
        loose = delivery.observe_installed_check(*self.make_copy("check_install", blind),
                                                 real, self.evidence)

        # Then
        self.assertEqual([c["passed"] for c in good], [True, True, True])
        self.assertEqual([c["passed"] for c in loose], [True, False, True])
        self.assertIn("missing skill", loose[1]["evidence"])

    def test_hanging_package_rejection_is_not_a_rejection(self):
        # Given: a packager that hangs instead of rejecting an existing output
        hanging = TOY_PACKAGE.replace('    sys.exit("output exists")',
                                      '    import time; time.sleep(30)')

        # When
        with mock.patch.object(delivery, "SCRIPT_WATCHDOG_SECONDS", 2):
            checks = delivery.observe_package(*self.make_copy("package", hanging), self.evidence)

        # Then
        self.assertFalse(checks[2]["passed"])
        self.assertIn("existing output", checks[2]["evidence"])

    def test_hanging_install_rejection_is_not_a_rejection(self):
        # Given: an installer that hangs on a damaged bundle or a collision
        hanging = TOY_INSTALL.replace("sys.exit(f\"damaged bundle: {entry['path']}\")",
                                      "__import__('time').sleep(30)").replace(
            "sys.exit(f\"collision at {entry['path']}\")", "__import__('time').sleep(30)")

        # When
        with mock.patch.object(delivery, "SCRIPT_WATCHDOG_SECONDS", 2):
            checks = delivery.observe_install(*self.make_copy("install", hanging), self.evidence)

        # Then
        self.assertFalse(checks[1]["passed"])
        for case in ("changed binary", "directory collision"):
            self.assertIn(case, checks[1]["evidence"])

    def test_absent_script_is_a_failed_feature_not_infra(self):
        # Given
        copy, _work = self.make_copy("other", "")
        shutil.rmtree(copy / "delivery")

        # When
        with mock.patch.object(evaluation, "run_regression",
                               return_value={"id": "regression", "passed": True,
                                             "evidence": "ok"}):
            checks = delivery.evaluate(copy, "C2-T1", self.evidence)

        # Then
        self.assertEqual([(c["id"], c["passed"]) for c in checks],
                         [("bundle_layout", False), ("manifest_integrity", False),
                          ("bundle_rejects_invalid_input", False), ("regression", True)])
        self.assertIn("scripts/package.py is absent", checks[0]["evidence"])


class OverlapObserverTests(unittest.TestCase):
    def test_overlap_observer_counts_two_connections(self):
        # Given
        with drain.BarrierServer() as server:
            clients = [threading.Thread(target=post_document, args=(server, marker))
                       for marker in ("alpha", "beta")]
            for client in clients:
                client.start()

            # When
            held = server.wait_for_held(2, 5)
            active = server.active
            server.release_all()
            for client in clients:
                client.join(5)

        # Then
        self.assertTrue(held)
        self.assertEqual((active, server.max_active), (2, 2))

    def test_sequential_control_counts_one(self):
        # Given
        with drain.BarrierServer() as server:
            first = threading.Thread(target=post_document, args=(server, "alpha"))
            first.start()

            # When
            second_overlapped = server.wait_for_held(2, 1)
            server.release_all()
            first.join(5)
            post_document(server, "beta")

        # Then
        self.assertFalse(second_overlapped)
        self.assertEqual((server.max_active, len(server.requests)), (1, 2))

    def test_held_reply_is_the_literal_vector_of_its_marker(self):
        # Given
        with drain.BarrierServer() as server:
            server.release_all()

            # When
            reply = post_document(server, "gamma")

        # Then
        self.assertEqual(reply, {"model": "fixture", "embeddings": [[0.0, 0.0, 1.0]]})


class DrainGradeTests(TempDirTestCase):
    WINDOW = {"held_before_release": 2, "requests_before_release": 2, "max_active": 2,
              "completed": True}

    def test_overlap_grade_requires_exactly_the_bound(self):
        for held, expected in ((2, True), (1, False), (3, False)):
            with self.subTest(held):
                # Given
                window = dict(self.WINDOW, held_before_release=held,
                              requests_before_release=held, max_active=max(held, 2))

                # When
                passed, _evidence = drain.grade_overlap(window)

                # Then
                self.assertIs(passed, expected)

    def test_identity_rejects_swapped_vectors(self):
        # Given
        hashes = {"alpha": "a" * 64, "beta": "b" * 64}
        correct = {"a" * 64: {"vector": [1.0, 0.0, 0.0]}, "b" * 64: {"vector": [0.0, 1.0, 0.0]}}
        swapped = {"a" * 64: {"vector": [0.0, 1.0, 0.0]}, "b" * 64: {"vector": [1.0, 0.0, 0.0]}}

        # When / Then
        self.assertTrue(drain.grade_identity(hashes, correct))
        self.assertFalse(drain.grade_identity(hashes, swapped))

    def test_shared_hash_rejects_duplicate_or_late_attempt(self):
        # Given
        good = {"overlap": True, "max_same_hash": 1, "held_attempts": {"alpha": 1, "beta": 1}}

        # When / Then
        self.assertTrue(drain.grade_shared(good))
        self.assertFalse(drain.grade_shared(dict(good, max_same_hash=2)))
        self.assertFalse(drain.grade_shared(dict(good, held_attempts={"alpha": 1, "beta": None})))
        self.assertFalse(drain.grade_shared(dict(good, overlap=False)))

    def test_retry_grade_requires_exact_attempt_limit(self):
        # Given
        good = {"overlap": True, "completed": True, "malformed_requests": 2, "attempts": 2,
                "error": True, "vector": None, "other_identity": True}

        # When / Then
        self.assertTrue(drain.grade_retry(good))
        for change in ({"attempts": 3, "malformed_requests": 3}, {"error": False},
                       {"completed": False}):
            self.assertFalse(drain.grade_retry(dict(good, **change)))

    def test_buffer_pass_requires_source_audit(self):
        # Given
        copy, commit = self.make_copy()
        annotation = {"captured_commit": commit, "checks": {"bounded_buffer": {
            "passed": True, "conclusion": "window and queue capacity 2, no corpus collection",
            "refs": [{"path": "src/service/embedding.rs", "line": 2, "symbol": "window"}]}}}

        # When
        absent = drain.grade_buffer(self.WINDOW, copy)
        self.write_annotation(copy, dict(annotation, captured_commit="0" * 40))
        foreign = drain.grade_buffer(self.WINDOW, copy)
        self.write_annotation(copy, annotation)
        audited = drain.grade_buffer(self.WINDOW, copy)
        sequential = drain.grade_buffer(dict(self.WINDOW, held_before_release=1), copy)

        # Then
        self.assertIsNone(absent["passed"])
        self.assertIsNone(foreign["passed"])
        self.assertIn("another captured commit", foreign["evidence"])
        self.assertIs(audited["passed"], True)
        self.assertIs(sequential["passed"], False)

    def test_audit_ref_must_show_symbol_on_captured_line(self):
        # Given
        copy, commit = self.make_copy()
        self.write_annotation(copy, {"captured_commit": commit, "checks": {"bounded_buffer": {
            "passed": True, "conclusion": "bounded",
            "refs": [{"path": "src/service/embedding.rs", "line": 1, "symbol": "window"}]}}})

        # When
        result = drain.grade_buffer(self.WINDOW, copy)

        # Then
        self.assertIsNone(result["passed"])
        self.assertIn("does not show 'window'", result["evidence"])

    def make_copy(self):
        copy = self.tmp / "study/operator/evaluation/C3-T1-baseline"
        source = copy / "repo/src/service/embedding.rs"
        source.parent.mkdir(parents=True)
        source.write_text(BUFFER_SOURCE)
        commit = copies.initialize_repository(copy / "repo", "captured")
        return copy, commit

    def write_annotation(self, copy, annotation):
        path = self.tmp / "study/operator/annotations/C3-T1-baseline.json"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(annotation))


TOY_PROBE = """#!/bin/sh
mode=$1; shift 2
if [ "$mode" = hashes ]; then
  printf '{'; sep=
  for file; do
    printf '%s"%s":"%s"' "$sep" "$file" "$(printf %s "$file" | shasum -a 256 | cut -c1-64)"
    sep=,
  done
  printf '}\\n'; exit 0
fi
printf '%s\\n' 'GARBAGE'
"""
TOY_BINARY = "#!/bin/sh\ncat >/dev/null\nprintf '{\"ok\":true,\"result\":{}}\\n'\n"
FAILING_BINARY = "#!/bin/sh\ncat >/dev/null\nprintf '{\"ok\":false}\\n'\nexit 1\n"


class EmbedLockObservationTests(TempDirTestCase):
    STATUSES = {"non-zero exit": "echo '{\"ok\":true,\"result\":{\"locks\":[]}}'; exit 1",
                "garbage": "echo GARBAGE", "json list": "echo '[1, 2]'",
                "not ok": "echo '{\"ok\":false,\"result\":{\"locks\":[]}}'"}

    def make_bench(self, status):
        binary = self.tmp / "letopis"
        binary.write_text("#!/bin/sh\ncat >/dev/null\n"
                          f"if [ \"$1\" = status ]; then {status}; fi\n"
                          "echo '{\"ok\":true,\"result\":{}}'\n")
        binary.chmod(0o755)
        (self.tmp / "home").mkdir(exist_ok=True)
        return drain.Bench(self.tmp, binary, binary, self.evidence)

    def test_release_requires_an_observed_status(self):
        self.enterContext(mock.patch.object(drain, "DRAIN_WATCHDOG_SECONDS", 0.5))
        for name, status in self.STATUSES.items():
            with self.subTest(name):
                # When
                released = drain.wait_for_release(self.make_bench(status), self.tmp)

                # Then
                self.assertFalse(released)

    def test_observed_empty_lock_list_is_released(self):
        # Given
        bench = self.make_bench("echo '{\"ok\":true,\"result\":{\"locks\":[]}}'; exit 0")

        # When / Then
        self.assertTrue(drain.wait_for_release(bench, self.tmp))
        self.assertEqual(drain.observe_embed_lock(bench, self.tmp), (True, None))

    def test_non_object_output_is_not_ok_and_does_not_raise(self):
        # When / Then
        self.assertFalse(drain.read_ok({"stdout": b"[1, 2]"}))
        self.assertEqual(drain.observe_embed_lock(self.make_bench("echo '[1, 2]'"), self.tmp),
                         (False, None))


class ChangeGradeTests(unittest.TestCase):
    OLD = {"alpha": "a" * 64, "beta": "b" * 64}

    def test_config_change_fails_closed_without_observations(self):
        cases = {"probe error": ({"error": "storage"}, False),
                 "identity strings": ({"a" * 64: "config_changed", "b" * 64: "config_changed"},
                                      False),
                 "empty": ({}, False),
                 "stale vector": ({"a" * 64: {"vector": [1.0, 0.0, 0.0]},
                                   "b" * 64: {"vector": None}}, False),
                 "no stale vector": ({"a" * 64: {"vector": None}, "b" * 64: {"vector": None}},
                                     True)}
        for name, (stale, expected) in cases.items():
            with self.subTest(name):
                # When / Then
                self.assertIs(drain.grade_change("config", self.OLD, stale, {}), expected)

    def test_text_change_needs_old_hash_empty_and_new_hash_vector(self):
        # Given
        stale = {"a" * 64: {"vector": [1.0, 0.0, 0.0]}, "b" * 64: {"vector": None}}
        fresh = {"c" * 64: {"vector": [1.0, 1.0, 0.0]}}

        # When / Then
        self.assertTrue(drain.grade_change("text", self.OLD, stale, fresh))
        self.assertFalse(drain.grade_change("text", self.OLD, stale,
                                            {"c" * 64: {"vector": [0.0, 1.0, 0.0]}}))
        self.assertFalse(drain.grade_change("text", self.OLD, stale, {"c" * 64: "x"}))
        self.assertFalse(drain.grade_change("text", self.OLD, {"error": "x"}, fresh))
        self.assertFalse(drain.grade_change("text", self.OLD, stale, {}))


class DrainRobustnessTests(TempDirTestCase):
    def make_bench(self, binary=TOY_BINARY, probe=TOY_PROBE):
        copy = self.tmp / "copy"
        (copy / "home").mkdir(parents=True)
        paths = []
        for name, text in (("letopis", binary), ("probe", probe)):
            path = self.tmp / name
            path.write_text(text)
            path.chmod(0o755)
            paths.append(path)
        return drain.Bench(copy, paths[0], paths[1], self.evidence)

    def fast_windows(self):
        for name, value in (("OVERLAP_WAIT_SECONDS", 0.2), ("QUIET_SECONDS", 0.05),
                            ("DRAIN_WATCHDOG_SECONDS", 5)):
            self.enterContext(mock.patch.object(drain, name, value))

    def test_owned_process_does_not_inherit_operator_environment(self):
        # Given
        out = self.tmp / "seen"
        script = f'printf "%s" "$TK30_CANARY" > "{out}"'

        # When
        with mock.patch.dict(os.environ, {"TK30_CANARY": "operator-secret"}):
            with drain.OwnedProcess(["/bin/sh", "-c", script], env={"PATH": "/usr/bin:/bin"},
                                    stdin=b"") as child:
                child.finish(5)

        # Then
        self.assertEqual(out.read_text(), "")

    def test_participant_reindex_failure_is_graded_false(self):
        # Given
        bench = self.make_bench(binary=FAILING_BINARY)

        # When
        checks = drain.observe_task(bench, "C3-T1")

        # Then
        self.assertEqual([(c["id"], c["passed"]) for c in checks],
                         [("two_requests_before_release", False), ("bounded_buffer", False),
                          ("one_owner_short_writes", False)])
        self.assertIn("reindex", checks[0]["evidence"])

    def test_participant_parse_failure_is_graded_false(self):
        # Given: the probe cannot parse the participant's notes
        bench = self.make_bench(probe="#!/bin/sh\necho '{\"error\":\"parse\"}'\nexit 2\n")

        # When
        checks = drain.observe_task(bench, "C3-T2")

        # Then
        self.assertEqual([c["passed"] for c in checks], [False, False, False])
        self.assertIn("hashes", checks[0]["evidence"])

    def test_probe_garbage_is_graded_without_traceback(self):
        for garbage in ("GARBAGE", "[1, 2]"):
            with self.subTest(garbage):
                # Given
                self.fast_windows()
                shutil.rmtree(self.tmp / "copy", ignore_errors=True)
                bench = self.make_bench(probe=TOY_PROBE.replace("GARBAGE", garbage))

                # When
                first = drain.observe_task(bench, "C3-T1")
                second = drain.observe_task(bench, "C3-T2")

                # Then
                self.assertEqual([c["passed"] for c in first + second], [False] * 6)


def post_document(server, marker):
    """Send one raw HTTP/1.0 embedding request over its own socket; return the JSON reply."""
    body = json.dumps({"model": "fixture", "input": [f"Note tk30-doc-{marker} body"]}).encode()
    port = int(server.url.rsplit(":", 1)[1])
    with socket.create_connection(("127.0.0.1", port), timeout=10) as connection:
        connection.sendall(b"POST /embed HTTP/1.0\r\nContent-Type: application/json\r\n"
                           + f"Content-Length: {len(body)}\r\n\r\n".encode() + body)
        response = b""
        while chunk := connection.recv(4096):
            response += chunk
    return json.loads(response.split(b"\r\n\r\n", 1)[1])


if __name__ == "__main__":
    unittest.main()
