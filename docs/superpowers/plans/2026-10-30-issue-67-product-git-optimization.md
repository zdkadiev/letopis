# Product Git and Provenance Optimization Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Eliminate product Git subprocesses and repeated provenance work, preserving snapshot guarantees and publishing a measured before/after comparison.

**Architecture:** Implement the existing application-owned Git interfaces with a synchronous `GitRepository` backed by gix. Reuse independent hashing and closure traversal; group historical provenance observations within explicit operations while continuing fresh live-file observations. Keep technology types inside `src/git/` and wiring in `src/lib.rs`.

**Tech Stack:** Rust 1.95.0, existing SQLite/SeaQuery stack, proposed exact pins `gix =0.90.0` and `gix-dir =0.32.0`, Python 3.12, samply; Git remains an independent fixture/oracle tool.

**Spec:** [Approved specification](../specs/2026-10-30-issue-67-product-git-optimization-design.md).

**Status:** Approved for Subagent-driven execution by the user on 2026-10-30. Task completion and compatibility findings are tracked below and in the plan-scoped execution ledger.

## Global Constraints

- Base product revision: `32ca0af282a1321d0137b4aba115c6fddc086259`; branch `issue-67` already exists in the current isolated worktree. Preserve unrelated dirty issue-65 documentation and R-10; do not stash, reset, stage or commit them.
- R-10 became staged during execution and must remain outside task commits. Use `git commit --only -- <exact owned paths>` or a disposable alternate index; plain commit can include unrelated staged work even after scoped git add. Preserve unrelated index entries; guards must fail fast, never continue to commit after a failed ownership assertion.
- All product Git object calls must execute inside the Letopis process. No permanent CLI fallback, hidden helpers, filters, lazy fetch, network command or shell substitute.
- Preserve existing Git snapshot storage, full SHA-1/SHA-256 OIDs, historical bytes, exact direct refs, retention, public JSON and error classifications; no storage migration or reduced snapshot membership.
- Private retention refs do not create/append reflogs; existing logs and user refs remain untouched, including under core.logAllRefUpdates=always. This explicit contract-scope implementation choice avoids unrequired Git identity/DNS emulation and does not weaken durable retention.
- Keep synchronous I/O, per-invocation processes, existing locks, pure domain data and application-owned interfaces. No async runtime, daemon, parallel drain or speculative framework.
- Preserve root verification versus addressed reads versus `retain` versus full-closure `pin`. Existing refs never bypass full audits. Reuse the existing closure algorithm, independent hashing and gitlink/type-conflict rules.
- Approved resource revision (2026-10-30): current-object/delta decoding buffers may scale with object size. Release payloads between closure objects; no whole-closure payload cache. All integrity/durability requirements remain binding.
- Chosen initial settings: 64 KiB hash chunks; library object/delta payload caches disabled; retain a reusable decode allocation only up to 4 MiB, dropping larger buffers after the current object; historical-hash reuse capped at 4,096 entries per observation operation. Cache exhaustion recomputes instead of rejecting valid inputs. Buffer reuse threshold is not an input-size limit. Do not change settings without recording why and remeasuring.
- Follow [AGENTS.md](../../../AGENTS.md): responsibility modules, verb names, approximately 40 lines/60 maximum, explicit imports, safe typed errors, Given/When/Then tests, meaningful mutations, KISS/DRY/YAGNI. No unrelated generic/visibility refactor.
- Measure immutable old/new binaries on equivalent synthetic fixtures, seed 65, 2,000 bytes/fragment, 30 elements/file, disabled embeddings. At least 10 successful samples for feasible initialized check/inspect/traverse scenarios. Keep profiles/counters separate from ordinary timing.
- Target at least 50% lower 60-element check median; it is a goal, not a promised result or formal NFR. Report misses, failures and timeouts. No paid/provider/model sessions.
- `make verify` and affected host/Linux native/build checks apply to the final changed tree. Historical evidence stays immutable; fresh artifacts belong under `.artifacts/issue-67/`.
- Avoid duplicate unchanged-root runs: use `make verify` once for an accepted Rust tree when full verification is required, since it includes `make check`. Use focused red/green tests while iterating; new edits/failures can justify another root run. Do not run a separate unchanged `make check` immediately before the same `make verify` merely to satisfy both names.

## Review Focus

1. A shared historical source changes live between two observations: detect the second state; never reuse a path-only live hash (Task 6).
2. A large delta-packed blob is audited: count its permitted decoding buffers honestly, release oversized capacity between objects and never retain the closure payloads (Task 2).
3. An alternate/symbolic ref appears after repository opening or retention races GC: preserve operation-entry policy and exact direct-ref/CAS behavior (Tasks 4, 7).
4. An ignored tracked file or linked-worktree index differs from the common repository: enumerate the same eligible corpus without helper commands (Task 4).
5. An old binary is measured after current product inputs change, or optimized reindex ends too quickly to overlap: validate frozen provenance and label unavailable overlap rather than inventing a comparison (Tasks 1, 8).

---

## File ownership and dependency order

