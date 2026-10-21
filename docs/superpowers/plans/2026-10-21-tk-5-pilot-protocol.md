# TK-5 Pilot Protocol Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Deliver an executable P-1 protocol, observation/report forms and a checked demonstration, without conducting a real utility pilot.

**Architecture:** A frozen study identifies paired independent conditions, prospective task chains, common correctness checks and evidence. Session records retain every planned task, annotation and model request. A read-only Python checker validates records/evidence and emits descriptive accounting with explicit completeness/correctness limits.

**Tech Stack:** Python 3.11+ standard library, JSON, Markdown; no provider requests or dependency installation.

**Spec:** `docs/zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-5-pilot-protocol.md`; `docs/zkit/letopis/requirements.md` NFR-8/FR-36; `docs/zkit/letopis/decisions.md` D-25/D-26/D-27/D-28/D-33; TK-17/TK-30…TK-33 cards.

## Global Constraints

- Same agent/model/config; same prospective real Letopis task prompts and correctness criteria; independent starting states; fresh session for every attempted task.
- No prefilled memory. Only the memory condition retains agent-written knowledge between sessions. Ordinary common repository documents remain readable in both conditions.
- Early pilot: two chains of two tasks. Final pilot: three chains of three tasks. Selecting/running these real tasks belongs to TK-17/TK-30…TK-33.
- Preserve correctness, measure repeated investigations, agreement violations and all model tokens. No predeclared improvement percentage or statistical significance claim.
- Synthetic demonstration records exercise the protocol only; no agent execution, model transport, resource measurement or benefit result is claimed.
- Use the shared Engram at `/Users/zdkadiev/Repos/letopis`. Do not import it into experimental conditions or initialize a second graph.
- User confirmed the existing design is the authority and requested continuation; implement inline with one fresh whole-change reviewer. Keep changes uncommitted until a separate commit/merge request, preserving the existing workflow.

## Review Focus

1. A missing/failed/not-run task must remain visible rather than improve metrics by exclusion.
2. Unknown request usage, hidden auxiliary calls and double-counted cached/reasoning tokens must not become a complete token total.
3. Reused sessions, prefilled/cross-chain memory, mismatched code states or foreign evidence paths must fail validation.
4. Semantic annotations and isolation attestations are operator evidence, not machine proof; the output must not overclaim security, causality or utility.
5. JSON edge cases, evidence corruption, wrong chain shape and inconsistent references must fail with actionable errors, without modifying inputs.

## Files and Interfaces

- `docs/zkit/letopis/pilot/protocol.md`: operational protocol, metric definitions, isolation/preflight, annotation rubric, interpretation, traceability and handoff.
- `docs/zkit/letopis/pilot/templates/{study,session}.json` and `report.md`: explicitly incomplete operator forms.
- `tests/pilot_protocol/validation.py`: `validate(study: dict, sessions: list[dict], evidence_root: Path) -> None`, raising `ValueError` with a field locator.
- `tests/pilot_protocol/check.py`: `summarize(study: dict, sessions: list[dict]) -> dict`; CLI `check.py STUDY SESSIONS --evidence-root ROOT` returns JSON, exit 0 for valid records and exit 2 for invalid records. Incomplete usage and failed outcomes remain reportable, with no eligible positive conclusion.
- `tests/pilot_protocol/fixtures.py`, `test_validation.py`, `test_check.py`: temporary, explicitly synthetic records and independent expected outcomes.
- `tests/pilot_protocol/examples/demonstration/`: self-contained synthetic study, sessions and hashed evidence.
- `tests/pilot_protocol/run.py`: reproducible unit/CLI/negative-control checks and input-hash receipt; `README.md` documents commands and limits.
- `docs/zkit/letopis/checks/TK-5/{report,review,manifest}.…`: evidence and whole-change review disposition. Update TK-5 card, EP-8 and roadmap only after checks pass; G-0 remains open until TK-6.

## Task 1: Protocol, forms and validated paired session records

**Consumes:** The existing spec and accepted sample sizes above.
**Produces:** `validate(study, sessions, evidence_root) -> None`; documented version-1 JSON forms and a deterministic synthetic fixture factory.

- [x] Write `test_validation.py` and a synthetic factory. A valid early/final dataset passes; invalid study shape, same environment/home, changed config/prompt, missing/duplicate slots or sessions, memory/code discontinuity, prefill, foreign/corrupt evidence and inconsistent correctness/annotation/request IDs raise field-specific `ValueError`.
- [x] Run `python3 -B -m unittest discover -s tests/pilot_protocol -p test_validation.py -v`; record the missing-module RED.
- [x] Write `protocol.md` and templates, then implement `validation.py`. Validate evidence as regular files confined to the evidence root with SHA256. Validate all planned slots, same-agent identity and independent per-chain trajectories; never execute a real agent.
- [x] Run the validation tests; compare each failure mode to the spec and record GREEN. Ledger author choices and operational boundaries.

## Task 2: Descriptive report, CLI, sensitivity and handoff

**Consumes:** Task 1's validated study/session interface and evidence files.
**Produces:** `summarize(study, sessions) -> dict`, standalone CLI, current example and repeatable check receipt.

- [x] Write `test_check.py` asserting literal demo totals: baseline 480 tokens; memory 462 including 12 embedding input tokens; cached/reasoning subsets are not added twice. Check unknown usage, failed/not-run tasks, agreement denominators, repeated research/revalidation, all-model accounting, workload export, CLI success/error codes and unchanged inputs.
- [x] Run `python3 -B -m unittest discover -s tests/pilot_protocol -p test_check.py -v`; record missing-implementation RED.
- [x] Implement `check.py` and operator README; output per-condition observed counts, request/model accounting, completeness and correctness limits. A demonstration always says `demonstration_only`; there is no automatic winner or benefit threshold.
- [x] Add a clearly labelled synthetic example and forms; verify it via the real CLI. Add `run.py` with source hashes and independently broken datasets (missing request inventory coverage, reused session, prefilled memory, corrupted evidence, suppressed failed task). They must be detected, not silently repaired.
- [x] Run `python3 -B tests/pilot_protocol/run.py --evidence NEW_DIRECTORY`; expect all unit tests, early/final fixtures, actual CLI and five negative controls PASS with zero real agent/model calls.
- [x] Run on macOS and the existing Linux Docker Python runtime, plus roadmap/model/document checks. Audit all source/evidence hashes, unchanged TK-1…TK-4 sources and local Markdown links.
## Final review and handoff

- [x] Dispatch one fresh whole-change reviewer using the most capable available model. Re-grade all findings and set-asides; one Important/Critical fix pass with RED→GREEN and full green suite, no second review. Record any deferred Minor plainly.
- [x] Update report/card/EP-8/roadmap, shared Engram and issue #5 with accurate evidence and limits. Keep branch/worktree uncommitted; do not merge/push/close the issue without a later user request.

## Self-review

The protocol covers both TK-5 acceptance bullets and the explicit TK-17/TK-30 sample sizes. Real task selection, agent/provider choice, live trials and final statistical interpretation remain later tasks. Task 2 uses Task 1's exact function/data interface; no untracked format fork or production CLI feature is planned.
