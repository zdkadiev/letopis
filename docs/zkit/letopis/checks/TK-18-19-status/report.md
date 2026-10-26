# TK-18/19 — current status integration

## Current disposition

The observational `status` implementation and its host verification are complete at
product source `96639e7046271bc6507760eb02c056a4ccdc6658`. Independent final
whole-branch review at `bd755c6` approved the source integration with no Critical or
Important finding. Its two nonblocking test-style findings are fixed in test-only
commit `6f8e181`; scoped fix re-review remains pending. The required Linux runner was
attempted on the exact native input map but did not pass. This report therefore does
not mark TK-18, TK-19 or TK-28 done.

No merge, push, GitHub edit, Engram write, paid inference, pilot, TK-16/17 or G-1/G-2
claim is part of this handoff.

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
`.artifacts/tk18-19-status/final-20261026/`.

## Final verification

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
`make verify V=1` records 730 passed, 0 failed and 5 ignored across 60 root result
blocks in each of debug and release. Formatting, all-target compilation/Clippy,
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

## Input identity

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
  `src/lib.rs::run_status` performs actual constructor calls and keeps borrowed
  collaborators alive for the call; `main.rs` is unchanged and thin.
- No domain dependency, schema, async runtime, process, dependency, provider registry,
  diagnostic table, forwarding service or speculative abstraction was added. Touched
  implementation files remain focused and below the project's file-size guidance except
  the existing composition root, whose status assembly is one direct function.

This physical/import/visibility/trait/composition review found no self-review issue.
The independent final whole-branch review agreed: no Critical/Important source or
architecture finding. Its two test-style findings are fixed in `6f8e181`; scoped
fix re-review is the remaining review action.

## Remaining gates

- Controller-owned scoped re-review of test-only fix `6f8e181` is pending. TK-18/19
  remain `in_progress` regardless because the required Linux gate is unavailable.
- Linux native/schema/standalone acceptance is not PASS for this source because the
  complete receipt is unavailable after the retained Docker storage failure above.
- TK-28 is `in_progress`. The current status surface is delivered, but TK-26-specific
  corruption/storage acceptance and TK-27-specific embedding lifecycle/exhaustion
  acceptance remain prerequisites; this report does not infer them from component tests.
- TK-16/17, G-1/G-2, paid/model/pilot/resource and publication/integration work remain
  unchanged.