| Files | Responsibility / disposition |
| --- | --- |
| `tests/performance/compare.py`, `test_compare.py` (new) | Paired orchestration, independently frozen builds, validated comparison/evidence; reuse existing scenario, measurement, profile and contention helpers |
| `tests/performance/README.md` | Document comparison commands, budgets, metric scopes and reproducibility |
| `src/git/reader.rs` (new), `src/git/durability.rs` and `durability/tests.rs` (new), `src/git/hashing.rs` (move existing hashing), `src/git/tree.rs` (move existing parser); `tests/fixtures/git/sha-mbles-1.bin` and its `README.md` (new) | In-process audit transport; narrowly owned publication/sync capabilities; existing independent hashes/tree-child interpretation; independently sourced collision-detection fixture |
| `src/git/validation.rs`, `src/git/validation/tests.rs` (new test child) | Strict in-process tree validation; remove subprocess spool after differential parity |
| `src/git/repository.rs`, `src/git/repository/tests.rs`, `src/git/corpus.rs` (new) | Concrete repository implementation, safe open/policy, candidate enumeration |
| `src/git/{objects,refs,constants,mod}.rs` | Adapt current object/ref owners and closure walk; remove CLI command constants |
| `src/git/{cli,process,batch}.rs`, `src/git/cli/tests.rs`, obsolete batch protocol tests | Remove when their subprocess responsibilities are replaced; preserve/move behavioral tests, not fake pipe-framing implementation tests |
| `src/lib.rs`, `src/commands.rs` | Share command-owned repository state and preserve public constructors/CLI wiring |
| `src/service/{mod,provenance,search,check_problems}.rs`, `src/service/provenance/observations.rs` (new), `src/service/search/comparisons.rs` (new) | One batched observation method; private bounded historical reuse and demand-driven revision-comparison reuse |
| `src/service/document_refresher/basis.rs`, `src/service/traversal.rs` | Adopt ordered observation batches where multiple bindings are already available; no graph-wide preload |
| Existing Git/provenance/inspection/read-verification tests; new `tests/git_no_subprocess.rs` and `tests/git_streaming.rs` | Behavioral parity, no Git dependency at runtime, audit memory and cancellation |
| `Cargo.toml`, `Cargo.lock`, `tests/git/Dockerfile` | Pinned minimal local library features; Linux execution tracing/test prerequisites |
| `AGENTS.md`, `docs/development.md`, `docs/zkit/letopis/{decisions,architecture,mechanics}.md`, `contracts/X-3-git.md`, `tests/git/README.md` | Dated accepted-decision updates and reproducible operational instructions |
| `docs/zkit/letopis/checks/issue-67/{compatibility,report,review}.md`, `evidence-index.json` (new) | Gate evidence, before/after table, review findings and artifact identities |

Tasks execute in order. Tasks 3–8 depend on Task 2 proving the approved resource/integrity contract. Do not silently replace that gate with an ignored test. Each task is one testable commit; split its commit only when preserving a compilable, reviewable boundary requires it.

### Task 1: Build a trustworthy comparison harness and freeze the baseline

**Files:** Create `tests/performance/{compare,test_compare}.py`; modify `tests/performance/README.md`; reuse `scenarios.py`, `measure.py`, `report.py`, `profile.py`, `contention.py`; create issue-67 evidence/report skeleton.

**Interfaces:**
- Consume `scenarios.build_release(source: Path, target: Path, evidence: Path) -> dict`, `prepare_fixture(...)`, `prepare_scenario(...)`, `measure.measure_process(...)`, `report.validate_result(...)` without weakening their existing protections.
- Produce `freeze_build(source: Path, evidence: Path) -> dict`, `verify_frozen_build(pin: dict) -> None`, `run_comparison(args) -> dict`, `summarize_comparison(before: list[dict], after: list[dict]) -> dict` in `compare.py`.
- New CLI: `compare.py --stage timings|initialization|contention|profiles --before <build.json> --after <build.json> --nodes 60|1000 --scenarios <existing names> --pairs 10 --timeout 120 --initial-timeout 600 --evidence <absent absolute directory>`. Freeze CLI: `compare.py --freeze-build --source <repository> --evidence <absent absolute directory>`; no comparison flags required in freeze mode.

- [x] **Step 1 — Pin comparison failure modes with failing tests.** Add `test_compare.py` tests for frozen binary/source tampering, unequal fixtures, failed/censored attempts, alternating order, independent inspect oracle, output drift and non-reuse of evidence paths. Feed successful before samples `[2, 4, 6]` seconds and after samples `[1, 2, 3]` seconds, plus a timeout excluded from successful medians; assert:
  ```python
  # Then: independently known medians and speedup, never censored durations.
  self.assertEqual(summary["before_median_seconds"], 4.0)
  self.assertEqual(summary["after_median_seconds"], 2.0)
  self.assertEqual(summary["speedup"], 2.0)
  self.assertEqual(summary["reduction_percent"], 50.0)
  ```
- [x] **Step 2 — Run red.** `/opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p test_compare.py -q`; expect assertions/imports to fail before implementation. A missing interpreter is an environment prerequisite, not a test result; use an available verified Python 3.12+ instead.
- [x] **Step 3 — Implement frozen builds and paired orchestration.** Preserve `scenarios.verify_build` for its existing single-revision callers. A frozen receipt archives the exact clean product source files/hashes plus immutable executable/symbols and original release receipt. Verify the archived source manifest and executable independently of the current checkout before every measured call; current source changes must neither invalidate an authentic old receipt nor permit altered old sources. Do not accept an arbitrary executable with a hand-written receipt. Share the existing exclusive-output and owned-process cleanup routines. Define the schedule/budgets below; flush every attempt and retain failures.
- [x] **Step 4 — Run green and mutate.** Run the new unit suite; temporarily include one timeout in successful medians and separately skip binary verification; each relevant test must fail, then restore. Run existing performance tests with explicit integration skips identified; do not claim a real-binary suite when its required pins are absent.
- [x] **Step 5 — Capture the pre-change baseline.** Run `make check`; require success before attributing later failures. Confirm clean product inputs and base-equivalent product hashes, then freeze a release build with `CARGO_PROFILE_RELEASE_DEBUG=1` and the pinned toolchain. Save under a new `.artifacts/issue-67/before-<unique>/`; record actual source revision and product-tree equality to the stated base. Run the initialized 60-element check baseline (two warmups, 10 successes or documented budget exhaustion) and archive fixtures/receipts. Do not alter Cargo or product code before freezing this baseline. No need to time every scale until the after binary exists.
- [x] **Step 6 — Commit intended harness/report files.** `test(perf): freeze builds and compare optimization pairs`. Exclude raw artifacts and pre-existing issue-65/R-10 changes. Required baseline records are evidence-indexed, never checked in as mutable binaries.

