"""Synthetic protocol records only: no agent/model execution or utility evidence."""
import hashlib
import json
from pathlib import Path


EMPTY_MEMORY_SHA256 = hashlib.sha256(b"[]\n").hexdigest()


def digest(text):
    return hashlib.sha256(text.encode()).hexdigest()


def make_dataset(root: Path, stage="early"):
    root.mkdir(parents=True, exist_ok=True)
    evidence = root / "evidence"
    evidence.mkdir(exist_ok=True)
    trace = evidence / "demonstration.txt"
    trace.write_text("SYNTHETIC DEMONSTRATION ONLY. No agent or provider ran.\n")

    def ref(locator):
        return {"path": "evidence/demonstration.txt", "sha256": digest(trace.read_text()),
                "locator": locator}

    study = {
        "protocol_version": 1, "kind": "demonstration", "stage": stage,
        "study_id": "demo-" + stage, "repository": "letopis",
        "token_accounting": "input_includes_cached_output_includes_reasoning",
        "agent": {"name": "synthetic-agent", "version": "fixture-only",
                  "model": "fake-agent", "model_revision": "fixture-only",
                  "config_sha256": "a" * 64, "auxiliary_models": ["fake-embedding"]},
        "scoring_guide": ref("operator rubric; never passed to the agent"),
        "conditions": {
            name: {"workspace_id": "workspace-" + name, "home_id": "home-" + name,
                   "memory_store_id": "memory-only" if name == "memory" else None,
                   "initial_memory": "empty", "external_memory_disabled": True,
                   "instruction": ref(name + " instruction"),
                   "isolation": ref(name + " isolation preflight")}
            for name in ["baseline", "memory"]},
        "chains": [],
    }
    count, length = (2, 2) if stage == "early" else (3, 3)
    sessions = []
    for chain_number in range(1, count + 1):
        chain_id = f"C{chain_number}"
        chain = {"id": chain_id, "base_commit": str(chain_number) * 40,
                 "starting_code_sha256": digest(chain_id + " base"),
                 "real_tasks": False, "selection_rationale": "Synthetic shape check, not task selection.",
                 "selection_evidence": ref(chain_id + " selection"), "tasks": []}
        for index in range(length):
            task_id = f"{chain_id}-T{index + 1}"
            chain["tasks"].append({
                "id": task_id, "prompt": ref(task_id + " prompt"),
                "acceptance": ref(task_id + " acceptance"),
                "check_ids": ["functional", "regression"],
                "agreement_ids": ["public_api"],
                "reuse_from": [] if index == 0 else [f"{chain_id}-T{index}"],
            })
        study["chains"].append(chain)
        for condition in ["baseline", "memory"]:
            code = chain["starting_code_sha256"]
            memory = {"items": 0, "sha256": EMPTY_MEMORY_SHA256}
            for index, task in enumerate(chain["tasks"]):
                ident = f"S-{condition}-{task['id']}"
                after_code = digest(ident + " code")
                after_memory = ({"items": index + 1, "sha256": digest(ident + " knowledge")}
                                if condition == "memory" else dict(memory))
                input_tokens, output_tokens = ((100, 20) if condition == "baseline"
                                               else ((110, 25) if index == 0 else (70, 20)))
                requests = [{"id": ident + ":agent", "model": "fake-agent", "purpose": "agent",
                             "input_tokens": input_tokens, "output_tokens": output_tokens,
                             "cached_input_tokens": 4, "reasoning_output_tokens": 2,
                             "evidence": ref(ident + " raw agent usage")}]
                if condition == "memory":
                    requests.append({"id": ident + ":embed", "model": "fake-embedding",
                                     "purpose": "embedding", "input_tokens": 3, "output_tokens": 0,
                                     "cached_input_tokens": 0, "reasoning_output_tokens": 0,
                                     "evidence": ref(ident + " raw embedding usage")})
                session = {
                    "study_id": study["study_id"], "chain_id": chain_id,
                    "task_id": task["id"], "condition": condition, "session_id": ident,
                    "agent_config_sha256": study["agent"]["config_sha256"],
                    "prompt_sha256": task["prompt"]["sha256"],
                    "code_before_sha256": code, "code_after_sha256": after_code,
                    "memory_before": dict(memory), "memory_after": after_memory,
                    "outcome": "correct", "reason": None, "trace": ref(ident + " full transcript"),
                    "checks": [{"id": name, "passed": True, "evidence": ref(ident + " " + name)}
                               for name in task["check_ids"]],
                    "request_inventory": {"ids": [r["id"] for r in requests],
                                          "evidence": ref(ident + " independent request inventory")},
                    "requests": requests,
                    "research": [{"id": ident + ":research", "fact_id": chain_id + ":fact",
                                  "kind": "first" if index == 0 else (
                                      "repeat" if condition == "baseline" else "revalidation"),
                                  "evidence": ref(ident + " annotated investigation")}],
                    "agreements": [{"id": "public_api", "violated": condition == "baseline"
                                    and chain_number == 1 and index == 1,
                                    "evidence": ref(ident + " annotated agreement")}],
                    "workload_profile": [] if condition == "baseline" else [
                        {"operation": "create", "uri": f"knowledge/{task['id']}.md#fact", "bytes": 80,
                         "timestamp_ms": 1000 + index * 100, "elapsed_ms": 2,
                         "group_id": ident + ":edit", "error": None,
                         "evidence": ref(ident + " observed file mutation")}],
                }
                if condition == "memory" and index:
                    session["workload_profile"].append({
                        "operation": "read", "uri": f"knowledge/{chain_id}-T1.md#fact", "bytes": 100,
                        "timestamp_ms": 1001 + index * 100, "elapsed_ms": 2,
                        "group_id": None, "error": None, "evidence": ref(ident + " observed read")})
                sessions.append(session)
                code, memory = after_code, dict(after_memory)
    return study, sessions


def write_dataset(root: Path, study, sessions):
    (root / "study.json").write_text(json.dumps(study, indent=2) + "\n")
    folder = root / "sessions"
    folder.mkdir(exist_ok=True)
    for session in sessions:
        name = session["condition"] + "-" + session["task_id"] + ".json"
        (folder / name).write_text(json.dumps(session, indent=2) + "\n")
