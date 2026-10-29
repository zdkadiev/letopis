# Read Workflow Performance Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Status:** implementation authorized by the user on 2026-10-29; execution in progress in isolated branch `perf/read-snapshot-verification`. Scope/invariant tests and hand mutations passed; final all-target gates, native receipt, matched profiling and independent review remain in progress. See the execution report for final results.

**Goal:** Make routine reads avoid exhaustive author-source verification, and batch that verification in full-corpus check/reindex while preserving freshness, addressed-byte integrity and atomic publication.

**Architecture:** Separate root/ref retention from strong closure-verifying pin through small application-owned capabilities. Ordinary refresh uses lightweight retention; full-corpus check uses verified incremental refresh, and reindex uses verified full publication. A private Git batch session implements strong verification; inspect still strongly pins its newly captured reading snapshot.

**Tech Stack:** Rust 1.95.0, Git CLI >=2.29, existing `sha2 = "=0.10.9"`; proposed `sha1-checked = "=0.10.0"`, gated by a compile/API probe and user approval of the spec.

**Spec:** [Read workflow performance](../specs/2026-10-29-read-workflow-performance-design.md).

## Global Constraints

- Synchronous I/O, existing module boundaries; no daemon, watcher, runtime, persistent verification cache or new command/configuration.
- `search`, `traverse`, `inspect` retain automatic database refresh; authored Markdown is never modified.
- Every strong pin verifies its complete required local closure, including repeated pins; gitlinks are not dereferenced. Routine source retention checks roots/refs only and cannot return RetainedTree.
- Omitted/empty check.paths means full audit; any nonempty paths list means filtered check with no exhaustive audit. Reindex always fully audits effective source snapshots before SQL publication.
- Preserve exact direct-ref checks, CAS publication, redacted causes, SHA-1/SHA-256 and no-lazy-fetch/no-alternates policy.
- Retain before SQL writes; keep index.lock ordering and transaction rollback behavior.
- Use Given/When/Then tests with independent oracles and hand mutations; run required checks against the actual final tree.
- Preserve current `knowledge/` and unrelated edits. The trial file was removed by the user after the historical profile; do not recreate it in the main checkout. In an isolated execution worktree, create an owned equivalent benchmark fixture from the captured inspect snapshot or literals, and freeze it for both baselines. Use the worktrees skill and the existing Engram store through `ENGRAM_DB`.

## Review Focus

- Binary bodies containing LF/NUL must not desynchronize the next object.
- Git-normalized raw modes, including `100664` and `100600`, must retain current behavior.
- Full audits and reads of damaged objects must fail even with existing refs; unrelated descendant damage must not force routine reads to walk the entire source closure.
- Early errors/stderr pressure must not deadlock or leak a child; SIGINT must close real Git's pipes and preserve one response.
- Missing roots, missing author bases, missing descendants and wrong types must preserve distinct existing error/diagnostic semantics.

## Physical file ownership

| File | Responsibility |
| --- | --- |
| `src/git/process.rs` | Hardened command construction reused by the owned batch transport |
| `src/git/batch.rs` (new) | Batch child lifetime, framing, streaming object hashing and typed failures |
| `src/git/batch/tests.rs` (new) | Private protocol/hash/cleanup tests with controlled child fixtures |
| `src/git/validation.rs` (new) | Owned private tree metadata spool and one Git structural-validation batch |
| `src/git/refs.rs` | Full closure traversal and unchanged ref publication |
| `src/git/mod.rs` | Private batch child declaration only |
| `src/git/cli.rs`, `src/service/snapshot_traits.rs` | Distinct lightweight retain and strong pin contracts |
| `src/service/mod.rs`, `src/service/provenance.rs` | Root retention versus exhaustive verification under index.lock |
| `src/service/document_traits.rs`, `src/service/document_refresher.rs`, `src/service/document_refresher/basis.rs` | Ordinary versus verified refresh, independently of incremental/full publication |
| `src/service/check.rs` | Select verified refresh only for whole-corpus check |
| Corresponding module tests and handwritten trait fixtures | Explicit expectations for the two verification scopes |
| `skills/letopis/SKILL.md`, CLI help, X-1/X-3, mechanics/decisions | New verification timing and unchanged auto-refresh behavior |
| `Cargo.toml`, `Cargo.lock` | Approved collision-detecting SHA-1 dependency |
| `tests/git_retention.rs`, `tests/git_corruption.rs`, `tests/git_snapshots.rs` | Public compatibility, closure integrity and process-count regressions |
| `tests/cli_cancellation.rs`, `tests/support/cancellation/` | Real batch-child exit on foreground cancellation, if shared fixture support is needed |
| `docs/zkit/letopis/checks/read-workflow-performance/` | Before/after evidence report and repeatable profiling procedure |