### Task 2: Prove library resource behavior and durability

**Files:** Modify `Cargo.toml`, `Cargo.lock`, `src/git/mod.rs`; create `src/git/reader.rs`, `src/git/durability.rs`, `src/git/durability/tests.rs`, `tests/git_streaming.rs`, `tests/fixtures/git/{sha-mbles-1.bin,README.md}`, `docs/zkit/letopis/checks/issue-67/compatibility.md`; move `batch/hashing.rs` to `hashing.rs` and update existing batch imports while it still exists.

**Interfaces:**
- Candidate dependencies: `gix = { version = "=0.90.0", default-features = false, features = ["sha1", "sha256", "index", "excludes"] }`; `gix-dir = { version = "=0.32.0", default-features = false, features = ["sha1", "sha256"] }`.
- Private `ObjectReader::new(repo: &gix::Repository, oid_len: usize) -> Result<Self, SnapshotError>` and `read_object(&mut self, oid: &str, expected: &str, missing: SnapshotErrorKind, wrong_type: SnapshotErrorKind) -> Result<VerifiedObject, SnapshotError>`. The reader owns its controlled raw-store handle, so an unused borrowed lifetime is unnecessary. Preserve `VerifiedObject::{Blob, Tree(Vec<u8>)}`; audit blobs produce no retained body.
- Preserve `ObjectHasher::{new, update, finish}` behavior and signatures; the old transport may import it until removed.
- Until Task 4 consumes it, compile `reader.rs` only under `cfg(test)`. Gate tests run its real code through an owned private library-test child (the existing CLI test-driver pattern), not through `open_git_snapshots`, which still uses the old adapter at this stage. This avoids both testing the wrong transport and dead-code exceptions in the interim product. Task 4 removes that temporary module gate when production uses the reader.
- Execution clarification: keep narrowly scoped durable object/ref publication helpers and probes in `durability.rs`/`durability/tests.rs`, also test-gated until Task 4 consumes them; keep reader read-only. The resolved gix dependency graph requires the root development `tempfile` exact pin to advance from 3.23.0 to 3.27.0; record the actual lock/MSRV and run existing tests. An unconditional transitive command-related crate is not permission to invoke a helper: audit actual selected features/reachable paths and retain the zero-subprocess runtime tests.
- Collision fixture build-input clarification: add its COPY line to `tests/storage/Dockerfile`, `tests/git/Dockerfile`, `tests/cli/Dockerfile` and `tests/markdown/Dockerfile` together, since all compile the root library tests with include_bytes. This is one same-shape context fix; full relevant Linux/native acceptance remains Task 7 rather than four duplicate root-suite campaigns.

- [x] **Step 1 — Specify failing capability tests.** `audit_loose_and_delta_packed_objects_with_scoped_buffers` audits deterministic 1 MiB and 64 MiB blobs in both object formats, with caches disabled. Independently verify fixture pack entries include actual delta objects; `git gc` alone is not proof. Assert exact OIDs, corruption rejection and release of oversized reusable capacity (at most 4 MiB retained after an object); collect separate-process allocation/RSS receipts, including permitted delta working buffers. Include base and delta corruption, no fetch, and safe causes. For collision rejection, copy the 640-byte `tests/data/sha-mbles-1.bin` from the already locked `sha1-checked =0.10.0` package (SHA-256 `3ead211681cec93d265c8ac123dd062e105408cebf82fa6e2b126f4f40bcb88c`) with source/license provenance. A private hash unit test feeds that unprefixed attack input to `ObjectHasher::Sha1`'s underlying hasher and asserts `finish().is_err()`; this tests collision propagation, not a claim that prepending a Git header preserves an arbitrary published collision. Ordinary Git-header identity tests remain independent.
- [x] **Step 2 — Run red and check dependency source.** Existing code has no reader/gate, so tests fail. After plan approval only, resolve/fetch the exact dependencies, lock them, inspect `cargo tree -e features`, and compile with Rust 1.95.0. Do not enable gix defaults, `dirwalk`, `attributes`, command/network/credentials/interrupt or parallel bundles merely for convenience. Direct `gix-dir` plus index/excludes avoids the high-level `dirwalk → attributes → command` chain; confirm the resolved graph rather than relying on names.
- [x] **Step 3 — Implement the smallest viable object reader.** Use raw local library object APIs, independent hashing in 64 KiB chunks and safe error mapping. The approved revision permits full current-object/delta decode buffers; report their peak/capacity separately from caches. Drop oversized reusable buffers after each object, keeping no more than 4 MiB reusable capacity. Return only Blob or the needed Tree bytes to the closure walker; do not retain blob payloads. No custom pack/delta engine, disk spool, input-size cap or allocation exception is needed to manufacture constant-memory claims.
- [x] **Step 4 — Establish write/ref feasibility in the same gate.** Inspect the pinned library's object and ref write paths for sync/atomic publication, direct-ref parsing, no-dereference CAS and fsync coverage against current `core.fsync=all`/`fsync` behavior. Record concrete API/source pointers and the minimal adapter-owned file/directory syncs needed. Add disposable failure/lock/concurrent-create probes. Private retention refs intentionally do not create/append reflogs; independently verify existing logs/user refs are untouched under configured always/existing-log cases, without identity/DNS emulation. Physical power loss remains unassessed; API documentation alone cannot prove it. Unsupported required operations are explicit gate failures.
- [x] **Step 5 — Verify and record the gate.** Run `cargo test --locked --offline --test git_streaming` plus reader/hash tests; mutation-check digest comparison, oversized-buffer release and collision rejection, restore, then `make check`. In `compatibility.md`, record exact versions/features, large-object receipts, per-object/delta working allocations and write/ref findings. **Stop dependent tasks if unsupported:** report the smallest unresolved contract, with evidence. Do not add a CLI fallback, weaken remaining requirements or declare ignored cases passed.
- [x] **Step 6 — Commit passing gate or an honest blocked-gate record.** `test(git): establish in-process library compatibility`. Do not continue migration on an unresolved gate; communicate whether the approved spec requires revision.

