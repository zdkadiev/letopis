# TK-27 Embedding Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Guard configuration resets against obsolete observations and prove durable bounded embedding attempts, vector reuse and untruncated FTS after provider rejection.

**Architecture:** Keep orchestration in EmbeddingService and atomic conditional updates in SQLite through the application-owned Storage trait. Reuse existing readiness, pinned reads, process fixtures and provider implementations; no new infrastructure subsystem.

**Tech Stack:** Rust 1.95.0; synchronous ureq; rusqlite/SQLite WAL and vec1; SeaQuery; serde/JCS; existing Unix process fixtures.

**Spec:** [Approved TK-27 design](../specs/2026-10-26-tk-27-embedding-lifecycle-design.md). Worktree `/private/tmp/letopis-tk-27`, branch `tk-27`, base main `6f59bbb`. Preserve unrelated checkouts and historical evidence.

## Global Constraints

- Configuration identity is SHA-256 of canonical JSON. Reordering keys or changing whitespace preserves identity, vectors and failures.
- `max_retries` means additional attempts: zero gives one total and three gives four. Bounds apply between resets; changing identity or deleting vectors.db resets that state.
- Computing happens outside the write transaction. Missing or invalid config never resets state.
- No unbounded reread/reset loop and no provider request using a mismatched identity.
- No persisted version, generation, job state, configuration lock, editing command, or requirement for external editors to cooperate.
- No splitting or silent truncation. Native Ollama sends `truncate: false`; do not introduce a guessed universal text limit.
- Keep domain infrastructure-free, storage independent of file parsing, handlers calling service interfaces and composition in lib.rs. KISS/DRY/YAGNI; no dependency changes expected.
- Follow current architecture, lint/development rules, explicit imports and meaningful constants. Keep new/touched functions focused and tests Arrange/Act/Assert. Mutation checks use disposable copies or exact byte restoration.
- Planning approval does not establish G-1/G-2 or execute TK-17, TK-28, TK-29 or TK-37. Do not equate a test child entrypoint with a normal executed scenario.

## Review Focus

- Initial absent identity and two competing initializations: conditional reset preserves whichever different configuration was committed first (Task 1).
- Same desired identity after another reset: a late observer keeps new vectors and attempts rather than clearing them (Task 1).
- Missing/invalid/unreadable config and a conflicting valid config: safe non-ready outcomes, usable FTS, no provider call or destructive fallback (Task 1).
- SIGINT after reservation with retries remaining: persisted attempts and released lock prevent budget renewal across actual processes (Task 2).
- Over-context provider error: final text marker remains searchable; request contains the full text and `truncate:false`; diagnostics exclude response canaries (Task 2).

## Execution prerequisite

- [ ] Before Task 1, save `make check V=1` on the unmodified product base in a new evidence location. If it fails, distinguish dependency/environment setup from product failure before proceeding. Reuse this baseline; do not repeat it after each task.

---

### Task 1: Guard reset and propagate a typed configuration conflict

**Files:**
- Modify `src/service/storage.rs`, `src/service/model.rs`, `src/service/embedding_traits.rs`, `src/service/embedding.rs`, `src/storage/sqlite.rs`, `src/storage/vectors.rs`.
- Create `tests/storage_configuration.rs` for independent storage concurrency/reset assertions.
- Create `src/service/embedding/tests/process/configuration.rs`; declare it in `src/service/embedding/tests/process.rs`. Reuse the existing owned process/barrier support and child entrypoint rather than creating another harness.
- Modify `src/service/search/tests.rs` and `src/service/traversal/tests/scoped.rs` for non-ready configuration behavior.
- Mechanically adapt all existing Storage implementations/forwarders and reset fixtures found by `rg -n 'fn reset_vectors|\.reset_vectors\(' src tests`; current callers include storage/schema.rs tests, service tests, integration suites and tests/support/reading.rs. Update only the signature/explicit observed identity and result handling; preserve the behavior each test verifies.

**Interfaces:**
- Replace `Storage::reset_vectors` with `fn reset_vectors(&mut self, observed: Option<&Sha256>, desired: &Sha256) -> StorageResult<VectorOutcome<bool>>`.
- `VectorOutcome::Current(true)` means reset committed; `Current(false)` means desired identity was already current and no data changed; `ConfigChanged` means current identity differs from both desired and the prior observation, and nothing changed.
- Add `ConfigState::Changed` for a valid observation superseded before synchronization. Keep `Ready` reserved for successful synchronization, not a known conflict.
- `Embedder::read_configuration` preserves its signature. Observe identity through a short `Storage::begin_read()` before reading config bytes, release the view, validate bytes/provider, then call guarded reset. Missing/invalid states preserve data; storage errors remain typed errors. Return `Changed` on reset conflict, without retrying.
- Drain maps `Changed` to `DrainOutcome::ConfigChanged`; absent/invalid remain `Unavailable`. Query returns unavailable; `has_work` returns false. Existing search capture treats non-Ready as FTS-only and traverse continues its graph read. No new public JSON state/code.

