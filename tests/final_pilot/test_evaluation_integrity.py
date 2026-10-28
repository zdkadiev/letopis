"""Regression checks for evaluation ownership and manual-evidence integrity."""
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile
import time
import unittest

import evaluation
from _copy_fixtures import make_source_repo


class EvaluationIntegrityTests(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def test_watchdog_kills_owned_descendants_before_returning(self):
        # Given
        child, marker = self.build_descendant_scenario()
        parent = self.build_parent(child, "time.sleep(10)")

        # When
        result = self.run_parent(parent, timeout=0.2)
        marker.with_suffix(".release").touch()
        time.sleep(0.2)

        # Then
        self.assertTrue((self.tmp / "ready").exists(), "child never reached its wait")
        self.assertTrue(result["timed_out"])
        self.assertLess(result["elapsed"], 2, "watchdog waited for the sleeping parent")
        self.assertFalse(marker.exists(), "owned descendant survived watchdog")

    def test_success_kills_owned_descendants_before_returning(self):
        # Given
        child, marker = self.build_descendant_scenario()
        parent = self.build_parent(child, "pass")

        # When
        result = self.run_parent(parent, timeout=2)
        marker.with_suffix(".release").touch()
        time.sleep(0.2)

        # Then
        self.assertEqual(result["exit"], 0)
        self.assertFalse(marker.exists(), "owned descendant outlived successful command")

    def test_annotation_changes_are_guarded_including_creation_and_deletion(self):
        # Given
        directory = self.tmp / "operator/annotations"
        directory.mkdir(parents=True)
        path = directory / "C3-T1-memory.json"
        before = evaluation.hash_operator_state(self.tmp, "C3-T1-memory")

        # When / Then
        path.write_text("{}")
        with self.assertRaisesRegex(ValueError, "operator state changed"):
            evaluation.require_operator_unchanged(self.tmp, "C3-T1-memory", before)
        existing = evaluation.hash_operator_state(self.tmp, "C3-T1-memory")
        path.unlink()
        with self.assertRaisesRegex(ValueError, "operator state changed"):
            evaluation.require_operator_unchanged(self.tmp, "C3-T1-memory", existing)

    def test_operator_guard_includes_copy_and_telemetry_sources(self):
        # When
        sources = evaluation.hash_operator_state(self.tmp, "C3-T1-memory")

        # Then
        directory = Path(evaluation.__file__).parent
        self.assertIn(str(directory / "copies.py"), sources)
        self.assertIn(str(directory / "telemetry.py"), sources)
        self.assertFalse(any("__pycache__" in path for path in sources))

    def test_malformed_operator_annotations_stay_unobserved(self):
        # Given
        repo, _base = make_source_repo(self.tmp)
        copy = self.tmp / "study/operator/evaluation/C3-T1-memory"
        copy.mkdir(parents=True)
        shutil.copytree(repo, copy / "repo")
        path = self.tmp / "study/operator/annotations/C3-T1-memory.json"
        path.parent.mkdir(parents=True)
        commit = evaluation.read_captured_commit(copy)
        malformed = ["{", "[]", "null", json.dumps({"captured_commit": commit, "checks": []}),
                     json.dumps({"captured_commit": commit, "checks": {"bounded_buffer": {
                         "passed": True, "refs": [42], "conclusion": "yes"}}})]

        # When / Then
        for value in malformed:
            with self.subTest(value=value):
                path.write_text(value)
                passed, detail = evaluation.read_audit(copy, "bounded_buffer")
                self.assertIsNone(passed)
                self.assertIn("operator audit", detail)

    def test_check_verdict_refuses_integer_and_float_boolean_lookalikes(self):
        # Given
        expected = ["check"]

        # When / Then
        for value in (0, 1, 0.0, 1.0):
            with self.subTest(value=value), self.assertRaises(ValueError):
                evaluation.require_check_ids([{"id": "check", "passed": value}], expected)

    def build_descendant_scenario(self):
        marker = self.tmp / "late-write"
        ready = self.tmp / "ready"
        child = "\n".join([
            "import pathlib,time",
            f"marker=pathlib.Path({str(marker)!r})",
            f"pathlib.Path({str(ready)!r}).touch()",
            "deadline=time.monotonic()+2",
            "while not marker.with_suffix('.release').exists() and time.monotonic()<deadline:",
            "    time.sleep(.01)",
            "if marker.with_suffix('.release').exists(): marker.write_text('late')"])
        return child, marker

    def build_parent(self, child, finish):
        ready = self.tmp / "ready"
        return "\n".join([
            "import pathlib,subprocess,sys,time",
            f"subprocess.Popen([sys.executable,'-c',{child!r}], "
            "stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)",
            f"ready=pathlib.Path({str(ready)!r})",
            "while not ready.exists(): time.sleep(.01)", finish])

    def run_parent(self, parent, timeout):
        return evaluation.run_observed([sys.executable, "-c", parent], cwd=self.tmp,
                                       env=dict(os.environ), evidence=self.tmp,
                                       label="owned child", timeout=timeout)