### Task 3: Replace tree structural validation while preserving exact behavior

**Files:** Modify `src/git/validation.rs`, `src/git/refs.rs`, `src/git/mod.rs`; create `src/git/validation/tests.rs`; move `batch/tree.rs` to `tree.rs` and update imports.

**Interfaces:**
- Produce private `validate_tree_bytes(bytes: &[u8], oid_len: usize) -> Result<(), SnapshotError>` in `validation.rs`.
- Retain `read_children(bytes: &[u8], oid_len: usize) -> Result<Vec<(String, String)>, SnapshotError>` and existing Git mode-normalization/gitlink semantics in `tree.rs`.
- `verify_closure` remains the only closure traversal owner; this task changes its validator, not its algorithm.

- [x] **Step 1 — Write differential tests against the old oracle.** Pin valid/raw-mode trees plus truncated OIDs/entries, invalid octal modes, empty/slash/dot/dotdot/reserved names, embedded NUL boundaries, duplicate/out-of-order entries, and SHA-1/SHA-256 widths. Obtain acceptance/error behavior from the exact old `hash-object -t tree --no-filters` structural operation, not a stronger `fsck --strict` substituted as ground truth:
  ```rust
  // Then: acceptance agrees with the old operation for each literal raw fixture.
  assert_eq!(validate_tree_bytes(&bytes, width).is_ok(), oracle_accepts);
  ```
- [x] **Step 2 — Run red.** `cargo test --locked --offline --lib git::validation`; require failure due to the missing new validator before implementing it. Oracle setup uses disposable Git repositories only.
- [x] **Step 3 — Implement strict parity and adapt closure validation.** Validate raw bytes without a process or normalizing their hashed identity; reuse the existing child parser where its semantics fit, and retain all errors instead of filtering bad entries out of iterators. Remove the old tree-path spool and `TreeValidator` once no caller remains. Do not change acceptance merely because a convenience decoder is stricter or more permissive.
- [x] **Step 4 — Run green and mutate.** Run validation/parser tests and `cargo test --locked --offline --test git_retention --test git_corruption --test snapshot_verification`. Temporarily omit a required structural rejection and confirm its independent fixture catches it; restore. Ensure existing normalized-mode fixtures retain their expected results.
- [x] **Step 5 — Commit.** `refactor(git): validate snapshot trees in process`.

### Task 4: Migrate discovery, listing, objects and refs to the library

**Files:** Create `src/git/{repository,corpus,cleanup,error}.rs`, `src/git/repository/tests.rs`, `src/git/cleanup/tests.rs`; modify `src/git/{objects,refs,reader,durability,constants,mod}.rs`, `src/lib.rs`, `src/commands.rs`, existing internal test imports; remove `cli.rs`, `cli/tests.rs`, `process.rs`, `batch.rs` and obsolete transport-only tests after moving behavior coverage. Move required tempfile to normal dependencies if the gated writer is reused; retain its exact locked version. Process/CLI adapters retain their existing generic callback/response interfaces.

**Interfaces:**
- `GitRepository::open(start: &Path) -> Result<Self, SnapshotError>` and `get_root(&self) -> &Path`; a cheap `Clone` shares `Rc<gix::Repository>` within a single synchronous command, with owned discovered paths/format. No `Arc`, mutex or global state.
- Implement existing `GitSnapshots`, `SnapshotVerifier`, `CorpusLister` signatures unchanged. `ObjectDb<'a>` borrows `GitRepository`; existing object and retention entry points keep their names/return types. Library types remain private to Git.
- Private `corpus::list_candidates(repo: &gix::Repository, root: &Path) -> DocumentResult<Vec<RepoPath>>`; `GitRepository::require_self_contained_objects(&self) -> Result<(), SnapshotError>` rechecks the resolved common-store alternates path at every existing policy boundary.
- Required adoption handoff from Task 2: `GitRepository::open_with_cleanup(start: &Path, cleanup: Arc<GitWriteCleanup>) -> Result<Self, SnapshotError>` shares one command-owned Git resource owner; public/default constructors remain independently usable. `GitWriteCleanup::new() -> Self`, `cancel_and_cleanup(&self) -> Result<(), SnapshotError>` and `is_cancelled(&self) -> bool` live inside Git and own actual temporary-directory/file/marker handles, without capturing a non-Send Repository. Preserve the process adapter's existing `Box<dyn FnOnce() + Send>` callback and once-response writer; compose cleanup in lib.rs, emitting/claiming cancellation before exposing its terminal state. No new service trait, runtime, global mutable cache or generic factory is needed.
- Public concurrent exact-target retention remains required. Use Task 2's typed `is_ref_lock_busy` distinction for documented bounded lock retries through the entire durable protocol; never retry arbitrary sync/integrity failures or restore an equality-only success shortcut. Test-only retry bounds and benchmark watchdogs are not product command deadlines.

