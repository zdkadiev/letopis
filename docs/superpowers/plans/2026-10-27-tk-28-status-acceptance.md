# TK-28 Status Acceptance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Demonstrate the two open TK-28 acceptance criteria — storage-failure reporting and query/drain/FTS status lifecycle — with reproducible tests, changing product code only if a test reproduces a real defect.

**Architecture:** Acceptance-only work on branch `tk-28`. Reuse the existing `StatusService` (observation-only, single read view), `EmbeddingService` (`vec.query_error` is one row), and `BackgroundRunnerService` (safe nonfatal drain warning). New coverage is added at the boundaries that own each behavior: a real product CLI test for query failure → status → recovery, a real product CLI test for a transient drain-start warning, and the existing production-process driver for an uncommitted file diagnostic.

**Tech Stack:** Rust 1.95.0, rusqlite/SQLite, Rust CLI subprocess tests, local `HttpFixture`, existing private `DocumentRefresher` process checkpoints.

**Spec:** `docs/superpowers/specs/2026-10-26-tk-28-status-acceptance-design.md`

## Global Constraints

- No error-history table, separate `errors` command, error-clearing command, new runtime, provider, configuration field, storage schema, or background process.
- Fix product code only for a reproduced TK-28 defect; name the owning module in the commit and report.
- Reuse TK-26/TK-27 evidence at its captured revisions; do not relabel it as a new run.
- Use local HTTP fixtures only; never perform paid provider/model requests.
- Every new test uses explicit `// Given`, `// When`, `// Then` comments.
- Mutation-check every new behavioral test: break the covered product path, confirm the test fails, restore exact bytes.
- Do not merge, push, or close the GitHub issue without instruction.
- Report narrower scope honestly where a test executes the service/process composition rather than the full product binary. Do not call a service-only double a product-binary scenario.

## Review Focus

- A failed publication must never surface staged `files.error` values in a later `status` (Task 3).
- Provider request/response canaries must not reach status output or the persisted safe `query_error` (Task 1).
- A drain-start warning must stay in the foreground response and never become a persistent status problem or turn success into failure (Task 2).
- A successful query must clear the single current query problem; status must not clear it itself (Task 1).
- Repeated query failures must keep exactly one current problem, not accumulate history (Task 1).

---

### Task 1: Query failure is current in status until a successful query

**Files:**
- Modify: `tests/search_filter_fallback.rs`
- Owner if a defect is reproduced: `src/service/status.rs` (status projection) or `src/service/embedding.rs` (`publish_query`).

**Interfaces:**
- Consumes: `ReadingRepo`, `HttpFixture::start_sequence`, `prepare_notes`, `prepare_vectors`, `invoke_search`, `build_input`, `read_ids` (already in the file).
- Produces: private test helpers `read_status(repo: &ReadingRepo) -> Value` and `get_query_problems(status: &Value) -> Vec<Value>`.

- [ ] **Step 1: Add the acceptance test and helpers**

Append three single-scenario tests and shared helpers (`prepare_query_scenario`,
`expected_query_problem`, `read_status`, `get_query_problems`) to
`tests/search_filter_fallback.rs`:

- `query_failure_keeps_fts_and_appears_in_status` — one failing reply; search keeps
  FTS results and status reports exactly one current problem.
- `repeated_query_failure_keeps_one_current_problem` — two failing replies in
  separate processes; the current record is replaced, not accumulated.
- `successful_query_clears_the_query_problem` — a failing reply followed by a
  successful one; semantic results appear and status no longer reports the problem.

- [ ] **Step 2: Run the tests**

Run: `cargo test --locked --offline --test search_filter_fallback query_ -- --nocapture`

Expected: PASS (5 passed). If it FAILS, stop: the failure is the reproduced defect. Fix only in the owner named above, rerun, and record the exact failure in the task report.

- [ ] **Step 3: Mutation-check the test**

Temporarily remove the `if view.read_query_error()... { problems.push(...) }` block in `src/service/status.rs`. Rerun Step 2. Expected: FAIL because status no longer reports `query_embedding_failed`. Restore the exact bytes (`git diff -- src/service/status.rs` must be empty).

- [ ] **Step 4: Commit**

```bash
git add tests/search_filter_fallback.rs
git commit -m "test: accept query failure status lifecycle"
```

---

### Task 2: A failed drain start warns without persisting

**Files:**
- Modify: `tests/status_command.rs`
- Owner if a defect is reproduced: `../../../src/service/background_runner.rs` (warning construction) or `src/cli/handlers.rs` (response warnings).

**Interfaces:**
- Consumes: existing `read_status(repo: &ReadingRepo) -> Value` helper in `tests/status_command.rs`, `ReadingRepo`, `TcpListener`.
- Produces: no shared API.

- [ ] **Step 1: Add the acceptance test**

Add `use std::os::unix::fs::symlink;` and `use std::path::PathBuf;` to the imports,
then append two single-scenario tests and the shared helper
`prepare_failed_drain(repo) -> (TempDir, PathBuf, TcpListener)`:

