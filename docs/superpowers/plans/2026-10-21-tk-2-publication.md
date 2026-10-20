# TK-2 publication interruption checks implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox syntax for tracking. Proposed execution: native in this session, followed by one independent whole-change review.

**Goal:** Retain reproducible filesystem/SQLite process-interruption and concurrency checks for file publication, full reindex, and config reset, with exact before/after oracles on macOS ARM64 and Linux ARM64.

**Architecture:** A standalone Rust test crate in `tests/publication/` runs a small publisher in child processes against temporary Markdown fixtures and file-backed SQLite databases. The parent controls deterministic checkpoints over child stdin/stdout, kills a paused child, opens the database independently, and invokes a fresh publisher to prove convergence. This tests real OS locks and transactions; the existing Python model remains a design model.

**Tech Stack:** TK-1's Rust 1.95.0, rusqlite 0.40.2, bundled SQLite 3.53.2 and checksum-verified native vec1 0.8; Python 3 runner; the pinned TK-1 Linux Docker image.

**Spec:** [TK-2](../../zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-2-durability-harness.md), [MC-1/MC-2/MC-3](../../zkit/letopis/mechanics.md), [D-93/D-94/D-95](../../zkit/letopis/decisions.md), [physical design](../../zkit/letopis/mechanics/storage-schema.sql). User accepted following D-95 on 2026-10-21; TK-2 wording is corrected accordingly.

## Global constraints

- SQLite 3.51.3+, WAL, synchronous=FULL, foreign_keys=ON, fullfsync on macOS; use TK-1's checked connection helper and exact dependency/toolchain pins.
- One persistent `.letopis/index.lock` protected by kernel flock; never remove the lock file. Redetect after acquiring it. No worker, recovery log, corpus writes, or production CLI.
- One SQLite transaction includes nodes, declared edges, contentless-delete FTS, files fingerprints/errors, and revision. Invalid files keep their previous valid projection while other changes publish.
- D-95 stores all declared edges without an activity flag. Missing targets are hidden by node joins; cycles are diagnosed from data and traversal terminates using a visited set. No cycle rejection at publication.
- Full reindex holds index.lock throughout and commits once. Preserve prior valid projections for invalid existing files, as required by MC-1; a fresh cache cannot recover a prior projection it never held.
- Config reset uses one independent vectors.db BEGIN IMMEDIATE transaction, repeats the identity check inside it, and does not take index.lock. Recreate native vec1 and clear vector_keys/embed_failures with identity in the same transaction. Old readers/dimension compatibility and drain correctness belong to TK-3.
- Use fresh temporary corpus/database directories. Record source hashes and do not overwrite evidence. Preserve the copied uncommitted design baseline and the original checkout.
- No physical power-loss, device durability, performance, full Markdown parsing, provider, or production-storage claims. Full parser belongs to TK-7; production publication to TK-10.

## Review focus

- A child killed inside a transaction must release both kernel and SQLite locks without parent cleanup making the test pass.
- File fingerprints must never advance independently of projections/FTS/revision; unchanged-size same-tick edits must be discovered by hashing.
- A waiting publisher must not overwrite a newer file version using the delta it saw before taking the lock.
- Invalid files must retain their last valid projection and current error, and fixing/deleting them must remove the error. Edges of unchanged files must become visible when their target reappears.
- Reindex/config reset killed before commit must retain the complete previous state; killed after commit must retain the complete new state, including loss of the success response.

## File structure and interfaces