Do not add a storage table, service wrapper, verification-certificate type or public batch interface. The new retain capability must not return the existing strong RetainedTree value. Keep existing one-shot object operations unless profiling identifies a concrete need to reuse batching there; a further change requires updating this plan.

### Task 1: Establish profile and dependency feasibility

**Files:** evidence report above; a disposable probe manifest outside the product crate. No product implementation.

**Interfaces:** consumes the existing CLI binary and request JSON. Produces a baseline manifest, resolved Rust samples, Git command counts, and a dependency-probe receipt.

- [ ] Freeze requests/corpus and record Git revision, binary SHA-256, platform, profile and cache status. Preserve the current dev-build samples as exploratory evidence; record any corpus changes during capture.
- [ ] Build an optimized binary with symbols using `CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --locked --offline --bin letopis`; record compiler/build settings. Do not compare its latency to an optimized candidate built differently.
- [ ] Profile warmed search, traversal and inspection with macOS `sample`, wall time and a separately labelled invocation-count wrapper. Keep provider inference out of the FTS comparison; capture three matched runs for medians. Record retained closure size and distinct object counts.
- [ ] In a disposable crate, compile `sha1-checked = "=0.10.0"` on Rust 1.95.0 and verify its collision-reporting API plus known independent SHA-1/Git object vectors. Fetch dependencies explicitly before offline checks. If the crate fails, revise the spec/dependency selection before product implementation; do not substitute unchecked SHA-1.
- [ ] Gate: proceed only if profiling confirms the expected hot path and the selected dependency compiles. Document any remaining platform or instrumentation limits.

### Task 2: Implement and verify the private batch reader

**Files:** `src/git/batch.rs`, `src/git/batch/tests.rs`, `src/git/process.rs`, `src/git/mod.rs`, `Cargo.toml`, `Cargo.lock`.

**Interfaces:** private `BatchReader::open(process: &GitProcess, root: &Path, oid_len: usize) -> Result<BatchReader, SnapshotError>`; `BatchReader::read_object(&mut self, oid: &str, expected: &str, missing: SnapshotErrorKind, wrong_type: SnapshotErrorKind) -> Result<VerifiedObject, SnapshotError>`; `BatchReader::finish(self) -> Result<(), SnapshotError>`. `VerifiedObject` is the private enum `Blob | Tree(Vec<u8>)`. The four read arguments match existing object-verification error policy, rather than introducing a public policy wrapper. Command construction remains owned by `GitProcess` and is exposed only within the Git module as necessary.

