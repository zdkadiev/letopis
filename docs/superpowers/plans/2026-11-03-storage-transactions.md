# Shared storage transactions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Replace the current storage components with CacheStorage and VectorStorage, supporting transaction closures across services through shared storage instances.

**Architecture:** Application-owned generic interfaces expose zero-argument closure scopes and ordinary storage methods. Rc shares each required instance; rusqlite guards and private transaction state provide joining, rollback and pinned reads. Filesystem checks move to filesystem, coordinated with database open/attach in lib.rs.

**Tech Stack:** Synchronous Rust, pinned rusqlite 0.40.2, SeaQuery 1.0.2, bundled SQLite and static vec1; no new dependencies.

**Spec:** [Approved design](../specs/2026-11-03-storage-transactions-design.md).

## Global Constraints

- Final storage implementation has exactly two concrete components: CacheStorage and VectorStorage.
- Closures take no transaction argument; services use their injected storage.
- An active compatible scope is joined; only the outer owner commits or rolls back.
- An inner scope error or failed database write marks the transaction rollback-only, even if caught.
- Shared ownership is Rc; no locks, async runtime, thread-local transaction context or process-global registry.
- Preserve read snapshots, vector identity checks, status observation-only behavior, filesystem protections and database schemas.
- Each changed test uses described Given/When/Then phases and a hand mutation check.
- Use branch rafactor. Preserve uncommitted src/embedding/http.rs and src/service/init.rs edits; task changes in init.rs must retain the user's changes.
- Check disk capacity before builds: earlier make verify failed with ENOSPC. Report inability to verify; do not delete user files.
- Old implementations may temporarily coexist while consumers migrate; no replacement compatibility wrapper structs are allowed, and all old storage components must be removed before final acceptance.

## Review Focus

1. Caught inner panic must poison the outer transaction and clear state after rollback (Task 1).
2. A read nested inside a write must see uncommitted writes; a write nested inside a read must fail without upgrading it (Task 1).
3. An incompatible nested vector identity skips its callback without discarding compatible outer work (Task 1).
4. A concurrent publication immediately after commit must not replace the returned publication revision (Task 4).
5. Replacing a directory or database leaf during open/attach must still be detected after moving checks into filesystem (Task 2).

## File structure

- Create src/storage/cache.rs and src/storage/vector.rs: each owns its concrete struct, constructors, scope implementation, operation trait impls and inline tests.
- Existing src/storage/read.rs, cache_write.rs and vector_write.rs become unnecessary after migration; move their methods to the two owners, retaining domain-specific query helpers in files.rs, graph.rs, fts.rs, neighbors.rs, vectors.rs, work.rs and status_counts.rs.
- src/storage/connection.rs retains SQLite flags, URI encoding, configuration and native capabilities, without directory inspection.
- Create src/filesystem/database.rs for filesystem-owned database-path observations and verification, with inline tests. No SQLite dependency enters this module.
- src/service/mod.rs owns all interfaces; service/error.rs owns TransactionError<E>; service/types.rs owns any shared pure values.
- src/lib.rs coordinates opening and injects shared storage; commands.rs retains command selection.
- Affected services remain flat; small existing helper receivers may receive the shared storage as a field rather than passing a transaction through helper/service methods.

## Interface decisions

Define TransactionError<E> in service/error.rs with Operation(E) and Storage(StorageError), Debug, Display and Error implementations that retain safe causes. Preserve the callback's error classification when mapping this value at service boundaries.

Define CacheDatabase in service/mod.rs. It includes existing cache operation roles (StorageRead, GraphRead, CacheWrite), whose methods use &self; remove commit from CacheWrite once migrated. Its scope methods are:

```rust,ignore
fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
where F: FnOnce() -> Result<T, E>;
fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
where F: FnOnce() -> Result<T, E>;
fn with_status_read_tx<T, E, F>(&self, operation: F)
    -> Result<Option<T>, TransactionError<E>>
where F: FnOnce() -> Result<T, E>;
fn get_publication_revision(&self) -> StorageResult<u64>;
```

get_publication_revision returns the pending revision inside the owning cache write. It is provisional until outer commit. Change formerly infallible get_revision/get_config_identity accessors to StorageResult values so operations outside a scope can fail explicitly.

