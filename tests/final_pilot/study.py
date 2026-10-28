"""TK-30 preparation: selection validation, draft manifest and explicit readiness.

Model-free: nothing here starts an agent, reads credentials or contacts a provider.
"""
from __future__ import annotations

import hashlib
import json
import re
from pathlib import Path

CONDITIONS = ("baseline", "memory")
FULL_OID = re.compile(r"[0-9a-f]{40}")
CHAIN_COUNT = 3
TASKS_PER_CHAIN = 3
ISSUE_URL_PREFIX = "https://github.com/"
AGENT_NAME = "codex"
AGENT_VERSION = "0.164.0"
GATE_SECTION = "gates"
EVIDENCE_KEY = "evidence"
# Each pending item is cleared only by explicitly supplied values at these manifest paths.
REQUIRED_INPUTS = (
    ("seeds", (("seed_commit",), ("starting_inventory",))),
    ("product", (("product", "final_base"),)),
    ("memory_versions", (("product", "memory_binary"), ("product", "skill"))),
    ("embedding", (("product", "embedding_config"),)),
    ("assets", (("environment", "image"), ("environment", "pinned_assets"))),
    ("model", (("agent", "model"), ("agent", "model_revision"), ("agent", "budget"))),
    ("agent_config", (("agent", "config_sha256"),)),
    ("g2", (("gates", "g2"),)),
    ("prompt_freeze", (("gates", "prompt_freeze"),)),
)


class SelectionError(ValueError):
    """The prospective selection violates the frozen 3 x 3 structure."""


def load_selection(path: Path) -> dict:
    """Read and validate selection.json; raise SelectionError on any structural violation."""
    selection = json.loads(Path(path).read_text(encoding="utf-8"))
    chains = selection.get("chains")
    if not isinstance(chains, list) or len(chains) != CHAIN_COUNT:
        raise SelectionError(f"expected exactly {CHAIN_COUNT} chains")
    catalog = selection.get("agreement_catalog")
    if not isinstance(catalog, dict) or not catalog:
        raise SelectionError("agreement_catalog is required")
    seen: set[str] = set()
    for chain in chains:
        _validate_chain(chain, catalog, seen)
    return selection


def build_manifest(selection: dict, inputs: dict) -> dict:
    """Return the private draft manifest; every unknown input stays null and pending."""
    base = inputs.get("original_base")
    if not isinstance(base, str) or not FULL_OID.fullmatch(base):
        raise ValueError("original_base must be a full lowercase 40-hex object id")
    manifest = {
        "preparation_version": 1,
        "state": "unready",
        "model_executions": 0,
        "original_base": base,
        "seed_commit": None,
        "starting_inventory": None,
        "product": {"final_base": None, "memory_binary": None, "skill": None,
                    "embedding_config": None},
        "environment": {"local_tools_ok": bool(inputs.get("local_tools_ok", False)),
                        "image": None, "pinned_assets": None},
        "agent": {"name": AGENT_NAME, "version": AGENT_VERSION, "model": None,
                  "model_revision": None, "budget": None, "config_sha256": None},
        "gates": {"g2": None, "prompt_freeze": None, "base_recheck": None},
        "order": build_order(selection),
        "source_receipts": {"base_commit": base,
                            "selection_sha256": hash_selection(selection),
                            "files": dict(inputs.get("file_sha256", {}))},
    }
    manifest["pending"] = assess_readiness(manifest)["pending"]
    return manifest


def assess_readiness(manifest: dict, observed: dict | None = None) -> dict:
    """Derive readiness only from explicitly supplied inputs, never from absent errors.

    Local tool checks and a zero model_executions count say nothing about model access
    or gate verdicts, so they never clear a pending item. Empty, blank and bare boolean
    or numeric values count as missing; a gate is supplied only as an object holding a
    non-blank `evidence` reference, and judging that evidence stays a human step.

    `observed` is the selection hash and input file hashes recomputed by the caller from
    the copied inputs; when given, any mismatch with the stored receipts is drift.
    """
    pending = [name for name, paths in REQUIRED_INPUTS
               if not all(_is_supplied(path, _read_path(manifest, path)) for path in paths)]
    if _has_input_drift(manifest, observed):
        pending.append("input_drift")
    if not _is_recheck_current(manifest, observed):
        pending.append("base_drift_recheck")
    return {"state": "unready" if pending else "ready", "pending": pending}


def build_order(selection: dict) -> list[dict]:
    """Return the 18 slots: chains in order, each chain's condition order, tasks T1..T3."""
    slots = []
    for chain in selection["chains"]:
        for condition in chain["condition_order"]:
            for task in chain["tasks"]:
                slots.append({"slot": f"{task['id']}-{condition}", "chain": chain["id"],
                              "task_id": task["id"], "condition": condition,
                              "position": len(slots) + 1})
    return slots


