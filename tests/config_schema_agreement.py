#!/usr/bin/env python3
"""Check the shared config corpus against X-1 and report runtime-only cases."""
from pathlib import Path
import json
import sys

import yaml
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]
SCHEMA_PATH = ROOT / "docs/zkit/letopis/contracts/schemas/X-1-cli.yaml"
CORPUS_PATH = ROOT / "tests/fixtures/config/corpus.json"


def main() -> int:
    schema = yaml.safe_load(SCHEMA_PATH.read_text())
    validator = Draft202012Validator(
        {"$ref": "#/$defs/Configuration", "$defs": schema["$defs"]}
    )
    corpus = json.loads(CORPUS_PATH.read_text())
    findings = []
    checked = 0
    for case in corpus["cases"]:
        if not case["schema_check"]:
            continue
        checked += 1
        try:
            value = json.loads(case["raw"])
        except json.JSONDecodeError:
            actual = False
        else:
            actual = validator.is_valid(value)
        if actual != case["schema_valid"]:
            findings.append(
                f"{case['name']}: schema_valid expected {case['schema_valid']}, got {actual}"
            )
    result = {
        "scope": "X-1 Configuration structure; duplicate keys and runtime numeric/provider checks remain Rust cases",
        "checked": checked,
        "runtime_only": sum(not case["schema_check"] for case in corpus["cases"]),
        "errors": findings,
    }
    print(json.dumps(result, ensure_ascii=False, indent=2))
    return bool(findings)


if __name__ == "__main__":
    sys.exit(main())