- [ ] **Step 1: Add the deterministic service regression using current APIs.** At a test-only checkpoint immediately after file validation and before reset, pause P with captured A. Q applies B and records a literal vector `[4.0,5.0,6.0]` for one current hash and one reserved attempt for another. Resume P; assert B identity, exact vector and attempts=1 remain, and no A HTTP request occurs. Use the existing root-test child and environment checkpoints only under `cfg(test)`; always release/terminate/reap owned children on failure.
- [ ] **Step 2: Prove the regression fails on baseline.** Run `cargo test --locked --offline --lib stale_configuration_observation -- --nocapture`. Require the stored-state assertion to fail because old reset erases B, not because of compilation, a watchdog or fixture setup. Save output in a new `.artifacts/tk27/` evidence directory.
- [ ] **Step 3: Add conditional storage tests with exact observations.** Assert `Current(true)` for absent-to-A and observed-A-to-B; `Current(false)` for observed-A/desired-B when B is already installed; `ConfigChanged` for observed-A/desired-A with current B and observed-None/desired-A with current B. In the latter cases preserve B vector and failure bytes. An older pinned A view still reads A after a successful B reset. Preserve the existing late-meta-failure rollback test with the new signature.
- [ ] **Step 4: Implement the interface and minimal conditional transaction.** Inside `BEGIN IMMEDIATE`, check desired identity first, then the observed identity (including None), then perform the existing reset. Read actual identity using typed storage-owned parsing, and fail safely for malformed metadata. Update service observation order, enum documentation, conflict mapping and every caller/fixture; do not retain a blind-reset compatibility route.
- [ ] **Step 5: Verify consumer behavior and regressions.** Add `Changed` to the existing search capture/non-ready and traversal cases: no query HTTP, no captured vectors, unchanged FTS/graph eligibility and one pinned consumer view. Run `cargo test --locked --offline --test storage_configuration`, `cargo test --locked --offline --test storage_embedding_work`, `cargo test --locked --offline --test embedding_service`, `cargo test --locked --offline --lib configuration`, `cargo test --locked --offline --lib service::search::tests`, and `cargo test --locked --offline --lib service::traversal::tests`. Confirm the new race test passes; use `cargo check --locked --offline --all-targets` to find omitted signature migrations.
- [ ] **Step 6: Mutation-check the new guards.** In disposable copies, remove the observed-identity rejection and separately remove the same-desired early return; each corresponding stored-vector/attempt assertion must fail after compiling. Restore exact bytes and rerun those checks.
- [ ] **Step 7: Commit this coherent change.** Use an explicit reviewed path list, including mechanically adapted fixtures: `fix: prevent stale embedding configuration resets`.

### Task 2: Prove retries, SIGINT, reuse and over-context behavior

**Files:**
- Modify `tests/cli_drain.rs` (actual drain and restart), `tests/reindex_command.rs` (reuse with observed provider calls).
- Create `tests/embedding_lifecycle.rs` (long-input contract) using `tests/support/reading.rs` and `tests/support/embedding.rs`; add narrow existing helper capabilities only if required by multiple consumers.
- Modify `tests/support/cli.rs` only to add a necessary owned-child signal method, keeping process ownership/reaping local to the fixture. No global test environment mutation.

**Interfaces:**
- Consume Task 1's typed reset/readiness contract and existing `Fixture`, `HttpFixture`, `ResponseBarrier`, `ReadingRepo` and product binary. Preserve all CLI request/response schemas.
- If needed, add `OwnedChild::send_sigint(&mut self)` to the existing Unix test helper: target the owned live child, deliver SIGINT, and retain ownership until bounded wait/reaping. Check termination by SIGINT and do not assert TK-29's future handled JSON response.

