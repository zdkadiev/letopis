"""TK-30 test delivery oracles; model-free. Shared fixtures are private."""
import os
import shutil
import subprocess
from unittest import mock
import evaluation
from oracles import delivery
from _oracle_fixtures import (
    TempDirTestCase,
)
import unittest


GIT_ENV = {**os.environ, "GIT_CONFIG_GLOBAL": os.devnull, "GIT_CONFIG_NOSYSTEM": "1"}


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
    (Path(repo) / "note.md").write_text("Note fixture body\n")
    for command, request in requests.items():
        try:
            result = subprocess.run([str(binary), command], cwd=repo, capture_output=True,
                                    input=json.dumps(request).encode())
        except OSError:
            sys.exit("bin/letopis cannot be executed")
        if result.returncode != 0 or not json.loads(result.stdout)["ok"]:
            sys.exit(f"bin/letopis {command} failed")
'''


TOY_PRODUCT = b'''#!/bin/sh
cat >/dev/null
case "$1" in
check) echo '{"ok":true,"result":{"errors":[]}}' ;;
search) echo '{"ok":true,"result":[{"element_id":"note.md","path":"note.md"}]}' ;;
inspect) echo '{"ok":true,"result":{"elements":{"note.md":{"path":"note.md"}}}}' ;;
reindex) echo '{"ok":true,"result":{"revision":1}}' ;;
esac
'''


class DeliveryOracleTests(TempDirTestCase):
    def test_installed_workflow_rejects_a_source_tree_dependency(self):
        # Given: a check that reads a source-only input before performing its workflow.
        real = self.tmp / "product"
        real.write_bytes(TOY_PRODUCT)
        real.chmod(0o755)
        dependent = TOY_CHECK.replace("CHECK_SKILL and", "True and").replace(
            'requests = {', '(Path(__file__).parent.parent / "source-only").read_text()\nrequests = {')
        copy, work = self.make_copy("check_install", dependent)
        (copy / "repo/source-only").write_text("source dependency")

        # When
        checks = delivery.observe_installed_check(copy, work, real, self.evidence)

        # Then
        self.assertTrue(checks[0]["passed"])
        self.assertFalse(checks[2]["passed"])

    def test_failed_workflow_commands_cannot_pass_when_script_ignores_results(self):
        # Given: all four invocations fail, but the script exits successfully.
        real = self.tmp / "product"
        real.write_bytes(b"#!/bin/sh\ncat >/dev/null\necho '{\"ok\":false}'\nexit 1\n")
        real.chmod(0o755)
        ignoring = TOY_CHECK.replace("CHECK_SKILL and", "True and").replace(
            '        if result.returncode != 0 or not json.loads(result.stdout)["ok"]:',
            '        if False:')

        # When
        checks = delivery.observe_installed_check(*self.make_copy("check_install", ignoring),
                                                  real, self.evidence)

        # Then
        self.assertFalse(checks[0]["passed"])

    def test_installed_workflow_accepts_supported_markdown_extension(self):
        # Given: the product supports .markdown notes as well as .md notes.
        real = self.tmp / "product"
        real.write_bytes(TOY_PRODUCT.replace(b"note.md", b"note.markdown"))
        real.chmod(0o755)
        script = TOY_CHECK.replace("CHECK_SKILL and", "True and").replace(
            "note.md", "note.markdown")

        # When
        checks = delivery.observe_installed_check(*self.make_copy("check_install", script),
                                                  real, self.evidence)

        # Then
        self.assertEqual([check["passed"] for check in checks], [True, True, True])

    def test_installed_workflow_rejects_reversed_successful_commands(self):
        # Given: each command succeeds, but the documented workflow runs backwards.
        real = self.tmp / "product"
        real.write_bytes(TOY_PRODUCT)
        real.chmod(0o755)
        reversed_script = TOY_CHECK.replace("CHECK_SKILL and", "True and").replace(
            "for command, request in requests.items():",
            "for command, request in reversed(list(requests.items())):")

        # When
        checks = delivery.observe_installed_check(
            *self.make_copy("check_install", reversed_script), real, self.evidence)

        # Then
        self.assertFalse(checks[0]["passed"])
        self.assertIn("workflow order", checks[0]["evidence"])

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


if __name__ == "__main__":
    unittest.main()
