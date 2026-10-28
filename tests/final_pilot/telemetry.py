"""TK-30 normalization of supplied Codex OTLP events into P-1 request rows.

Model-free: it reads collector files already written by the operator and never contacts
a provider. The OTLP flattening is adapted from the pinned TK-17 `evidence.read_otel`
(see TK17_SOURCES). Its count-only `collect_usage` is deliberately not carried over: equal
numbers of requests and completions say nothing about which completion belongs to which
request, so pairing needs an explicit correlation identity on both events.
"""
from __future__ import annotations

import json
from pathlib import Path

TK17_SOURCES = {
    "tests/early_pilot/evidence.py":
        "2eb639cbae82783f992d9a6a3081cf45659ba443850561b4aa98d605a8c54d0f",
}
REQUEST_EVENTS = ("codex.api_request", "codex.websocket_request")
RESPONSES_ENDPOINT_SUFFIX = "/responses"
COMPLETION_KIND = "response.completed"
CONTEXT_KEYS = ("slot", "model", "purpose")
# The context names the correlation attribute explicitly. Codex 0.164.0 is not known to
# emit a per-request identity, so the name must be confirmed against a real Codex 0.164.0
# collector capture; without it nothing pairs and every request's usage stays unknown.
CORRELATION_KEY = "correlation_attribute"
# P-1 field -> raw Codex counter. Cached and reasoning are subsets of input and output.
COUNTERS = {"input_tokens": "input_token_count", "output_tokens": "output_token_count",
            "cached_input_tokens": "cached_token_count",
            "reasoning_output_tokens": "reasoning_token_count"}


def normalize_events(events: list[dict], context: dict) -> dict:
    """Return `{correlation_attribute, request_inventory, requests, complete, unresolved}`.

    The inventory lists request events only. A completion fills a request row only when
    both carry the same value of the context's `correlation_attribute`; when the context
    names none, nothing pairs and `correlation_attribute_unknown` is reported. Identical duplicate events count once, a
    conflicting duplicate leaves the row unknown and is reported, and a completion without
    its request is kept in `unresolved`, never merged into another row. Unknown counters
    stay None. No request events is reported as `no_request_events`, never as zero usage.
    `complete` is true only when every row has known input and output tokens and nothing
    is unresolved, so no request events is never complete.

    Raises ValueError unless context fixes non-blank `slot`, `model` and `purpose`.
    """
    _require_context(context)
    attribute = _read_attribute(context)
    requests, uncorrelated = _group((event for event in events if _is_request(event)),
                                    attribute)
    completions, orphans = _group((event for event in events
                                   if event.get("event.kind") == COMPLETION_KIND), attribute)
    conflicts = [request_id for request_id, group in requests.items()
                 if len(group) > 1 or len(completions.get(request_id, [])) > 1]
    rows = [_build_row(context, name, [] if name in conflicts else completions.get(name, []))
            for name in requests]
    reserved = set(requests) | set(completions)
    number = 0
    for _event in uncorrelated:
        number += 1
        while f"uncorrelated-{number}" in reserved:
            number += 1
        rows.append(_build_row(context, f"uncorrelated-{number}", []))
    unresolved = [
        *({"reason": "conflicting_duplicate", "id": name, "usage": None} for name in conflicts),
        *({"reason": "completion_without_correlation", "id": None, "usage": read_usage(event)}
          for event in orphans),
        *({"reason": "completion_without_request", "id": name,
           "usage": read_usage(group[0]) if len(group) == 1 else None}
          for name, group in completions.items() if name not in requests),
        *_list_gaps(attribute, rows)]
    # An empty row list always carries the `no_request_events` gap, so it is never complete.
    complete = not unresolved and all(
        row["input_tokens"] is not None and row["output_tokens"] is not None for row in rows)
    return {"correlation_attribute": attribute,
            "request_inventory": {"ids": [row["id"] for row in rows]}, "requests": rows,
            "complete": complete, "unresolved": unresolved}


def read_collector(path: Path) -> list[dict]:
    """Flatten the collector's OTLP/HTTP JSON lines into attribute dicts.

    OTLP JSON encodes 64-bit integers as strings, so `intValue` is converted to int.
    A missing file raises FileNotFoundError: absent telemetry is not an empty capture.
    """
    events = []
    for line in path.read_text(encoding="utf-8").splitlines():
        if not line.strip():
            continue
        for resource in json.loads(line)["payload"].get("resourceLogs", []):
            for scope in resource.get("scopeLogs", []):
                events += [_read_attributes(log) for log in scope.get("logRecords", [])]
    return events


def _require_context(context: dict) -> None:
    for key in CONTEXT_KEYS:
        value = context.get(key)
        if not isinstance(value, str) or not value.strip():
            raise ValueError(f"telemetry context must fix a non-blank {key}")


def _read_attribute(context: dict) -> str | None:
    value = context.get(CORRELATION_KEY)
    return value if isinstance(value, str) and value.strip() else None


def _list_gaps(attribute: str | None, rows: list[dict]) -> list[dict]:
    """Report why usage cannot be complete beyond per-event problems."""
    gaps = []
    if attribute is None:
        gaps.append({"reason": "correlation_attribute_unknown", "id": None, "usage": None})
    if not rows:
        gaps.append({"reason": "no_request_events", "id": None, "usage": None})
    return gaps


def _is_request(event: dict) -> bool:
    name = event.get("event.name")
    if name == "codex.api_request":
        return str(event.get("endpoint", "")).endswith(RESPONSES_ENDPOINT_SUFFIX)
    return name in REQUEST_EVENTS


def _group(events, attribute: str | None) -> tuple[dict[str, list[dict]], list[dict]]:
    """Group events by correlation identity, dropping byte-identical collector repeats.

    Returns `(distinct events per identity, events without an identity)`; more than one
    distinct event under one identity is a conflict for the caller to report.
    """
    grouped: dict[str, list[dict]] = {}
    missing = []
    for event in events:
        identity = event.get(attribute) if attribute is not None else None
        if not isinstance(identity, str) or not identity.strip():
            missing.append(event)
            continue
        group = grouped.setdefault(identity, [])
        if event not in group:
            group.append(event)
    return grouped, missing


def _build_row(context: dict, request_id: str, completions: list[dict]) -> dict:
    """Build one P-1 request row; usage is known only from exactly one paired completion."""
    usage = read_usage(completions[0]) if len(completions) == 1 else dict.fromkeys(COUNTERS)
    return {"id": f"{context['slot']}:{request_id}", "model": context["model"],
            "purpose": context["purpose"], **usage}


def read_usage(event: dict) -> dict:
    """Map raw counters to P-1 fields; a missing or non-integer counter stays None."""
    return {public: _read_count(event.get(raw)) for public, raw in COUNTERS.items()}


def _read_attributes(log: dict) -> dict:
    record = {}
    for field in log.get("attributes", []):
        kind, value = next(iter(field["value"].items()), (None, None))
        record[field["key"]] = int(value) if kind == "intValue" else value
    return record


def _read_count(value) -> int | None:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        return None
    return value
