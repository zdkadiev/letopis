"""Validate supplied P-1 records; never execute agents or attest OS isolation."""
import hashlib
import math
from pathlib import Path, PurePosixPath
import re

EMPTY_MEMORY_SHA256 = hashlib.sha256(b"[]\n").hexdigest()
CONDITIONS = ("baseline", "memory")


def require(condition, where, message):
    if not condition:
        raise ValueError(f"{where}: {message}")


def fields(value, names, where):
    require(isinstance(value, dict), where, "expected object")
    require(set(value) == set(names.split()), where,
            f"fields must be {names}; missing {sorted(set(names.split()) - set(value))}; "
            f"unexpected {sorted(set(value) - set(names.split()))}")


def text(value, where):
    require(isinstance(value, str) and bool(value.strip()), where, "expected nonempty string")


def identifier(value, where):
    require(isinstance(value, str) and re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.:-]*", value),
            where, "expected stable identifier")


def sha256(value, where):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{64}", value),
            where, "expected lowercase SHA256")


def integer(value, where, nullable=False):
    if nullable and value is None:
        return
    require(type(value) is int and value >= 0, where, "expected nonnegative integer")


def sequence(value, where):
    require(isinstance(value, list), where, "expected array")
    return value


def identifiers(value, where):
    for item in sequence(value, where):
        identifier(item, where)
    require(len(value) == len(set(value)), where, "duplicate identifier")
    return set(value)


def evidence(value, where, root):
    fields(value, "path sha256 locator", where)
    text(value["path"], where + ".path")
    text(value["locator"], where + ".locator")
    sha256(value["sha256"], where + ".sha256")
    path = PurePosixPath(value["path"])
    require(not path.is_absolute() and ".." not in path.parts and "\\" not in value["path"],
            where, "evidence path must be relative and confined")
    try:
        actual = (root / path).resolve(strict=True)
        require(actual.is_relative_to(root), where, "evidence escapes root")
        require(actual.is_file(), where, "evidence must be a regular file")
        with actual.open("rb") as stream:
            digest = hashlib.file_digest(stream, "sha256").hexdigest()
    except (OSError, RuntimeError) as error:
        raise ValueError(f"{where}: evidence unavailable: {error}") from error
    require(digest == value["sha256"], where + ".sha256", "evidence checksum mismatch")


def snapshot(value, where):
    fields(value, "items sha256", where)
    integer(value["items"], where + ".items")
    sha256(value["sha256"], where + ".sha256")
    require((value["items"] == 0) == (value["sha256"] == EMPTY_MEMORY_SHA256),
            where, "empty content inventory must use canonical empty hash")