- Create `tests/publication/{Cargo.toml,Cargo.lock,rust-toolchain.toml,.gitignore}`: standalone crate using the pinned TK-1 helper by path, the same rusqlite, plus locked SHA-256/fixture serialization dependencies.
- Create `tests/publication/src/{lib.rs,corpus.rs,publication.rs,config.rs,bin/publisher.rs}`: narrow fixture decoder and detector, transactional publisher/reindex, vectors reset, and child-process/checkpoint entry point. No production API is introduced.
- Create `tests/publication/tests/{support/mod.rs,publication.rs,interruptions.rs,concurrency.rs}`: independent SQL/filesystem oracles, bounded child lifecycle/control, test matrix, and real competing processes.
- Create `tests/publication/{run.py,README.md}`: standalone reproducer, native extension compilation using TK-1's source verifier/pins, command logs and manifest. Keep TK-1's runner behavior unchanged.
- Create `docs/zkit/letopis/checks/TK-2/{report.md,review.md,...evidence}`; update only TK-2's acceptance/report/status and its roadmap/EP-8 status after verification.
- Child interface: `publisher ROOT MODE [CHECKPOINT]`, MODE = `publish|reindex|reset`; extension path from `LETOPIS_VEC1_EXTENSION`. A checkpoint emits one flushed `checkpoint:<name>` line and waits for one stdin line; normal completion emits `done`. A parent-side deadline bounds every wait, and child RAII cleanup kills/reaps survivors even on assertion failure.
- Library interface: `publish(root: &Path, reindex: bool, checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>) -> CheckResult<()>`; `reset_config(root: &Path, checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>) -> CheckResult<()>`. `CheckResult` comes from TK-1. Hooks exist only in this test harness.
- Fixtures use real `.md`/`.markdown` files containing a first-line JSON comment with one file element and explicit links, followed by searchable text; this deliberately bounded grammar is documented as a fixture format, not X-4 conformance. Corrupt JSON represents invalid input. Fingerprints use real size/mtime/inode plus SHA-256, with a conservatively documented filesystem timestamp tick and stored checked_at time.
- Main-cache oracle compares ordered nodes/edges/files/FTS search results and revision; vector oracle compares identity, keys, failures, native vec1 rows and sqlite_master definition. Expected states are explicitly built from fixture versions, not by invoking the publisher to compute an expectation. Corpus bytes are snapshotted immediately before each invocation and must remain unchanged afterward.

## Task 1: File publication and exact observable state

**Files:** Crate setup, corpus/publication modules, publisher entry point, publication tests and shared support.

**Interfaces:** Produces the `publish` function and child `publish|reindex` modes above; consumes TK-1's checked connection and native extension.

- [x] Write `valid_delta_publishes_beside_invalid_file`: start with two valid files, corrupt A and change B; assert old A projection, new B projection, current A error/fingerprint, correct FTS, and one revision increment. Observe failure against a compiling no-op implementation.
- [x] Implement only the fixture decoder, fingerprint detector, SQLite schema and one transactional publication needed to pass. Preserve row IDs on update; delete outgoing edges before node deletion; store declarations without target FK.
- [x] Add `fixed_and_deleted_files_clear_errors`, `unchanged_invocation_is_a_noop`, `new_deleted_and_renamed_files_update_fts`, `same_tick_same_size_edit_is_hashed`, and `declared_edges_follow_target_lifecycle_and_cycles_terminate`. Assert exact rows/diagnostics/search results and corpus bytes; watch new behavior fail before adding it.
- [x] Implement persistent flock, re-detection under lock, racy hashing and query-time diagnostics/traversal. Test real mtime equality using fixed file timestamps; do not depend on a lucky fast edit.
- [x] Run the entire new crate suite to GREEN; save RED logs and the before/after oracle description.

## Task 2: Real interruptions, reindex and config reset

**Files:** Config module, child checkpoints, interruption tests and support extensions.

**Interfaces:** Adds `reset_config`/child `reset` and checkpoint protocol; Task 1's publisher remains the implementation under test.

- [x] Write `publication_crash_matrix` with cuts after initial detection, after lock acquisition, after locked redetection, after projection writes, after FTS writes, after fingerprint writes, immediately before COMMIT and immediately after COMMIT before response. Kill using the parent's real child-process kill operation, confirm abnormal exit, independently compare exact committed state, then run a new publisher and assert convergence and unchanged corpus. Observe failure before adding the checkpoint/control implementation.
- [x] Implement deterministic checkpoints and bounded parent waits; ensure a process is actually paused at the reported point before it is killed. The parent must not perform recovery/rollback/unlock itself.
- [x] Write `reindex_crash_matrix`: baseline valid A/B, then changed/new/deleted/invalid files; cuts during clearing/rebuilding and before/after COMMIT. Old reader sees the old committed snapshot; reopening after kill sees complete old/new state and next full reindex converges. Implement full reindex in one transaction holding index.lock.
- [x] Write `config_reset_crash_matrix`: seed identity, keys, native vec1 rows and failures; replace fixture config; interrupt after BEGIN, after clearing/recreating vec1, after identity write and before/after COMMIT. Assert complete old/new vector state, unchanged main-cache snapshot and corpus; repeat reset is a no-op. Observe RED before implementing independent transactional reset and its in-transaction identity check.
- [x] Run the full crate suite to GREEN. Do not claim a deterministic pause exercises the CPU instructions inside SQLite COMMIT; process-kill coverage is before/after and inside the application transaction, not torn device writes.