Define VectorDatabase with VectorWrite operations using &self (without commit), and a VectorRead role containing only the existing vector-state reads needed by the embedding executor. Its scope methods return Result<VectorOutcome<T>, TransactionError<E>> and accept expected: &Sha256 before the zero-argument callback. Supply both with_read_tx and with_write_tx with that shape. reset_vectors retains its existing observed/desired arguments and VectorOutcome<bool> behavior and owns its own atomic scope; reject reset during an incompatible active transaction.

CacheDatabase's StorageRead retains vector reads needed for pinned joined queries; they execute on the cache view's attached vector database. VectorRead uses the main vector schema. Do not route pinned search reads through the independent writer.

Services become generic only over required non-object-safe storage roles and hold Rc<C>/Rc<V>. Other injected interfaces retain their current dispatch. GraphTraversal::traverse becomes traverse(&self, request: &TraversalRequest) -> CommandResult<Graph>; GraphService<C> holds Rc<C> and joins a read scope. Services never accept ReadTransaction or rusqlite objects as arguments.

## Task 1: Implement closure scopes on the two storage owners

**Files:** create src/storage/cache.rs, src/storage/vector.rs; modify src/storage/mod.rs, src/service/mod.rs, src/service/error.rs; reuse queries in src/storage/{read,cache_write,vector_write,connection,vectors}.rs.

**Consumes:** pinned rusqlite guard API and existing SQL helpers.
**Produces:** CacheDatabase, VectorDatabase, VectorRead, TransactionError<E>, CacheStorage and VectorStorage with real scope/operation implementations. Keep existing storage entry points compiling until migration is complete.

- [x] Write inline tests using real temporary SQLite databases and a separate observing connection: standalone_b_commits, a_and_b_join_without_inner_commit, outer_error_rolls_back_b, caught_inner_error_prevents_commit, caught_sql_error_prevents_commit. Assert literal row counts/revisions; observer sees no new rows before outer commit.
- [x] Run cargo test --locked --offline storage::cache::tests and the corresponding vector module test filter. Confirm new tests fail for the missing behavior before implementing it.
- [x] Implement the exact interfaces above. Outermost scopes use Transaction::new_unchecked with Deferred reads or Immediate writes. Store only necessary mode/revision/identity/failed state in Cell values; joined scopes never issue BEGIN/COMMIT/SAVEPOINT.
- [x] Implement database operation traits on the two owners through their active connection. Writes outside a write scope fail; failed writes poison it. Ordinary reads outside a compatible scope fail. Increment project revision only at the successful outer cache commit.
- [x] Add nested_read_sees_pending_write, write_cannot_upgrade_read, panic_rolls_back_and_allows_reuse, caught_nested_panic_prevents_commit, failed_cleanup_disables_connection, identity_mismatch_skips_callback, incompatible_nested_identity_keeps_outer_work tests. Use a real SQLite commit/rollback fault (e.g. an authorizer or a scoped SQLite hook) for cleanup tests; do not assert mocked calls alone.
- [x] Implement unwind cleanup with catch_unwind/resume_unwind and the library guard. Preserve original operation errors; report finalization failures as storage errors when no operation error exists. Verify autocommit state before reuse after cleanup.
- [x] Re-run tests, then temporarily remove poisoning, force an inner commit and invert the identity check one at a time. Confirm the appropriate tests fail, restore the code and re-run. Record mutations in the execution report.
- [x] Commit only task paths: "refactor: add shared storage transaction scopes".

## Task 2: Move path policy into filesystem and assemble database owners

**Files:** create src/filesystem/database.rs; modify src/filesystem/{mod,local}.rs, src/storage/connection.rs, src/storage/{cache,vector}.rs, src/lib.rs; migrate src/storage/connection_tests.rs and connection_policy_tests.rs.

**Consumes:** Task 1 constructors and existing ConfinedDirectory race tests.
**Produces:** filesystem-owned observation/verification APIs and composition helpers that check before/after each database open or attachment. The exact observation representation stays private to filesystem, exposed crate-wide only to composition.

