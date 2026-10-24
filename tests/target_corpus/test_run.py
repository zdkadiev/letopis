"""Deterministic runner orchestration; fake processes never count as model acceptance."""

import contextlib
import fcntl
import io
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
import sqlite3
from pathlib import Path
import sys
import tempfile
import threading
import unittest
from unittest.mock import patch

from generate import CorpusOptions, generate_corpus
import run as runner


class RunnerTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temporary = tempfile.TemporaryDirectory(prefix="target-runner-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name).resolve()
        generate_corpus(self.root / "corpus", CorpusOptions(15, 10))
        self.corpus = self.root / "corpus/corpus.json"
        self.repository = self.corpus.parent / "repository"
        self.binary = self.root / "product"
        self.observer = self.root / "observer"
        self.binary.write_text(_get_product_script(), encoding="utf-8")
        self.observer.write_text(_get_observer_script(), encoding="utf-8")
        self.binary.chmod(0o700)
        self.observer.chmod(0o700)
        self.model_patch = patch.object(runner, "inspect_model", return_value={
            "name": "embeddinggemma:300m-qat-q4_0", "digest": "a" * 64})
        self.model_patch.start()
        self.addCleanup(self.model_patch.stop)

    def test_load_pins_explicit_config_and_holds_barrier_through_observer(self) -> None:
        result = self._run("load", "load")
        self.assertEqual(result, 0)
        receipt = self._read("load/runner-receipt.json")
        self.assertEqual(receipt["status"], "backlog")
        self.assertEqual(receipt["observation"]["unique_search_hashes"], 10)
        self.assertEqual(receipt["observation"]["covered_nodes"], 0)
        config = json.loads((self.repository / ".letopis/config.json").read_text())
        self.assertEqual(config, {"provider": "ollama", "model": "embeddinggemma:300m-qat-q4_0",
                                 "parameters": {"base_url": "http://127.0.0.1:11434/api"},
                                 "max_retries": 2})
        self.assertEqual(self._read("load/refresh-process.json")["stdin_hex"],
                         b'{"query":"","limit":1}'.hex())
        self.assertEqual((self.repository / ".letopis/embed.lock").stat().st_mode & 0o777, 0o600)

    def test_evidence_collision_keeps_old_receipt_byte_identical(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        old = (self.root / "load/runner-receipt.json").read_bytes()
        self.assertEqual(self._run("load", "load"), 1)
        self.assertEqual((self.root / "load/runner-receipt.json").read_bytes(), old)

    def test_config_and_model_drift_rejected_before_product(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        config = self.repository / ".letopis/config.json"
        original = config.read_bytes()
        config.write_bytes(original + b" ")
        self.assertEqual(self._run("drain", "config-drift"), 1)
        self.assertFalse((self.root / "config-drift/drain-process.json").exists())
        config.write_bytes(original)
        with patch.object(runner, "inspect_model", return_value={"name": runner.MODEL,
                                                                "digest": "b" * 64}):
            self.assertEqual(self._run("drain", "model-drift"), 1)
        self.assertEqual(self._read("model-drift/runner-receipt.json")["verdict"], "not_assessed")
        self.assertFalse((self.repository / ".letopis/coverage").exists())

    def test_binary_and_input_drift_rejected_before_commands(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        self.binary.write_text(self.binary.read_text() + "\n# changed\n")
        self.assertEqual(self._run("drain", "binary-drift"), 1)
        path = self.repository / "knowledge/part-0000.md"
        path.write_bytes(path.read_bytes() + b"\n")
        self.assertEqual(self._run("observe", "input-drift"), 1)
        self.assertFalse((self.root / "input-drift/observer-process.json").exists())

    def test_timeout_observes_partial_coverage_and_resume_preserves_vectors(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "partial"}):
            self.assertEqual(self._run("drain", "partial", "--max-seconds", "0.1"), 1)
        partial = self._read("partial/runner-receipt.json")
        self.assertEqual(partial["observation"]["covered_nodes"], 3)
        self.assertEqual(self._read("partial/drain-process.json")["outcome"], "timeout")
        saved = (self.root / "partial/observation.json").read_bytes()
        self.assertEqual(self._run("drain", "complete"), 0)
        self.assertEqual(self._read("complete/runner-receipt.json")["verdict"], "populated")
        self.assertEqual((self.root / "partial/observation.json").read_bytes(), saved)
        self.assertEqual((self.repository / ".letopis/preserved").read_text(), "three-vectors")

    def test_nonzero_still_observes_and_sanitizes_process_output(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "failure"}):
            self.assertEqual(self._run("drain", "failure"), 1)
        self.assertEqual(self._read("failure/drain-process.json")["outcome"], "nonzero")
        self.assertEqual(self._read("failure/runner-receipt.json")["observation"]["missing_nodes"], 10)
        all_bytes = b"".join(path.read_bytes() for path in (self.root / "failure").iterdir())
        self.assertNotIn(b"private-provider-canary", all_bytes)

    def test_observe_never_drains_or_rewrites_config_and_is_incomplete(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        config = self.repository / ".letopis/config.json"
        before = config.stat()
        self.assertEqual(self._run("observe", "observe"), 1)
        self.assertEqual(config.stat().st_mtime_ns, before.st_mtime_ns)
        self.assertFalse((self.root / "observe/drain-process.json").exists())
        self.assertFalse((self.repository / ".letopis/coverage").exists())

    def test_interrupted_load_reuses_pin_and_config_without_reset(self) -> None:
        with patch.dict(os.environ, {"CONTROL": "load-failure"}):
            self.assertEqual(self._run("load", "failed-load"), 1)
        pin = (self.corpus.parent / "runner-pin.json").read_bytes()
        config = self.repository / ".letopis/config.json"
        before = config.stat().st_mtime_ns
        self.assertEqual(self._run("load", "retry-load"), 0)
        self.assertTrue((self.root / "retry-load/before-observation.json").exists())
        self.assertEqual(config.stat().st_mtime_ns, before)
        self.assertEqual((self.corpus.parent / "runner-pin.json").read_bytes(), pin)

    def test_missing_config_after_initial_pin_recovers_only_before_databases(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        config = self.repository / ".letopis/config.json"
        original = config.read_bytes()
        config.unlink()
        self.assertEqual(self._run("load", "deleted-config"), 1)
        for leaf in ("cache.db", "vectors.db"):
            (self.repository / ".letopis" / leaf).unlink()
        self.assertEqual(self._run("load", "pin-before-config"), 0)
        self.assertEqual(config.read_bytes(), original)

    def test_occupied_lock_and_symlink_lock_are_rejected_without_touching_owner(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        lock = self.repository / ".letopis/embed.lock"
        with lock.open("r+b") as stream:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
            self.assertEqual(self._run("drain", "busy"), 1)
            self.assertFalse((self.root / "busy/drain-process.json").exists())
        lock.unlink()
        other = self.root / "unrelated"
        other.write_text("owner")
        lock.symlink_to(other)
        self.assertEqual(self._run("load", "symlink"), 1)
        self.assertEqual(other.read_text(), "owner")

    def test_collapsed_hashes_and_false_populated_coverage_fail(self) -> None:
        with patch.dict(os.environ, {"CONTROL": "collapsed"}):
            self.assertEqual(self._run("load", "collapsed"), 1)
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "false-populated"}):
            self.assertEqual(self._run("observe", "false-populated"), 1)
        self.assertEqual(self._read("false-populated/runner-receipt.json")["verdict"], "not_assessed")

    def test_observer_failure_has_durable_not_assessed_receipt(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "observer-failure"}):
            self.assertEqual(self._run("observe", "observer-failure"), 1)
        result = self._read("observer-failure/runner-receipt.json")
        self.assertEqual(result["status"], "failed")
        self.assertIsNone(result["observation"])
        self.assertEqual(result["verdict"], "not_assessed")

    def test_environment_isolates_git_and_provider_credentials(self) -> None:
        with patch.dict(os.environ, {"GIT_DIR": "/bad", "OPENAI_API_KEY": "private-key",
                                    "LETOPIS_CORPUS_REQUEST": "/bad"}):
            self.assertEqual(self._run("load", "load"), 0)

    def test_invalid_cli_success_is_rejected_even_with_valid_observation(self) -> None:
        with patch.dict(os.environ, {"CONTROL": "bad-json"}):
            self.assertEqual(self._run("load", "bad-json"), 1)
        self.assertEqual(self._read("bad-json/refresh-process.json")["outcome"], "invalid_response")
        self.assertIsNotNone(self._read("bad-json/runner-receipt.json")["observation"])

    def test_exhaustion_is_incomplete_even_when_eligible_is_zero(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "exhausted"}):
            self.assertEqual(self._run("drain", "exhausted"), 1)
        receipt = self._read("exhausted/runner-receipt.json")
        self.assertEqual(receipt["observation"]["eligible_hashes"], 0)
        self.assertEqual(receipt["observation"]["exhausted_hashes"], 10)
        self.assertEqual(receipt["verdict"], "not_assessed")

    def test_missing_cached_identity_rejected_before_drain_can_reset_vectors(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "observer-failure"}):
            self.assertEqual(self._run("drain", "before-failure"), 1)
        self.assertFalse((self.root / "before-failure/drain-process.json").exists())
        self.assertFalse((self.repository / ".letopis/coverage").exists())

    def test_explicit_config_rejects_remote_provider_before_product(self) -> None:
        config = self.root / "unsafe-config.json"
        config.write_text(json.dumps({"provider": "openai", "model": runner.MODEL,
                                      "max_retries": 2, "parameters": {"base_url": "https://api.example"}}))
        self.assertEqual(self._run("load", "remote", "--config", str(config)), 1)
        self.assertFalse((self.root / "remote/refresh-process.json").exists())

    def test_watchdog_reaps_owned_child_and_records_its_identity(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        with patch.dict(os.environ, {"CONTROL": "partial"}):
            self.assertEqual(self._run("drain", "reaped", "--max-seconds", "0.1"), 1)
        record = self._read("reaped/drain-process.json")
        self.assertEqual(record["outcome"], "timeout")
        with self.assertRaises(ProcessLookupError):
            os.kill(record["child_pid"], 0)

    def test_lock_rejects_parent_replaced_with_symlink_while_owned(self) -> None:
        service = self.repository / ".letopis"
        service.mkdir(mode=0o700)
        moved = self.repository / "moved-service"
        try:
            with self.assertRaises(ValueError):
                with runner._own_lock(service / "embed.lock"):
                    service.rename(moved)
                    service.symlink_to(moved, target_is_directory=True)
        finally:
            service.unlink()
            moved.rename(service)

    def test_cli_structural_errors_and_duplicate_keys_are_rejected(self) -> None:
        for output in (b'[]\n', b'{"ok":true,"ok":true,"result":[]}\n',
                       b'{"ok":true,"result":[],"warnings":"bad"}\n'):
            with self.subTest(output=output), self.assertRaises(ValueError):
                runner._verify_response(output, "search", 0)
        with self.assertRaises(ValueError):
            runner._verify_response(b'{"ok":false,"error":{"code":"internal_error"}}\n', "search", 1)

    def test_resumed_load_rejects_stored_identity_before_refresh_preserving_state(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        old_receipt = (self.root / "load/runner-receipt.json").read_bytes()
        vectors = self.repository / ".letopis/vectors.db"
        with sqlite3.connect(vectors) as database:
            database.execute("UPDATE meta SET config_identity = ?", ("b" * 64,))
        (self.repository / ".letopis/coverage").write_text("3")
        (self.repository / ".letopis/preserved").write_text("three-vectors")
        before = vectors.read_bytes()
        self.assertEqual(self._run("load", "stale-load"), 1)
        self.assertFalse((self.root / "stale-load/refresh-process.json").exists())
        self.assertEqual(vectors.read_bytes(), before)
        self.assertEqual((self.repository / ".letopis/coverage").read_text(), "3")
        self.assertEqual((self.repository / ".letopis/preserved").read_text(), "three-vectors")
        self.assertEqual((self.root / "load/runner-receipt.json").read_bytes(), old_receipt)

    def test_first_initialization_resumes_missing_empty_and_unconfigured_vectors(self) -> None:
        self.assertEqual(self._run("load", "load"), 0)
        vectors = self.repository / ".letopis/vectors.db"
        vectors.unlink()
        self.assertEqual(self._run("load", "missing-vectors"), 0)
        vectors.write_bytes(b"")
        self.assertEqual(self._run("load", "empty-vectors"), 0)
        vectors.unlink()
        with sqlite3.connect(vectors) as database:
            database.execute("PRAGMA user_version=0")
        self.assertEqual(self._run("load", "empty-version-zero"), 0)
        with sqlite3.connect(vectors) as database:
            database.execute("DELETE FROM meta")
        self.assertEqual(self._run("load", "unconfigured-vectors"), 0)
        with sqlite3.connect(vectors) as database:
            database.execute("DELETE FROM meta")
            database.execute("INSERT INTO vector_keys VALUES(1, 'existing-key')")
        before = vectors.read_bytes()
        self.assertEqual(self._run("load", "unconfigured-with-keys"), 1)
        self.assertEqual(vectors.read_bytes(), before)
        self.assertFalse((self.root / "unconfigured-with-keys/refresh-process.json").exists())

    def _run(self, phase: str, evidence: str, *extra: str) -> int:
        arguments = ["--corpus", str(self.corpus), "--binary", str(self.binary),
                     "--observer", str(self.observer), "--evidence", str(self.root / evidence),
                     "--phase", phase, *extra]
        with contextlib.redirect_stdout(io.StringIO()), contextlib.redirect_stderr(io.StringIO()):
            return runner.main(arguments)

    def _read(self, path: str) -> dict:
        return json.loads((self.root / path).read_text())


class PathTests(unittest.TestCase):
    def test_evidence_guard_rejects_lexical_parent_traversal_without_writes(self) -> None:
        base = Path(tempfile.gettempdir()).resolve() / f"letopis-path-runner-{os.getpid()}"
        intermediate, destination = base / "absent", base / "evidence"
        self.assertFalse(base.exists())
        with self.assertRaisesRegex(ValueError, "parent traversal"):
            runner._verify_path(intermediate / ".." / "evidence", exists=False)
        self.assertFalse(intermediate.exists())
        self.assertFalse(destination.exists())
        self.assertFalse(base.exists())
        self.assertIsNone(runner._verify_path(base / "canonical", exists=False))


class LocalHttpTests(unittest.TestCase):
    def test_installed_model_inspection_never_pulls_or_uses_remote_model(self) -> None:
        with _serve_provider() as server:
            config = {"parameters": {"base_url": f"http://127.0.0.1:{server.server_port}/api"}}
            result = runner.inspect_model(config)
            self.assertEqual(result, {"name": "embeddinggemma:300m-qat-q4_0", "digest": "c" * 64})
            server.remote_model = True
            with self.assertRaises(ValueError):
                runner.inspect_model(config)
            self.assertEqual(server.paths, ["/api/tags", "/api/tags"])

    @unittest.skipUnless(os.environ.get("LETOPIS_TARGET_BINARY") and os.environ.get("LETOPIS_TARGET_OBSERVER"),
                         "set explicit compiled binary paths for actual CLI controlled-HTTP integration")
    def test_actual_cli_lock_barrier_then_timeout_and_resume(self) -> None:
        with tempfile.TemporaryDirectory(prefix="target-http-") as temporary, _serve_provider() as server:
            root = Path(temporary).resolve()
            generate_corpus(root / "corpus", CorpusOptions(15, 60))
            config = root / "config.json"
            config.write_text(json.dumps({"provider": "ollama", "model": runner.MODEL, "max_retries": 2,
                "parameters": {"base_url": f"http://127.0.0.1:{server.server_port}/api"}}))
            common = ["--corpus", str(root / "corpus/corpus.json"), "--binary",
                os.environ["LETOPIS_TARGET_BINARY"], "--observer", os.environ["LETOPIS_TARGET_OBSERVER"]]
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main([*common, "--phase", "load", "--evidence", str(root / "load"),
                                               "--config", str(config)]), 0)
                self.assertEqual(server.paths, ["/api/tags"])
                before = (root / "corpus/repository/.letopis/config.json").read_bytes()
                vectors = root / "corpus/repository/.letopis/vectors.db"
                with sqlite3.connect(vectors) as database:
                    database.execute("DELETE FROM meta")
                self.assertEqual(runner.main([*common, "--phase", "load", "--evidence", str(root / "first-init")]), 0)

                self.assertEqual(runner.main([*common, "--phase", "drain", "--evidence", str(root / "bounded"),
                                               "--max-seconds", "0.15"]), 1)
                self.assertEqual(runner.main([*common, "--phase", "drain", "--evidence", str(root / "complete"),
                                               "--max-seconds", "30"]), 0)
            receipt = json.loads((root / "complete/observation.json").read_text())
            self.assertEqual((receipt["knowledge_nodes"], receipt["source_nodes"], receipt["edges"]), (60, 60, 300))
            self.assertEqual((receipt["covered_nodes"], receipt["dimensions"], receipt["verdict"]), (60, 2, "populated"))
            self.assertEqual((root / "corpus/repository/.letopis/config.json").read_bytes(), before)
            self.assertIn("/api/embed", server.paths)
            vectors = root / "corpus/repository/.letopis/vectors.db"
            with sqlite3.connect(vectors) as database:
                database.execute("UPDATE meta SET config_identity = ?", ("d" * 64,))
            saved = vectors.read_bytes()
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main([*common, "--phase", "load", "--evidence", str(root / "stale")]), 1)
            self.assertFalse((root / "stale/refresh-process.json").exists())
            self.assertEqual(vectors.read_bytes(), saved)
            identity = json.loads((root / "complete/observation.json").read_text())["config_identity"]
            with sqlite3.connect(vectors) as database:
                database.execute("UPDATE meta SET config_identity = ?", (identity,))
            with contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(runner.main([*common, "--phase", "observe", "--evidence", str(root / "preserved")]), 0)
            self.assertEqual(json.loads((root / "preserved/observation.json").read_text())["covered_nodes"], 60)