- [ ] Write `reads_binary_objects_without_losing_framing`: two requested objects containing LF/NUL return the expected independent tree bytes; blobs are streamed and fully hashed. Cover SHA-1 and SHA-256 with Git-generated fixture OIDs and literal source bytes.
- [ ] Write `rejects_invalid_batch_responses`: parameterize missing/header-overflow/wrong-OID/wrong-type/invalid-size/truncated-body/wrong-trailer cases. Assert the intended typed failure and safe diagnostics. Bound header parsing without bounding legitimate blob size; convert sizes with checked arithmetic.
- [ ] Write `batch_child_is_reaped_on_failure` and `stderr_pressure_does_not_block_batch`: controlled fixture children prove EOF, nonzero exit, early write failure and ordinary Drop cleanup are bounded. Raw canary payloads must not appear in formatted public errors.
- [ ] Run `cargo test --locked --offline --lib git::batch` and confirm the new tests fail before the reader exists or before the corresponding behavior is implemented.
- [ ] Implement the session using one plain `git cat-file --batch` child and the existing sanitized Git command construction. Send one full OID, flush, consume its complete response, then send the next. Drain stderr concurrently, retain only safe causes, stream blob hashing and keep tree bodies only as needed. Check child exit on success; kill/wait on ordinary failed teardown. No pool or new runtime.
- [ ] Hash the canonical Git header and bytes with the format selected by the repository's OID width; reject detected SHA-1 collisions. Validate expected response ID, type, byte count and delimiter before returning. Keep tree/body verification independent of Git's reported header identity.
- [ ] Rerun the focused tests, then hand-mutate OID comparison, body hashing and trailing-LF consumption separately. Each relevant test must fail; restore byte-for-byte and rerun green.
- [ ] Commit only the intended private Git implementation, dependency and test paths after focused validation.

### Task 3: Integrate batch traversal while preserving the strong pin contract

**Files:** `src/git/refs.rs`, `src/git/batch.rs` (private raw-tree decoder), `src/git/batch/tests.rs`, public Git tests listed above, cancellation fixtures if required.

**Interfaces:** retain `GitSnapshots::pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError>`. The walker uses one batch session; the private decoder returns expected child OID/type pairs from verified tree bytes. No change to the strong pin signature or its full-integrity guarantee; the separate retain/refresh capabilities are added in Task 4.

- [ ] Add `batch_pin_process_count_is_independent_of_blob_count`: count child argv using an owned Git wrapper for small and large flat/nested closures. Assert one batch reader per pin and no per-object `cat-file`, `hash-object` or `ls-tree` commands inside the closure walk. Fixed repository/root/ref checks remain permitted.
- [ ] Add `raw_tree_modes_match_git_normalization`: independently compare raw `100664`, `100600`, executable, symlink, tree and gitlink fixtures with `git ls-tree`; include binary names and both OID widths. Cover malformed/truncated tree entries and conflicting child types, checking decoding separately from the existing strict Git structural policy retained by the batched tree validator.
- [ ] Run the new tests against the old closure walker and confirm the invocation-count regression fails. Retain existing tests that corrupt a tree/blob after successful pin and reopen.
- [ ] Implement traversal with the existing pending/visited OID/type semantics, verified raw-tree parsing and gitlink skipping. Fully verify the root and closure, finish the batch child successfully, then perform the existing exact-ref check/CAS publication. Do not short-circuit on an existing ref.
- [ ] Add/extend a foreground cancellation test to signal only its owned CLI while a real batch Git child is alive; assert one cancellation envelope and prompt child exit from pipe closure. Preserve the existing process::exit listener rather than claiming destructors run after SIGINT.
- [ ] Run `cargo test --locked --offline --test git_snapshots --test git_retention --test git_corruption --test cli_cancellation` and `cargo test --locked --offline --lib service::document_refresher::tests::basis`.
- [ ] Run `cargo test --locked --offline --test document_refresh --test search_consistency --test knowledge_inspection --test source_inspection --test cli_traversal --test corruption_diagnostics --test corruption_reindex`. Confirm additions/edits/source-only changes still refresh, failure preserves the prior publication and inspect snapshot hashes match addressed bytes.
- [ ] Mutation-check closure traversal, existing-ref corruption and new process-count assertions; restore the exact code and rerun focused green. Commit intended paths only.

### Task 4: Separate routine retention from full check/reindex verification