- [x] Inventory the current open/attach tests and write equivalent filesystem/composition tests for service-directory replacement, final symlink, nonregular leaf, cache/vector hard-link alias, replaced existing leaf and valid missing leaves. Preserve the existing deterministic race hooks in their new owner.
- [x] Run the new test filters and confirm the protections fail without the new handoff.
- [x] Implement LocalFileSystem database observations and verification in database.rs. Observe retained directory identity and both leaves. Existing leaves must retain identity; creation is allowed only for writable open, must produce regular distinct leaves, and must not bypass directory/symlink checks. Observation-only open requires the original leaf identity.
- [x] Expose SQLite-only crate-private connection/bootstrap/attach functions from storage. In lib.rs, surround each such call with filesystem verification, dropping connections and returning the original safe error on failed verification. No storage function receives a filesystem collaborator or performs path traversal/stat checks.
- [x] Implement CacheStorage/VectorStorage construction for writable and observation-only modes, preserving zero cache/read busy wait and five-second vector-writer wait, native registration, read-only cache attachment for vector pruning and proper drop order. Initialize schemas only in writable mode. Absent status data needs no durable database creation.
- [x] Preserve public open_storage fixture use by routing arbitrary supplied paths through filesystem-owned checks in composition. Move prepare_paths filesystem logic out of storage, retaining SQLite URI logic there.
- [x] Run migrated race/policy tests. Mutation-check removal of one before/after identity verification and cache/vector alias detection, restore, then run again.
- [x] Commit only task paths: "refactor: move database path policy into filesystem".

## Task 3: Migrate read services and joined query views

**Files:** src/service/{graph,graph_diagnostics,search,traversal,inspection,source,status}.rs, src/service/mod.rs, src/lib.rs, src/commands.rs; associated tests and tests/support fixtures.

**Consumes:** CacheDatabase scopes and shared CacheStorage constructor.
**Produces:** service read paths using their injected Rc<C>, and GraphTraversal without a transaction argument. Composition shares one cache instance across collaborators participating in a scope.

- [x] Extend existing pinned-view tests with two services sharing one Rc: first reads membership, independent SQLite writer publishes, second reads within the joined scope and still sees the original revision/membership. Assert known literal revisions and IDs.
- [x] Run affected read service tests and observe failure before the migration.
- [x] Inject Rc<C> and replace begin_read_tx handles with zero-argument with_read_tx closures. Move transaction-dependent helper calls to receiver methods using injected cache. GraphService joins the same read scope and graph remains dependency-free.
- [x] Preserve existing per-service storage-error classification when mapping TransactionError, including Operation(E) pass-through. Preserve bounded graph paging, source binding hydration, original revision and configuration identity.
- [x] Migrate status to with_status_read_tx: callback skipped with None for absent publication, malformed state remains an error, missing vector DB remains empty observation. No filesystem or durable DB is created by status.
- [x] Test inference failure during a pinned search: vector query-error recording uses the separate vector writer and FTS completes from the original view; no cache writer is held across HTTP.
- [x] Run affected unit tests plus cargo test --locked --offline --test search_consistency --test status_command --test read_verification --test source_inspection. Retain the existing checkpoint/reader-release assertions. Mutation-check joined-scope reuse versus opening a fresh view.
- [x] Commit only task paths: "refactor: share closure read scopes across services".

## Task 4: Migrate publication, initialization and vector writes

**Files:** src/service/{document_refresher,embedding,configuration,init}.rs where affected; src/lib.rs, src/service/mod.rs; tests/{embedding_service,cancellation_concurrency,corruption_recovery,thin_slice}.rs and affected fixtures.

**Consumes:** CacheDatabase/VectorDatabase and existing read migration.
**Produces:** all production write paths through closure scopes; initialization roles implemented through the two owners and composition.

- [x] Add a publication test with a nested service write, pending revision, an error caught by the parent, and an independent observer: parent cannot commit, rows/revision remain unchanged. Add successful nested publication advancing revision exactly once.
- [x] Add a revision race test: arrange a second publisher immediately after first commit and assert the first operation returns its own exact revision, not a later re-read value.
- [x] Run tests and observe failures, then migrate publication to with_write_tx. Build the returned RefreshOutcome using get_publication_revision inside the scope; return it publicly only after owning scope success. Keep index.lock and preparation/final verification boundaries.
- [x] Migrate embedding attempt reservation/failure/query-error/vector publication to identity-guarded vector scopes. Preserve ConfigChanged callback skipping, attempt-budget atomicity, dimension checks and pre-HTTP durable attempt reservation. Keep each provider call outside all write scopes.
- [x] Wire initialization using the two concrete owners and filesystem/composition opening; remove reliance on SqliteBootstrap/SqliteStatusStorage. Preserve the user's uncommitted init.rs edits and constructor behavior.
- [x] Run cargo test --locked --offline --test embedding_service --test cancellation_concurrency --test corruption_recovery --test thin_slice plus publication/init unit tests. Mutation-check premature revision increment, removal of expected-identity guard and a swallowed inner write error; restore and verify again.
- [x] Commit only task paths: "refactor: migrate publication and embedding transaction scopes".