@contextlib.contextmanager
def _serve_provider():
    server = ThreadingHTTPServer(("127.0.0.1", 0), _ProviderHandler)
    server.paths, server.remote_model = [], False
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    try:
        yield server
    finally:
        server.shutdown()
        server.server_close()
        thread.join()


class _ProviderHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        self.server.paths.append(self.path)
        model = {"name": "embeddinggemma:300m-qat-q4_0", "digest": "c" * 64}
        if self.server.remote_model:
            model["remote_host"] = "https://ollama.com"
        self._respond({"models": [model]})

    def do_POST(self) -> None:
        import time
        self.server.paths.append(self.path)
        data = json.loads(self.rfile.read(int(self.headers["Content-Length"])))
        time.sleep(0.04)
        self._respond({"model": "embeddinggemma:300m-qat-q4_0",
                       "embeddings": [[0.25, 0.75] for _ in data["input"]]})

    def _respond(self, data: dict) -> None:
        encoded = json.dumps(data).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(encoded)))
        self.end_headers()
        try:
            self.wfile.write(encoded)
        except (BrokenPipeError, ConnectionResetError):
            pass

    def log_message(self, format: str, *args: object) -> None:
        pass


def _get_product_script() -> str:
    return f'''#!{sys.executable}
import fcntl, hashlib, json, os, pathlib, sqlite3, sys, time
root = pathlib.Path('.letopis')
control = os.environ.get('CONTROL', '')
assert 'GIT_DIR' not in os.environ and 'OPENAI_API_KEY' not in os.environ
assert 'LETOPIS_CORPUS_REQUEST' not in os.environ
if sys.argv[1:] == ['search']:
    assert sys.stdin.buffer.read() == b'{{"query":"","limit":1}}'
    lock = (root / 'embed.lock').open('r+b')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        pass
    else:
        raise RuntimeError('missing backlog barrier')
    (root / 'cache.db').touch()
    config = json.loads((root / 'config.json').read_text())
    identity = hashlib.sha256(json.dumps(config, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
    with sqlite3.connect(root / 'vectors.db') as database:
        for definition in ('meta(id INTEGER PRIMARY KEY, config_identity TEXT)',
                           'vector_keys(row_id INTEGER PRIMARY KEY, search_hash TEXT)',
                           'embed_failures(search_hash TEXT, attempts INTEGER)', 'query_error(id INTEGER)'):
            database.execute('CREATE TABLE IF NOT EXISTS ' + definition)
        stored = database.execute('SELECT config_identity FROM meta WHERE id=1').fetchone()
        if stored != (identity,):
            for name in ('coverage', 'preserved'):
                (root / name).unlink(missing_ok=True)
            database.execute('DELETE FROM vector_keys')
            database.execute('INSERT OR REPLACE INTO meta VALUES(1, ?)', (identity,))
        database.execute('PRAGMA user_version=1')
    print('not-json' if control == 'bad-json' else (json.dumps(dict(ok=False, error=dict(code='internal_error',message='safe',details={{}}))) if control == 'load-failure' else json.dumps(dict(ok=True, result=[]))))
    sys.exit(1 if control == 'load-failure' else 0)
assert sys.argv[1:] == ['embed', '--drain'] and sys.stdin.buffer.read() == b''
lock = (root / 'embed.lock').open('r+b')
fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
if control == 'failure':
    print('private-provider-canary', file=sys.stderr)
    print(json.dumps(dict(ok=False, error=dict(code='embedding_failed',message='safe',details={{}}))))
    sys.exit(1)
if control == 'partial':
    (root / 'coverage').write_text('3')
    (root / 'preserved').write_text('three-vectors')
    time.sleep(30)
if (root / 'coverage').exists():
    assert (root / 'coverage').read_text() == '3'
    assert (root / 'preserved').read_text() == 'three-vectors'
if control != 'exhausted':
    (root / 'coverage').write_text('10')
print(json.dumps(dict(ok=True, result={{}})))
'''