- [ ] **Step 1 — Add runtime-independent parity tests that fail on the CLI adapter.** After fixture setup, poison/empty Git's executable path and use old snapshots through all snapshot operations. Extend discovery/listing coverage for linked worktrees, staged deletion, tracked ignored files, ignored/untracked Markdown, nested ignores/negation, global/info excludes, directories/submodules/symlinks, non-UTF-8 boundaries and hostile `GIT_*` routing. Candidate assertions compare independently to Git's exact fixture list, not to the new walker.
- [ ] **Step 2 — Add object/ref regression cases and run red.** Extend existing snapshot/retention tests for packed objects/refs, exact bytes without filters/EOL normalization, path preflight before any write, foreign format/type/missing versus corruption, dangling symbolic refs, identical-target races, conflicting targets, `.lock` failures and late alternates. Save HEAD/index/worktree state and assert unchanged. Existing tests should fail only on the newly required child-free behavior, not be disabled.
- [ ] **Step 3 — Implement the concrete repository and listing.** Discover from the explicit start path using restricted open/config options; preserve current environment-isolation and untrusted-root behavior. Use the actual linked-worktree index plus gix-dir/exclude machinery, avoiding checkout/filter APIs. Cache resolved paths/format, not permanent policy success. Keep payload caches disabled. Do not change process-global environment to sanitize library calls.
- [ ] **Step 4 — Adapt objects and refs.** Read/write raw Git objects, independently hash returned/written identities, enforce all-request path validation before writes, sort serialized tree entries by Git's byte/mode ordering, and sync/publish as proven in Task 2. For `create_tree_from_blobs`, verify each distinct supplied blob once per call; arbitrary caller IDs remain untrusted. Reuse the existing closure traversal with `ObjectReader` and Task 3's validator. Full pins always re-read/revalidate; no success cache bypasses a new audit. Preserve exact direct-ref compare/create/recheck semantics.
- [ ] **Step 5 — Establish handled-SIGINT ownership before product adoption.** Add a command-owned actual-resource owner with narrow create/register and publish/unregister gates. Share it across every writing Git adapter in that invocation, including nested refreshers; preserve standalone public constructors and ordinary RAII release. Do not hold its mutex across compression/hashing/fsync or rely on Drop after process::exit. The source-backed proposal detaches an owned stage onto a fresh owned sibling before recursive removal (retry DirectoryNotEmpty, NotFound succeeds), and clones writable ref descriptors under the gate so write/fsync can run outside it while the actual gix lock remains cancellable. Cover writable ref, postpublication ref and packed-ref markers; never unlink by a remembered foreign .lock name. The pinned loose writer cannot recreate a detached outer directory, but already-entered namespace syscalls require non-quiescent tests. A simpler implementation is welcome only if it proves the same ownership/race properties without a new store/pack engine. Detach/remove failure must retain ownership, clean other resources, preserve one cancelled response/exit1 and disclose residual cleanup limits; do not claim arbitrary filesystem failures are leak-free. Add focused owned-child tests now; Task 7 broadens platform/error coverage. A global gix registry hook alone is insufficient.
- [ ] **Step 6 — Switch imports and remove the old transport.** Mechanically replace concrete construction with `GitRepository::open` or its private cleanup-aware constructor in composition/command wiring and internal tests; retain public `open_git_snapshots` signatures. Update touched public API/module comments with the behavior change. Remove version subprocess gates and CLI-only constants/tests. Move semantic tests to the new owner. Update existing tests that asserted pipe framing or exact command counts to assert integrity/behavior instead; do not retain a second product transport or closure verifier.
- [ ] **Step 7 — Verify and mutate.** Run `cargo test --locked --offline --test git_snapshots --test git_retention --test git_corruption --test snapshot_verification --test read_verification`, plus Git unit tests and focused non-quiescent cancellation cases; require all relevant parents/child fixtures executed. Mutations: skip checksum comparison, accept a symbolic ref, bypass late-alternates policy or lose owned-resource registration/cleanup; independent tests must fail. Restore, then `make check` and required current-tree verification.
- [ ] **Step 8 — Commit.** `perf(git): replace product subprocesses with in-process repository access`.

### Task 5: Share repository state across command composition

**Files:** Modify `src/lib.rs`, `src/commands.rs`, `src/git/repository/tests.rs`; update focused construction tests only.

**Interfaces:**
- Keep all public `wire_*`/`open_*` signatures and `commands::run_request` unchanged.
- Private constructors in `lib.rs`: `wire_document_refresher_with_repository(git: GitRepository) -> DocumentResult<impl Refresher + use<>>`, `wire_source_provenance_with_repository(git: GitRepository) -> DocumentResult<impl SourceProvenance + use<>>`, and `wire_embedding_executor_at_root(root: &Path) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>>`. Their bodies perform real existing wiring; no wrapper argument bags/factory layer.
- Add private same-pattern helpers only for commands that currently reopen the repository: public entry constructors open once, collaborators receive cheap shared-handle clones or the already resolved root.

