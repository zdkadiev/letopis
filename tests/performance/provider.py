"""Owned loopback-only deterministic embedding fixture, never a forwarding proxy."""

from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
from pathlib import Path
import threading
import time

from scenarios import verify_new_output, write_json


def start_provider(evidence: Path):
    """Return a resource-owning context; no model is loaded or contacted."""
    return FakeProvider(evidence)


class FakeProvider:
    """Own a bounded local HTTP fixture and its request/barrier accounting."""

    def __init__(self, evidence: Path):
        self.evidence = evidence
        self.lock = threading.Lock()
        self.arrived = threading.Event()
        self.release = threading.Event()
        self.release.set()
        self.requests = 0
        self.active = 0
        self.max_active = 0
        self.cpu_seconds = 0.0

    def __enter__(self):
        verify_new_output(self.evidence)
        self.evidence.mkdir(parents=True)
        self.server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = True
        self.server.owner = self
        self.url = f"http://127.0.0.1:{self.server.server_port}/api"
        self.thread = threading.Thread(target=self.server.serve_forever, daemon=True)
        self.thread.start()
        return self

    def __exit__(self, *_):
        self.release.set()
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(timeout=2)
        write_json(self.evidence / "provider.json", self.get_counters())

    def arm_barrier(self) -> None:
        """Pause only the next document request after drain acquires its lock."""
        self.arrived.clear()
        self.release.clear()

    def get_counters(self) -> dict:
        with self.lock:
            return {"requests": self.requests, "max_active": self.max_active,
                    "handler_cpu_seconds": self.cpu_seconds, "scope": "fake HTTP handler threads only",
                    "rss": {"unavailable": "fake runs in runner process; not separately attributable"}}


class Handler(BaseHTTPRequestHandler):
    """Implement the native Ollama fixture route and bounded request parsing."""

    def do_POST(self):
        owner = self.server.owner
        started = time.thread_time()
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if self.path != "/api/embed" or not 0 < length <= 1024 * 1024:
                raise ValueError("invalid request")
            self.connection.settimeout(5)
            request = json.loads(self.rfile.read(length))
            texts = request["input"]
            texts = [texts] if isinstance(texts, str) else texts
            if request["model"] != "performance-fixture" or not texts or not all(isinstance(t, str) for t in texts):
                raise ValueError("invalid fixture model/input")
            with owner.lock:
                owner.requests += 1
                owner.active += 1
                owner.max_active = max(owner.max_active, owner.active)
            if not owner.release.is_set() and not owner.arrived.is_set():
                owner.arrived.set()
                if not owner.release.wait(timeout=10):
                    raise ValueError("fixture barrier timed out")
            self.send_json(200, {"model": "performance-fixture", "embeddings": [build_vector(t) for t in texts]})
        except (ValueError, KeyError, TypeError, OSError):
            try:
                self.send_json(400, {"error": "invalid fixture request"})
            except OSError:
                pass
        finally:
            with owner.lock:
                owner.active = max(0, owner.active - 1)
                owner.cpu_seconds += time.thread_time() - started

    def send_json(self, status: int, value: dict):
        data = json.dumps(value).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_):
        # Fixture payloads/headers never enter HTTP diagnostic logs.
        return


def build_vector(text: str) -> list[float]:
    """Literal vectors distinguish a vector-only canary from an unrelated document."""
    if "vectorprobe" in text or "beta" in text:
        return [0., 1., 0.]
    if "alpha" in text:
        return [1., 0., 0.]
    return [0., 0., 1.]
