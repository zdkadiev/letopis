# Audit Corrections Implementation Plan

> **For agentic workers:** Use superpowers:executing-plans inline; verification and one fresh independent review follow all four corrections.

**Goal:** Close A-1…A-4 in the existing design without product implementation or new benchmarks.
**Architecture:** Keep the existing worker, Git snapshots, SQLite units and asynchronous embeddings. Clarify publication scope, project existing diagnostic state, and generalize the existing snapshot metadata.
**Tech Stack:** Markdown/JSON Schema and finite stdlib Python models; existing schema check dependencies.
**Spec:** [full audit](full-audit-2026-10-20.md) and the user's explicit request to fix all four findings.

## Global constraints
- YAGNI/KISS; no new services, tables, commands or benchmark runs.
- Preserve 45 task IDs, dependencies, weekly allocations and estimates; no implementation task becomes done.
- Preserve historical evidence and the original audit; save fresh results separately.

## Review focus
- An invalid external A and valid changed B must both reach the repaired projection; rollback only touches requested files.
- Failed and recovered retained diagnostics must be distinguishable; query errors remain visible.
- Edge hashes and coordinates must come from the same canonical snapshot, including mixed batches and missing IDs.
- Resource gates need actual current vector coverage and a defined payload profile, without a queue completion deadline.

### Task 1 — A-1 publication scope
- [x] Add an isolated external_delta mutation check; observe failure on the old model.
- [x] Extend model_commit.py with external repair/update/create/delete cases, including rollback boundaries.
- [x] Update mechanics, projection port and TK-2/TK-10/TK-25 integration criteria; verify the new model.

### Task 2 — A-2 stored diagnostic state
- [x] Add invalid missing/nonboolean details.active examples for stored status/errors; observe schema-check failure.
- [x] Require details.active only for stored diagnostics; derive it from existing errors.active, without a duplicate DB field.
- [x] Add model lifecycle/mutation cases; align schema, examples, read/diagnostic ports and TK-28.

### Task 3 — A-3 edge inspection
- [x] Add isolated edge_inspection mutation check; observe failure on the old model.
- [x] Use generic element_ids for capture and shared snapshot inspection metadata for node/edge hashes and locations.
- [x] Keep projection node/edge metadata separate; align mixed-batch examples and TK-24.

### Task 4 — A-4 resource acceptance
- [x] Define backlog and populated-vector fixture states, matching configuration/search hashes and observed A-2 bytes.
- [x] Update resource-math and TK-15/TK-16/TK-34/TK-35 acceptance; keep timing goals and schedule unchanged.
- [x] Record the authorized corrections as D-90; reconcile headers/links and downstream references.

### Closeout
- [x] Run all eight zkit stage checks, layout/linker, contract examples, internal models/mutations, resilience arithmetic and weekly check.
- [x] Review semantics independently, resolve findings, then install only changed files after checking the baseline hashes.
- [x] Rerun checks in the actual repository and save fresh evidence and closure report.

## Execution rulings
- User already authorized execution of all four corrections; no additional planning approval is needed.
- Inline staged-copy editing protects unrelated repository work; only changed project files will be installed.
- A-2 uses details.active rather than active-only status so completed query-embedding diagnostics remain visible as required by FR-32.

## Completion evidence
All four corrections passed the repository checks and independent semantic review. Original audit and archived evidence retained. See [closure](audit-fixes-2026-10-20.md) and [fresh checks](audit-fixes-checks-2026-10-20.json). No benchmarks/provider calls.
