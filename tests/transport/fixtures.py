"""Real local X-2 endpoints; all payloads/keys are public synthetic canaries."""
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
import json
import os
import ssl
import subprocess
import threading

ROOT = Path(__file__).resolve().parent
BINARY = Path(os.environ.get('TK6_BINARY', ROOT / 'target/debug/transport_client'))
NATIVE = {'model': 'fixture-model', 'embeddings': [[1, 2, 3], [4, 5, 6]]}
COMPATIBLE = {'data': [{'index': 1, 'embedding': [4, 5, 6]}, {'index': 0, 'embedding': [1, 2, 3]}]}
KEY = 'PUBLIC_KEY_CANARY'
TEXTS = ['KNOWLEDGE_CANARY', 'βeta']


def child_env(extra=None):
    env = dict(os.environ)
    for name in list(env):
        if name.lower() in ('http_proxy', 'https_proxy', 'all_proxy', 'no_proxy') or name in ('OPENAI_API_KEY', 'RUST_LOG'):
            del env[name]
    env['OPENAI_API_KEY'] = KEY
    env.update(extra or {})
    return env


def payload(endpoint, provider='ollama'):
    return {'config': {'provider': provider, 'model': 'fixture-model', 'parameters': {'base_url': endpoint + '/api'}, 'max_retries': 2}, 'texts': TEXTS, 'expected_dimensions': 3}


def start(value, args=(), env=None):
    p = subprocess.Popen([str(BINARY), *args], stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=child_env(env))
    p.stdin.write(json.dumps(value))
    p.stdin.close()
    p.stdin = None
    return p


def invoke(value, args=(), env=None, timeout=8):
    p = start(value, args, env)
    try:
        out, err = p.communicate(timeout=timeout)
    except BaseException:
        p.kill()
        p.communicate()
        raise
    return p.returncode, json.loads(out), out, err


@contextmanager
def endpoint(*, mode='normal', status=200, body=None, location=None, tls=None):
    requests = []
    accepted, release = threading.Event(), threading.Event()
    data = json.dumps(NATIVE if body is None else body).encode() if not isinstance(body, bytes) else body

    class Handler(BaseHTTPRequestHandler):
        def log_message(self, *_):
            pass

        def do_POST(self):
            raw = self.rfile.read(int(self.headers.get('Content-Length', '0')))
            requests.append({'method': self.command, 'path': self.path, 'headers': {k.lower(): v for k, v in self.headers.items()}, 'body': json.loads(raw)})
            accepted.set()
            receipts = os.environ.get('TK6_REQUESTS')
            if receipts:
                with open(receipts, 'a') as f:
                    f.write(json.dumps(requests[-1], ensure_ascii=False) + '\n')
            try:
                if mode == 'reset':
                    self.connection.close()
                    return
                if mode == 'silent_headers':
                    release.wait(85)
                    return
                self.send_response(status)
                self.send_header('X-Provider-Canary', 'RESPONSE_HEADER_CANARY')
                self.send_header('Content-Length', str(len(data) + (100 if mode == 'truncated' else 0)))
                if location:
                    self.send_header('Location', location)
                self.end_headers()
                self.wfile.flush()
                if mode == 'silent_body':
                    release.wait(85)
                    return
                if mode == 'trickle':
                    for byte in data:
                        if release.wait(0.08):
                            return
                        self.wfile.write(bytes([byte]))
                        self.wfile.flush()
                else:
                    self.wfile.write(data)
            except (OSError, ssl.SSLError):
                pass

        def do_CONNECT(self):
            requests.append({'method': 'CONNECT', 'path': self.path, 'headers': dict(self.headers)})
            self.send_error(502)

        def do_GET(self):
            requests.append({'method': 'GET', 'path': self.path, 'headers': dict(self.headers)})
            self.send_response(200)
            self.send_header('Content-Length', str(len(data)))
            self.end_headers()
            self.wfile.write(data)

    server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
    server.daemon_threads = True
    if tls:
        context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
        context.load_cert_chain(ROOT / 'certs' / (tls + '.pem'), ROOT / 'certs' / (tls + '.key'))
        server.socket = context.wrap_socket(server.socket, server_side=True)
    thread = threading.Thread(target=lambda: server.serve_forever(poll_interval=0.01), daemon=True)
    thread.start()
    try:
        yield {'url': f'{"https" if tls else "http"}://127.0.0.1:{server.server_port}', 'requests': requests, 'accepted': accepted, 'release': release}
    finally:
        release.set()
        server.shutdown()
        server.server_close()
        thread.join(timeout=2)
