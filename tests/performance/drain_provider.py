"""Owned loopback-only concurrent native-protocol provider; never forwards requests."""

import hashlib
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time

from scenarios import verify_new_output

QUERY_TEXT = "OnlyVectorQuerySentinel"
MODEL = "drain-fixture"


class DrainProvider:
    def __init__(self, vectors: dict, evidence: Path, delay: float = .025):
        self.vectors = vectors
        self.evidence = evidence
        self.delay = delay
        self.condition = threading.Condition()
        self.live = self.active = self.document_active = 0
        self.tag = "setup"
        self.counters = {}
        self.sequence = 0

    def __enter__(self):
        verify_new_output(self.evidence)
        self.evidence.mkdir(parents=True, mode=0o700)
        self.events = (self.evidence / "request-events.jsonl").open("x")
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = False
        self.server.owner = self
        self.url = f"http://127.0.0.1:{self.server.server_port}/api"
        self.begin_window("setup", self.delay)
        self.worker = threading.Thread(target=self.server.serve_forever,
                                       kwargs={"poll_interval": .01})
        self.worker.start()
        return self

    def __exit__(self, *_):
        self.server.shutdown()
        self.server.server_close()
        self.worker.join(timeout=3)
        self.events.close()

    def begin_window(self, tag: str, delay: float) -> None:
        self.get_counters()
        with self.condition:
            self.tag, self.delay = tag, delay
            self.counters = {"document_requests": 0, "query_requests": 0, "failures": 0,
                             "max_document_active": 0, "max_total_active": 0,
                             "queries_during_document_work": 0, "handler_cpu_seconds": 0.}

    def get_counters(self) -> dict:
        with self.condition:
            if not self.condition.wait_for(lambda: self.live == 0, timeout=5):
                raise RuntimeError("controlled provider did not finish its owned handlers")
            return dict(self.counters, service_delay_seconds=self.delay,
                        concurrency_scope="parsed requests until response release; query/document separated",
                        rss={"unavailable": "fixture shares the Python runner process"})

    def enter_request(self, text: str) -> tuple:
        kind = "query" if text == QUERY_TEXT else "document"
        vector = [0., 1., 0.] if kind == "query" else self.vectors[text]
        with self.condition:
            self.live += 1
            self.active += 1
            if kind == "document":
                self.document_active += 1
            elif self.document_active:
                self.counters["queries_during_document_work"] += 1
            self.counters[kind + "_requests"] += 1
            self.counters["max_document_active"] = max(self.counters["max_document_active"], self.document_active)
            self.counters["max_total_active"] = max(self.counters["max_total_active"], self.active)
            self.sequence += 1
            ticket = (self.sequence, self.tag, kind)
            self.write_event(ticket, "arrived", input_sha256=hashlib.sha256(text.encode()).hexdigest())
            return ticket, list(vector), self.delay

    def release_response(self, ticket: tuple) -> None:
        with self.condition:
            self.active -= 1
            if ticket[2] == "document":
                self.document_active -= 1
            self.write_event(ticket, "response_ready")

    def finish_request(self, ticket: tuple | None, failed: bool, cpu: float) -> None:
        with self.condition:
            self.counters["failures"] += int(failed)
            self.counters["handler_cpu_seconds"] += cpu
            if ticket is not None:
                self.live -= 1
                self.write_event(ticket, "finished", failed=failed)
            self.condition.notify_all()

    def write_event(self, ticket: tuple, phase: str, **fields) -> None:
        self.events.write(json.dumps({"request": ticket[0], "window": ticket[1], "kind": ticket[2],
                                      "phase": phase, "monotonic": time.monotonic(), **fields}) + "\n")
        self.events.flush()


class Handler(BaseHTTPRequestHandler):
    def setup(self):
        self.request.settimeout(5)
        super().setup()

    def do_POST(self):
        owner, started = self.server.owner, time.thread_time()
        ticket, released, failed = None, False, False
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if self.path != "/api/embed" or not 0 < length <= 1024 * 1024:
                raise ValueError("invalid fixture request")
            request = json.loads(self.rfile.read(length))
            texts = request["input"]
            if request["model"] != MODEL or not isinstance(texts, list) or len(texts) != 1:
                raise ValueError("one fixture text required")
            ticket, vector, delay = owner.enter_request(texts[0])
            time.sleep(delay)
            payload = json.dumps({"model": MODEL, "embeddings": [vector]}).encode()
            owner.release_response(ticket)
            released = True
            self.send_payload(200, payload)
        except (ValueError, KeyError, TypeError, OSError):
            failed = True
            try:
                self.send_payload(400, b'{"error":"invalid controlled request"}')
            except OSError:
                pass
        finally:
            if ticket is not None and not released:
                owner.release_response(ticket)
            owner.finish_request(ticket, failed, time.thread_time() - started)

    def send_payload(self, status: int, data: bytes) -> None:
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        # Make the fixture's one-response sockets explicit to pooled clients.
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        return