## Task 3: Competing processes and reproducible platform evidence

**Files:** Concurrency tests, runner, README, TK-2 report/evidence and status updates.

**Interfaces:** Consumes Tasks 1–2 child protocol; produces `python3 -B tests/publication/run.py --evidence NEW_DIRECTORY` and equivalent Linux Docker command.

- [x] Write `waiting_publisher_redetects_after_agent_edit`: pause P1 holding index.lock, start P2 with the same discovered delta, publish a newer fixture version between P2 detection and lock acquisition, release P1, then let P2 proceed. Assert final state is the new version, no redundant revision for already-published deltas, and corpus unchanged by publishers. Exercise different release/edit orderings with handshakes, not sleeps.
- [x] Write `killed_lock_owner_does_not_block_next_process`, `reindex_serializes_with_publish`, and `competing_config_resets_recheck_identity`. Assert the next process completes within a bounded deadline, exact final rows, and reset does not erase a sentinel vector inserted after the first reset commits.
- [x] Observe RED for missing serialization/redetection/reset identity guards; implement minimal guards and rerun the whole suite.
- [x] Implement runner using TK-1's verified source inputs and the current crate's resolved SQLite headers; record OS/arch, Rust/SQLite/vec1 versions, all source/lockfile hashes, exact commands/statuses and full logs. Refuse existing evidence destinations. Build native libraries in unique temporary directories and keep OS-specific Cargo targets.
- [x] Run a controlled redetection-guard mutation and a fingerprint-outside-transaction mutation; each corresponding test must fail. Restore source byte-for-byte and rerun GREEN.
- [x] Run all new tests, formatting and Clippy with warnings denied on actual macOS ARM64 and Linux ARM64 Docker processes. Run TK-1's full runner as a regression check plus the roadmap checker. Verify final manifests match the tested files.
- [x] Request one independent whole-change review; fix material findings and rerun affected checks. Then fill the interruption matrix, commands/expected/actual results and limits in the report, update TK-2 acceptance/status and roadmap/EP-8, and verify no original-checkout/baseline changes were lost. Leave the branch uncommitted for review unless the user asks for a commit/merge.

## Plan self-review and baseline

All TK-2 section 3/5 cases map to Tasks 1–3. Reindex/reset isolation is covered without taking over TK-3's old-reader/dimension/drain scenarios. D-95 resolves the obsolete cycle-rejection criterion; no additional product behavior is selected here. The five review-focus cases each have a named test above. Interfaces and checkpoints are shared by the harness and tests; no production scaffolding is added.

Branch `TK-2`, worktree `/Users/zdkadiev/.codex/worktrees/4cf8/letopis`, base `ea86eada6d45b5fee81b2d74ac5ed659f4d82723`. Copied documentation baseline: 288 files, hashes in `/private/tmp/letopis-TK-2-baseline.json`. Initial checks passed: roadmap 45 tasks/329 hours/33 weeks, no dependency or gate errors; TK-1 Python runner tests 4/4; publication design model has no normal violations and detects its broken guards. Full native TK-1 rerun belongs to execution verification, not yet claimed here.

## Execution record

2026-10-21: native execution approved in chat. Tasks 1–2 and Task 3 implementation/verification pass. Both final platforms: 15 TK-2 tests, 25 SIGKILL cuts, full TK-1 runner (10 Rust + 4 Python), fmt, Clippy and roadmap checker. Four mutations detected and restored; final source hashes verified. Independent reviewer `/root/review_tk2` found one Important timing-dependent fixture; reproduced RED and fixed with explicit stored racy-clock preconditions, full suite GREEN, then both complete platform runs refreshed. No deferred findings. TK-2 acceptance/status updated; G-0 remains open. See [execution report](../../zkit/letopis/checks/TK-2/report.md).

Execution decisions: full reindex retains invalid prior projections (MC-1); config identity covers the bounded string/integer fixture subset; Linux uses read-only Cargo source cache after a recorded network failure and an image with pinned rustfmt/clippy preinstalled. See report for rationale and unproven boundaries. The branch/worktree and task changes remain uncommitted as approved.

Commit packaging (subsequent user request): snapshot the tested local physical schema as `tests/publication/schema.sql` and compile that fixture, so the isolated TK-2 commit is reproducible without publishing unrelated design edits. Refresh both platform manifests and verify a clean staged tree before commit. TK-2 scope/status is published with the original committed schedule metadata; broader estimate/schedule revisions stay uncommitted.