- [ ] **Step 1 — Write a construction-count test.** A test-only counter counts actual library repository opens, not `Clone`. For check/search/inspect/traverse/config/status/init/direct drain assert exactly one repository open per command; a second independent invocation opens again. Include the source/provenance/refresher collaborators that currently reopen it.
- [ ] **Step 2 — Run red.** `cargo test --locked --offline --lib repository_open`; expect repeated-open counts on the migrated but not yet rewired composition.
- [ ] **Step 3 — Rewire through the existing composition root.** Implement private helpers above; use concrete `GitRepository` clones sharing one library handle. Do not put service orchestration into Git or environment access into CLI parsing. Independent public constructor calls each own their own state; no cross-invocation cache. Avoid adding service interfaces or generic parameter lists for this wiring.
- [ ] **Step 4 — Verify and mutate.** Run construction tests and `cargo test --locked --offline --test cli_process --test cli_reading --test cli_traversal --test init_command --test config_command`. Change one clone back to `open`, confirm the count test fails, restore; run `make check`.
- [ ] **Step 5 — Commit.** `perf: reuse command-owned repository state`.

### Task 6: Deduplicate historical provenance within explicit observations

**Files:** Modify `src/service/{mod,provenance,search,check_problems}.rs`, `src/service/document_refresher/basis.rs`, `src/service/traversal.rs`; create private `src/service/provenance/observations.rs`, `src/service/search/comparisons.rs`; extend provenance/service tests.

**Interfaces:**
- Add one method to the existing `SourceProvenance` trait: `fn observe_sources_batch(&self, requests: &[ProvenanceRequest<'_>]) -> DocumentResult<Vec<ProvenanceObservation>>`. Contract: results match input order and one-result-per-request, stop at the same first demanded error as sequential observation, read-only, no persistent cache. Default body loops through `observe_sources` so existing implementations remain compatible; concrete `ProvenanceService` overrides it. This batch has a concrete shared-work use case, not a generic cache API.
- Private `HistoricalObservations::new() -> Self`, `observe_sources<G: GitSnapshots + SnapshotVerifier, F: FileSystem>(&mut self, provenance: &ProvenanceService<G, F>, request: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation>` in `observations.rs`, using the existing `G`/`F` bounds only. State: capped verified-available roots and `(TreeOid, SourceUri) → Option<Sha256>` maps, total at most 4,096 entries; no byte cache, cached infrastructure failure or permanent missing/invalid-root classification. Missing/invalid roots are reclassified when demanded so an object appearing later cannot be hidden by a negative cache.
- Existing singleton `observe_sources` creates fresh state for its one call. Each batch owns fresh state and drops it on return/error. `retain_snapshots`, `verify_snapshots` and full pins never use that cache.
- Private `RevisionComparisons<'a>::new(provenance: &'a dyn SourceProvenance) -> Self` and `compare_revision(&mut self, request: SourceRevisionRequest<'_>) -> Result<SourceRevisionComparison, SnapshotError>` in `search/comparisons.rs`: cap 4,096 results keyed by exact optional basis, source/range and requested tree. Lifetime: one search's eligibility computation, after the existing unconditional requested-root validation. No errors cached, no eager comparisons beyond existing short-circuit order.

- [ ] **Step 1 — Add failing independent call-count tests.** Two knowledge bindings sharing one exact tree/source in a batch return equal observations with one historical root/hash lookup but two live-file reads. A second batch/singleton reads again. For separate ranges, verify literal expected hashes/flags and group historical file reads only if exact range results can be calculated without retaining bodies. Exact identical-range reuse is mandatory; cross-range reuse is optional only when it remains simple and bounded.
- [ ] **Step 2 — Pin freshness, errors and limits.** Tests `batch_observation_detects_edit_between_live_reads`, `batch_preserves_first_error_and_result_order`, `batch_does_not_cache_policy_or_io_failure`, `batch_releases_state_between_operations`, `historical_cache_overflow_recomputes_without_rejecting`, and `revision_cache_keeps_demand_order_and_resets_between_searches` assert independent flags, read counts and error kinds. Use recorded collaborators with literal byte changes, not the implementation as an oracle. Run `cargo test --locked --offline --lib provenance`; expect failure before batching.
- [ ] **Step 3 — Implement bounded historical reuse.** Process requests/sources in the same order as existing calls; lazily cache only verified immutable classifications/hashes in that batch. Call existing confined filesystem capture for every live observation, preserving missing/range behavior and `build_observation` semantics. If either map cannot accept another entry, compute normally without insertion. No `RefCell` cache on a reusable service and no global cache/reset hook. Preserve all verification/retention policy boundaries.
- [ ] **Step 4 — Adopt batches where metadata already exists.** `BasisPreflight::validate_basis` and check/status problem collection can collect request slices from their existing pinned metadata. Bound new requests to chunks of 256, keep owner-to-result mapping and current diagnostic ordering. Search batches only demanded post-source-filter observations and scopes revision-comparison reuse to `read_eligible_links`; preserve requested-root validation before inference/early exits and filter short circuits. Traversal batches at most 256 already selected knowledge bindings; do not fetch graph-wide metadata or change BFS/page limits. Document defaults for test doubles and prove single/batch equivalence with a live file changing between calls.
- [ ] **Step 5 — Verify and mutate.** Run `cargo test --locked --offline --test provenance_observation --test provenance_revisions --test provenance_indexing --test provenance_commands --test search_filters --test source_inspection --test read_verification`, affected unit tests and `make check`. Mutate a cache key to omit tree/range and separately reuse a live hash; tests must fail. Restore and report counted historical-work reduction separately from release timing.
- [ ] **Step 6 — Commit.** `perf(provenance): reuse historical checks within observation batches`.

### Task 7: Prove child-free product behavior and preserve inspect/operational contracts

**Files:** Create `tests/git_no_subprocess.rs`; extend `tests/{knowledge_inspection,source_inspection,read_verification,cli_cancellation,cancellation_concurrency}.rs`, relevant Git tests; update `tests/git/Dockerfile`, `tests/git/README.md`, accepted-design/development/agent documents from the file map. Modify `src/service/inspection.rs` only if these tests expose a remaining redundant operation that can be removed without broadening its interfaces.

