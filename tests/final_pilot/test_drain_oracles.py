"""TK-30 test drain oracles; model-free. Shared fixtures are private."""
import json
import os
import shutil
import socket
import threading
import unittest
from unittest import mock
import copies
from oracles import drain, http_fixture
from _oracle_fixtures import (
    TempDirTestCase,
)


BUFFER_SOURCE = "fn drain_ready() {\n    let window = 2;\n}\n"


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


class HoldTimeoutTests(unittest.TestCase):
    def test_a_hold_that_times_out_is_finished_and_no_longer_active(self):
        # Given
        with mock.patch.object(http_fixture, "HOLD_LIMIT_SECONDS", 0.2):
            with drain.BarrierServer() as server:
                # When
                reply = post_document(server, "alpha")

        # Then
        self.assertEqual(reply["embeddings"], [http_fixture.VECTORS["alpha"]])
        self.assertEqual((server.active, [r.get("done") for r in server.requests]), (0, [True]))


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

    def make_bench(self, status):
        binary = self.tmp / "letopis"
        binary.write_text("#!/bin/sh\ncat >/dev/null\n"
                          f"if [ \"$1\" = status ]; then {status}; fi\n"
                          "echo '{\"ok\":true,\"result\":{}}'\n")
        binary.chmod(0o755)
        (self.tmp / "home").mkdir(exist_ok=True)
        return drain.Bench(self.tmp, binary, binary, self.evidence)


class ChangeGradeTests(unittest.TestCase):
    OLD = {"alpha": "a" * 64, "beta": "b" * 64}

    def test_removed_document_rejects_its_stale_vector(self):
        # Given: alpha remains valid, beta was removed while its request was held.
        removed = {"a" * 64: {"vector": [1.0, 0.0, 0.0]}, "b" * 64: {"vector": None}}
        stale = {**removed, "b" * 64: {"vector": [0.0, 1.0, 0.0]}}

        # When / Then
        self.assertTrue(drain.grade_change("removed", self.OLD, removed, {}))
        self.assertFalse(drain.grade_change("removed", self.OLD, stale, {}))

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
