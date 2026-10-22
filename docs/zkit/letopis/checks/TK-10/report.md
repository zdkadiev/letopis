# TK-10 — Production refresh acceptance

Date: 2026-10-22. The original Tasks 1–4 acceptance below is historical evidence for source `2378731`. The final whole-branch review identified startup contention (I1) and dependency wording (M1); the correction is source `15fcc196b554c5a77b8b83818ddc2274d916fb70`. A separate final-review correction section records its verification. Focused fix review and the final Engram record remain controller responsibilities.

Source checkpoint: `23787312a8b7e714280158d04bc9ea19f1d72006` (`test(documents): prove process death and concurrent publication`). Tasks 1–3 end at `ac30e180c6efa4922c306dc0231c89606d996dc5`. Only the controller's three documentation edits were dirty during platform runs. All 191 host input hashes matched that restored source tree after the original negative controls; this is a statement about the captured historical revision.

Design: [approved specification](../../../../superpowers/specs/2026-10-22-tk-10-change-detection-design.md), [task](../../roadmap/epics/EP-1-thin-slice/tasks/TK-10-change-detection.md), [architecture](../../architecture.md#program-structure), [MC-2](../../mechanics.md), [I-2](../../contracts/I-2-projections.md), [D-101](../../decisions.md#D-101).

## Production process proof

The private [process tests](../../../../../src/service/document/tests/process.rs) spawn the current Rust unit-test executable with `--exact service::document::tests::process::run_child --ignored --nocapture`. Each child uses real `DocumentService`, `GitCli`, `LocalFileSystem`, X-4 `MarkdownParser`, confined `SqliteStorage`, native SQLite/FTS5/vec1 and kernel `flock`. There is no copied TK-2 parser or publication algorithm. A private explicit clock override fixes `checked_at_ns=42` so the oracle compares every FileRow field exactly; file bytes, stat identity, Git candidates, locks, SQLite units and commits remain real. Existing detection tests separately cover clocks and the one-second racy boundary.

Unix stream acknowledgements establish each schedule, with 20-second startup/read/write/exit watchdogs. A waiter emits `contended` only from the filesystem callback after real nonblocking `flock` returns `WOULDBLOCK`. No sleep decides schedule order; the one-millisecond exit watchdog only polls process termination. Dropping the parent worker kills/reaps its child on assertion failure. Child logs and parent event/exit receipts are retained separately.

Fixture: a changed valid file, a deleted file, a newly invalid file whose prior projection must survive, an untouched valid file, and an external source shared with the invalid peer. The old oracle is captured before edits. The new oracle applies literal expected hashes, visible text/token changes, validity/error, deletion, and shared-source minimum changes to the retained identities; it never invokes the production parser or publisher to derive expected results. Current physical stat values and a fixed test timestamp determine expected FileRows.

| Checkpoint | Expected reopened cache after SIGKILL |
|---|---|
| `detected`, `locked`, `redetected` | Exact old publication |
| `begun` | Exact old publication |
| `cleared`, `projections`, `fts`, `fingerprints` | Exact old publication; cuts use the second file, after an actual replacement, following the first file's deletion |
| `before_commit` | Exact old publication after all file/source writes |
| `after_commit` | Exact complete new publication |

Every cut compares revision, all nodes including stable row IDs and source metadata, all declared edges, every FileRow field, and every FTS5 vocabulary token instance `(term, doc, col, offset)`. It asserts signal 9 termination, reopens storage, verifies lock availability, retries refresh to the complete new state and rechecks corpus/source bytes. Seeded vector contents, configuration identity, durable embedding failure/attempts, query error, and vector DB bytes remain unchanged. Git object inventory remains unchanged through initial publication, interruption and recovery.

Six parent tests execute on both platforms:

- `sigkill_keeps_exact_old_or_complete_new_publication`: ten cuts and ten restarts.
- `duplicate_publishers_commit_once_after_real_contention`: both detect a delta, the second contends, one commit advances revision to 2, and the second returns a no-op.
- `waiting_publisher_redetects_newer_edit_after_owner_is_killed`: edit again after confirmed contention, kill the lock owner, then publish the newest bytes; the lock inode is unchanged.
- `noop_finishes_while_another_process_holds_index_lock`: the no-op completes before the holder is killed.
- `pinned_wal_reader_keeps_old_snapshot_and_new_reader_sees_complete_commit`: held production read-unit metadata and a pinned FTS read stay old while a new reader sees the complete new state.
- `late_storage_error_rolls_back_every_cache_mutation`: after all projection/source mutations, the real cache writer rejects an out-of-range FileRow size with `StorageError`; the whole cache remains exactly old, and a normal retry succeeds.

## Sensitivity and development evidence

Behavioral RED before adding checkpoint/failure hooks: `cargo test --locked --offline --lib service::document::tests::process -- --test-threads=1` produced **3 passed / 3 failed / 1 ignored**. Failures identified absent `locked`/`detected` acknowledgements and the absent late failure. Initial scaffolding compile/import failures and a noncanonical macOS fixture path were corrected before this behavioral RED. The test builder now uses the canonical Git root, matching `lib.rs` composition.

The runner retention regression initially failed because `LETOPIS_TEST_EVIDENCE` was absent. The fix passes that receipt-local path alongside existing per-command overrides, including strict Rustdoc. Its macOS expected path was corrected to match the runner's existing `resolve()` semantics. All nine runner tests pass.

Mutation sensitivity was executed on macOS ARM64. Accepted mutation evidence is exclusively `.artifacts/tk10/mutations-verified/`:

| Temporary compiling mutation | Observed RED | After byte restoration |
|---|---|---|
| Reuse preliminary state/delta instead of locked redetection | Newer-edit child reports `documents changed during refresh`; expected successful publication fails; exit 101 | Same exact test passes, exit 0 |
| Commit changed FileRows separately during preliminary detection, after the initial revision exists | After SIGKILL at `detected`, exact State comparison fails: new fingerprints and revision 2 coexist with old graph/FTS; exit 101 | Same ten-cut matrix passes, exit 0 |

Both mutations were applied one at a time, compiled, and restored in `finally` before GREEN. Original/restored `src/service/document.rs` SHA-256: `ca933ad03195d9e738a8b046c434d4c554b43b4cea147b10f469720b6ec3ae9c`. No-redetect mutated hash: `9500c22f1bf173c5f584230b52990ccbc8392a197d5b6caa68240ea404f76146`. Escaped-files mutated hash: `5a1e87a403f3d95736476f09360785934178de479b3b170d81df0006ff228ea7`.

Earlier `mutations/` and `mutations-final/` receipts remain preserved but are superseded: their escaped-files mutation failed fixture initialization and is **not** accepted as crash-oracle evidence. The corrected run explicitly requires the `cut=detected` equality failure. Raw patches, exact commands, RED/GREEN logs, child receipts, and baseline input hashes are in `mutations-verified`; development logs and the reproduction script are in `.artifacts/tk10/development/`.

## Platform acceptance

Executed from the TK-10 worktree:

```sh
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk10/host-2378731
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk10/linux-2378731
```

| Result | macOS host | Linux container |
|---|---|---|
| Platform | macOS 26.6.2, arm64 | Linux 7.0.0-22-generic, aarch64, glibc 2.36 |
| Rust/Cargo | 1.95.0 / 1.95.0 | 1.95.0 / 1.95.0 |
| Git / Python | 2.50.1 / 3.14.7 | 2.39.5 / 3.11.2 |
| Root Rust tests | 161 passed, 0 failed | 161 passed, 0 failed |
| Breakdown | 48 unit + 113 integration | 48 unit + 113 integration |
| Top-level ignored child | 1, explicitly invoked by six parent tests | 1, explicitly invoked by six parent tests |
| Child executions | 29: 12 signal-9 exits, 17 normal exits | 29: 12 signal-9 exits, 17 normal exits |
| Python runner tests | 9 passed | 9 passed |
| Command receipts | 15 commands, every exit 0 | 8 outer + 13 inner commands, every exit 0 |
| Runner manifest | PASS | Outer and inner PASS |

Each runner executes `./scripts/check-rust.sh`: formatting, compiler diagnostics, Clippy with `-D warnings`, and locked/offline root tests. It also runs strict `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`, schema parity, the native capability probe, and a copied standalone executable. Native runtime: SQLite 3.53.2; vec1 0.8 (NEON, multi-threaded); WAL, synchronous=2, foreign_keys=1; macOS fullfsync=1. Both standalone receipts report FTS/shared native vector/exact bytes/reopen/revision=1 PASS; `otool`/`ldd` reject external SQLite/vec1 shared dependencies.

Linux is built from the committed source with the existing pinned Dockerfile; Cargo dependencies are prepared at build time, and acceptance runs with Docker `--network none`. Image SHA-256: `5cd24367e77f5b61582cb572f14dfbec89c9a5d191efadcce06ce96dec54f4a6`. The container is copied out and removed by the existing runner. Receipts survive at `.artifacts/tk10/linux-2378731/container/`, including 58 process log/receipt files.

Input inventories: host/outer Linux each 191 files; inner Linux 86 files; **all 86 common SHA-256 values match**, with zero Linux-only files. The 105 host-only inputs are historical or otherwise unselected harness files that the existing Dockerfile does not copy; their suites are not represented as executed. Canonical sorted compact JSON of the common hash map has SHA-256 `76567dc7f946d84ff2b1d6e2547cf136172419c0a8192f26c6307e54142263e0`. `.artifacts/tk10/acceptance-summary.json` retains this map, the full host-only list, command/count summaries, current-tree equality, and hashes of individual receipts.

Copied-executable SHA-256: macOS `7e76870ce88f0d06d040a303b90113e722fb991dc5a6b5cdcc5560d722c5c444`; Linux `6df1c0269504ff517fb8f94510fdc310015579e6d61b5fe6420f7a046c385ca0`.

## Acceptance mapping for the controller

| TK-10 criterion | Evidence and precise scope |
|---|---|
| Completed valid edits visible before search/traverse/inspect/check | Real `Documents::refresh` and independent storage readers prove freshness, including a newer edit made during lock contention. Public document-refresh tests cover initial/changed/removal cases. Wiring actual CLI commands remains TK-11/TK-14, as approved; no CLI command was executed here. |
| Invalid file keeps old projection, error visible, other files publish | Mixed-invalid integration tests and the process fixture verify retained nodes/outgoing edges/FTS, exact invalid FileRow/error, valid-peer commit, shared-source minimum and repair. Future status/check presentation remains CLI integration. |
| No mixed cache after interruption; fingerprints share publication unit | Ten real SIGKILL cuts, old/new full-state equality, restart convergence, late StorageError rollback, and the compiled escaped-files negative control. |
| Unmarked Markdown tracked; Git ignore semantics; marking later publishes | `skip_body_reads_for_unchanged_non_racy_marked_unmarked_and_invalid_files`, `handle_tracked_rename_deletion_and_unmarked_transitions`, and real Git candidate tests, all rerun on both platforms. Exact underscore/percent path enumeration is also rerun. |
| Empty delta avoids lock; changed files only; no Git publication tree | Held-lock no-op process test; unchanged-invalid/racy/non-racy/stat-only private tests and FTS segment preservation; Git object inventory unchanged by initial publication, process matrix and recovery. |
| Physical module and dependency boundaries | Inspection below, plus compiler/Clippy/Rustdoc and source input inventories. |

## Architecture review and limits

Inspected physical files, visibility, constructors, implementations and import direction. `src/lib.rs::open_documents` constructs actual Git, confined filesystem, Markdown and SQLite implementations and passes them to `DocumentService`. `service/mod.rs` owns traits; `service/document.rs` owns orchestration; `service/projection.rs` applies one file inside the caller's unit and reconciles source metadata. Concrete structs and inherent/trait implementations remain together in their responsibility files. Production services import application/domain contracts, not technology implementations. Storage keeps SQL/query builders private; the test oracle's native FTS5 vocabulary query is private test code. No new schema, async runtime, public test/fault API, future subsystem, or CLI scaffold was introduced. The branch adds a direct pinned `rustix` production dependency; its version already existed transitively in Cargo.lock, so there is no locked-version upgrade. Task 4 adds no production dependency.

Task 4 source changes: private `service/document/tests/process.rs`, its declaration in `tests.rs`, `cfg(test)` checkpoints in document/projection, and a private test-only clock/contention setter in `filesystem/local.rs`. All control environment reads are inside the private test module. `tests/storage/run.py` plus its regression test preserve child receipts without changing product runtime configuration. There are no blanket lint allowances. `./scripts/check-rust.sh` passes the configured maximum-function-length lint.

The source-built Linux run includes the prior confinement regressions: static candidate/parent/database symlinks, nonregular/FIFO replacements, late database-leaf/ATTACH substitutions and observed service-directory identity changes. Document-owned storage uses SQLite NOFOLLOW, inherited by vector ATTACH. Residual limit: there is no atomic descriptor-relative guarantee against an actively hostile actor concurrently renaming ancestors during SQLite's internal pathname/open sequence. No custom VFS or process-global directory change was added.

These are acknowledged process-death cuts **between** SQLite operations, not cuts inside COMMIT and not physical power-loss proof. Live concurrent edits are not an atomic corpus snapshot. The one-second racy policy does not cover coarser timestamp granularity or deliberately restored old non-racy timestamps. Unrelated external-source freshness and source_snapshot validation/pinning remain later tasks. No x86_64, resource/capacity/performance, model quality, full CLI/drain/cancellation JSON, physical power loss, or unrelated historical proof-harness execution is claimed. Historical TK-2 receipts were untouched.


## Independent Task Reviews

All four stages are approved. Task 1: metadata enumeration, no findings. Task 2:
raw excluded service metadata now filters before filename validation; two RED
regressions and the scoped re-review confirm the fix. Task 3: automatic database
opening retains directory identity and rejects cache/vector symlinks; public and
late-boundary regressions and scoped re-review confirm the fix. Task 4: no findings;
reviewer independently checked full process assertions, mutation failures,
receipt counts and source equality without repeating the suites.

The final whole-branch review identified I1 and M1. The correction and new evidence are recorded below; focused fix review remains pending. The controller's implementation
rulings and their costs are preserved in the
[plan](../../../../superpowers/plans/2026-10-22-tk-10-change-detection.md#execution-rulings).

## Final-review correction and new acceptance

The consolidated correction addresses I1 and M1 from the final whole-branch
review. Source checkpoint **`15fcc196b554c5a77b8b83818ddc2274d916fb70`** was
committed before either new platform run. The older `2378731` receipts and
publication negative controls above remain unchanged historical evidence.

Initialized cache/vector schemas are now validated in one coherent read-only
transaction. Only a confirmed absent schema requests initialization write
ownership, then rechecks the schema after acquiring it. WAL readiness reads can
encounter `SQLITE_BUSY_RECOVERY` during concurrent startup; the initial journal
probe and coherent schema probe handle that readiness condition. The private
initialization helper retries only SQLite BUSY/LOCKED, at one-millisecond polling
intervals, with a five-second budget **per readiness probe or ownership-acquisition
step**. This is not a five-second limit on the entire open/CLI command. Failed read
transactions drop before retrying. Connection `busy_timeout` remains zero;
ordinary cache/vector writes receive no retry. A prolonged competing initializer
can still cause a safe error after the budget; a later caller can retry. No startup
index lock or new schema/dependency/runtime was added by this correction.

The normal concurrent first-refresh regression also exposed a macOS creation
race: two `openat` calls against the same live directory descriptor with
`O_CREAT|O_NOFOLLOW` can produce one success and one ENOENT while the file exists.
A standalone Python reproducer isolates it without SQLite/Git/Rust orchestration.
The index-lock opener now handles only that creation ENOENT by reopening once
against the same pinned directory/name, retaining the flags except CREATE.
NOFOLLOW/NONBLOCK and regular-file/path-identity checks remain. There is no sleep,
unlink, repeated creation, or fallback for other errors. If the file remains
absent or unsafe, the operation fails safely. The reproducer, original/repeated
observations and development failures are preserved under
`.artifacts/tk10/final-fix-development/`.

### Covering regressions

- `normal_open_waits_at_index_lock_during_cache_write_then_redetects`: A pauses at
  real `begun`; B invokes normal `open_documents`, acknowledges opening, reaches
  real kernel-lock contention, then redetects A's commit and returns a no-op.
- `normal_open_redetects_after_active_cache_writer_is_killed`: the same opening
  schedule, followed by a newer edit and owner SIGKILL; B publishes exactly the
  latest complete state. Both tests compare full nodes/row IDs/source metadata,
  edges, FileRows, revision and FTS tokens, plus preserved vectors/corpus/Git.
- `normal_open_and_noop_finish_during_vector_write_and_held_index_lock`: a normal
  opener/no-op finishes while both independent owners are still active.
- `existing_schemas_validate_on_query_only_connections`: both schemas validate
  while SQLite forbids writes.
- `waiting_initializer_rechecks_committed_schema_for_both_databases` and
  `waiting_initializer_rejects_incompatible_schema_committed_by_owner`: channel
  acknowledgements after real SQLite contention prove the ownership recheck.
- `initialization_contention_expires_and_can_be_retried_safely`: the bound returns
  BUSY without partial schema, and a later retry succeeds with zero busy timeout.
- `parallel_initializers_leave_a_complete_schema`: both virgin openers must succeed;
  the previous allowance for one or both contention failures is removed.
- `initialized_open_succeeds_but_cache_write_still_returns_busy`: replaces the
  historical expectation that opening during an active writer itself must fail.
- `simultaneous_document_openers_initialize_and_publish_once`: two normal first
  invocations both initialize and refresh successfully, committing once.
- `concurrent_first_lock_creation_preserves_one_regular_file`: twenty actual
  two-caller creation schedules preserve one lock inode. The selected ENOENT-path
  tests cover exact existing bytes/inode, no fallback for ACCESS, and rejection of
  absent/symlink/FIFO/directory names. The full suites also rerun held-lock no-op,
  schema rejection, static/late database path and other confinement regressions.

Before the implementation fix, the three normal-open regressions, query-only
validation and strict simultaneous initialization all failed. Removing the schema
recheck as a compiling negative control makes both deterministic recheck tests
fail (exit 101). Removing the lock reopen makes the selected-error-path regression
fail (exit 101). Both files were restored byte-for-byte, then the complete gate
passed. Restored SHA-256: schema.rs
`06c657bd749f6ef42f3fa2f8bd76aee7be6ded504893106d91d6db30b3a29b13`;
filesystem/local.rs
`8330e0d3f8f515b244495c874fbfe64ac4379634b52ddfcfe62e8ba70f5cdd95`.
The old publication mutations were not rerun against the new checkpoint.

### New commands and receipts

```sh
./scripts/check-rust.sh
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk10/host-final-15fcc19
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk10/linux-final-15fcc19
```

All three commands exit 0. Each platform passes **172 Rust tests** (58 unit plus
114 integration, with one ignored child entry explicitly driven by parent tests)
and **nine Python runner tests**. Host manifest: 15 commands, all zero exits;
outer Linux: 8, all zero; inner source-built Linux: 13, all zero. Formatting,
compiler, Clippy, strict Rustdoc, schema parity, native capabilities, standalone
copied executable and dynamic-dependency checks pass. Each platform retains
**35 child receipts: 14 SIGKILL and 21 normal exits**. Linux acceptance runs with
network disabled after the source image build; image SHA-256 is
`7e93fb8d3475abf9224004d527ab413b155e41988a9c5c4028ecd906f9d42ce5`.

| Receipt | Manifest SHA-256 |
|---|---|
| `host-final-15fcc19/manifest.json` | `34bf4ce50b4780847cf003d12549dd8ecf858ca6bdd114b92635769adb01b94f` |
| `linux-final-15fcc19/manifest.json` | `857bb2360133bbeeb16bc572147da068ea837300e371a3dfdda352f7f6258929` |
| `linux-final-15fcc19/container/manifest.json` | `b733d1a8b655ddd7771c9a7a577c41e01f063f0edafe6bd24b1bcfd9fc08db7e` |

All **191 host inputs** match the committed checkpoint blobs and current files.
All **86 common host/inner-Linux inputs** match; the 105 host-only files are the
existing unselected historical/other harness inputs, with zero Linux-only files.
The sorted compact common-map SHA-256 is
`cb8376a01f87dcca94da409b3f11bef1378bacd8cd911a92244a251857ab7304`.
Copied executable hashes: macOS
`c01b9ae950849c1782043390ef6d115304bea252e554e17a49c65303234a5c9d`;
Linux `ebd3f881165ab8e97df4df845ccac1bc81cb4ad0ba26912308fc31d58b451709`.
`.artifacts/tk10/final-fix-summary.json` records the complete hash audit, per-command
results, receipt hashes and host-only list. Only the controller's plan document
was dirty when platform runners captured source state; no source changed after
the checkpoint. Subsequent acceptance/controller document edits do not change
those runner inputs.

The architecture check inspected the actual constructor path in lib.rs, service
interfaces/imports, storage ownership and private filesystem helper. No public
API/trait wiring changed; the new storage coordination stays inside storage and
the one lock-creation recovery stays inside filesystem. All added control hooks
remain `cfg(test)`/private. M1 is corrected above: the branch's direct pinned
rustix dependency is acknowledged without claiming a locked-version upgrade.

The existing live-read/timestamp, hostile ancestor-rename, physical power-loss,
CLI integration, platform and resource limits still apply. The new bound does
not guarantee opening when an initializer stays contended beyond its budget.
Focused independent fix review remains pending; this report does not claim that
review, merge, push, or GitHub issue work has occurred.
