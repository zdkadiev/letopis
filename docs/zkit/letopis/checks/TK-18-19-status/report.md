# TK-18/19 — current status integration

## Current disposition

TK-18 and TK-19 are functionally complete at exact merged source
`ad81a8c07e3aec2e8715f7762a3dd82f8c9b1e7b`. Host acceptance passed without
profile qualification. Linux ARM64 functional acceptance passed with the explicit
tmpfs and debug-symbol limits below; the earlier standard-overlay and 3 GiB attempts
remain preserved failures. Independent source, main-sync and evidence reviews found
no Critical/Important issue and approved the qualified completion.

TK-28 remains `in_progress`: this work delivers its status fields, file/graph
diagnostics and architecture, while TK-26-specific storage-failure acceptance and the
combined TK-27 lifecycle criterion remain pending.

Main synchronization is recorded below. No push, GitHub edit, Engram write, paid
inference, pilot, TK-16/17 or G-1/G-2 claim is part of this documentation handoff.

## Revisions and implemented behavior

The implementation sequence is:

- `d034aef` through `518f01e`: observation-only published storage and distinct
  current-hash counts, including missing-vector integrity validation.
- `46e84b4` through `0e41dbb`: stable configuration reads and nonblocking persistent
  lock observations with bounded positive-PID parsing.
- `deac241`: shared check/status diagnostics, `StatusService`, strict transport and
  real `lib.rs` composition.
- `96639e7`: the final review cleanup makes `sort_problems` consume and return its
  `Vec<CommandProblem>` instead of using an output-container parameter.
- `4173506`: merges verified local `main@870c275` after the earlier `origin/main`
  merge, preserving status and TK-24 inspection behavior; independent sync review
  approved the integration with no finding.
- `ad81a8c`: applies the incoming simplification rule by removing the
  constructor-argument-only `StatusDependencies` wrapper. `StatusService` directly
  owns storage and borrows its four collaborators; `lib.rs` and tests construct the
  service with struct literals. Behavior and interfaces are unchanged.

The final behavior observes one optional published database view. It reports the
published revision, distinct current knowledge hashes lacking usable vectors,
exhausted current hashes, busy index/embed locks and current safe problems. Sources
and orphan vector/failure rows do not inflate counts. Missing publication returns a
null revision and zero cache-derived counts without creating service files. Missing,
invalid or changed configuration never resets vector identity; query/config/exhaustion,
graph and source findings are sorted and deduplicated. Status does not refresh,
infer, launch drain or route through foreground background completion.

Lock PID values are best-effort observations rather than verified current ownership.
The kernel lock can transfer before the next owner writes its PID, briefly exposing a
previous valid value. Busy invalid/unreadable PID state fails safely; unlocked stale
PID bytes are not reported as a held lock.

## Behavior evidence and controls

- Final focused cleanup checks: `service::status` 8 passed and `service::check` 7
  passed, both with exit 0.
- The real status/check lifecycle test covers invalid markup, dangling file and
  anchor targets, self-loop, indirect includes cycle, missing source snapshot,
  repair/rename/delete/restore and restart. Unrefreshed edits retain the prior
  publication; explicit refresh changes current findings without rewriting sources.
- Real CLI checks cover strict empty-object transport, all five result fields,
  missing-cache noncreation, held index lock/PID reporting, no provider connection and
  no drain-side work. One late failure yields one safe error without partial success.
- Task 1/2 reports retain their focused RED/GREEN and mutation evidence. Task 2's
  destructive PID-file write mutation was rejected by auto-review and was not retried
  or bypassed; direct tests still assert unchanged bytes and inode.
- Tasks 3/4 have fresh green receipts and caught/restored behavior-changing mutations,
  but no reliable retained original implementation RED chronology. Those receipts do
  not retroactively establish test-first order.
