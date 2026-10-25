"""Validate source address spelling independently of future CLI consumers."""

import json
from pathlib import Path
import unittest

from jsonschema import Draft202012Validator


class SourceContractTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        schema_path = (
            Path(__file__).resolve().parents[1]
            / "docs/zkit/letopis/contracts/schemas/X-1-cli.yaml"
        )
        schema = json.loads(schema_path.read_text())
        cls.validator = Draft202012Validator(schema["$defs"]["SourceId"])

    def test_accept_source_addresses(self):
        # Arrange
        for address in [
            "src/a.rs", "src/Я%2f.rs#L1-L20",
            "HTTP://Host/a?x=%2f#L01-L02", "https://host#arbitrary",
        ]:
            with self.subTest(address=address):
                # Act
                errors = list(self.validator.iter_errors(address))
                # Assert
                self.assertEqual(errors, [])

    def test_reject_malformed_addresses(self):
        # Arrange
        for address in [
            "/absolute", "a\n", "a\0", "https://", "http:///a",
            "https://?query", "HTTP:/host/a", "https:host", "http:bad://host", "https://host/a b",
            "https://host/\\a", "https://host/\x7f", "https://host/\u2003",
        ]:
            with self.subTest(address=address):
                # Act
                errors = list(self.validator.iter_errors(address))
                # Assert
                self.assertTrue(errors)


if __name__ == "__main__":
    unittest.main()