## Task 5: Remove old components and update contracts

**Files:** delete src/storage/{sqlite,status,bootstrap,read,cache_write,vector_write}.rs after migrating their tests; modify src/storage/mod.rs, src/service/mod.rs, src/lib.rs, examples/storage_check.rs, tests/support and remaining direct storage consumers; AGENTS.md; docs/zkit/letopis/{architecture,decisions}.md; contracts/I-8-read-view.md; letopis/architecture/storage.md and its incoming source bindings.

**Consumes:** all production consumers use the new interfaces.
**Produces:** final two-component implementation, up-to-date active guidance and source-linked knowledge.

- [x] Find every old type, begin_*_tx method, ReadTransaction parameter and trait impl with rg across src, tests and examples. Migrate remaining active consumers/fixtures; preserve historical proof harness source against its captured revision.
- [x] Remove old storage component declarations and obsolete boxed transaction traits, then delete the files. Keep useful regression tests inline in the new owning files. Database-specific query helpers stay in their responsibility modules.
- [x] Audit architecture: exactly two concrete storage component structs; no filesystem stat/traversal imports in storage; application-owned interfaces in service/mod.rs; concrete dependencies only in composition; no extra transaction/connection wrapper component, async runtime or pool.
- [x] Update active AGENTS.md transaction/naming guidance and D-98/D-99 representation clarifications without rewriting history. Document shared-instance and same-database joining limits, separate query diagnostics and status observation guarantees in I-8/architecture.
- [x] Apply the Letopis skill to update storage knowledge and affected source links from a verified source tree. Run knowledge check and inspect; distinguish knowledge errors from unavailable tooling.
- [x] Run git diff --check, cargo fmt --check, cargo check --locked --offline --all-targets. Review physical files and constructor calls; lint success alone is insufficient.
- [x] Commit only task paths: "refactor: remove legacy storage components and align contracts".

## Task 6: Run acceptance and independent final review

**Files:** new evidence directory under .artifacts/storage-transactions/; execution report at docs/superpowers/plans/2026-11-03-storage-transactions-report.md.

**Consumes:** final changed tree; no unreviewed product changes after validation.
**Produces:** evidence for all eight spec acceptance groups, explicit remaining platform limits and independent final review.

- [x] Measure available build space and inspect current branch/dirty paths. Preserve unrelated edits; no blanket staging or cleanup.
- [x] Run python3 -B tests/storage/run.py --platform host --evidence .artifacts/storage-transactions/host-<unique-run>. The runner already includes make verify, schema parity, native/standalone linkage and provider deadline checks; do not duplicate a successful full run unnecessarily.
- [x] If Docker/Linux is available, run the same runner with --platform linux and a different new evidence directory. If unavailable, record that limit explicitly.
- [x] Run targeted knowledge/link consistency checks for changed Markdown. Review the final diff for transaction lifetimes, SQL parameter binding, callback invocation counts, same-instance injection and missing old-type consumers.
- [x] Record each invariant's result, mutation checks, exact commands, evidence locations, unrun checks and disk/platform failures in the report.
- [x] Request an independent final code/architecture review using the applicable Superpowers review skill; resolve material findings and rerun only checks justified by those changes.
- [x] Commit the report and intended changes only. Do not merge, push or create a PR unless authorized by the user.

## Self-review

The tasks cover all spec acceptance groups: shared ownership and joining (1/3/4),
rollback/panic/finalization (1), pinned reads/status (2/3), identity and provider
boundaries (1/4), filesystem protections (2), architecture/guidance (5) and evidence
(6). The five Review Focus cases have explicit tests. Cache and vector scopes have
different identity/result signatures by design; service mappings preserve them.
Implementation and Native execution were approved by the user; execution receipts and final review are recorded in the completion report.