**Interfaces:** Public commands/snapshot interfaces remain unchanged; no new user flags, Graph dependency, direct-blob service method or capture framework. Inspect may keep its necessary provisional corpus tree to read a previously captured marked source. The unique-OID assembly check from Task 4 removes redundant per-entry verification while public requests and final strong pin still validate independently.

Carry-forward Task-2 review M1: in `tests/git_streaming.rs`, keep the real delta
assertion separately for each 1 MiB/64 MiB pair and both formats. Archived gate
instrumentation proves current fixtures, but an any-delta assertion alone must
not silently reduce future large-object coverage. Resolve and mutation-check
this small maintained-test strengthening alongside the relevant parity tests.

- [ ] **Step 1 — Write child-denial command matrix tests.** Build fixtures with real Git before entering the measured/denied region. Invoke the absolute product binary with Git unavailable and a poison executable that records any attempt. Exercise init/config, check/status, FTS search, inspect, traverse, refresh/reindex and direct drain, plus corrupted objects, missing IDs, outside/bare repositories, malformed config and late alternates. Assert correct literal outcomes and empty trap log. Build an owned fake-provider fixture for a required nonempty drain path only; do not contact a real model. Help/version require no repository access.
- [ ] **Step 2 — Audit paths beyond PATH trapping.** Linux verification traces the product and all descendants with `strace -ff -e trace=process`; fixture/oracle processes run outside the traced interval. Assert only expected Letopis foreground/detached-process execution and no Git, shell, helper, filter or network subprocess. Use a network-denied container for the model-free matrix and add `strace` to its declared build tools. Audit `src/` process spawn sites, resolved gix feature paths and command/helper call sites; preserve the valid existing detached-drain behavior. No claim of absolute child absence from PATH trapping alone.
- [ ] **Step 3 — Pin inspect/cancellation parity and run red where newly uncovered.** Assert an unrequested marked file and declared local source are still in the retained tree, paths/ranges/hashes match independent bytes, marked-source mutation after capture reads its captured blob, and any unknown ID fails the whole request. Count repeated supplied blob OIDs once per assembly, but prove corrupt caller-supplied OIDs and final closure damage still fail. Replace obsolete tests waiting for a Git subprocess with an acknowledged in-process object-read/audit boundary, using the repository's existing private `cfg(test)` driver/checkpoint pattern; no product environment failpoints. Exercise one cancelled foreground envelope and released locks/no leaked spools. Do not satisfy them with sleeps or by removing the command from coverage.
- [ ] **Step 4 — Make only necessary product/test fixes and update current docs.** Dated D-96/X-3/architecture/mechanics/D-105 updates preserve historical evidence and the strong/weak verification distinction. Change the AGENTS Git-process sentence to the local-library boundary; development/deployment docs distinguish runtime independence from Git needed for author workflows/fixtures. Do not rewrite old proof receipts or pretend power-loss/x86_64 is assessed.
- [ ] **Step 5 — Run the integrated gates.** `make verify`; run host native standalone verification with `python3 -B tests/storage/run.py --platform host --evidence <new absolute directory>` using the selected Python. Build/run `tests/git/Dockerfile` with the updated lockfile and explicit copying of the new Git collision fixtures; run the relevant Linux native runner as build/linkage changed. Record exact commands/image IDs/platforms and skipped/unavailable cases. Mutation-check child denial by temporarily restoring a Git spawn, and inspect oracle by omitting an unrequested capture member; restore before verification. Check local documentation destinations and `git diff --check`.
- [ ] **Step 6 — Commit.** `test: verify child-free Git access and snapshot parity`.

### Task 8: Measure optimized performance and complete independent review

**Files:** `tests/performance/{compare,test_compare}.py` only if campaign defects require changes; `docs/zkit/letopis/checks/issue-67/{report,review,compatibility}.md`, `evidence-index.json`; approved spec/plan completion annotations.

**Interfaces:** Task 1's freeze/comparison commands, existing samply profile/owned contention helpers, and this exact final-table schema:
`scenario | before median/n | after median/n | reduction/speedup | Git subprocesses before→after | peak memory before→after/scope | correctness/limits`.