def hash_selection(selection: dict) -> str:
    """SHA-256 of the canonical JSON of the selection, so any edit changes the freeze key."""
    canonical = json.dumps(selection, sort_keys=True, separators=(",", ":"),
                           ensure_ascii=False)
    return hashlib.sha256(canonical.encode("utf-8")).hexdigest()


def _has_input_drift(manifest: dict, observed: dict | None) -> bool:
    """True when the base is not a full object id or the inputs differ from the receipts."""
    receipts = manifest.get("source_receipts")
    base = manifest.get("original_base")
    if not isinstance(receipts, dict) or not _is_full_oid(base):
        return True
    if receipts.get("base_commit") != base:
        return True
    return observed is not None and (
        receipts.get("selection_sha256") != observed["selection_sha256"]
        or receipts.get("files") != observed["file_sha256"])


def _is_recheck_current(manifest: dict, observed: dict | None) -> bool:
    """A recheck counts only for the exact base and selection hash it verified."""
    receipts = manifest.get("source_receipts")
    if not isinstance(receipts, dict) or not _is_full_oid(manifest.get("original_base")):
        return False
    selection_sha256 = observed["selection_sha256"] if observed else receipts.get(
        "selection_sha256")
    expected = {"base_commit": manifest["original_base"], "selection_sha256": selection_sha256}
    return _read_path(manifest, ("gates", "base_recheck")) == expected


def _is_full_oid(value) -> bool:
    return isinstance(value, str) and FULL_OID.fullmatch(value) is not None


def _is_supplied(path: tuple[str, ...], value) -> bool:
    """Missing means None, blank text, an empty container, or a bare bool/number."""
    if path[0] == GATE_SECTION:
        return isinstance(value, dict) and _is_text(value.get(EVIDENCE_KEY))
    return _is_text(value) or (isinstance(value, (dict, list)) and len(value) > 0)


def _is_text(value) -> bool:
    return isinstance(value, str) and bool(value.strip())


def _read_path(manifest: dict, path: tuple[str, ...]):
    value = manifest
    for key in path:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value


def _validate_chain(chain: dict, catalog: dict, seen: set[str]) -> None:
    if sorted(chain.get("condition_order", [])) != sorted(CONDITIONS):
        raise SelectionError(f"{chain.get('id')}: condition_order must list each condition once")
    issue = chain.get("issue", {})
    if not isinstance(issue.get("number"), int) or not str(issue.get("url", "")).startswith(
            ISSUE_URL_PREFIX):
        raise SelectionError(f"{chain.get('id')}: issue number and GitHub url are required")
    tasks = chain.get("tasks", [])
    if len(tasks) != TASKS_PER_CHAIN:
        raise SelectionError(f"{chain.get('id')}: expected {TASKS_PER_CHAIN} tasks")
    earlier: list[str] = []
    for task in tasks:
        _validate_task(task, catalog, seen, earlier)
        earlier.append(task["id"])
    _validate_facts(chain, earlier)


def _validate_task(task: dict, catalog: dict, seen: set[str], earlier: list[str]) -> None:
    task_id = task.get("id")
    if not isinstance(task_id, str) or task_id in seen:
        raise SelectionError(f"task id missing or repeated: {task_id!r}")
    seen.add(task_id)
    if not str(task.get("prompt", "")).strip():
        raise SelectionError(f"{task_id}: prompt is required")
    for key in ("check_ids", "agreement_ids"):
        values = task.get(key)
        if not values or len(set(values)) != len(values):
            raise SelectionError(f"{task_id}: {key} must be a non-empty unique list")
    if not set(task["agreement_ids"]) <= set(catalog):
        raise SelectionError(f"{task_id}: unknown agreement id")
    reuse = task.get("reuse_from")
    if not isinstance(reuse, list) or not set(reuse) <= set(earlier):
        raise SelectionError(f"{task_id}: reuse_from may name only earlier tasks of its chain")


def _validate_facts(chain: dict, task_ids: list[str]) -> None:
    facts = chain.get("recurring_facts")
    if not facts:
        raise SelectionError(f"{chain['id']}: recurring_facts are required")
    for fact in facts:
        established = fact.get("established_in")
        recurs = fact.get("recurs_in")
        valid = (established in task_ids and str(fact.get("fact", "")).strip()
                 and recurs and set(recurs) <= set(task_ids[task_ids.index(established) + 1:]))
        if not valid:
            raise SelectionError(f"{chain['id']}: fact {fact.get('id')!r} is invalid")
