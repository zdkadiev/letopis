"""Loopback fake behavior: never a proxy to a model service."""

import json
from pathlib import Path
import tempfile
import unittest
from urllib.error import HTTPError
from urllib.request import Request, urlopen

from provider import start_provider


class ProviderTests(unittest.TestCase):
    def test_returns_known_vectors_and_records_only_counters(self):
        # Given: an owned loopback server and two literal document inputs.
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            with start_provider(root / "server") as server:
                request = Request(server.url + "/embed", data=json.dumps({
                    "model": "performance-fixture", "input": ["document alpha", "document beta"]}).encode(),
                    headers={"Content-Type": "application/json"})
                # When: making a local fake-provider request.
                with urlopen(request, timeout=2) as response:
                    result = json.load(response)
                # Then: fixture vectors and ownership are explicit.
                self.assertEqual(result["embeddings"], [[1., 0., 0.], [0., 1., 0.]])
                self.assertTrue(server.url.startswith("http://127.0.0.1:"))
                self.assertEqual(server.get_counters()["requests"], 1)
            self.assertNotIn("document alpha", (root / "server/provider.json").read_text())

    def test_rejects_invalid_input_without_forwarding(self):
        # Given: a managed fake server.
        with tempfile.TemporaryDirectory() as directory:
            with start_provider(Path(directory).resolve() / "server") as server:
                # When / Then: malformed JSON and unknown routes fail locally.
                for suffix, body in (("/embed", b"invalid"), ("/unexpected", b"{}")):
                    with self.assertRaises(HTTPError):
                        urlopen(Request(server.url + suffix, data=body), timeout=2)


if __name__ == "__main__":
    unittest.main()
