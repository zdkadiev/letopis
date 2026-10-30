"""Coverage claims and tiny real-CLI ownership checks."""

import os
from pathlib import Path
import tempfile
import unittest

from contention import (configure_provider, embed_lease, invoke_cli, observe_vectors, run_canary,
                        run_contention, run_drain_overlap, verify_coverage)
from provider import start_provider


class ContentionTests(unittest.TestCase):
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


if __name__ == "__main__":
    unittest.main()
