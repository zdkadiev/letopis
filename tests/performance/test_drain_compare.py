"""Independent literal oracles and owned-process cleanup for the drain campaign."""

import argparse
import copy
import json
import os
import signal
from pathlib import Path
import sys
import tempfile
import time
import unittest
from unittest.mock import patch, MagicMock

from measure import measure_process as actual_measure
from drain_compare import prepare_campaign, run_drain, summarize, validate_sample, write_config, run_campaign, DrainInterrupted


class DrainComparisonTests(unittest.TestCase):
    def test_campaign_arms_watchdog_before_observer_setup(self):
        # Given: valid pins and an observer build cancelled during bounded setup.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            pin = root / "pin.json"
            pin.write_text('{"rustc":"fixture","inputs":{}}')
            args = argparse.Namespace(before=pin, one_worker=pin, after=pin, evidence=root / "campaign")
            deadlines = []
            def stop_setup(*_):
                deadlines.append(signal.getitimer(signal.ITIMER_REAL)[0])
                raise KeyboardInterrupt
            with patch("drain_compare.verify_frozen_build"), \
                    patch("drain_compare.read_product_inputs", return_value={}), \
                    patch("drain_compare.build_observer", side_effect=stop_setup):
                # When: setup observes the live campaign deadline and is interrupted.
                result = run_campaign(args)
            # Then: setup cancellation retains an incomplete summary and disarms the timer.
            self.assertGreater(deadlines[0], 0)
            self.assertEqual(result["outcome"], "incomplete")
            self.assertTrue((args.evidence / "summary.json").is_file())
            self.assertEqual(signal.getitimer(signal.ITIMER_REAL)[0], 0)

    def test_campaign_stops_after_persisting_an_interrupted_case(self):
        # Given: one cancelled owned case and three already verified frozen build receipts.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            pin = root / "pin.json"
            pin.write_text('{"rustc":"fixture","inputs":{},"executable":"/fixture"}')
            args = argparse.Namespace(before=pin, one_worker=pin, after=pin, evidence=root / "campaign")
            cancelled = {"outcome": "interrupted", "validation": {"valid": False}}
            provider = MagicMock()
            with patch("drain_compare.verify_frozen_build"), \
                    patch("drain_compare.read_product_inputs", return_value={}), \
                    patch("drain_compare.build_observer", return_value={}), \
                    patch("drain_compare.capture_harness", return_value={}), \
                    patch("drain_compare.DrainProvider", return_value=provider), \
                    patch("drain_compare.FixtureProcesses"), \
                    patch("drain_compare.build_summary", side_effect=lambda rows, outcome: {"outcome": outcome}), \
                    patch("drain_compare.measure_case", side_effect=DrainInterrupted(cancelled)) as measure:
                # When: the first measurement propagates its retained interruption receipt.
                result = run_campaign(args)
            # Then: no subsequent case starts and the partial receipt survives cancellation.
            self.assertEqual(measure.call_count, 1)
            self.assertEqual(result["outcome"], "interrupted")
            rows = (args.evidence / "samples.jsonl").read_text().splitlines()
            self.assertEqual(len(rows), 1)
            self.assertEqual(json.loads(rows[0])["outcome"], "interrupted")

    def test_switches_owned_fixture_configuration(self):
        # Given: a private fixture with disabled inference during initialization.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / ".letopis").mkdir()
            write_config(root, "http://127.0.0.1:1/api", False)
            # When: the measured phase enables the same private configuration.
            write_config(root, "http://127.0.0.1:1/api", True)
            # Then: only the fixture switch changes; the same endpoint is retained.
            config = json.loads((root / ".letopis/config.json").read_text())["embedding_model"]
            self.assertIs(config["enabled"], True)
            self.assertEqual(config["parameters"]["base_url"], "http://127.0.0.1:1/api")

    def test_refuses_existing_evidence(self):
        # Given: an existing artifact with bytes that must remain untouched.
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory).resolve() / "receipt"
            output.mkdir()
            (output / "sentinel").write_text("keep")
            # When / Then: the campaign refuses output reuse before writing anything.
            with self.assertRaises(ValueError):
                prepare_campaign(output)
            self.assertEqual((output / "sentinel").read_text(), "keep")

    def test_rejects_wrong_literal_vector_or_request_count(self):
        # Given: four independently specified vectors and a valid process receipt.
        record, observed, expected, provider = build_literal_case()
        # When / Then: either wrong stored bytes or an extra HTTP attempt is rejected.
        wrong = copy.deepcopy(observed)
        wrong["vectors"]["b.md"] = [9., 9., 9.]
        with self.assertRaises(ValueError):
            validate_sample(record, wrong, expected, provider)
        provider["document_requests"] = 5
        with self.assertRaises(ValueError):
            validate_sample(record, observed, expected, provider)

    def test_keeps_failed_samples_in_denominators(self):
        # Given: one completed sample and one failed sample with verified partial progress.
        records = [
            {"duration_seconds": 2., "outcome": "success", "validation": {"valid": True},
             "completed_vectors": 4, "resources": {}},
            {"duration_seconds": 3., "outcome": "timeout", "validation": {"valid": False},
             "completed_vectors": 1, "resources": {}},
        ]
        # When: the comparison summarizes occupied drain time.
        result = summarize(records)
        # Then: failure time stays in the denominator and is not a successful latency.
        self.assertEqual(result["attempts"], 2)
        self.assertEqual(result["failures"], 1)
        self.assertEqual(result["seconds_total"], 5.)
        self.assertEqual(result["vectors_per_second"], 1.)
        self.assertEqual(result["median_seconds"], 2.)
        self.assertIsNone(result["p95_seconds"])

    def test_distinguishes_document_and_query_concurrency(self):
        # Given: four document calls overlap one independently launched query call.
        record, observed, expected, provider = build_literal_case()
        expected["query_requests"] = 1
        provider.update(query_requests=1, max_total_active=5)
        # When / Then: the document cap does not incorrectly reject the extra query.
        validate_sample(record, observed, expected, provider)
        provider["max_document_active"] = 5
        with self.assertRaises(ValueError):
            validate_sample(record, observed, expected, provider)

    def test_controlled_provider_really_overlaps_requests(self):
        # Given: a loopback native-protocol fixture with literal vectors and fixed service delay.
        from concurrent.futures import ThreadPoolExecutor
        from urllib.request import Request, urlopen
        from drain_provider import DrainProvider, QUERY_TEXT
        vectors = {"alpha": [1., 0., 0.], "beta": [0., 1., 0.],
                   "gamma": [0., 0., 1.], "delta": [1., 1., 0.]}
        with tempfile.TemporaryDirectory() as directory:
            with DrainProvider(vectors, Path(directory).resolve() / "provider", delay=.1) as provider:
                provider.begin_window("test", .1)
                def post(text):
                    payload = json.dumps({"model": "drain-fixture", "input": [text]}).encode()
                    request = Request(provider.url + "/embed", data=payload,
                                      headers={"Content-Type": "application/json"})
                    with urlopen(request, timeout=3) as response:
                        self.assertEqual(response.headers.get("Connection"), "close")
                        return json.load(response)["embeddings"][0]
                # When: four document requests and one query use actual concurrent sockets.
                with ThreadPoolExecutor(max_workers=5) as pool:
                    replies = list(pool.map(post, [*vectors, QUERY_TEXT]))
                observed = provider.get_counters()
                # Then: payloads are literal, the fixture is concurrent, and query accounting is separate.
                self.assertEqual(replies, [*vectors.values(), [0., 1., 0.]])
                self.assertEqual(observed["document_requests"], 4)
                self.assertEqual(observed["query_requests"], 1)
                self.assertGreater(observed["max_document_active"], 1)
                self.assertEqual(observed["failures"], 0)

    def test_cleans_owned_drain_after_interruption(self):
        # Given: a real child records its PID before blocking indefinitely.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            binary = root / "fixture-cli"
            marker = root / "ready.pid"
            binary.write_text(f"#!{sys.executable}\nimport os,pathlib,time\n"
                              f"pathlib.Path({str(marker)!r}).write_text(str(os.getpid()))\ntime.sleep(60)\n")
            binary.chmod(0o755)
            def measure_ready(argv, repository, data, options):
                def cancel(pid):
                    deadline = time.monotonic() + 3
                    while not marker.exists():
                        if time.monotonic() > deadline:
                            raise AssertionError("child never became ready")
                        time.sleep(.005)
                    self.assertEqual(int(marker.read_text()), pid)
                    raise KeyboardInterrupt
                return actual_measure(argv, repository, data, dict(options, on_spawn=cancel))
            # When: cancellation occurs only after the owned child is observably running.
            with patch("drain_compare.measure_process", side_effect=measure_ready), \
                    self.assertRaises(KeyboardInterrupt):
                run_drain(binary, root, root / "attempt")
            # Then: interrupted evidence exists and that exact child has been reaped.
            record = json.loads((root / "attempt/process.json").read_text())
            self.assertEqual(record["outcome"], "interrupted")
            with self.assertRaises(ProcessLookupError):
                os.kill(int(marker.read_text()), 0)


def build_literal_case():
    vectors = {"a.md": [1., 0., 0.], "b.md": [0., 1., 0.],
               "c.md": [0., 0., 1.], "d.md": [1., 1., 0.]}
    record = {"outcome": "success", "exit_code": 0, "cli_ok": True}
    observed = {"vectors": copy.deepcopy(vectors), "unique_vectors": 4,
                "missing": [], "failed_hashes": 0, "query_error": None}
    expected = {"vectors": vectors, "unique_vectors": 4, "document_requests": 4,
                "query_requests": 0, "concurrency_limit": 4}
    provider = {"document_requests": 4, "query_requests": 0, "max_document_active": 4,
                "max_total_active": 4, "failures": 0}
    return record, observed, expected, provider


if __name__ == "__main__":
    unittest.main()
