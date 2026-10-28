"""TK-30 private preparation slots and the separately labelled P-1 demonstration.

Neither output is a P-1 observation: the templates are unexecuted preparation slots and
the demonstration is the existing synthetic P-1 fixture, unchanged. Real records are
supplied later and validated by the unchanged `tests/pilot_protocol/check.py`.
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

PILOT_PROTOCOL = Path(__file__).resolve().parent.parent / "pilot_protocol"
# Appended, not prepended, so P-1 modules never shadow this directory's own modules.
sys.path.append(str(PILOT_PROTOCOL))
import fixtures  # noqa: E402

SLOT_COUNT = 18
FINAL_STAGE = "final"
DEMONSTRATION_KIND = "demonstration"


def build_templates(manifest: dict, output: Path) -> dict:
    """Write one unexecuted preparation slot per manifest order entry into a new directory.

    Each slot is `kind=preparation`, `state=unready`, with null session, outcome, checks
    and request inventory and no requests; nothing is invented. Raises ValueError unless
    the order holds the 18 P-1 slots, and FileExistsError when output exists.
    """
    order = manifest.get("order")
    if not isinstance(order, list) or len(order) != SLOT_COUNT:
        raise ValueError(f"manifest order must hold {SLOT_COUNT} slots")
    output.mkdir(parents=True, exist_ok=False)
    for slot in order:
        template = {"kind": "preparation", "state": "unready", "slot": slot["slot"],
                    "chain_id": slot["chain"], "task_id": slot["task_id"],
                    "condition": slot["condition"], "position": slot["position"],
                    "session_id": None, "outcome": None, "checks": None,
                    "request_inventory": None, "requests": []}
        (output / f"{slot['slot']}.json").write_text(json.dumps(template, indent=2) + "\n",
                                                     encoding="utf-8")
    return {"kind": "preparation", "state": "unready", "slots": len(order),
            "output": str(output)}


def write_demo(output: Path) -> Path:
    """Write the unchanged P-1 final-stage demonstration dataset into a new directory.

    Returns output, holding `study.json`, `sessions/` and `evidence/` for the P-1 checker.
    Raises FileExistsError when output exists, and ValueError if the fixture ever stops
    being a final-stage demonstration, so it can never be presented as a pilot.
    """
    output.mkdir(parents=True, exist_ok=False)
    dataset, sessions = fixtures.make_dataset(output, stage=FINAL_STAGE)
    if (dataset["kind"], dataset["stage"]) != (DEMONSTRATION_KIND, FINAL_STAGE):
        raise ValueError("P-1 fixture is no longer a final demonstration")
    fixtures.write_dataset(output, dataset, sessions)
    return output
