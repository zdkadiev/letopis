"""Coverage claims and tiny real-CLI ownership checks."""

import os
from pathlib import Path
import tempfile
import unittest
import time
import threading
import sys
import json
import signal
from unittest.mock import patch

from measure import measure_process as actual_measure

from contention import measure_contention_call as actual_contention_call
from contention import (configure_provider, embed_lease, invoke_cli, observe_vectors, run_canary,
                        run_contention, run_drain_overlap, verify_coverage)
from provider import start_provider


class ContentionTests(unittest.TestCase):
    def test_interrupted_baseline_is_persisted_before_any_writer_launch(self):
        # Given: an owned baseline reader still running when cancellation arrives.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary, scenario = _prepare_interruptible_cli(root, slow_baseline=True)
            def measure_ready_child(argv, repository, data, options):
                return actual_measure(argv, repository, data, dict(
                    options, on_spawn=lambda _: _interrupt_after_baseline_ready(root)))
            previous = signal.signal(signal.SIGALRM, _interrupt)
            try:
                # When: the real baseline child records readiness before cancellation arrives.
                with patch("contention.measure_process", side_effect=measure_ready_child), \
                        self.assertRaises(KeyboardInterrupt):
                    run_contention(scenario, {"executable": str(binary)}, root / "evidence")
            finally:
                signal.signal(signal.SIGALRM, previous)
            # Then: the baseline and aborted schedule are durable; no writer starts.
            self.assertEqual((root / "events").read_text().splitlines(), ["check"])
            baseline = json.loads((root / "evidence/baseline.json").read_text())
            self.assertEqual(baseline["outcome"], "interrupted")
            self.assertTrue(json.loads((root / "evidence/receipt.json").read_text())["interrupted"])

    def test_active_writer_interruption_preserves_subcalls_and_reaps_writer(self):
        # Given: a completed baseline, concurrent reader and a long-lived writer.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary, scenario = _prepare_interruptible_cli(root)
            def interrupt_after_reader(scenario, build, evidence, options):
                record = actual_contention_call(scenario, build, evidence, options)
                if options["role"] == "reader":
                    signal.raise_signal(signal.SIGALRM)
                return record
            previous = signal.signal(signal.SIGALRM, _interrupt)
            try:
                # When: the real reader's durable receipt precedes cancellation of its writer.
                with patch("contention.measure_contention_call", side_effect=interrupt_after_reader), \
                        self.assertRaises(KeyboardInterrupt):
                    run_contention(scenario, {"executable": str(binary)}, root / "evidence")
            finally:
                signal.signal(signal.SIGALRM, previous)
            # Then: completed and cancelled subcalls survive; the writer is gone.
            evidence = root / "evidence"
            self.assertEqual(json.loads((evidence / "baseline.json").read_text())["outcome"], "success")
            self.assertEqual(json.loads((evidence / "reader.json").read_text())["outcome"], "success")
            self.assertEqual(json.loads((evidence / "writer.json").read_text())["outcome"], "interrupted")
            self.assertTrue(json.loads((evidence / "receipt.json").read_text())["interrupted"])
            with self.assertRaises(ProcessLookupError):
                os.kill(int((root / "writer.pid").read_text()), 0)

    def test_verification_rejects_reader_and_cleans_owned_writer(self):
        # Given: a writer holding the fixture lock and a rejected third launch.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / ".letopis").mkdir()
            binary = root / "cli"
            binary.write_text(f"#!{sys.executable}\n" + _VERIFIED_CLI)
            binary.chmod(0o755)
            scenario = {"repository": root, "argv": ["check"], "stdin": b"{}",
                        "expected": {"command": "check"}, "timeout": 5}
            def verify():
                with (root / "events").open("a") as stream:
                    stream.write("verify\n")
                if (root / "events").read_text().count("verify") == 3:
                    raise ValueError("binary drift")
            # When: verification refuses the concurrent reader before it launches.
            with self.assertRaisesRegex(ValueError, "binary drift"):
                run_contention(scenario, {"executable": str(binary)}, root / "evidence", verify=verify)
            # Then: each actual launch was preceded by verification; writer is reaped.
            events = (root / "events").read_text().splitlines()
            self.assertEqual(events, ["verify", "check", "verify", "reindex", "verify"])
            with self.assertRaises(ProcessLookupError):
                os.kill(int((root / "writer.pid").read_text()), 0)

    @unittest.skipUnless(os.environ.get("PERF_LETOPIS_BINARY"), "requires pinned release executable")
    def test_early_writer_exit_stops_reader_and_records_aborted_schedule(self):
        # Given: the owned drain is interrupted while a deliberately slow reader is active.
        binary = Path(os.environ["PERF_LETOPIS_BINARY"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run_canary(binary, root / "canary")
            repository = root / "canary/repository"
            with start_provider(root / "provider") as provider:
                configure_provider(repository, provider.url, True)
                with embed_lease(repository):
                    invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "", "limit": 1}})
                scenario = {"repository": repository, "argv": ["check"], "stdin": b"{}",
                            "expected": {"command": "check"}, "timeout": 5}
                def route(argv, cwd, data, options):
                    if argv[-1] == "check":
                        import sys
                        return actual_measure([sys.executable, "-c", "import time;time.sleep(60)"],
                                              cwd, b"", dict(options, timeout=5))
                    def interrupt_writer():
                        if provider.arrived.wait(timeout=3):
                            time.sleep(.15)
                            options["cancel_event"].set()
                    fault = threading.Thread(target=interrupt_writer, daemon=True)
                    fault.start()
                    return actual_measure(argv, cwd, data, options)
                # When: the supervisor observes early writer termination.
                started = time.monotonic()
                with patch("contention.measure_process", side_effect=route):
                    with self.assertRaises(ValueError):
                        run_drain_overlap(scenario, {"executable": str(binary)}, provider, root / "overlap")
                # Then: the five-second reader does not outlive writer ownership.
                self.assertLess(time.monotonic() - started, 3)
                import json
                receipt = json.loads((root / "overlap/receipt.json").read_text())
                self.assertTrue(receipt["early_writer_exit"])
                self.assertEqual(receipt["reader"]["outcome"], "interrupted")

    @unittest.skipUnless(os.environ.get("PERF_OBSERVER_BINARY") and os.environ.get("PERF_LETOPIS_BINARY"),
                         "requires pinned product and observer")
    def test_native_observer_checks_current_coverage_and_rejects_wrong_counts(self):
        # Given: real populated native vectors and an independent two-node count.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run_canary(Path(os.environ["PERF_LETOPIS_BINARY"]), root / "canary")
            scenario = {"repository": root / "canary/repository", "expected": {
                "counts": {"knowledge": 2, "source": 0, "edges": 0}}}
            observer = Path(os.environ["PERF_OBSERVER_BINARY"])
            # When: reading native vec1 without contacting the stopped fake server.
            receipt = observe_vectors(scenario, observer, root / "valid")
            # Then: full coverage passes, while an inconsistent independent count fails.
            self.assertEqual(receipt["covered_nodes"], 2)
            scenario["expected"]["counts"]["knowledge"] = 3
            with self.assertRaises(ValueError):
                observe_vectors(scenario, observer, root / "invalid")
    def test_rejects_partial_or_stale_vector_receipts(self):
        # Given: a complete native observation and deliberately inconsistent variants.
        receipt = {"covered_nodes": 2, "missing_nodes": 0, "knowledge_nodes": 2,
                   "eligible_hashes": 0, "dimensions": 3, "verdict": "populated",
                   "config_identity": "a" * 64, "invalid_files": 0}
        # When / Then: only full current coverage permits a vector claim.
        verify_coverage(receipt, 2)
        for change in ({"covered_nodes": 1}, {"missing_nodes": 1}, {"dimensions": None},
                       {"verdict": "not_assessed"}, {"config_identity": "invalid"}):
            with self.assertRaises(ValueError):
                verify_coverage(dict(receipt, **change), 2)

    @unittest.skipUnless(os.environ.get("PERF_LETOPIS_BINARY"), "requires pinned release executable")
    def test_real_cli_returns_vector_only_canary_and_finishes_owned_drain(self):
        # Given: a separately pinned release executable and an isolated two-node corpus.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve() / "canary"
            # When: populate through an owned drain and query a token absent from FTS.
            receipt = run_canary(Path(os.environ["PERF_LETOPIS_BINARY"]), root)
            # Then: beta is retrieved exclusively through the vector pathway.
            self.assertEqual(receipt["fts_ids"], [])
            self.assertEqual(receipt["vector_ids"], ["b.md"])
            self.assertEqual(receipt["drain"]["outcome"], "success")
            self.assertTrue(receipt["embed_lock_interoperable"])
            self.assertEqual(receipt["final_check"]["errors"], [])
            self.assertGreater(receipt["query_request_delta"], 0)

    @unittest.skipUnless(os.environ.get("PERF_LETOPIS_BINARY"), "requires pinned release executable")
    def test_real_owned_drain_overlaps_reader_and_cancels_after_reader(self):
        # Given: a tiny committed corpus and a new fake config requiring fresh vectors.
        binary = Path(os.environ["PERF_LETOPIS_BINARY"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run_canary(binary, root / "canary")
            repository = root / "canary/repository"
            for cancelled in (False, True):
                with start_provider(root / f"provider-{cancelled}") as provider:
                    configure_provider(repository, provider.url, True)
                    with embed_lease(repository):
                        invoke_cli(binary, repository, {"args": ["search"], "request": {"query": "", "limit": 1}})
                    scenario = {"repository": repository, "argv": ["search"],
                                "stdin": b'{"query":"alpha","limit":1}\n',
                                "expected": {"command": "search", "id": "a.md"}, "cancel_writer": cancelled}
                    # When: a reader finishes while the writer owns the inference barrier.
                    receipt = run_drain_overlap(scenario, {"executable": str(binary)}, provider,
                                                root / f"overlap-{cancelled}")
                    # Then: owned completion/cancellation releases the persistent lock.
                    self.assertTrue(receipt["reader_validation"]["valid"])
                    self.assertGreater(receipt["overlap_seconds"], 0)
                    self.assertTrue(receipt["owned_embed_lock_released"])
                    self.assertEqual(receipt["writer"]["outcome"], "interrupted" if cancelled else "success")

    @unittest.skipUnless(os.environ.get("PERF_LETOPIS_BINARY"), "requires pinned release executable")
    def test_real_reindex_reader_schedule_preserves_results(self):
        # Given: a tiny corpus whose provider is explicitly disabled before the schedule.
        binary = Path(os.environ["PERF_LETOPIS_BINARY"])
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            run_canary(binary, root / "canary")
            repository = root / "canary/repository"
            configure_provider(repository, "http://127.0.0.1:1/api", False)
            scenario = {"repository": repository, "argv": ["search"],
                        "stdin": b'{"query":"alpha","limit":1}\n',
                        "expected": {"command": "search", "id": "a.md"}}
            # When: directly owned reader and reindex processes overlap.
            receipt = run_contention(scenario, {"executable": str(binary)}, root / "reindex")
            # Then: both results validate and there is observed lifetime overlap.
            self.assertTrue(receipt["reader_validation"]["valid"])
            self.assertTrue(receipt["writer_validation"]["valid"])
            self.assertTrue(receipt["baseline_validation"]["valid"])
            self.assertTrue(receipt["writer_index_lock_observed"])
            self.assertGreater(receipt["overlap_seconds"], 0)


def _prepare_interruptible_cli(root, slow_baseline=False):
    (root / ".letopis").mkdir()
    binary = root / "cli"
    script = _VERIFIED_CLI
    if slow_baseline:
        script = script.replace("if sys.argv[1] == 'reindex':", "if sys.argv[1] == 'check':\n    time.sleep(60)\nif sys.argv[1] == 'reindex':")
    binary.write_text(f"#!{sys.executable}\n" + script)
    binary.chmod(0o755)
    return binary, {"repository": root, "argv": ["check"], "stdin": b"{}",
                    "expected": {"command": "check"}, "timeout": 5}


def _interrupt_after_baseline_ready(root):
    deadline = time.monotonic() + 5
    events = root / "events"
    while not events.exists() or events.read_text() != "check\n":
        if time.monotonic() >= deadline:
            raise AssertionError("baseline child did not record readiness")
        time.sleep(.005)
    signal.raise_signal(signal.SIGALRM)


def _interrupt(*_):
    raise KeyboardInterrupt


_VERIFIED_CLI = '''import fcntl, json, os, pathlib, sys, time
with open('events', 'a') as stream:
    stream.write(sys.argv[1] + '\\n')
if sys.argv[1] == 'reindex':
    pathlib.Path('writer.pid').write_text(str(os.getpid()))
    with open('.letopis/index.lock', 'a') as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        time.sleep(60)
print(json.dumps({'ok': True, 'result': {'errors': []}}))
'''


if __name__ == "__main__":
    unittest.main()