- [ ] **Step 1: Add durable restart/SIGINT scenario.** Set `max_retries=1`; start the actual `embed --drain` with HTTP held at a response barrier. Independently observe attempts=1, then SIGINT the owned process and reap it. Release the fixture barrier. Assert the kernel lock is free and attempts remains 1. Start a new drain against the same unchanged config and a provider error: exactly one further HTTP call, attempts=2, no vector. A third drain makes zero further calls. Count the interrupted request as consuming its reserved attempt and check safe persisted errors/corpus unchanged.
- [ ] **Step 2: Run that scenario against current behavior.** Run `cargo test --locked --offline --test cli_drain sigint_preserves_attempt_budget -- --nocapture`. If it is green, retain it as new evidence; if red, identify the violated contract and make the smallest production correction, then rerun. Existing retry cases with `(max_retries,total_attempts)=(0,1),(3,4)` remain regression coverage.
- [ ] **Step 3: Add cache-loss/reuse with request counts.** Seed a healthy vector for `a.md`, retain another current unembedded hash, delete only the disposable cache with all handles closed, and run reindex while holding the existing embed lock to prevent automatic background races. Release the lock and run the real drain. The seed vector stays exact; the provider receives only the missing hash's text once, not `a.md`. Run `cargo test --locked --offline --test reindex_command reindex_reuses_vectors` and retain the existing no-wait/healthy-vector test.
- [ ] **Step 4: Add long-input rejection.** Publish a fixed synthetic text with a unique `EndOfLongKnowledgeToken` at its end. A local HTTP fixture rejects it with status 400 and a private response canary; `max_retries=0`. Assert one full-text request with `truncate:false`, one durable safe failure, no vector, and FTS lookup of the final marker returns the knowledge ID. The text length is a fixture choice, not a product/model limit. A repeated drain sends nothing. Run `cargo test --locked --offline --test embedding_lifecycle`.
- [ ] **Step 5: Mutation-check new evidence and commit.** Increase the computed total attempt budget by one in a disposable copy and require a request-count/attempt-count assertion to fail; a watchdog alone is not sufficient; verify removing the final searchable portion during projection is caught by the final-marker assertion (without changing the expected literal or input). Restore exact source bytes and rerun. Commit intended paths as `test: verify durable embedding lifecycle across interruptions`.

### Task 3: Verify final tree and publish the task report

**Files:**
- Create `docs/zkit/letopis/checks/TK-27/report.md` and a compact `verification.json` with captured revision, commands, results and source/evidence hashes.
- Update the TK-27 task card, approved spec and this plan. Add the accepted conditional reset clarification to MC-3 in `docs/zkit/letopis/mechanics.md` and the affected `contracts/X-2-embeddings.md` / `contracts/I-8-read-view.md`; keep a single authoritative description and links instead of repeating the algorithm everywhere.
- Extend `docs/zkit/letopis/mechanics/model_embedding.py` only with the specified observed-identity conditional-reset schedule and a negative control; do not rewrite unrelated models or historical receipts.

**Interfaces:**
- Consume Tasks 1–2's final source, tests and scoped mutation evidence. No new runtime API.

- [ ] **Step 1: Align canonical explanation and model with the approved guard.** Add the old-observer schedule to the existing small model; correct guards yield no violations and disabling conditional rejection loses the literal B vector/failure state. Run `python3 -B docs/zkit/letopis/mechanics/model_embedding.py` and its existing mechanics checks where applicable.
- [ ] **Step 2: Format and verify changed product once.** Apply `make fmt`, then run `python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk27/final-host-<unique>`. The host runner already runs `make verify V=1`, strict docs, schema parity, native/standalone/linkage and the explicit 60s provider deadline. Do not run another identical full gate without a new source change or failure. Keep output in logs and report small summaries. Record each ignored test separately from executed parent scenarios.
- [ ] **Step 3: Verify Linux when Docker is available.** Run `python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk27/final-linux-<unique>`. Confirm the new top-level Rust suite and process tests are in the manifest/container inputs and common source hashes match. If infrastructure is unavailable, record the exact limitation; do not claim Linux PASS or retry unchanged infrastructure indefinitely. The real deadline test consumes wall time, not model tokens; no paid agent/model sessions are launched.
- [ ] **Step 4: Review the actual final diff and evidence.** Check storage/service import direction, typed interfaces, scoped transactions, lib.rs composition, and safe public errors. Map every TK-27 criterion to actual commands/assertions and platform limits. Check changed documentation links and `git diff --check`. Task 1's service/API migrations require this whole-tree review even if most tests are unchanged.
- [ ] **Step 5: Record results and commit.** Mark only demonstrated criteria complete; include unresolved findings and deferred project gates. Update the spec status and task report consistently, check source/evidence hashes, and commit only intended documentation/model changes as `docs: report TK-27 embedding lifecycle verification`. Merge, push and GitHub publication wait for the user's instruction.

## Self-review

Coverage: conditional reset and late result isolation → Task 1; retry durability/SIGINT, vector reuse and long FTS → Task 2; rollback/pinned views → Task 1 plus retained regression; architecture, canonical model, platform evidence and reporting → Task 3. Initial absence, duplicate desired identity, unavailable config and actual-process restart are explicit cases. Public signatures and outcome meanings are defined once above; no unchecked file/database atomicity or lifetime retry guarantee is introduced.