- `drain_start_failure_warns_and_keeps_fts` — a search with an unsafe embed lock
  returns FTS results, one safe warning, no provider call, and leaves the canary
  untouched.
- `drain_start_failure_is_absent_from_status` — after the same failed drain, a fresh
  `status` contains no persisted launch warning and the canary is untouched.

- [ ] **Step 2: Run the tests**

Run: `cargo test --locked --offline --test status_command drain_start_failure -- --nocapture`

Expected: PASS (2 passed). If it FAILS, stop and fix only in the owner named above.

- [ ] **Step 3: Mutation-check the test**

In `../../../src/service/background_runner.rs`, change the lock-probe branch `Err(_) => return Ok(Some(build_warning()))` to `Err(_) => return Ok(None)`. Rerun Step 2. Expected: FAIL because `warnings` is absent. Restore exact bytes.

- [ ] **Step 4: Commit**

```bash
git add tests/status_command.rs
git commit -m "test: accept transient drain launch warning"
```

---

### Task 3: An uncommitted file diagnostic stays out of status

**Files:**
- Modify: `src/service/document_refresher/tests/process.rs`
- Owner if a defect is reproduced: `src/storage/cache_write.rs` (commit guard) or `src/service/document_refresher.rs` (failure path).

**Interfaces:**
- Consumes: the private `fail_late` checkpoint, `Worker`, `Fixture`, `SqliteStatusStorage`, `StatusService`, `CurrentGraphDiagnostics`, `ProvenanceService`, `build_embedding_provider`.
- Produces: `Fixture::invalidate_retained_file()`, `Fixture::restore_retained_file()`, `read_status(&Path) -> StatusReport`, `get_invalid_file_problems(&StatusReport) -> Vec<(String, String)>`.

- [ ] **Step 1: Make the invalid fixture content carry a canary**

Replace the `INVALID` constant so the diagnostic path has a privacy sentinel:

```rust
const DIAGNOSTIC_CANARY: &str = "TK28_DIAGNOSTIC_CANARY";
const INVALID: &str = "<!-- letopis: invalid TK28_DIAGNOSTIC_CANARY -->\n";
```

Add the retained-file constant and use it both in `Fixture::new` and in `repair_i`:

```rust
const RETAINED: &str = "<!-- letopis: {\"created_at\":2} -->\nRetainedToken [source](source.txt \"letopis:type=external\")\n";
```

In `Fixture::new`, replace the inline `i.md` literal with `("i.md", RETAINED)`.

- [ ] **Step 2: Assert the failed child never exposes the canary**

In `run_child`, inside `if mode.ends_with("fail")` after the `StorageError` assertion, add:

```rust
assert!(!format!("{error:?}").contains(DIAGNOSTIC_CANARY));
```

- [ ] **Step 3: Add the status reader and fixture helpers**

Append to `src/service/document_refresher/tests/process.rs`:

```rust
fn read_status(root: &Path) -> StatusReport {
    let git = GitCli::open(root).unwrap();
    let files = LocalFileSystem::open(git.get_root()).unwrap();
    let pinned = files.open_existing_service_directory().unwrap().unwrap();
    let storage = SqliteStatusStorage::open_confined(&git.get_root().join(".letopis"), &pinned)
        .unwrap();
    let provenance = ProvenanceService::new(
        GitCli::open(root).unwrap(),
        LocalFileSystem::open(root).unwrap(),
    );
    let provider = crate::build_embedding_provider().unwrap();
    let graph = CurrentGraphDiagnostics::new();
    let mut status = StatusService {
        storage,
        files: &files,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    };
    status.read_status().unwrap()
}

fn get_invalid_file_problems(report: &StatusReport) -> Vec<(String, String)> {
    report
        .problems
        .iter()
        .filter_map(|problem| {
            let path = problem.details.get("path")?.as_str()?.to_owned();
            let reason = problem.details.get("reason")?.as_str()?.to_owned();
            (path == "i.md" && reason == "invalid_metadata").then_some((path, reason))
        })
        .collect()
}
```

Add the imports `SqliteStatusStorage`, `StatusReader`, `StatusReport`, `StatusService`, and `CurrentGraphDiagnostics` to the existing `use crate::{...}` blocks.

Add these methods to `impl Fixture`:

```rust
fn invalidate_retained_file(&self) {
    fs::write(self.root.path().join("i.md"), INVALID).unwrap();
}

fn restore_retained_file(&self) {
    fs::write(self.root.path().join("i.md"), RETAINED).unwrap();
}
```

- [ ] **Step 4: Add the acceptance test**

Add three single-scenario tests:

- `uncommitted_file_diagnostic_is_absent_from_status` — a late storage failure
  leaves the old publication visible, and a fresh status derives no file problem
  and exposes no canary.