def validate(study: dict, sessions: list[dict], evidence_root: Path) -> None:
    """Raise ValueError for invalid records; unknown measurements remain null."""
    root = Path(evidence_root).resolve()
    require(root.is_dir(), "evidence_root", "expected directory")
    fields(study, "protocol_version kind stage study_id repository token_accounting agent "
           "scoring_guide conditions chains", "study")
    require(type(study["protocol_version"]) is int and study["protocol_version"] == 1,
            "protocol_version", "only version 1 is supported")
    require(study["kind"] in ("demonstration", "pilot"), "kind", "expected demonstration or pilot")
    require(study["stage"] in ("early", "final"), "stage", "expected early or final")
    identifier(study["study_id"], "study_id")
    require(study["repository"] == "letopis", "repository", "P-1 uses Letopis (D-27)")
    require(study["token_accounting"] == "input_includes_cached_output_includes_reasoning",
            "token_accounting", "normalized totals must include cached/reasoning subsets")
    agent = study["agent"]
    fields(agent, "name version model model_revision config_sha256 auxiliary_models", "agent")
    for key in ("name", "version", "model", "model_revision"):
        text(agent[key], "agent." + key)
    sha256(agent["config_sha256"], "agent.config_sha256")
    for model in sequence(agent["auxiliary_models"], "agent.auxiliary_models"):
        text(model, "agent.auxiliary_models")
    auxiliaries = set(agent["auxiliary_models"])
    require(len(auxiliaries) == len(agent["auxiliary_models"]), "agent.auxiliary_models", "duplicate model")
    evidence(study["scoring_guide"], "scoring_guide", root)
    fields(study["conditions"], "baseline memory", "conditions")
    for name in CONDITIONS:
        condition = study["conditions"][name]
        where = "conditions." + name
        fields(condition, "workspace_id home_id memory_store_id initial_memory "
               "external_memory_disabled instruction isolation", where)
        for key in ("workspace_id", "home_id"):
            text(condition[key], where + "." + key)
        require(condition["initial_memory"] == "empty", where + ".initial_memory", "no prefill")
        require(condition["external_memory_disabled"] is True, where + ".external_memory_disabled",
                "other persistent memory must be disabled")
        if name == "baseline":
            require(condition["memory_store_id"] is None, where + ".memory_store_id", "baseline has no store")
        else:
            text(condition["memory_store_id"], where + ".memory_store_id")
        for key in ("instruction", "isolation"):
            evidence(condition[key], where + "." + key, root)
    for key in ("workspace_id", "home_id"):
        require(study["conditions"]["baseline"][key] != study["conditions"]["memory"][key],
                "conditions." + key, "independent conditions need distinct identities")

    count, length = (2, 2) if study["stage"] == "early" else (3, 3)
    chains = sequence(study["chains"], "chains")
    require(len(chains) == count, "chains", f"{study['stage']} requires {count} chains")
    all_tasks, all_chains, planned = set(), set(), {}
    for chain in chains:
        fields(chain, "id base_commit starting_code_sha256 real_tasks selection_rationale "
               "selection_evidence tasks", "chain")
        identifier(chain["id"], "chain.id")
        require(chain["id"] not in all_chains, "chain.id", "duplicate chain")
        all_chains.add(chain["id"])
        require(isinstance(chain["base_commit"], str)
                and re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", chain["base_commit"]),
                "chain.base_commit", "expected full Git object ID")
        sha256(chain["starting_code_sha256"], "chain.starting_code_sha256")
        require(type(chain["real_tasks"]) is bool, "chain.real_tasks", "expected boolean")
        require(study["kind"] != "pilot" or chain["real_tasks"], "chain.real_tasks",
                "a pilot requires prospective real tasks, not synthetic examples")
        text(chain["selection_rationale"], "chain.selection_rationale")
        evidence(chain["selection_evidence"], "chain.selection_evidence", root)
        require(len(sequence(chain["tasks"], "chain.tasks")) == length,
                "chain.tasks", f"{study['stage']} requires {length} tasks per chain")
        previous = set()
        for task in chain["tasks"]:
            fields(task, "id prompt acceptance check_ids agreement_ids reuse_from", "task")
            identifier(task["id"], "task.id")
            require(task["id"] not in all_tasks, "task.id", "duplicate task")
            all_tasks.add(task["id"])
            for key in ("prompt", "acceptance"):
                evidence(task[key], "task." + key, root)
            require(bool(identifiers(task["check_ids"], "task.check_ids")), "task.check_ids", "checks required")
            identifiers(task["agreement_ids"], "task.agreement_ids")
            reuse = identifiers(task["reuse_from"], "task.reuse_from")
            require(reuse.issubset(previous) and bool(reuse) == bool(previous),
                    "task.reuse_from", "later task must reuse preceding work in this chain")
            previous.add(task["id"])
            for condition in CONDITIONS:
                planned[condition, chain["id"], task["id"]] = task

    slots, session_ids, request_ids, episode_ids = {}, set(), set(), set()
    session_fields = ("study_id chain_id task_id condition session_id agent_config_sha256 "
                      "prompt_sha256 code_before_sha256 code_after_sha256 memory_before memory_after "
                      "outcome reason trace checks request_inventory requests research agreements workload_profile")
    for record in sequence(sessions, "sessions"):
        fields(record, session_fields, "session")
        require(record["condition"] in CONDITIONS, "session.condition", "unknown condition")
        text(record["chain_id"], "session.chain_id")
        text(record["task_id"], "session.task_id")
        slot = record["condition"], record["chain_id"], record["task_id"]
        require(slot in planned, "session.slot", f"unplanned slot {slot}")
        require(slot not in slots, "session.slot", f"duplicate slot {slot}")
        slots[slot] = record
    require(set(slots) == set(planned), "sessions", f"missing planned slot(s): {sorted(set(planned) - set(slots))}")

    for chain in chains:
        for condition in CONDITIONS:
            code, memory = chain["starting_code_sha256"], {"items": 0, "sha256": EMPTY_MEMORY_SHA256}
            prior_outcome, prior_facts = "correct", set()
            for task in chain["tasks"]:
                record = slots[condition, chain["id"], task["id"]]
                where = f"sessions.{condition}.{task['id']}"
                require(record["study_id"] == study["study_id"], where + ".study_id", "wrong study")
                require(record["agent_config_sha256"] == agent["config_sha256"],
                        where + ".agent_config_sha256", "agent configuration changed")
                require(record["prompt_sha256"] == task["prompt"]["sha256"],
                        where + ".prompt_sha256", "prompt changed")
                for key in ("code_before_sha256", "code_after_sha256"):
                    sha256(record[key], where + "." + key)
                require(record["code_before_sha256"] == code, where + ".code_before", "code trajectory changed")
                for key in ("memory_before", "memory_after"):
                    snapshot(record[key], where + "." + key)
                require(record["memory_before"] == memory, where + ".memory_before", "prefill or memory discontinuity")
                if condition == "baseline":
                    require(record["memory_after"] == memory, where + ".memory_after", "baseline retained memory")
                outcome = record["outcome"]
                require(outcome in ("correct", "failed", "incomplete", "not_run"), where + ".outcome", "unknown outcome")
                require(prior_outcome == "correct" or outcome == "not_run", where + ".predecessor",
                        "dependent followup must be explicitly not_run after unsuccessful predecessor")
                if outcome == "not_run":
                    require(record["session_id"] is None, where + ".session_id", "not_run has no session")
                    text(record["reason"], where + ".reason")
                    require(record["code_after_sha256"] == code and record["memory_after"] == memory,
                            where + ".outcome", "not_run cannot change state")
                else:
                    identifier(record["session_id"], where + ".session_id")
                    require(record["session_id"] not in session_ids, where + ".session_id", "fresh sessions required")
                    session_ids.add(record["session_id"])
                if record["reason"] is not None:
                    text(record["reason"], where + ".reason")
                evidence(record["trace"], where + ".trace", root)
                checks = sequence(record["checks"], where + ".checks")
                values, check_ids = [], []
                for check in checks:
                    fields(check, "id passed evidence", where + ".checks")
                    identifier(check["id"], where + ".checks.id")
                    check_ids.append(check["id"])
                    require(check["passed"] is None or type(check["passed"]) is bool,
                            where + ".checks.passed", "expected boolean or null")
                    values.append(check["passed"])
                    if check["passed"] is not None or check["evidence"] is not None:
                        evidence(check["evidence"], where + ".checks.evidence", root)
                require(len(check_ids) == len(set(check_ids)) and set(check_ids) == set(task["check_ids"]),
                        where + ".checks", "all common acceptance checks must be retained")
                derived = "failed" if False in values else ("incomplete" if None in values else "correct")
                require((outcome == "not_run" and all(x is None for x in values)) or outcome == derived,
                        where + ".outcome", "outcome contradicts acceptance checks")
                inventory = record["request_inventory"]
                fields(inventory, "ids evidence", where + ".request_inventory")
                inventory_ids = identifiers(inventory["ids"], where + ".request_inventory.ids")
                evidence(inventory["evidence"], where + ".request_inventory.evidence", root)
                local_requests = []
                for request in sequence(record["requests"], where + ".requests"):
                    label = where + ".requests"
                    fields(request, "id model purpose input_tokens output_tokens cached_input_tokens "
                           "reasoning_output_tokens evidence", label)
                    identifier(request["id"], label + ".id")
                    require(request["id"] not in request_ids, label, "duplicate request ID")
                    request_ids.add(request["id"])
                    local_requests.append(request["id"])
                    require(request["purpose"] in ("agent", "embedding", "rerank", "auxiliary"), label + ".purpose", "unknown purpose")
                    text(request["model"], label + ".model")
                    expected_models = {agent["model"]} if request["purpose"] == "agent" else auxiliaries
                    require(request["model"] in expected_models, label + ".model", "model outside frozen configuration")
                    for key in ("input_tokens", "output_tokens", "cached_input_tokens", "reasoning_output_tokens"):
                        integer(request[key], label + "." + key, nullable=True)
                    for subset, total in (("cached_input_tokens", "input_tokens"), ("reasoning_output_tokens", "output_tokens")):
                        if request[subset] is not None and request[total] is not None:
                            require(request[subset] <= request[total], label + "." + subset, "subset exceeds normalized total")
                    evidence(request["evidence"], label + ".evidence", root)
                require(set(local_requests) == inventory_ids, where + ".request_inventory", "request inventory coverage mismatch")
                if outcome == "correct":
                    require(any(r["purpose"] == "agent" for r in record["requests"]), where + ".requests", "correct task needs agent usage")
                facts = set()
                for event in sequence(record["research"], where + ".research"):
                    fields(event, "id fact_id kind evidence", where + ".research")
                    identifier(event["id"], where + ".research.id")
                    identifier(event["fact_id"], where + ".research.fact_id")
                    require(event["id"] not in episode_ids, where + ".research", "duplicate episode")
                    episode_ids.add(event["id"])
                    require(event["kind"] in ("first", "repeat", "revalidation", "unresolved"), where + ".research.kind", "unknown classification")
                    if event["kind"] != "unresolved":
                        require((event["kind"] == "first") == (event["fact_id"] not in prior_facts),
                                where + ".research", "classification contradicts prior-session fact inventory")
                    evidence(event["evidence"], where + ".research.evidence", root)
                    facts.add(event["fact_id"])
                agreements, agreement_ids = sequence(record["agreements"], where + ".agreements"), []
                for agreement in agreements:
                    fields(agreement, "id violated evidence", where + ".agreements")
                    identifier(agreement["id"], where + ".agreements.id")
                    agreement_ids.append(agreement["id"])
                    require(agreement["violated"] is None or type(agreement["violated"]) is bool,
                            where + ".agreements.violated", "expected boolean or null")
                    if agreement["violated"] is not None or agreement["evidence"] is not None:
                        evidence(agreement["evidence"], where + ".agreements.evidence", root)
                require(len(agreement_ids) == len(set(agreement_ids)) and set(agreement_ids) == set(task["agreement_ids"]),
                        where + ".agreements", "all planned agreement opportunities must be retained")
                profile = sequence(record["workload_profile"], where + ".workload_profile")
                require(condition == "memory" or not profile, where + ".workload_profile", "baseline cannot use the memory tool")
                for operation in profile:
                    label = where + ".workload_profile"
                    fields(operation, "operation uri bytes timestamp_ms elapsed_ms group_id error evidence", label)
                    require(operation["operation"] in ("search", "read", "create", "edit", "delete"), label + ".operation", "unknown operation")
                    text(operation["uri"], label + ".uri")
                    integer(operation["bytes"], label + ".bytes", nullable=True)
                    for key in ("timestamp_ms", "elapsed_ms"):
                        value = operation[key]
                        require(value is None or ((type(value) is int or
                                                   type(value) is float and math.isfinite(value)) and value >= 0),
                                label + "." + key, "expected nonnegative finite number or null")
                    for key in ("group_id", "error"):
                        if operation[key] is not None:
                            text(operation[key], label + "." + key)
                    evidence(operation["evidence"], label + ".evidence", root)
                if record["memory_after"] != memory:
                    require(any(o["operation"] in ("create", "edit", "delete") and o["error"] is None for o in profile),
                            where + ".workload_profile", "memory changed without a recorded successful mutation")
                if outcome == "not_run":
                    require(not local_requests and not record["research"] and not profile
                            and all(a["violated"] is None for a in agreements),
                            where + ".outcome", "not_run cannot contain executed work")
                prior_facts.update(facts)
                code, memory, prior_outcome = record["code_after_sha256"], record["memory_after"], outcome
