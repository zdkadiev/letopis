"""Concurrent, model-free HTTP fixture for independent TK-30 drain observations."""
from __future__ import annotations

import json
import re
import threading
from collections import Counter
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import evaluation
from evaluation import read_json

VECTORS = {"alpha": [1.0, 0.0, 0.0], "beta": [0.0, 1.0, 0.0], "gamma": [0.0, 0.0, 1.0],
           "changed": [1.0, 1.0, 0.0], "delta": [0.0, 1.0, 1.0]}
QUERY_VECTOR = [0.5, 0.5, 0.5]
MALFORMED_MARKER = "delta"
MALFORMED_REPLY = b'{"model":"fixture","embeddings":"malformed"}'
MARKER = re.compile(r"tk30-doc-([a-z]+)")
HOLD_LIMIT_SECONDS = 60
RELEASE_WAIT_SECONDS = 1.0


class BarrierServer:
    """Concurrent loopback provider that holds document requests until released."""

    def __init__(self) -> None:
        self.condition = threading.Condition()
        self.requests: list[dict] = []
        self.active = 0
        self.max_active = 0
        self.max_same = 0
        self.open = False
        try:
            self.server = ThreadingHTTPServer(("127.0.0.1", 0), make_handler(self))
        except OSError as error:
            raise evaluation.InfraError(f"loopback fixture cannot bind: {error}") from error
        self.server.daemon_threads = True
        self.url = f"http://127.0.0.1:{self.server.server_address[1]}"
        threading.Thread(target=self.server.serve_forever, daemon=True).start()

    def __enter__(self) -> "BarrierServer":
        return self

    def __exit__(self, *_exc) -> None:
        self.release_all()
        self.server.shutdown()
        self.server.server_close()

    def handle_document(self, marker: str) -> tuple[bytes, dict]:
        """Count the request as in flight and block until released or the hold limit."""
        with self.condition:
            record = {"marker": marker, "released": self.open}
            self.requests.append(record)
            self.active += 1
            self.max_active = max(self.max_active, self.active)
            self.max_same = max(self.max_same, self.count_active(marker))
            self.condition.notify_all()
            self.condition.wait_for(lambda: record["released"], HOLD_LIMIT_SECONDS)
        if marker == MALFORMED_MARKER:
            return MALFORMED_REPLY, record
        reply = json.dumps({"model": "fixture", "embeddings": [VECTORS[marker]]})
        return reply.encode(), record

    def handle_query(self) -> bytes:
        """Query embeddings are answered at once; only document requests are held."""
        return json.dumps({"model": "fixture", "embeddings": [QUERY_VECTOR]}).encode()

    def finish_document(self, record: dict) -> None:
        with self.condition:
            self.active -= 1
            record["done"] = True
            self.condition.notify_all()

    def wait_for_held(self, count: int, timeout: float) -> bool:
        with self.condition:
            return self.condition.wait_for(lambda: self.active >= count, timeout)

    def release_reversed(self) -> None:
        """Answer the held requests newest first, then everything that follows."""
        with self.condition:
            held = [r for r in self.requests if not r["released"]]
        for record in reversed(held):
            with self.condition:
                record["released"] = True
                self.condition.notify_all()
                self.condition.wait_for(lambda: record.get("done"), RELEASE_WAIT_SECONDS)
        self.release_all()

    def release_all(self) -> None:
        with self.condition:
            self.open = True
            for record in self.requests:
                record["released"] = True
            self.condition.notify_all()

    def count(self, marker: str) -> int:
        return Counter(r["marker"] for r in self.requests)[marker]

    def count_active(self, marker: str) -> int:
        return sum(1 for r in self.requests if r["marker"] == marker and not r.get("done"))


def make_handler(owner: BarrierServer) -> type:
    class Handler(BaseHTTPRequestHandler):
        def do_POST(self) -> None:
            body = self.rfile.read(int(self.headers.get("Content-Length", "0")))
            text = " ".join(map(str, (read_json(body) or {}).get("input", [])))
            match = MARKER.search(text)
            record = None
            if match:
                reply, record = owner.handle_document(match[1])
            else:
                reply = owner.handle_query()
            try:
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(reply)))
                self.end_headers()
                self.wfile.write(reply)
            except OSError:
                pass
            finally:
                if record:
                    owner.finish_document(record)

        def log_message(self, *_args) -> None:
            pass

    return Handler