- `failed_publication_retry_persists_the_file_diagnostic` — after the fault is gone,
  publication persists the real safe diagnostic and a fresh status reports it.
- `repaired_file_problem_disappears_after_publication` — after the authored file is
  repaired and republished, the current problem disappears.

- [ ] **Step 5: Run the tests**

Run: `LETOPIS_TEST_EVIDENCE=.artifacts/tk-28/process cargo test --locked --offline --lib service::document_refresher::tests::process:: -- --nocapture`

Expected: PASS (16 passed, 1 ignored). If it FAILS, stop and fix only in the owner named above.

- [ ] **Step 6: Mutation-check the test**

In `src/storage/cache_write.rs`, remove the `if self.failed { return Err(...) }` guard from `commit` so a transaction with a caught statement failure proceeds to commit. Rerun Step 5. Expected: FAIL because the staged file diagnostic is published despite the failure. Restore exact bytes.

- [ ] **Step 7: Commit**

```bash
git add src/service/document_refresher/tests/process.rs
git commit -m "test: accept uncommitted file diagnostic status"
```

---

### Task 4: Verify, record evidence, and close the card

**Files:**
- Create: `docs/zkit/letopis/checks/TK-28-status-acceptance/report.md`
- Modify: `docs/zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-28-diagnostics.md`

**Interfaces:**
- Consumes: results from Tasks 1–3 and the referenced TK-26/TK-27 evidence.
- Produces: the persisted task report and completion state.

- [ ] **Step 1: Run the required checks**

Run: `make check`
Expected: one `ok` line.

Run: `make verify`
Expected: one `ok` line (strict Rustdoc plus release-profile root tests).

Record exact output, platform (`uname -sm`), revision (`git rev-parse HEAD`), and passed/failed/ignored counts. Do not run the native storage runner: no storage schema, native stack, build or vendored input changes are expected; if a defect fix touches those, run `python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk-28/storage`.

- [ ] **Step 2: Write the evidence report**

Create `docs/zkit/letopis/checks/TK-28-status-acceptance/report.md` with:

- task and spec links, branch/revision, platform;
- one row per open criterion with the exact command, expected result, actual result, and test name;
- the executed scope for each test (real product CLI process / service-process composition) and the narrower-scope statement;
- reused TK-26/TK-27 evidence and its captured revisions;
- mutation controls performed and observed failures;
- limitations: x86_64, physical power loss, G-1/TK-17, G-2, and provider quality remain unassessed.

- [ ] **Step 3: Update the TK-28 card**

Mark the two remaining acceptance checkboxes complete. Set `> **Статус:** done`. Replace §9 with the actual result, changed files, exact checks, report link, and remaining limits. Do not claim G-2.

- [ ] **Step 4: Record in the existing Engram store**

Use the `engram` skill to append observations and an action to the existing task `entity_6716d12276e642f28834f019f4ba2b59`: acceptance scope, test results, evidence path, mutation controls, and limits. Do not initialize another store and do not edit the database directly.

- [ ] **Step 5: Commit**

```bash
git add docs/zkit/letopis/checks/TK-28-status-acceptance/report.md \
        docs/zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-28-diagnostics.md
git commit -m "docs: report TK-28 status acceptance"
```

---

## Self-Review

- **Spec coverage:** storage write failure with fresh status and recovery → Task 3; query failure → FTS → status → restart → recovery and one-row semantics → Task 1; drain launch failure → transient safe warning and FTS → Task 2; current-state preservation and no stronger synchronization claims → preserved by using existing `StatusService`; verification, mutation controls and limits → Task 4.
- **Step scan:** each step names an exact test, command, expected outcome, or file edit. Conditional fixes name one owner.
- **Type consistency:** `read_status` returns `Value` in the CLI tests and `StatusReport` in the process driver; names match the types they return. `get_query_problems` and `get_invalid_file_problems` are separate because their value types differ.
- **Review Focus:** all five lines map to Tasks 1–3 as listed above.
- **Proportion:** three focused tests plus one evidence task; no production code changes are planned.

## Implementation deviation (2026-10-27)

The user required one scenario per test function. The combined lifecycle tests in
Tasks 1 and 3 were split; each `#[test]` now covers one scenario, with shared
preconditions in private helpers. Final names:

- `query_failure_keeps_fts_and_appears_in_status`
- `repeated_query_failure_keeps_one_current_problem`
- `successful_query_clears_the_query_problem`
- `uncommitted_file_diagnostic_is_absent_from_status`
- `failed_publication_retry_persists_the_file_diagnostic`
- `repaired_file_problem_disappears_after_publication`
- `drain_start_failure_warns_and_keeps_fts`
- `drain_start_failure_is_absent_from_status`

Committed as `52b317a`; the drain test was split in `b2efec9`. Mutation controls for the affected behaviors were rerun
against the split tests and still fail as intended; the plan-specified
`cache_write` commit-guard mutation still passes (the guard is defense-in-depth
relative to statement-error propagation) and the compound mutation is detected.