- Final review found no Critical/Important code or architecture defect. Test-only
  cleanup `6f8e181` replaces one wildcard import with explicit names and renames the
  custom `lock_path` accessor to `get_lock_path`; no product behavior changed.

The plan-scoped reports and receipts are retained under
`.artifacts/tk18-19-status/sdd-final-96639e7/`. Final native receipts are under
`.artifacts/tk18-19-status/final-20261026/` and the current completion receipts under
`.artifacts/tk18-19-status/final-host-ad81a8c-20261026-1/` and
`.artifacts/tk18-19-status/linux-tmpfs-reduced-debug-ad81a8c-20261026-1/`.

## Final current acceptance at `ad81a8c`

The clean merged source and both outer platform manifests contain the same 333 inputs.
Canonical compact sorted-JSON SHA-256 is
`aab63834ebc8fb093780736680fbae936b7c83f6ed9782cd7dde5f7bf77555c6`.
Both source receipts name
`ad81a8c07e3aec2e8715f7762a3dd82f8c9b1e7b`; both dirty receipts are empty.
Later documentation commits do not relabel that native source.

| Platform | Result | Captured evidence |
| --- | --- | --- |
| macOS arm64 host | **PASS**; all 16 commands exit 0 | Schema parity and Python runner 10/10; `make verify V=1`; debug and release each have 740 distinct root passes plus 8 nested provider-child executions, 0 failed and 5 ignored, represented by 61 complete result-summary blocks; deadline 1/0 in 60.01 s; SQLite 3.53.2/vec1 0.8; strict docs, native probe, copied standalone and linkage PASS. Executable SHA-256 `24c281bb18857571134e0f74ec8e6483b1a4932ec11a2561bc7aad5917f63517`. |
| Linux arm64/LinuxKit bounded variant | **Qualified functional PASS**; all 14 commands exit 0 | Exact immutable image, network none, target tmpfs 4 GiB, `/tmp` tmpfs 1 GiB, host-bound receipts, `CARGO_INCREMENTAL=0`, `CARGO_BUILD_JOBS=2`, `RUST_TEST_THREADS=2`, `CARGO_PROFILE_DEV_DEBUG=0` and `CARGO_PROFILE_TEST_DEBUG=0`. Schema parity and Python runner 10/10; `make verify V=1`; both profiles have 740 distinct root passes plus 8 nested provider-child executions, 0 failed and 5 ignored; deadline 1/0 in 63.29 s; SQLite/vec1, strict docs, native probe, copied standalone and `ldd` PASS. Executable SHA-256 `90912a552e97cad2b536a777bcdad5595a3d379e22466efa04b4ed6b355cc581`. |

The Linux runner recorded a 215-path inner map with SHA-256
`9fbf3587dd3d533efe82dd80902193bcf05aa9db050cb6d48ab2b0ffe64b3550`.
Every inner path and hash matches the 333-path outer map; the 118 outer-only paths are
selectively uncopied `tests/` content. Receipt review found no mismatch. The inner
manifest SHA-256 is
`40834493d91e4785dca36fe39f6c0209c4b9ef52d1bce174ce24e1c81438ba57`.
One release child summary was split across two output lines; the child and its parent
both report `ok`, and review reconciled all eight child executions without a count gap.

This qualification changes only debug-symbol production for Cargo dev/test profiles;
optimizations, debug assertions, overflow checks, test selection, dependencies, source
and the release profile are unchanged. It establishes Linux functional, schema,
native, strict-doc, deadline, standalone and linkage behavior on ARM64 under the stated
mount/resource limits. It does **not** establish standard-overlay health, persistence
across machine/container shutdown, power-loss recovery, persistent-filesystem fsync
behavior, performance/resource or minimum-hardware gates, or x86_64 acceptance.

Historical platform attempts remain evidence rather than being renamed:

- the standard-overlay run failed after `make verify V=1` exited 2 and Docker later
  produced read-only overlay/ext4 errors; complete inner acceptance was unavailable;