def _get_observer_script() -> str:
    return f'''#!{sys.executable}
import fcntl, hashlib, json, os, pathlib, sqlite3, sys
assert sys.argv[1:] == ['--ignored', '--exact', 'observe_existing_corpus', '--nocapture']
request = json.loads(pathlib.Path(os.environ['LETOPIS_CORPUS_REQUEST']).read_text())
root = pathlib.Path(request['root']) / '.letopis'
assert request['expected_knowledge'] == 10 and request['expected_sources'] == 10
assert request['expected_edges'] == 50
if not (root / 'coverage').exists():
    lock = (root / 'embed.lock').open('r+b')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        pass
    else:
        raise RuntimeError('observer missing barrier')
control = os.environ.get('CONTROL', '')
if control == 'observer-failure':
    sys.exit(7)
config = json.loads((root / 'config.json').read_text())
identity = hashlib.sha256(json.dumps(config, sort_keys=True, separators=(',', ':')).encode()).hexdigest()
with sqlite3.connect(root / 'vectors.db') as database:
    assert database.execute('SELECT config_identity FROM meta WHERE id=1').fetchone() == (identity,)
covered = int((root / 'coverage').read_text()) if (root / 'coverage').exists() else 0
receipt = dict(schema_version=1, project_revision=1, config_identity=identity,
               dimensions=2 if covered else None, knowledge_nodes=10, source_nodes=10, edges=50,
               unique_search_hashes=9 if control == 'collapsed' else 10, covered_nodes=covered,
               missing_nodes=10-covered, eligible_hashes=0 if control == 'exhausted' else 10-covered,
               failed_hashes=10 if control == 'exhausted' else 0,
               exhausted_hashes=10 if control == 'exhausted' else 0, invalid_files=0,
               verdict='populated' if covered == 10 or control == 'false-populated' else 'not_assessed')
with pathlib.Path(request['output']).open('x') as stream:
    json.dump(receipt, stream)
'''


if __name__ == "__main__":
    unittest.main()