- [ ] **Step 1 — Freeze the optimized build.** Require a committed clean product tree and passing Task 7 checks; capture actual optimized revision/source manifests, executable hash and symbols with the same release/debug/toolchain settings as Task 1. Verify baseline archives again. Harness-only later edits require a pinned harness identity and its tests, not pretending the binary changed. A product fix invalidates affected after samples and final checks; rebuild and rerun only affected evidence into new paths.
- [ ] **Step 2 — Run ordinary paired controls.** Use this task's baseline and optimized build receipts, fresh equivalent isolated fixture copies and two unmeasured warmups per side. Alternate before/after order for 10 pairs of initialized check at 60, inspect/traverse at 1k and FTS search at 1k. For ordinary series, each side uses a stable independent initialized repository across repetitions; sample setup refreshes copied file identities outside timing and records revision counters so copied inodes do not become hidden measured refreshes. Mutation/publication samples use a fresh equivalent copy per attempt. Do not strip external declarations or reduce membership. Append the validated result before the next call.
- [ ] **Step 3 — Run bounded initialization/audit/contention probes.** Per ordinary call: 120 s; per initialization: up to 600 s explicitly recorded; stage watchdog: 1,200 s. Retry each censored scenario at most once after resolving a specific setup/product defect, not repeatedly to select a favorable result. Collect 1k first-publication separately (`n=1` initially) and full check/reindex/status/refresh-bulk with three attempts per feasible side; report censored counts. Run three reindex/reader schedules per side at 60, using kernel-lock acknowledgement and lifetime overlap receipts. If optimization removes observable overlap, report `not assessed at this workload` rather than a zero wait or a fabricated slowdown. Do not add arbitrary product sleeps. Larger scales are optional after 1k viability and a stated new budget.
- [ ] **Step 4 — Run separate counters/profiles/memory evidence.** Count historical baseline Git calls in separate wrapper runs; prove optimized zero with Task 7 and a separate counter run. Use samply save-only at 1,000 Hz, no browser/upload, three alternating unprofiled/profiled pairs for feasible check/inspect/traverse. Keep wrapper CPU/wall overhead outside ordinary samples. Record direct CLI peak RSS with an owned-PID sampler where available and OS wait4 lifetime RSS/accounted-descendant scope separately; neither is whole-stack concurrent peak. Keep sampling limits explicit and use Task 2's allocation/capacity assertions for per-object buffer-release claims. Include recorded unit-level object/hash/source counters as such; do not relabel mock/cfg(test) counts as release workload measurements. Explain remaining profile/lock bottlenecks with the strongest available evidence.
- [ ] **Step 5 — Populate the before/after table and acceptance ledger.** Use the spec's six scenario rows; include full successful/censored counts, median plus min/max, ratios, correctness results, source/config/build/host identities and exact commands. Emit a numeric reduction only from comparable successful observations. Report whether the 50% check goal was met. Do not compute p95 from 10 samples, subtract Python child timings, transfer inclusive CPU shares to wall time, or infer a new formal NFR verdict. Hash/archive raw receipts, profiles, frozen sources and runner scripts without checking binaries/raw payloads into Git. Preserve all issue-65 evidence.
- [ ] **Step 6 — Complete branch review and fix material findings.** Review against the approved spec, issue checklist and AGENTS boundaries; request an independent reviewer using `superpowers:requesting-code-review` with exact base/head, commands, unrun checks and artifact index. No silent model override. Record findings and resolutions in `review.md`; any product fix triggers affected tests/mutations and a fresh `make verify`, plus affected paired measurements. Report unavailable external review/platform verification honestly. The writing-plan self-review itself is not delegated.
- [ ] **Step 7 — Commit final report and intended fixes.** `docs(perf): report Git and provenance optimization results`. Summarize completed versus blocked criteria and remaining bottlenecks. Do not close/update GitHub, create a PR, merge or push until the user authorizes those actions for this implementation.

## Commands and campaign layout

All paths below are examples of new, absent outputs. Resolve the repository root
and selected Python interpreter once; do not reuse mutable `target/release/letopis`
as a measured executable.

```sh
# Task 1, before product changes (resolved absolute paths required by the runner).
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py \
  --freeze-build --source "$PWD" --evidence "$PWD/.artifacts/issue-67/before-01"

# Task 8, after final product verification/commit.
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py \
  --freeze-build --source "$PWD" --evidence "$PWD/.artifacts/issue-67/after-01"
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --stage timings \
  --before "$PWD/.artifacts/issue-67/before-01/build.json" \
  --after "$PWD/.artifacts/issue-67/after-01/build.json" \
  --nodes 60 --scenarios check --pairs 10 --timeout 120 \
  --evidence "$PWD/.artifacts/issue-67/check-60-01"
/opt/homebrew/bin/python3.12 -B tests/performance/compare.py --stage timings \
  --before "$PWD/.artifacts/issue-67/before-01/build.json" \
  --after "$PWD/.artifacts/issue-67/after-01/build.json" \
  --nodes 1000 --scenarios inspect traverse search --pairs 10 --timeout 120 \
  --evidence "$PWD/.artifacts/issue-67/reads-1k-01"
```

The same build flags select separate `initialization`, `contention` and `profiles`
stages, each with a new evidence directory. CLI rejects invalid stage/scenario
combinations rather than silently ignoring options. Existing scenario result
oracles, projection observers and cleanup helpers are reused; do not create a
second general-purpose benchmarking framework.

## Coverage and self-review

| Approved requirement | Task |
| --- | --- |
| Approved direction, pinned local features, version/platform gate | 2, 7 |
| Discovery, worktree/index/ignore fidelity, object/hash parity, direct refs | 2–4 |
| Existing closure/hash/parser reuse, strict tree parity, scoped per-object buffers/bounded caches | 2–4 |
| One repository handle, real lib.rs constructor wiring and component boundaries | 4–5 |
| Bounded historical grouping, live freshness, revision filters and errors | 6 |
| Complete inspect capture, independent caller validation/final strong pin | 4, 7 |
| Zero product Git subprocesses, safe cancellation/offline/helper behavior | 4, 7 |
| Immutable baseline, paired results/table, memory/profiles/contention | 1, 8 |
| Mutations, host/Linux checks, independent final review, honest limits | 2–8 |
| Dated design/docs updates; preserved old evidence/unrelated work | 1, 7–8 |

Self-review: specification coverage mapped above; task interfaces use existing
types except the one concrete batch operation and private helpers documented here;
the five review risks have owning tests. Remaining unknowns are explicit early
compatibility gates, not implied promises or authorization to weaken the spec.
Implementation is conditional on plan approval and a passing Task 2 gate.

Planning sources checked read-only: the pinned [gix feature manifest](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix/Cargo.toml)
and [gix-dir manifest](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-dir/Cargo.toml),
current repository interfaces/tests/harness, and the locally cached locked
`sha1-checked 0.10.0` test fixture. These establish plan inputs, not a passing
library compilation, streaming/durability gate or measured optimization result.
