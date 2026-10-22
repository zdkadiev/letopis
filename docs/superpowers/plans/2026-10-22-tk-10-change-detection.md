# TK-10 Change Detection and Publication Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Refresh the knowledge cache from Git-selected Markdown files, preserving each invalid file's last valid projection and atomically publishing all valid changes.

**Architecture:** One synchronous DocumentService implements Documents and owns lock/redetect/publication orchestration. Application-owned interfaces connect GitCli, LocalFileSystem, MarkdownParser and SqliteStorage; lib.rs constructs them. Pure parser/domain rules stay independent of filesystem, Git and SQLite.

**Tech Stack:** Rust 1.95.0, existing pinned SeaQuery 1.0.2/rusqlite 0.40.2/native vec1 stack, Git argv/stdin, kernel flock. Approved direct dependency: rustix = "=1.1.5", default-features = false, features = ["fs", "std"], already in Cargo.lock through tempfile; its cached source supplies safe openat/statat/flock on the target Unix platforms. The implementation retains all previously locked package versions.

**Spec:** [Approved TK-10 specification](../specs/2026-10-22-tk-10-change-detection-design.md), approved by the user on 2026-10-22. Plan approved via Go on; the user explicitly selected Subagent-driven execution on2026-10-22.

## Global Constraints

- Work only in /Users/zdkadiev/.codex/worktrees/tk-10/letopis, branch TK-10; base main@bf9b6b9c46ddc9af0864a3a3924e8d0826da5abb. Preserve unrelated dirty files and skills-lock.json in the primary checkout.
- Synchronous I/O, real flock, one cache unit and one revision increment; no Tokio, watcher, worker daemon, job table, publication tree or embedding call.
- Git candidates: ls-files -z --cached --others --exclude-standard; normalize/deduplicate .md/.markdown, exclude .git and .letopis. Tracked paths remain candidates despite ignore patterns; consistently missing tracked paths are deletions.
- Racy window exactly 1_000_000_000 ns; clock regression or arithmetic uncertainty forces hashing. Unchanged racy bytes cause no lock/revision/checked_at update; non-racy stat matches cause no body read.
- Reject candidate symlinks, symlink parents, nonregular files and unsupported/non-UTF8 candidate paths safely, preserving the old publication. Detect observed path/stat replacement around reads; concurrent live edits are not an atomic snapshot.
- Preserve stable node row_id, old invalid-file nodes/outgoing edges/FTS, all declared dangling/cyclic edges and unaffected incoming edges. Vectors and failures do not enter cache publication.
- Prepare/publish at most one file's transient bytes/search text at a time. Metadata for the publication may be held in memory; no raw-text corpus copy or SQL document-body column.
- Follow AGENTS.md: service-owned traits, concrete struct/impl together, explicit imports, verb names, selective lint policy, AAA tests, aim 40/max 60 lines per function, actual lib.rs wiring.
- Verify macOS ARM64 and source-built Linux ARM64 separately with new receipts. No claim of physical power loss, x86_64, resource/model-quality/full CLI or unrelated historical harness execution.

## Review Focus

1. Git lists a tracked-but-deleted Markdown path; detect its deletion without failing or repeatedly publishing it (Tasks 2/3).
2. A filename containing underscore or percent is not a SQL LIKE pattern; enumerate exact normalized identities and group knowledge by parsed path (Tasks 1/3).
3. An unchanged invalid or racy file must retain its fingerprint and FTS without a lock or a revision increment (Task 3).
4. A parent directory becomes a symlink or a regular path becomes FIFO during a read; do not follow it or block indefinitely (Task 2).
5. Shared external sources may lose the earliest referring knowledge while an invalid peer retains old references; recompute minimum creation time without deleting still-referenced sources (Task 3).

## File Map and Interface Decisions

Modify existing service/mod.rs for small traits/reexports, service/model.rs for application-owned transfer values, and service/error.rs for safe DocumentError. Do not split every type/helper into its own file.

New production files: service/document.rs (DocumentService and impls), service/projection.rs (cohesive per-file preparation/application and source reconciliation), filesystem/mod.rs (declarations/reexports) and filesystem/local.rs (LocalFileSystem plus lock/read helpers). Create service/document/tests.rs only for the substantial private deterministic service/process test suite. Keep small unit tests with their code.

