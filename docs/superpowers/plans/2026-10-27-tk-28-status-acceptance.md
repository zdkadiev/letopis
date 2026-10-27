# TK-28 Status Acceptance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Demonstrate the two open TK-28 acceptance criteria — storage-failure reporting and query/drain/FTS status lifecycle — with reproducible tests, changing product code only if a test reproduces a real defect.

**Architecture:** Acceptance-only work on branch `tk-28`. Reuse the existing `StatusService` (observation-only, single read view), `EmbeddingService` (`vec.query_error` is one row), and `BackgroundService` (safe nonfatal drain warning). New coverage is added at the boundaries that own each behavior: a real product CLI test for query failure → status → recovery, a real product CLI test for a transient drain-start warning, and the existing production-process driver for an uncommitted file diagnostic.

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

Append this test and helpers to `tests/search_filter_fallback.rs`:

```rust
#[test]
fn query_failure_is_visible_in_status_until_a_successful_query() {
    // Given: two failing provider replies followed by one successful reply.
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start_sequence(vec![
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
        FixtureReply::Status {
            status: 200,
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        },
    ]);
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    prepare_vectors(&repo, &config, 2);
    let input = build_input(&basis);
    let expected = json!({
        "code": "embedding_failed",
        "message": "query embedding failed",
        "details": {"reason": "query_embedding_failed"}
    });

    // When: the first query embedding fails.
    let first = invoke_search(&repo, &input);
    // Then: FTS results remain and status reports exactly one current problem.
    assert_eq!(read_ids(&first), ["a.md", "b.md"]);
    assert_eq!(get_query_problems(&read_status(&repo)), vec![expected.clone()]);
    assert_eq!(server.take_requests().len(), 1);

    // When: a second query embedding fails in a separate process.
    let second = invoke_search(&repo, &input);
    // Then: the one current record is replaced, not accumulated.
    assert_eq!(read_ids(&second), ["a.md", "b.md"]);
    assert_eq!(get_query_problems(&read_status(&repo)), vec![expected]);
    // `take_requests` drains its log, so this counts requests since the previous call.
    assert_eq!(server.take_requests().len(), 1);

    // When: a later query embedding succeeds.
    let recovered = invoke_search(&repo, &input);
    // Then: semantic results appear and status no longer reports the problem.
    assert_eq!(read_ids(&recovered), ["a.md", "b.md", "semantic.md"]);
    assert!(get_query_problems(&read_status(&repo)).is_empty());
    assert_eq!(server.take_requests().len(), 1);
}

fn read_status(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["status"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for canary in ["PRIVATE", "fixture-secret", "stable"] {
        assert!(!stdout.contains(canary), "{stdout}");
    }
    serde_json::from_str(&stdout).unwrap()
}

fn get_query_problems(status: &Value) -> Vec<Value> {
    status["result"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|problem| problem["details"]["reason"] == "query_embedding_failed")
        .cloned()
        .collect()
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --locked --offline --test search_filter_fallback query_failure_is_visible_in_status_until_a_successful_query -- --nocapture`

Expected: PASS (1 passed). If it FAILS, stop: the failure is the reproduced defect. Fix only in the owner named above, rerun, and record the exact failure in the task report.

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
- Owner if a defect is reproduced: `src/service/background.rs` (warning construction) or `src/cli/handlers.rs` (response warnings).

**Interfaces:**
- Consumes: existing `read_status(repo: &ReadingRepo) -> Value` helper in `tests/status_command.rs`, `ReadingRepo`, `TcpListener`.
- Produces: no shared API.

- [ ] **Step 1: Add the acceptance test**

Add `use std::os::unix::fs::symlink;` to the imports, then append:

```rust
#[test]
fn drain_start_failure_warns_without_persisting_and_keeps_fts() {
    // Given: pending embedding work, a valid config, and an unsafe embed lock.
    let repo = ReadingRepo::new();
    repo.write_knowledge("known.md", "needle");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    repo.write_config("ollama", &url, "fixture");
    let outside = tempfile::tempdir().unwrap();
    let canary = outside.path().join("embed-canary");
    fs::write(&canary, b"DRAIN_CANARY").unwrap();
    let lock = repo.get_root().join(".letopis/embed.lock");
    symlink(&canary, &lock).unwrap();

    // When: a real product search runs and the drain cannot start.
    let output = repo.invoke(
        &["search"],
        json!({"query":"needle","limit":10}).to_string().as_bytes(),
    );

    // Then: FTS succeeds with one safe warning and no provider call.
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"][0]["element_id"], "known.md");
    assert_eq!(
        value["warnings"],
        json!([{
            "code": "internal_error",
            "message": "cannot start background embedding drain",
            "details": {}
        }])
    );
    assert!(matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    assert_eq!(fs::read(&canary).unwrap(), b"DRAIN_CANARY");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("DRAIN_CANARY"));

    // Given: the injected lock fault is removed.
    fs::remove_file(&lock).unwrap();
    // When / Then: a fresh status contains no persisted launch warning.
    let status = read_status(&repo);
    assert_eq!(status["result"]["problems"], json!([]));
    assert!(!status.to_string().contains("cannot start background embedding drain"));
    assert_eq!(fs::read(&canary).unwrap(), b"DRAIN_CANARY");
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test --locked --offline --test status_command drain_start_failure_warns_without_persisting_and_keeps_fts -- --nocapture`

Expected: PASS (1 passed). If it FAILS, stop and fix only in the owner named above.

- [ ] **Step 3: Mutation-check the test**

In `src/service/background.rs`, change the lock-probe branch `Err(_) => return Ok(Some(build_warning()))` to `Err(_) => return Ok(None)`. Rerun Step 2. Expected: FAIL because `warnings` is absent. Restore exact bytes.

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

```rust
#[test]
fn uncommitted_file_diagnostic_is_absent_from_status_until_republished() {
    // Given: a valid publication, then one authored file becomes invalid.
    let fixture = Fixture::new();
    fixture.invalidate_retained_file();

    // When: the cache commit fails after the invalid file projection is staged.
    Worker::spawn(fixture.root.path(), "fail").finish();

    // Then: the old publication remains and a fresh status derives no file problem.
    assert_eq!(read_state(fixture.root.path()), fixture.old);
    let status = read_status(fixture.root.path());
    assert_eq!(status.revision, Some(1));
    assert!(get_invalid_file_problems(&status).is_empty());
    assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));

    // When: the injected fault is gone and publication retries.
    Worker::spawn(fixture.root.path(), "publish").finish();

    // Then: the real safe diagnostic is persisted and survives a fresh status.
    let status = read_status(fixture.root.path());
    assert_eq!(status.revision, Some(2));
    assert_eq!(
        get_invalid_file_problems(&status),
        vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
    );
    assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));

    // Given: the authored file is repaired.
    fixture.restore_retained_file();

    // When / Then: a new publication makes the current problem disappear.
    Worker::spawn(fixture.root.path(), "publish").finish();
    let status = read_status(fixture.root.path());
    assert_eq!(status.revision, Some(3));
    assert!(get_invalid_file_problems(&status).is_empty());
}
```

- [ ] **Step 5: Run the test**

Run: `LETOPIS_TEST_EVIDENCE=.artifacts/tk-28/process cargo test --locked --offline --lib service::document_refresher::tests::process::uncommitted_file_diagnostic_is_absent_from_status_until_republished -- --nocapture`

Expected: PASS (1 passed). If it FAILS, stop and fix only in the owner named above.

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