**Files:** `src/service/snapshot_traits.rs`, `src/git/cli.rs`, `src/git/refs.rs`, `src/service/mod.rs`, `src/service/provenance.rs`, `src/service/document_traits.rs`, `src/service/document_refresher.rs`, `src/service/document_refresher/basis.rs`, `src/service/check.rs`, their existing tests/trait fixtures, CLI corruption fixtures and contract documentation listed in the file map.

**Interfaces:** add `GitSnapshots::retain(&self, tree: &TreeOid) -> Result<(), SnapshotError>`; add `BasisRetainer::verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>`; redefine existing `retain_snapshots` as root/ref retention. Add `Refresher::refresh_verified(&mut self) -> DocumentResult<RefreshOutcome>`. Keep `refresh` and `reindex` signatures. Add private `SnapshotVerification::{Root, Closure}` alongside `PublicationMode`, and pass verification scope through publication/basis helpers. No CLI option or schema migration.

- [ ] Write `whole_check_requests_verified_refresh` and `scoped_check_requests_routine_refresh` in `src/service/check/tests.rs`. Use explicit recording collaborators to assert omitted/empty paths select verified incremental refresh and nonempty paths select ordinary refresh. A list naming every current file is still scoped. Both collect findings from one pinned resulting read view.
- [ ] Write `routine_refresh_retains_roots_without_verifying_closures`, `verified_unchanged_refresh_audits_without_sql_write` and `verified_refresh_failure_preserves_previous_publication` in document-refresher tests. Cover initial build, changed files, new bases, unchanged files and full reindex. Assert exact retain/verify/begin/commit event order under index.lock; a full audit must run before SQL writes, not after publishing another revision.
- [ ] Write public retention tests showing lightweight retain validates root type/hash and exact refs, rejects symbolic/conflicting refs, survives ordinary GC for healthy closures, and returns no strong RetainedTree. A root with a missing unrelated child can be retained but must fail strong pin/full audit. Keep wrong-format/root versus missing-child errors distinct and retain no-alternates policy.
- [ ] Run the new focused tests to prove they fail before adding capabilities. Update handwritten Refresher, GitSnapshots and BasisRetainer implementations explicitly; do not add a default verified method delegating to the weaker operation.
- [ ] Implement lightweight `retain` using existing root verification and exact direct-ref/CAS helpers; factor only shared ref logic with two real consumers. Keep the strong pin traversal from Task 3. Route routine `retain_snapshots` through lightweight retain, and `verify_snapshots` through strong pin for each available unique root. Missing/invalid author roots remain current diagnostics.
- [ ] Add `full_audit_uses_resulting_valid_projection`: invalid edited Markdown keeps and audits its last-valid roots; valid replacement/deletion drops obsolete roots, including an obsolete damaged root. Build a metadata-only prospective root set before SQL writes, reusing the existing ownership/projection data, and test no unrelated historical roots are audited.
- [ ] Implement `refresh_verified` through the same index.lock/publication pipeline as refresh. Select verification independently of publication mode: root retention for ordinary refresh, closure verification for verified incremental refresh and reindex, including the empty-delta case. Keep successful ordinary root retention before SQL; preserve source-capture stat/hash rechecks and existing process checkpoints.
- [ ] Select refresh mode from `paths.is_empty()` in CheckService. Keep search/traversal/inspection on ordinary refresh; inspect's final strong pin applies only to the new reading snapshot. Keep existing lib.rs composition intact unless constructor signatures actually require changes.
- [ ] Add `whole_check_rejects_a_revision_published_after_its_audit`: a recording storage collaborator publishes an unaudited revision after verified refresh returns but before begin_read pins the view. Assert safe `CommandErrorKind::InternalError`, no clean report and no unbounded retry. Implement equality of `RefreshOutcome.revision` and `StorageRead::get_revision()` for full check before collecting findings. Also prove publication after read-view pin still returns a report from that audited view. Keep scoped-check behavior unchanged.
- [ ] Add CLI regressions in owned repositories: (a) mutate an unrelated child after pin and prove routine search/traverse/inspect succeed without reading it, while full check and reindex fail; (b) corrupt an addressed object and prove relevant reads fail safely; (c) introduce a new source_snapshot and prove ordinary reads retain the root without starting a full audit; (d) full-audit failure prevents publication of newly edited knowledge; (e) restored missing roots can be retained; (f) inspect bytes/hashes and external source membership remain consistent.
- [ ] Add subprocess-count assertions for unchanged reads and reads after Markdown edits/new bases: no closure traversal of author-source roots. Count inspection's separately captured tree by its OID so legitimate verification cannot hide a source-tree walk. Test via owned child environments; never mutate the test runner's global PATH.
- [ ] Run `cargo test --locked --offline --lib service::check`, `cargo test --locked --offline --lib service::document_refresher`, `cargo test --locked --offline --lib service::provenance`, and Task 3's integration commands. Amend obsolete command-level corruption expectations only where this user decision changes detection timing; keep direct strong pin tests.
- [ ] Mutation-check scope selection and full-audit-before-SQL ordering: routing whole check to weak refresh, routing reads to strong refresh, and removing full verification must each cause an appropriate test failure. Restore exact bytes and rerun green.
- [ ] Update X-1/X-3, help, trait docs, mechanics/decisions and the Letopis skill with the accepted verification boundary and no claim of globally certified integrity after routine reads. Add dated notes pointing to historical TK-26 evidence; preserve all old receipts and captured revisions. Commit intended paths only.

