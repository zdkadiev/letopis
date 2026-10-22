# TK-10 — Production refresh acceptance

Date: 2026-10-22. Production implementation and process acceptance pass on macOS ARM64 and source-built Linux ARM64. Task 4 is ready for independent review; the controller owns the task/spec/plan acceptance marks and Engram record.

Source checkpoint: `23787312a8b7e714280158d04bc9ea19f1d72006` (`test(documents): prove process death and concurrent publication`). Tasks 1–3 end at `ac30e180c6efa4922c306dc0231c89606d996dc5`. Only the controller's three documentation edits were dirty during platform runs. All 191 host input hashes still match the restored current source tree after negative controls.

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

Inspected physical files, visibility, constructors, implementations and import direction. `src/lib.rs::open_documents` constructs actual Git, confined filesystem, Markdown and SQLite implementations and passes them to `DocumentService`. `service/mod.rs` owns traits; `service/document.rs` owns orchestration; `service/projection.rs` applies one file inside the caller's unit and reconciles source metadata. Concrete structs and inherent/trait implementations remain together in their responsibility files. Production services import application/domain contracts, not technology implementations. Storage keeps SQL/query builders private; the test oracle's native FTS5 vocabulary query is private test code. No new schema, dependency, async runtime, public test/fault API, future subsystem, or CLI scaffold was introduced.

Task 4 source changes: private `service/document/tests/process.rs`, its declaration in `tests.rs`, `cfg(test)` checkpoints in document/projection, and a private test-only clock/contention setter in `filesystem/local.rs`. All control environment reads are inside the private test module. `tests/storage/run.py` plus its regression test preserve child receipts without changing product runtime configuration. There are no blanket lint allowances. `./scripts/check-rust.sh` passes the configured maximum-function-length lint.

The source-built Linux run includes the prior confinement regressions: static candidate/parent/database symlinks, nonregular/FIFO replacements, late database-leaf/ATTACH substitutions and observed service-directory identity changes. Document-owned storage uses SQLite NOFOLLOW, inherited by vector ATTACH. Residual limit: there is no atomic descriptor-relative guarantee against an actively hostile actor concurrently renaming ancestors during SQLite's internal pathname/open sequence. No custom VFS or process-global directory change was added.

These are acknowledged process-death cuts **between** SQLite operations, not cuts inside COMMIT and not physical power-loss proof. Live concurrent edits are not an atomic corpus snapshot. The one-second racy policy does not cover coarser timestamp granularity or deliberately restored old non-racy timestamps. Unrelated external-source freshness and source_snapshot validation/pinning remain later tasks. No x86_64, resource/capacity/performance, model quality, full CLI/drain/cancellation JSON, physical power loss, or unrelated historical proof-harness execution is claimed. Historical TK-2 receipts were untouched.