- the 3 GiB target-tmpfs diagnostic passed schema/runner/fmt/check/Clippy but failed
  debug-test linking with `ENOSPC`; later stages were not reached.

The independent Linux evidence review approved this explicitly qualified functional
acceptance for TK-18/19 and requested no rerun. Main synchronization through `4173506`
and the wrapper flattening at `ad81a8c` were separately reviewed with no finding.

## Historical pre-main verification at `96639e7`

Commands ran from the repository root with two test threads and incremental builds
disabled:

```sh
RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/letopis-task3-target cargo test --locked --offline --lib service::status
RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/letopis-task3-target cargo test --locked --offline --lib service::check
RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/letopis-task3-target make verify
RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/letopis-task3-target /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk18-19-status/final-20261026/native-host-96639e7
RUST_TEST_THREADS=2 CARGO_INCREMENTAL=0 CARGO_TARGET_DIR=/private/tmp/letopis-task3-target /opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk18-19-status/final-20261026/native-linux-96639e7
```

The first sandboxed `make verify` attempt was not accepted as a product result: ten
owned-child signal operations returned `Operation not permitted` while 278 tests had
passed and three were ignored. The same exact command reran with the required process
permissions and exited 0 with the compact `ok` result. The host runner's explicit
`make verify V=1` records 730 passed, 0 failed and 5 ignored across 60 complete
result-summary blocks in each of debug and release. Formatting, all-target compilation/Clippy,
strict Rustdoc and all root tests passed.

| Platform | Result | Captured evidence |
| --- | --- | --- |
| macOS arm64 host | **PASS**; all 16 manifest commands exited 0 | Schema parity; 10/10 runner tests; debug/release 730/0/5 each; explicit deadline 1/0 in 60.01 s; SQLite 3.53.2, vec1 0.8 NEON/multi-threaded; native probe, copied standalone executable and linkage passed. Executable SHA-256: `c3f911da1c8d74c87dd627adfe72da3f8faf0514965f0f486802966cba9b0559`. |
| Linux arm64 Docker | **FAIL / platform acceptance unavailable** | Image build and inspect passed. Inner `make verify V=1` exited 2; its step log was not recovered, so the precise failed make step and cause are unavailable. Writing `/receipt/manifest.json` then failed with `EROFS`; receipt copy and owned-container cleanup also failed with `overlay2` read-only errors. Owned container `letopis-tk9-29c587e3f9184a4cad1bd3b39462bfef-run` is dead, exit 1, not OOM-killed. No complete inner profile/native/standalone result exists and none is claimed. |

At the Linux failure the host data volume had 117 MiB available. The task had already
removed only this worktree's inactive, rebuildable 1.2 GiB `target/` cache; no other
project/agent evidence, Docker image, Docker volume or unrelated cache was removed.
The failed owned container and task-owned image remain for recovery. Its cleanup error
is `driver "overlay2" failed to remove root filesystem: ... read-only file system`.
No capacity retry, Docker restart or broad Docker cleanup was attempted.

## Historical input identity at `96639e7`

Both outer native runners captured the same 330 per-file SHA-256 entries and exact
source commit `96639e7046271bc6507760eb02c056a4ccdc6658`. Canonical compact sorted-JSON
SHA-256 of each input map is
`c5a15f04c15c695351ac4cee820552e8e859baccdeadd3369c9a2e48e62aae03`;
the maps compare equal. Their `source-dirty.log` contains only the controller-owned
plan approval/checklist edit. Exact maps and command exits remain in the two manifests.
Later report/card commits must not be relabeled as the native source revision.

Final test-only cleanup commit `6f8e181` changes exactly
`src/service/status/tests/configuration.rs` and `tests/status_files.rs`. Its 330-entry
input map hashes to
`58180dd3fc3e3bd762e528950b49d222309be8082adf83e396aabe7a909c13dc`;
the other 328 native inputs are byte-identical. Formatting, status 8/0, status-files
9/0 and `make check` pass on these bytes. Per the controller ruling, native/release
acceptance was not duplicated for test-style-only changes.