### Task 5: Verify the final tree and measure the result

**Files:** report/evidence, spec and this plan for completion notes; product paths only if new measured failures require a scoped correction.

**Interfaces:** the unchanged CLI requests from Task 1 and its fixed build/environment metadata.

- [ ] Run `make fmt`, `make check`, `make verify` against the final product tree; report exact results. Because the proposal changes a dependency, run `python3 -B tests/storage/run.py --platform host --evidence <new-absolute-directory>` as required by AGENTS.md; never overwrite old evidence. State Linux and actual Git 2.29 execution limits.
- [ ] Rebuild using Task 1's exact optimized settings and repeat all three requests three times against an equivalent warmed corpus/cache. Capture Rust samples and separately counted invocations again; compare result IDs, graph edges, hashes and provenance flags independently of changing snapshot OIDs/revision counters.
- [ ] Report medians, full measured ranges, subprocess counts and remaining stack hot spots. Evaluate the tenfold-reduction target and sub-second aspiration without claiming either before measurement. Measure full check/reindex separately: they retain exhaustive correctness work but should benefit from batching. Ordinary read latency should no longer scale with unrelated source-closure size. No ordinary test should fail on a wall-clock threshold.
- [ ] Review physical module files, visibility, imports, ordinary/SIGINT child lifetimes and `lib.rs` composition boundaries; a passing linter alone is insufficient.
- [ ] Close every item in the [invariant matrix](../../zkit/letopis/checks/read-workflow-performance/invariants.md) with actual test commands/results, or state the specific unverified limit. Never mark planned tests as passed based on the unchanged baseline.
- [ ] Obtain an independent final code review, resolve findings, rerun checks justified by any changes, record Engram findings/verification, and update the spec/plan/report with actual outcomes. Do not push, merge or publish without existing user authorization.

## Execution corrections from independent review

The old `hash-object -t tree` command checked structure as well as identity. Full pin now uses one cat-file batch plus one hash-object --stdin-paths batch over private transient tree files, preserving Git's exact strict format policy (including noncanonical-mode rejection) before refs/SQL. No subprocess per object. Tests independently compare decoder normalization with ls-tree, and separately prove strict nested-tree rejection through the public strong pin. The scratch directory owns 0700/0600 inputs, quotes arbitrary path bytes and removes them on all ordinary paths. This replaces the earlier incomplete assumption that ls-tree normalization alone defined accepted strong-pin input. See execution report for process counts, tests and limits.