Modify existing git/cli.rs and git/cli/tests.rs, markdown/parser.rs/diagnostic.rs/mod.rs, storage/files.rs/graph.rs/read.rs, and lib.rs. Keep existing parser API paths working through reexports if its value definitions move to service/model.rs. No CLI/main/embedding scaffolding.

Root integration tests: new tests/document_refresh.rs and tests/support/documents.rs, plus tests/support/mod.rs declaration. Extend existing tests/storage_cache.rs for storage enumeration. Reuse tests/fixtures/markdown and the existing tests/storage/{run.py,Dockerfile} platform acceptance path; it already compiles root tests and copies src, tests/*.rs and tests/support. Change that runner only if a demonstrated new input is omitted.

Application interfaces (public through service; implementations remain private):

```rust
trait Documents {
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome>;
}
trait CorpusFiles {
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>>;
}
trait FileSystem {
    type IndexLock;
    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>>;
    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>>;
    fn get_time_ns(&self) -> DocumentResult<i64>;
    fn lock_index(&self) -> DocumentResult<Self::IndexLock>;
}
trait Markdown {
    fn parse_file(&self, path: &RepoPath, bytes: &[u8])
        -> Result<Option<ParsedFile>, Vec<Diagnostic>>;
}
```

FileStat carries size:u64, mtime_ns:i64, inode:u64, device:u64; device participates in within-read identity checks, not a schema change. CapturedFile carries bytes:Vec<u8>, stat:FileStat and checked_at_ns:i64 captured before reading. RefreshOutcome carries revision:u64, published:bool, files_updated:usize, files_removed:usize and invalid_files_updated:usize; counts describe only this call's publication, and all are zero for a no-op.

Move the existing ParsedFile/ParsedElement and Diagnostic/DiagnosticKind definitions into application model ownership, retaining public markdown reexports and the pure parse function. Their field shapes and diagnostic behavior do not change. MarkdownParser lives with its parse implementation and implements Markdown. This avoids service importing concrete markdown types without inventing duplicate projection formats.

DocumentErrorKind has InvalidRequest and InternalError, mapped later to the existing X-1 catalog. DocumentError supplies fixed safe message/get_kind/get_message plus an internal Error::source; DocumentResult<T> is its Result alias. Expected missing files use Option; parser diagnostics stay per-file outcomes, not fatal DocumentError. Preserve existing StorageError/SnapshotError APIs and causes when wrapping failures.

StorageRead gains list_files(&mut self) -> StorageResult<Vec<FileRow>> and list_nodes(&mut self) -> StorageResult<Vec<StoredNode>>, sorted by exact path/id. Existing list_edges(None) supplies declarations. No new cache-unit read API is necessary: capture metadata under the index lock in a short read unit, drop it, then begin the cache write unit. All cooperating publishers use index.lock; vector-only writers do not change this metadata.

Private service projection types implement I-2's role: PreparedProjection carries base_revision, node changes, declared edge changes, removed IDs, file rows and per-file PreparedText { id, uri, search_hash, search_text }. A metadata-only invalid/stat update has no text. publish<W: CacheWrite>(unit: &mut W, prepared: PreparedProjection) applies mutations without commit; DocumentService checks base and commits once.

## Task 1: Enumerate Published State Through Storage

**Files:** service/mod.rs, storage/files.rs, storage/graph.rs, storage/read.rs, tests/storage_cache.rs.
**Consumes:** existing Storage::begin_read, StorageRead::read_file/read_node, FileRow/StoredNode and private build_query binding helper.
**Produces:** the exact list_files/list_nodes signatures above; no schema or driver changes.

- [x] **Step 1 — Write enumeration regression tests.** Add list_files_preserves_exact_paths_and_state and list_nodes_preserves_metadata_and_row_ids. Populate marked, unmarked and invalid FileRows with paths a_b.md, a%b.md and nested/a.markdown; insert both source and knowledge nodes. Assert complete field equality, exact deterministic ordering, no pattern matching, and the same row_id before/after node upsert. Keep AAA assertions visible.
  Required assertions in the enumeration test:

  ```rust
  assert_eq!(rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
      ["a%b.md", "a_b.md", "nested/a.markdown"]);
  assert_eq!(rows, expected_file_rows);
  assert_eq!(updated.row_id, original.row_id);
  ```

- [x] **Step 2 — Run RED.** Run `cargo test --locked --offline --test storage_cache list_`. Expect missing list_files/list_nodes methods; capture the real compiler failure before implementation.
- [x] **Step 3 — Implement enumeration.** Reuse existing decoding logic in files.rs and graph.rs and SeaQuery typed identifiers/order. Avoid reparsing Markdown or filtering nodes by SQL LIKE; retain the existing one-row reads. Add proper # Errors docs on public trait methods.
- [x] **Step 4 — Run GREEN and root checks.** Repeat the targeted test command, then ./scripts/check-rust.sh. All tests and diagnostics must pass; no expected count is invented in advance.
- [x] **Step 5 — Commit only these paths.** Commit message: feat(storage): enumerate published files and node metadata.

## Task 2: Supply Safe Filesystem, Candidate and Parser Implementations

**Files:** Cargo.toml/Cargo.lock, service/mod.rs/model.rs/error.rs, filesystem/mod.rs/local.rs, git/cli.rs and its tests, markdown/parser.rs/diagnostic.rs/mod.rs, lib.rs.
**Consumes:** RepoPath, existing GitProcess argv execution, existing pure parse and its unchanged return/diagnostic fields; Task 1 APIs remain untouched.
**Produces:** FileSystem/CorpusFiles/Markdown and their concrete implementations, transfer values/errors above; GitCli::get_root(&self) -> &Path is crate-private for composition. LocalFileSystem::open(root: &Path) -> DocumentResult<Self> and MarkdownParser::new() construct real implementations. lib.rs::open_filesystem(root:&Path) -> DocumentResult<impl FileSystem + use<>> constructs a real LocalFileSystem for this independently testable stage; the existing free parse API calls the concrete parser. See the execution ruling in the ledger for the denied-dead-code constraint.

- [x] **Step 1 — Write RED interface/implementation tests.** In filesystem/local.rs's test module, add read_regular_file_preserves_bytes_and_stat, reject_final_and_parent_symlinks, reject_fifo_without_blocking, missing_file_returns_none, replacement_during_read_fails and lock_releases_on_drop. Test a UTF-8 filename with spaces and bytes including NUL/non-UTF8 content; content validation belongs to Markdown, not file I/O. Use a private test hook for deterministic replacement, not sleeps.
  Required file/lock assertions, using prepared regular/symlink/missing fixtures:

  ```rust
  assert_eq!(captured.bytes, b"literal\0\xffbytes");
  assert!(filesystem.read_file(&symlink_path).is_err());
  assert!(filesystem.read_file(&missing_path)?.is_none());
  // After dropping the first guard, a distinct fd obtains the real kernel lock.
  assert!(rustix::fs::flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_ok());
  ```

- [x] **Step 2 — Add candidate/parser parity tests.** GitCli tests cover tracked and untracked Markdown, ignored untracked files, tracked ignored files, tracked deletion still listed, duplicate entries/staged paths, .markdown, spaces and unsupported raw paths. Assert no create_tree/pin/ref writes. Verify MarkdownParser::parse_file produces exactly the same whole/section/unmarked/invalid results as the current free parse function, preserving public imports.
- [x] **Step 3 — Run RED before implementation.** cargo test --locked --offline --lib. Missing new traits/types/constructors must explain failure; the existing parser tests are parity checks and are not falsely labeled behavioral RED.
- [x] **Step 4 — Implement safe synchronous I/O.** Add direct rustix = "=1.1.5" with default-features=false and fs/std features. Run cargo check --offline --lib once to update the root package dependency entry in Cargo.lock; preserve and compare all previously locked package/version pairs before locked verification. Walk parent components relative to an owned root directory fd using safe openat with DIRECTORY/NOFOLLOW/CLOEXEC. Open final paths NOFOLLOW/NONBLOCK, require regular files before reading; compare device/inode/size/mtime via fd and a fresh root-relative path observation after reading. Nonblocking open avoids hanging on a FIFO replaced into the path. Missing stable paths return None; observed replacement, permissions and other I/O fail safely. No canonicalize-then-unchecked-open sequence for candidates.
- [x] **Step 5 — Implement index lock and Git/parser interfaces.** Keep one persistent .letopis/index.lock, opened safely without truncation, with exclusive flock owned by an RAII guard. Retry EINTR only to complete the same lock operation; do not unlink or replace the file. Use a nonblocking flock probe before blocking so a private cfg(test) hook can report confirmed contention. Validate the service directory is a directory rather than a symlink. Git uses the exact specified command and NUL framing, normalizes candidates and excludes service metadata. Keep all Git invocation code in git/cli.rs/GitProcess. Move only shared parser value definitions; keep concrete parser implementation and old free parse behavior.
- [x] **Step 6 — Prepare dependencies and run GREEN.** cargo fetch --locked, then cargo test --locked --offline --lib and ./scripts/check-rust.sh. Run cargo doc --locked --offline --no-deps with RUSTDOCFLAGS='-D warnings'. Verify Cargo.lock retains old package versions and no runtime/extra SQL driver was added.
- [x] **Step 7 — Commit only task paths.** Commit message: feat(documents): add confined file reads and Git candidates.

## Task 3: Implement Refresh, Per-File Projection and Source Reconciliation

**Files:** service/document.rs/projection.rs/mod.rs/model.rs, lib.rs, tests/document_refresh.rs, tests/support/documents.rs/mod.rs, private service/document/tests.rs as needed.
**Consumes:** Tasks 1/2 interfaces, existing Storage/CacheWrite, parser fields, domain derive_element_kind and source/knowledge identities.
**Produces:** DocumentService<G,F,M,S>::new(git:G, filesystem:F, markdown:M, storage:S) -> Self where G:CorpusFiles,F:FileSystem,M:Markdown,S:Storage; Documents::refresh; lib.rs open_documents(start:&Path) -> DocumentResult<impl Documents + use<>> constructing these concrete implementations at one Git root and opening .letopis/cache.db plus .letopis/vectors.db. Preserve open_storage/open_git_snapshots. cache.db is a plan-level naming choice for the previously unnamed main cache, distinct from the already agreed vectors.db.

- [x] **Step 1 — Write RED public refresh tests.** Initial empty build yields revision1/published=true; second refresh returns the identical revision/published=false/counts0. Add a whole file plus anchored sections, edit with stable ID/row_id, remove one section, rename/delete a tracked path, and transition valid->unmarked->marked. Assert exact nodes/edges/FTS/FileRows and preserved corpus bytes through independent storage reads.
  Required initial/no-op assertions:

  ```rust
  assert_eq!(first.revision, 1);
  assert!(first.published);
  assert_eq!(second.revision, first.revision);
  assert!(!second.published);
  assert_eq!((second.files_updated, second.files_removed, second.invalid_files_updated), (0, 0, 0));
  ```

- [x] **Step 2 — Write invalid and prefilter tests.** A mixed update publishes a valid peer while retaining all old nodes/edges/FTS of an invalid file and storing only its safe diagnostic/hash; new-invalid, unchanged-invalid, repaired and deleted-invalid cases have explicit assertions. Use a counting FileSystem/Markdown wrapper and controlled times for non-racy no-body-read, same-tick equal size/mtime/inode changed bytes, unchanged-racy no-op, clock regression/near-i64-limit safe handling, and stat-only update without parse or FTS replacement. Parser-projected invalid_utf8 remains a per-file finding, unlike fatal read errors.
- [x] **Step 3 — Write graph/source tests.** Store dangling and cyclic declarations, remove/readd a target while keeping its incoming edge, and share a normalized source across two referring files. Remove/edit the earliest referrer and assert min created_at recalculation; an invalid peer retains its old reference. After the last reference disappears, source row is gone. Check whole-file and #L2-L3 source hashes, CRLF/exact bytes, missing file and out-of-range source bytes (nullable hash), including a non-Markdown source path. Reading source bytes must obey confinement as well; an unsafe path is fatal, not silently missing. Do not validate/pin historical source_snapshot here (TK-21). Reject final knowledge/source SQL-id collisions atomically with the existing InvalidRequest class; verify both role-transition directions preserve the previous publication on failure, and allow a transition after its last conflicting reference is removed (execution ruling: X-4 distinguishes source from corpus elements; no new namespace/schema).
- [x] **Step 4 — Run RED.** cargo test --locked --offline --test document_refresh and the selected private document tests; require absence/unimplemented refresh to be the relevant failure.
- [x] **Step 5 — Implement unlocked detection.** Read current files/revision through a short StorageRead and Git candidates; hash only necessary files. Detection retains path/stat/hash metadata, not bytes/parse results for the whole corpus. Drop temporary bytes before the next file. Treat revision0 as initial/full publication even with zero candidates. Consistent stat None removes an existing row; a never-published missing candidate creates no delta.
- [x] **Step 6 — Implement lock/redetect.** If no delta, return current revision without calling lock_index. Otherwise obtain its RAII guard, discard the preliminary observations, read fresh metadata and re-run detection. Drop the read view before beginning a write; check prepared base against the fresh revision. If another publisher already handled the delta, return without a write/revision bump. A changed read between the locked detection and preparation fails/rolls back; do not publish a stale preliminary capture.
- [x] **Step 7 — Implement file-at-a-time preparation/application.** In one existing cache unit, clear outgoing edges of each valid changed/deleted element, UPSERT retained IDs, remove disappeared IDs, insert actual declarations, replace transient parser FTS text, and store FileRows. For same-hash stat changes carry forward previous validity/error and alter only observations. Invalid parse creates only a files-row mutation; no old projection cleanup. Capture checked_at before file read. Prepare one file and apply it before reading the next; never commit inside publish(unit, prepared).
- [x] **Step 8 — Reconcile affected sources and commit once.** Maintain a metadata-only effective node/edge map from the locked snapshot plus changes. For source addresses touched by removed/added/replaced external declarations compute remaining references and minimum knowledge created_at; read affected current addressed bytes once per path where practical, release source text before advancing, UPSERT source metadata or delete the orphan. Preserve unrelated source metadata. A missing/out-of-range source yields hash=None; genuine I/O/unsupported path failure aborts. Commit once, return exact outcome counts, and release index.lock on every path. Never write vectors, create a tree or invoke an embedding provider.
- [x] **Step 9 — Compose and verify GREEN.** lib.rs creates GitCli, LocalFileSystem, MarkdownParser and SqliteStorage and passes them into DocumentService; service imports no concrete implementations. Run the focused tests and ./scripts/check-rust.sh. Inspect public errors for secret/raw-text leakage, especially Git and read-failure causes.
- [x] **Step 10 — Commit only implementation/tests.** Commit message: feat(documents): refresh file projections atomically.

## Task 4: Prove Production Concurrency/Interruption and Record Acceptance

**Files:** private service/document/tests.rs (one cohesive child service/document/tests/process.rs may hold the subprocess driver/oracle if needed), minimal cfg(test) hooks in document/projection/local implementation, tests/support/documents.rs as needed; docs/zkit/letopis/checks/TK-10/report.md; task card and this plan. Reuse existing platform runner and historical TK-2 oracle patterns without importing its simplified parser or overwriting its receipts.
**Consumes:** production DocumentService, real Git/X-4/filesystem flock/SQLite, outcomes from Task 3; exact checkpoints below.
**Produces:** reproducible production process-death/concurrency tests, platform receipts and task acceptance with honest limits.

- [x] **Step 1 — Add test-only child entry and checkpoint transport.** Spawn the current Rust unit-test executable with a single ignored child test selected by --exact; only cfg(test) code reads its control environment. Use pipes/Unix streams for ready/continue messages with bounded20-second waits and child kill/reap on test failure. Child invokes the actual service, not a copied publication algorithm. Keep task helpers private and functions within60 lines.
- [x] **Step 2 — Add interruption matrix assertions.** At detected, locked, redetected, begun, cleared, projections, fts, fingerprints, before_commit and after_commit, parent waits for an acknowledged checkpoint and sends real SIGKILL. Every precommit case equals the exact initial nodes/edges/FTS token instances/all FileRow fields/revision; after_commit equals a separately constructed literal new oracle. Restart converges, kernel lock is available, corpus bytes and vector state unchanged. Assert signal termination and retain logs; checkpoints between SQLite operations do not claim cuts inside COMMIT or physical power loss.
- [x] **Step 3 — Add concurrency schedules.** Two duplicate deltas cause one publication; a waiting publisher signals only after a real nonblocking flock reports contention and re-detects a newer file version after acquiring it. Killing a lock owner needs no lock-file repair. A no-op completes while another process holds index.lock. A held WAL read view remains old through commit; a new reader observes the complete new publication. Use deterministic acknowledgements, not scheduling sleeps.
- [x] **Step 4 — Prove test sensitivity.** Temporarily remove redetection and require the newer-edit schedule to fail; separately make FileRow writes escape the unit and require the crash oracle to reject it. Mutations must compile, be made one at a time, restore byte-for-byte, and rerun the same tests GREEN. Keep mutation evidence outside Git. If a particular mutation cannot be applied safely, record that gap rather than claiming it tested.
- [x] **Step 5 — Run production acceptance on both platforms.** From the worktree use /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk10/host-<unique> and the same command with --platform linux and a separate evidence directory. The existing Linux path builds source with Cargo dependencies prepared and runs acceptance without network; all new root/unit subprocess tests must execute there. Compare common input hashes, record actual counts/toolchain/platform/exit statuses, run strict Rustdoc and schema/native/copied-executable checks supplied by that runner. If a suite fails, investigate before marking acceptance complete.
- [x] **Step 6 — Review architecture and report.** Inspect physical files, visibility, import direction, struct/impl placement and actual constructors in lib.rs. Record how every TK-10 criterion is proved, including why CLI command integration remains TK-11/TK-14. Document symlink/one-second timestamp/live-read limits, unrun physical/x86/resource/model/historical suites. Preserve raw receipts under ignored .artifacts and summarize reproducible commands/source hashes in checks/TK-10/report.md.
- [x] **Step 7 — Update docs/memory and review.** Check task acceptance boxes only after evidence exists; record Engram findings/changes/verification, keeping historical provenance and other writers intact. Complete the selected workflow's independent code review and resolve consequential findings. Commit intended code/test/report/task/plan paths only, without raw receipts. Merge/push/GitHub changes remain explicit follow-up actions unless the user authorizes them for TK-10.

## Plan Self-Review and Current State

- [x] Spec coverage: detection/path safety -> Task2/3; lock/redetect/streamed publication/source ownership -> Task3; process-death/WAL/concurrency -> Task4; exact SQLite enumeration -> Task1.
- [x] Interfaces match across tasks; model relocation preserves old Markdown API paths. No service imports a concrete technology module.
- [x] Five review-focus cases have named tests in their owning tasks. Ordinary CRUD uses typed/bound SeaQuery.
- [x] Scope exclusions retained: full reindex command/corruption, full CLI handlers, source basis validation, embedding/drain and physical power loss are later tasks.
- [x] Planning-stage documentation validation: 828 local links resolved; D-101 anchor was unique and whitespace checks passed. That planning-stage tree contained only documentation.
- [x] Baseline at bf9b6b9: ./scripts/check-rust.sh PASS on macOS ARM64,112 Rust tests, fmt/compiler/Clippy. No product changes or Linux execution occurred during planning.
- [x] User approved plan and explicitly selected Subagent-driven execution on2026-10-22.
- [x] Tasks 1–4 implemented and independently approved; final review fixes independently approved.


## Execution Rulings

The controller made the following implementation decisions during the selected
Subagent-driven workflow. They preserve the accepted design boundaries and remain
visible for review:

1. **Typed filesystem constructor.** Add `open_filesystem` in `lib.rs` and route
   the existing free parse API through `MarkdownParser`, keeping the independently
   checked Task 2 stage free of dead-code allowances before Task 3 consumes these
   components. If wrong, the extra typed opener can be removed after integration;
   concrete implementation types remain private.
2. **Source/knowledge identity collisions.** Reject a final effective projection
   requiring both roles for the same SQL ID with safe `InvalidRequest` and roll
   back the entire refresh. X-4 distinguishes external source addresses from
   corpus elements, and SQL IDs are globally unique. If wrong, ambiguous corpora
   require author correction until a broader representation is considered in
   TK-20. Both directions and a permitted transition after the final conflicting
   reference disappears have regression coverage.
3. **Service database path protection.** Retain and revalidate the service
   directory descriptor, use native SQLite `NOFOLLOW` on uncanonicalized automatic
   cache/vector paths, and reject observed directory replacement. Preserve generic
   explicit storage opening. A custom VFS or process-global `fchdir` is outside
   this change. If wrong, atomic protection against actively hostile concurrent
   ancestor renames remains unproved; this residual limit must be reported.

4. **Initialization-only contention bound.** Schema/WAL initialization and its first
   read-only readiness probes each have a five-second bound, including
   `BUSY_RECOVERY` during competing startup. Each initialization-ownership step
   also has its own five-second bound; this is not a global open/CLI deadline. Recheck schema after obtaining
   initialization ownership. Already-ready ordinary opens perform no writes;
   ordinary writes retain zero busy timeout and no retries. No unconditional
   startup index lock is introduced. Mechanics defines no global CLI deadline. If this bound is too
   short, initialization can fail safely and require retry; success beyond five
   seconds is not guaranteed.

5. **Concurrent lock-file creation on macOS.** An independent two-thread
   `openat` reproducer against the same live directory descriptor shows that
   competing `O_CREAT | O_NOFOLLOW` opens can return `ENOENT` after the other
   creator succeeds. For this creation error alone, reopen once through the
   same directory/name with identical flags except `CREATE`, retaining
   `NOFOLLOW`, `NONBLOCK` and regular-file/path identity checks. Do not unlink,
   sleep or add a general retry. If this diagnosis is incomplete, a remaining
   transient failure is safe; arbitrary filesystem-race success is not promised.

## Verification and Review Record

Initial production checkpoint `2378731`; initial acceptance report `8e29067`
are retained as historical evidence. Final source checkpoint: `15fcc19`.
See the [durable report](../../zkit/letopis/checks/TK-10/report.md) for exact
commands, source hashes, counts, limits and criterion mapping. Each platform
(macOS ARM64 and source-built Linux ARM64) passed 172 Rust tests and nine Python
runner tests, formatting/compiler/Clippy, strict Rustdoc, schema/native and
copied-executable checks. All 86 common input hashes match.

Independent task reviews approved all four stages. Task 2's excluded-metadata
validation order and Task 3's automatic database-symlink opening were corrected,
tested and independently re-reviewed. Task 4 review found no issues and checked
raw receipts, both compiling negative controls, exact crash oracles and real
flock contention. No open task-review findings remain. Final whole-branch review
found startup-contention and dependency-wording findings. The consolidated fix
now validates ready schemas read-only, coordinates missing-schema initialization
with recheck and bounded readiness probes, and handles the reproduced macOS
lock-create race without weakening path protections. Both platforms pass on the
amended source; scoped re-review confirmed I1/M1 addressed and no new breakage. At that initial acceptance gate, no merge, push or GitHub mutation
had been performed for TK-10.

Final gate: complete on 2026-10-22. Independent whole-branch review and its
single consolidated fix wave are complete; the scoped re-review approved
I1/M1 and the additional lock-creation correction with no new findings.
There are no parked or deferred review findings. Five execution rulings are
listed above. Raw workflow reports/ledger/diff packages are archived under
`.artifacts/tk10/workflow/`; earlier logs under `.artifacts/tk10/prior-task-logs/`.
The existing shared Engram task records results, decisions and verification;
the initial branch acceptance is complete. Integration into `main` is recorded below.

## Integration into main — 2026-10-22

User authorized commit, merge, GitHub issue update and push. The reviewed
`TK-10@352eace` was merged with preserved history as
`64b218801f6a74c0eada0295bb4b7f91d7006dcf`. The committed merge tree exactly
matches the reviewed branch. On that result, `./scripts/check-rust.sh` passes
formatting, compiler diagnostics, Clippy and 172 Rust tests; one private child
entry is ignored by the outer harness and invoked explicitly by parent tests.
The unrelated primary `.gitignore` edit is byte-for-byte preserved and excluded
from these commits. Receipt: `.artifacts/tk10/integration-352eace/verification.json`.

The earlier Linux/macOS platform receipts remain pinned to their captured
source revisions; this integration reruns the root gate on macOS and does not
claim another Linux platform run. Publication and issue outcome are recorded in
[GitHub issue #10](https://github.com/zaurkadiev/letopis/issues/10) and Engram.
The managed task worktree is retained with its ignored evidence.