## Preserved implementation rulings

- All `StatusFiles::read_config` failures are represented as one safe invalid/unreadable
  configuration finding. The confined reader cannot escape its root; storage, lock and
  provenance observation failures remain command errors. A future need to distinguish
  path-security classes would require a typed split rather than interpreting the current
  embedding error kind.
- Tasks 3/4 were implemented and reviewed as one unit because the private borrowed
  `StatusService` could not be meaningfully production-reachable before the same-call
  `lib.rs`/CLI composition. Splitting them would have required an unused allowance or an
  artificial public wrapper; neither abstraction was added.
- After the preserved standard-overlay failure, an authorized 3 GiB target-tmpfs run
  isolated Docker VM/ext4 writes while keeping the immutable image, standard inner
  runner, network none, host-bound receipts, incremental off, build jobs 2 and test
  threads 2. It proved the overlay was not required but reached a measured link-time
  capacity limit; it is retained as FAIL, not acceptance.
- One final authorized corrective run increased only target tmpfs to 4 GiB and disabled
  dev/test debug symbols. Its exact profile/resource limits are mandatory acceptance
  qualifiers; it does not become a standard-overlay, durability, minimum-hardware or
  unqualified Linux PASS.

## Architecture review

- `src/service/status.rs`, `status_types.rs` and private `check_problems.rs` own the
  use-case, result values and one shared diagnostic rule. They depend on domain values
  and application-owned traits, not concrete SQLite, filesystem, Git or HTTP types.
- `service/storage.rs`, `command_traits.rs` and `service/mod.rs` own the scoped
  `StatusStorage`, `StorageRead`, `StatusReader` and `StatusFiles` contracts and expose
  only required types. The final helper takes ownership and returns its result; no
  output-container parameter remains.
- `storage/status.rs` and `status_counts.rs` contain the concrete SQLite implementation;
  normal aggregates use SeaQuery identifiers and bound values. `filesystem/status.rs`
  contains confined config/lock implementation and never invokes the owner PID-writing
  path. Concrete structs and trait implementations stay together in their responsibility
  files; module files only declare/reexport them.
- `cli/input.rs`, `handlers.rs` and `output.rs` parse, dispatch through `StatusReader`
  and format X-1. They import no concrete storage/filesystem/Git/provider implementation.
  `src/lib.rs::run_status` performs actual constructor calls and uses a direct
  `StatusService` struct literal, keeping borrowed collaborators alive for the call;
  `main.rs` is unchanged and thin. No `StatusDependencies` wrapper or forwarding
  constructor remains after the accepted main simplification.
- No domain dependency, schema, async runtime, process, dependency, provider registry,
  diagnostic table, forwarding service or speculative abstraction was added. Touched
  implementation files remain focused and below the project's file-size guidance except
  the existing composition root, whose status assembly is one direct function.

This physical/import/visibility/trait/composition review found no self-review issue.
The independent final whole-branch review agreed: no Critical/Important source or
architecture finding. Its two test-style findings are fixed in `6f8e181`; scoped
fix re-review approved both changes with no new Critical/Important finding.

## Completion and remaining scope

- TK-18 and TK-19 are `done` for their delivered implementation and functional
  criteria under the exact Linux qualification above.
- TK-28 remains `in_progress`. Revision, lock PID, vector backlog/exhaustion, current
  file/graph/config/query problems and architecture are delivered. TK-26-specific
  storage-failure acceptance and the combined TK-27 lifecycle criterion remain open;
  this report does not infer them from component tests.
- TK-16/17, TK-26/27, G-1/G-2, paid/model/pilot/resource, Docker overlay health,
  power-loss/durability and minimum-hardware readiness remain unchanged.
- Merge, push and GitHub issue updates are separate controller publication actions.
