# TK-8 — implementation and verification report

Date: 2026-10-22 (Europe/Moscow). Implementation is ready for the controller's independent whole-branch review. This report is the implementer's self-review, not independent acceptance. Task/epic/roadmap status and review/Engram bookkeeping remain unchanged.

Base: `2a29ab16603569bb95e0e5d63ce4942180050686`.
Implementation commit: `9d9413ba415648dea76a2adc41cb275c7dee089f` (`feat: add immutable Git snapshots and verified retention`). A separate evidence commit records these logs/report; its own hash is available in the task handoff/Git history, avoiding a self-referential receipt.

## Implementation and architectural boundaries

| Files | Responsibility and checked boundary |
|---|---|
| `src/service/ports/git.rs`, `src/service/{mod.rs,ports/mod.rs}` | Application-owned `GitSnapshotPort`, borrowed `SnapshotFile`, typed safe `SnapshotError`, `RetainedTree` with private OID/accessor. No concrete adapter, Git commands, process-status types, CLI/SQL DTOs or domain changes. |
| `src/adapters/git/mod.rs` | Private concrete `GitCliAdapter`; version-first repository discovery, absolute root and SHA-1/SHA-256 format; `impl GitSnapshotPort` delegates to the three private implementation modules. |
| `src/adapters/git/process.rs` | Sole production `Command` use. Direct argv, concurrent scoped stdin/stdout/stderr handling, close/drain/wait on failure, safe private I/O/exit causes. Removes inherited `GIT_*`, sets deny-transport/no-replace/no-lazy-fetch/no-prompt/C locale; disables hooks per command, adds fsync settings to writes. No production timeout, shell, async, retries or new dependency. |
| `src/adapters/git/objects.rs` | Validate all input paths before writing; borrowed input bytes become mode-100644 blobs and bottom-up NUL-delimited trees. Exact object type/existence; raw content is rehashed without `-w`; literal per-component tree traversal never treats paths as revision/pathspec/shell input. |
| `src/adapters/git/refs.rs` | Candidate-only tree/blob closure walk with type-consistent visited OIDs; skip gitlink dereference. Check exact direct ref; reject symbolic/conflicting/corrupt refs; zero-OID `update-ref --no-deref` CAS and confirm exact ref after success/conflict. Only successful pin constructs `RetainedTree`. |
| `src/lib.rs`, `src/adapters/mod.rs` | Explicit composition via public `open_git_snapshots(start) -> Result<impl GitSnapshotPort, SnapshotError>`; concrete adapter module is crate-private; tests use the public factory/trait. No forwarding service, future subsystem or CLI scaffold. |
| `tests/support/git.rs`, `src/adapters/git/tests.rs`, `tests/git_{snapshots,retention}.rs` | Std-only isolated fixtures, exact HEAD/index/config/user-ref/live-file state, fake-executable seam, disposable corruption/GC, child supervisors, public contract tests. Test-only `Command` use is kept out of production paths. |
| `tests/git/{Dockerfile,README.md}`, `.dockerignore` | Digest-pinned Rust Linux build with Git/rustfmt/Clippy, locked fetch, source/tests/Markdown fixtures copied and compiled in-container. Runtime uses `--network none` with no host binary mount. |

Architecture self-review followed `architecture.md#program-structure-review`: inspected actual files, visibility and imports. Domain and existing Markdown parser are unchanged. Service imports domain/contracts, Git implements the service-owned port, and only lib.rs chooses the concrete implementation. `logs/architecture-command-location.log` distinguishes sole production process.rs commands from cfg(test) fixture commands. `git diff --check`, fmt and Clippy passed. No claim of an independent architecture review is made.

## TDD and debugging evidence

| Cycle | Behavioral RED | GREEN and interpretation |
|---|---|---|
| A: discovery/process | `tdd/A-red.log`: compiling minimal declarations fail version rejection, root discovery, version-before-object boundary and I/O result assertions (4 failures, 1 pass). No compiler/import failure counted as RED. | `tdd/A-green.log`: 6 private tests pass, including the process policy assertion added within the A boundary; `tdd/A-final-green.log` passes again after final refactor. Initial logs contain expected temporary unused-stub warnings; final verification has no warnings. |
| B: exact objects | `tdd/B-red.log`: public factory/trait stubs fail all 5 creation/type scenarios. | `tdd/B-green.log`: 5 public scenarios pass, each looping both formats. `tdd/B-full-green.log`: original 30 plus A6/B5 = 41 tests pass. Pin was still an explicit internal stub at this boundary and was not advertised as a completed component. |
| C: retention | `tdd/C-red.log`: successful pin and concurrent retention assertions fail on the pin stub (2 failures, 4 passes). | `tdd/C-green.log`: final 8 retention tests pass, each running both formats. The separate unretained unique-tree control really disappears after reflog expiry/GC/prune, so positive survival is not Git grace-period luck. |
| Self-review: dangling symbolic ref | `tdd/C-dangling-symbolic-red.log`: pin incorrectly returned `RetainedTree` for a pre-existing dangling symref. | `tdd/C-dangling-symbolic-green.log`: fixed. Root cause: `for-each-ref` omits dangling symbolic refs and zero-OID CAS can replace one. Explicit `symbolic-ref -q` probe now rejects symbolic refs; only exit 1 means non-symbolic, other errors propagate. Exact NUL-field `for-each-ref` verification remains. |
| Self-review: contradictory descendant types | `tdd/C-conflicting-types-red.log`: malformed tree names the same real blob as both tree/blob; OID-only deduplication incorrectly accepted it. | `tdd/C-conflicting-types-green.log`: fixed with visited OID-to-type consistency, retaining deduplication. Missing or corrupt descendants still fail before any ref is written. |

Two setup defects were identified separately from product failures: macOS `/var` versus `/private/var` canonical temp roots, and copying over Git's read-only loose-object files. The fixture now canonicalizes its root and explicitly removes only its owned object before writing the deliberate corruption. Neither setup error was counted as behavioral RED or a successful negative control. Supervisors now guard and reap only their owned children even during assertion unwinding. The process fake writes at least 256 KiB to stderr and stdout before consuming 1 MiB stdin and exiting nonzero; the bounded worker terminates and verifies the Git PID is reaped and canary bytes are absent from diagnostics.

## Acceptance and named test evidence

All snapshot/retention integration scenarios run fresh disposable SHA-1 and SHA-256 repositories through the public factory/trait. The original 30 root integration tests remain intact.

| TK-8/spec criterion | Named tests / actual result |
|---|---|
| Consistent tree, exact bytes, no user commit/index/HEAD/config/live changes | `exact_bytes_without_index_or_commit`: empty/binary/Unicode/LF/CRLF/no-final-newline/nested input, exact file set, reordered determinism, pre-existing staged+unstaged state, active clean/EOL trap untouched; S1 still reads original after S2 and live deletion. PASS. |
| Literal paths/no shell | `paths_are_literal_and_never_shell_input`: colon, leading dash, wildcard, spaces and `$(touch SENTINEL).md`; exact readback and sentinel absent. PASS. |
| Empty tree and invalid input rejected before writes | `empty_tree_and_invalid_paths`: duplicate, file/prefix collision, `.git` component/root `.letopis`; object counts unchanged on invalid requests. PASS. |
| Exact OID length/type, meaningful missing versus internal errors, corruption redaction | `tree_types_and_missing_objects_are_distinct`: wrong repo OID length, commit/tag, absent root/path, dangling child, tree/gitlink leaf, valid compressed content copied under wrong OID. `replacement_refs_do_not_change_bytes` confirms replace refs do not alter explicit snapshot bytes. PASS. |
| Inspect-like tree and external source basis retained, reopen/GC closure | `retained_snapshots_survive_gc`: both create-tree and externally built nested tree without a user commit, exact service refs, repeated pin, unchanged user state, reflog expiry/GC/prune/pack-refs, reopen/read and strict no-reflogs fsck. PASS. |
| GC negative control | `unretained_unique_tree_is_collectable`: unique snapshot is readable before GC, has no service ref and loses its unreachable tree after prune. PASS. |
| Partial tree/ref failure not accepted | `incomplete_tree_and_ref_failure_do_not_pin`, `conflicting_descendant_types_do_not_pin`: missing/corrupt descendant, incompatible entry type, forced update-ref lock failure yield InternalError with no service ref. PASS. |
| Symbolic/conflicting/corrupt refs rejected, user branch preserved | `symbolic_and_conflicting_refs_are_rejected`: includes both live and dangling symbolic refs; other value/corrupt ref; safe errors and user state unchanged. PASS. |
| Concurrent idempotency | `concurrent_pin_is_idempotent`: barrier starts two separately owned test processes, both pin successfully and resolve the identical direct ref. PASS. |
| No lazy fetch; gitlinks are not dereferenced | `promisor_miss_never_fetches`: reachable disposable remote/transport trap is proved before enabling promisor config; protocol allow config cannot override subprocess deny policy; read/pin fail and helper marker/object remain absent. `gitlinks_are_retained_without_dereferencing`: missing gitlink target is not fetched/dereferenced and read-as-blob rejects it. PASS. |
| Version/discovery/process safety | `version_gate`, `open_rejects_old_git_before_object_calls`, `discovers_root_from_subdirectory`, `open_rejects_non_repository_and_bare`, `subprocess_drains_and_reaps_on_io_failure`, `process_removes_inherited_overrides_and_hardens_writes`. PASS. |
| Environment routing isolation | Separate process with all four trap overrides runs all 13 public Git tests. Every trap file hash and set remains identical (`logs/routing-trap-unchanged.log`). Private policy worker additionally covers config-env removal and exact safe/write flags. PASS. |
| Physical module structure and public composition | Files/imports inspected as above; tests call root factory/trait; only production process.rs owns Command. PASS for implementer's self-review; independent review remains pending. |

## Commands, versions and actual platform results

Full stdout/stderr logs and exact argv/exit codes are recorded in `verification.json` and `logs/`.

- Host: Darwin arm64; `git version 2.50.1 (Apple Git-155)`; `rustc 1.95.0 (59807616e 2026-04-14)`; `cargo 1.95.0 (f2d3ce0bd 2026-03-21)`.
- `cargo +1.95.0 test --locked --offline`: exit 0; 49 tests = 6 private Git unit + original 30 integration + 5 snapshot + 8 retention; 0 failures/ignored. Doc tests: 0.
- `cargo +1.95.0 fmt --all --check`: exit 0.
- `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings`: exit 0, no warnings.
- Separate `env GIT_DIR=<trap>/git GIT_WORK_TREE=<trap>/work GIT_INDEX_FILE=<trap>/index GIT_OBJECT_DIRECTORY=<trap>/objects cargo +1.95.0 test --locked --offline --test git_snapshots --test git_retention`: exit 0; 13 tests, trap unchanged.
- `docker build -f tests/git/Dockerfile -t letopis-tk8-check .`: exit 0. Rust base digest exactly `sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1`.
- Final image `sha256:53b869cc11b204ee9ff7baad617ef81276652a1da279826b1d21f6b1629f2a04`, Linux/arm64. Git `2.39.5`; Rust/Cargo same versions as host; `uname -sm`: `Linux aarch64`.
- `docker run --rm --network none letopis-tk8-check`: exit 0; 49 tests pass, fmt and Clippy `-D warnings` pass. Container compiles copied source, with locked dependencies fetched at build time.
- `git diff --check`: exit 0.

The SHA-256 receipt includes all copied source/test/config inputs (including unchanged dependency/Markdown inputs), every full log, TDD RED/GREEN evidence and this report. Existing TK-7 receipts were not edited or attributed to this new suite. No persistent Docker/host runtime defect or network blocker occurred.

## Limits, concerns and handoff

No known implementation blocker remains after the two self-review fixes and final platform checks. These checks establish ordinary object integrity/closure, direct-ref CAS behavior, exact-byte reads, process isolation policy and GC reachability. They do not prove physical power-loss durability, platform file/directory/full-flush barriers, x86_64, resource limits or atomic capture of concurrently edited live files. The real tested Git versions are 2.50.1 and 2.39.5; minimum 2.29 is a version-gate/documented-subset contract, not an actual Git 2.29 execution claim.

The component has no publication tree, before/after history, SQL copy, future service framework, TTL, background worker, purge or automatic commit/GC. Creation alone is deliberately unretained; future inspect orchestration must call pin before exposing a snapshot. Concurrent hostile same-UID repository mutation is outside this boundary, as accepted in the spec.

Independent whole-branch review, any resulting fixes, review.md, task/epic/roadmap acceptance and schedule/Engram updates belong to the controller. No merge, push, GitHub write, issue closure or other-worktree mutation was performed.

Evidence packaging note: raw Cargo logs intentionally preserve their original blank lines and trailing spaces. An unscoped staged whitespace check therefore reports log-only whitespace. The final whole-branch `git diff 2a29ab16603569bb95e0e5d63ce4942180050686 --check -- . :!docs/zkit/letopis/checks/TK-8/logs :!docs/zkit/letopis/checks/TK-8/tdd` exits 0 for source, tests, config and prose; raw logs were not edited to hide this distinction.


# Fix round 1 — preserve the discovered repository path

Fix base: `2df19b12e0aeb20ba3ccab030326678ad61adee6`.
Implementation commit: `fa1fff52e459eecc2afb43d460c524320902cc2c` (`fix: preserve newlines in discovered Git root paths`). Evidence is a following local commit. Independent scoped re-review and TK-8 acceptance remain controller-owned.

## Changed behavior and root cause

`rev-parse --show-toplevel` adds one newline delimiter. `trim_end_matches('\n')` also removed newline bytes belonging to the actual directory name. When both `repo` and `repo\n` existed, opening the latter bound to the former and wrote its objects/service ref there. The fix in `src/adapters/git/mod.rs` removes exactly one output delimiter with `strip_suffix('\n')`, rejecting an absent delimiter as an invalid response. No other product behavior or module boundary changed.

Covering regression: `adapters::git::tests::discovers_trailing_newline_repository_without_touching_sibling` in `src/adapters/git/tests.rs`. For each SHA-1/SHA-256 format it initializes sibling repositories, opens the newline-suffixed target through the public factory, creates/pins a snapshot, verifies the sibling object counts and refs remain identical, verifies the ref exists in the target, checks exact readback and checks the discovered root. Setup commands must succeed; setup errors are not accepted as a negative control.

## Behavioral RED/GREEN

Exact focused command for both runs:

```sh
cargo +1.95.0 test --locked --offline --lib adapters::git::tests::discovers_trailing_newline_repository_without_touching_sibling -- --nocapture
```

- `red.log`: exit **101**; assertion failed: `opening repo\n must not write objects into sibling repo`. The previously empty sibling gained two loose objects. Result: **0 passed, 1 failed, 6 filtered out**. This was a compiling behavioral failure before the product fix.
- `green.log`: exit **0**; named regression **ok**, **1 passed, 0 failed, 6 filtered out**. Both formats execute inside that test.

## Host/Linux verification of changed source

All commands and statuses/full logs are in this directory and `verification.json`.

| Exact command | Actual result |
|---|---|
| `cargo +1.95.0 test --locked --offline` | exit 0; **50 tests PASS** (7 private Git + original 30 integration + 5 snapshot + 8 retention), 0 failed/ignored; 0 doc tests |
| `cargo +1.95.0 fmt --all --check` | exit 0 |
| `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings` | exit 0, no warnings |
| `docker build -f tests/git/Dockerfile -t letopis-tk8-check .` | exit 0 |
| `docker run --rm --network none letopis-tk8-check` | exit 0; **50 tests PASS**, fmt and Clippy `-D warnings` PASS |
| `git diff 2df19b12e0aeb20ba3ccab030326678ad61adee6 --check -- src` | exit 0 |

Host: Darwin arm64, Git 2.50.1 (Apple Git-155), rustc 1.95.0 (59807616e 2026-04-14), Cargo 1.95.0 (f2d3ce0bd 2026-03-21). Container: Linux aarch64, Git 2.39.5, same Rust/Cargo. New image: `sha256:5df3cfade229b5a574a2c6efd9f65a1d1a9d8d94c746b12f2f3ed6f08fdf29d9`. The container compiles source and runs with no network or host binary mounts.

The original TK-8 logs/report/receipt remain byte-for-byte unchanged. This separate round-1 receipt records updated source SHA-256, new full logs, and the original receipt SHA-256. Raw Cargo logs retain their original whitespace. No extra broad reruns were performed after these checks on unchanged source.

## Self-review and remaining concerns

The production diff changes only delimiter handling in repository discovery; the regression exercises the visible wrong-sibling writes rather than just comparing a string helper. No fixture framework, public API, process policy, roadmap, task acceptance, Engram or unrelated worktree changes were made. No known blocker remains for this fix. Actual Git 2.29 execution, physical durability, x86_64, atomic live capture and resource limits remain the earlier explicitly unverified limits. Independent review is pending.

Round-1 handoff commits: implementation `fa1fff52e459eecc2afb43d460c524320902cc2c`; evidence/final HEAD `a6af7d7b6536a65d85f11f6f02807a906b0e640a`. Independent scoped re-review remains pending.


# Final fix wave — reject external object alternates

Date: 2026-10-22 (Europe/Moscow). Fix base: `a6af7d7b6536a65d85f11f6f02807a906b0e640a`.
Implementation/boundary commit: `92cf8bd72708e9c91c506b3e342c14bc1ea18b78` (`fix: reject Git object alternates before snapshot operations`). Evidence is a following local commit; its hash is available in Git history/the handoff. This is the implementer's fix report; the controller owns the one scoped final re-review and acceptance. Task, epic and roadmap are not marked done here.

## Finding, accepted boundary and implementation

The Important final-review finding was reproduced: Git resolves objects via `objects/info/alternates`, including ordinary `git clone --shared`. A borrower service ref does not protect donor objects from donor GC. `hash-object -w` can deduplicate against the donor without materializing the blob locally. The prior closure check therefore accepted a tree whose closure could disappear while its exact service ref remained.

The controller chose explicit refusal, rather than object materialization or repacking. `src/adapters/git/mod.rs` now resolves `objects/info/alternates` using `git rev-parse --git-path`, with the actual gitdir/common-dir layout. Relative output resolves from the same repository root used as Git cwd; exactly one LF delimiter is removed. The adapter uses `symlink_metadata` and refuses **any existing alternates path**, including an empty/inactive file, directory or symlink. It does not read or parse donor configuration. Missing path is allowed; inspection/discovery failure is a safe InternalError. Opening and each create/read/pin recheck this invariant, including repeated pin of an existing ref. Rejection precedes object/ref writes and RetainedTree construction; diagnostics contain static context, without donor paths or canary payload.

No public API shape, async/retry/timeout framework, new dependency, materialization, repack or user configuration mutation was added. The established process boundary still removes inherited Git routing/alternate environment overrides. The new command belongs to the Git 2.29 plumbing subset; this is not an actual Git 2.29 execution claim.

The accepted cost is explicit in the design spec, factory rustdoc and test README: shared/reference clones require a self-contained clone; empty/inactive alternates configuration must be removed separately. The adapter performs neither conversion nor configuration cleanup. Checkout roots containing non-UTF-8 names remain safely unsupported by the existing discovery conversion; a byte-path API was deliberately not implemented. The added linked-layout test verifies discovery only and does not add multi-worktree product semantics. Concurrent object-store configuration mutation during an operation remains outside the accepted same-UID-writer boundary; entry checks are not a lock or sandbox.

## Named regressions and real RED/GREEN

All three public factory/trait regressions are in `tests/git_retention.rs` and run both SHA-1 and SHA-256 fixtures. `tests/support/git.rs` adds only a test-owned ordinary shared-clone helper.

| Test | Behavioral RED before product change | GREEN after fix |
|---|---|---|
| `shared_clone_is_refused_before_donor_gc_can_break_retention` | Exit 101; both formats printed that an accepted shared snapshot lost its donor blob after GC while its service ref remained exact; final assertion showed `[false, false]` refusal results. This proves real public create/pin success, borrowed nonlocal blob, actual donor prune, failed read and exact surviving ref. | Exit 0; open explicitly refuses. The disposable donor's unreferenced blob is still really pruned. Borrower has no service ref; donor/borrower HEAD, index, config, user refs, fixture live bytes and alternates bytes remain unchanged by adapter operations. |
| `alternates_added_after_open_refuse_operations_without_writes` | Exit 101; `alternates introduced after open must be refused` assertion. Create, pin and read all execute before this assertion. | Exit 0; each returns the safe alternates InternalError. Object counts, user state and alternates bytes are identical, and no service ref is created. |
| `linked_worktree_checks_common_object_store_and_refuses_inactive_alternates` | Exit 101; a linked worktree accepted an empty common-store alternates file. | Exit 0; first proves create/pin/read without alternates; then open and repeated pin refuse empty/inactive common-store alternates. User state, exact refs and configuration bytes stay unchanged. Removing only the fixture-owned inactive file restores successful read. |

Exact focused command for each named test, before and after the product change:

```sh
cargo +1.95.0 test --locked --offline --test git_retention shared_clone_is_refused_before_donor_gc_can_break_retention -- --exact --nocapture
cargo +1.95.0 test --locked --offline --test git_retention alternates_added_after_open_refuse_operations_without_writes -- --exact --nocapture
cargo +1.95.0 test --locked --offline --test git_retention linked_worktree_checks_common_object_store_and_refuses_inactive_alternates -- --exact --nocapture
```

Full output is preserved in `shared-clone-{red,green}.log`, `late-alternates-{red,green}.log` and `linked-alternates-{red,green}.log`. Each compiling RED had 0 passed/1 failed, exit 101; each GREEN had 1 passed/0 failed, exit 0. No compiler/setup failure was counted as RED. The shared-clone test deliberately runs donor GC before its final refusal assertion so that the RED demonstrates the retention failure in both formats rather than stopping at the first open mismatch.

## Fresh host/Linux verification

| Exact command | Observed result |
|---|---|
| `cargo +1.95.0 test --locked --offline` | Exit 0; **53 tests PASS** = 7 private Git + original 30 integration + 5 snapshot + 11 retention; 0 failed/ignored, 0 doc tests |
| `cargo +1.95.0 fmt --all --check` | Exit 0, no output |
| `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings` | Exit 0, no warnings |
| `docker build -f tests/git/Dockerfile -t letopis-tk8-check .` | Exit 0 |
| `docker image inspect letopis-tk8-check --format '{{json .}}'` | Exit 0; Linux/arm64 image `sha256:295896e484d95fab93c6e9493605f8a48c056d8aeef7d1a666627d442508798d` |
| `docker run --rm --network none letopis-tk8-check` | Exit 0; **53 tests PASS**, fmt and Clippy `-D warnings` PASS |
| `git diff a6af7d7b6536a65d85f11f6f02807a906b0e640a --check -- src tests docs/superpowers/specs/2026-10-22-tk-8-git-snapshots-design.md` | Exit 0 |

Host: Darwin arm64; Git `2.50.1 (Apple Git-155)`; rustc `1.95.0 (59807616e 2026-04-14)`; Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`. Container: Linux aarch64; Git `2.39.5`; same Rust/Cargo. Rust base digest remains `sha256:6258907abe69656e41cd992e0b705cdcfabcbbe3db374f92ed2d47121282d4a1`. The container compiles copied source with build-time fetched locked dependencies, runtime `--network none` and no host binary mounts.

Exact argv, exit codes, elapsed time and full untouched output are in `commands.json`, `verification.json` and the corresponding logs. Source did not change after this full successful host/Linux refresh; no additional broad test runs were made. The separate historical routing-trap run was not repeated or presented as fresh evidence in this wave.

## Evidence integrity, self-review and concerns

`verification.json` records SHA-256 for all current copied source/test/config inputs, the amended design spec, the appended SDD task report and every new log/report/command record. It also records hashes of both prior verification receipts and verifies every file in their historical evidence manifests against its original hash. Original TK-8 and fix-round-1 reports/logs/receipts remain byte-for-byte unchanged; this report is appended to the existing SDD task report, preserving the earlier report text. Raw Cargo logs preserve their original whitespace; they are not rewritten to make whitespace checks appear clean.

Self-review checked the actual six-path product/test/spec diff: Git-specific discovery remains private to the Git adapter; public factory docs state its boundaries; the port/domain/API shapes and process policy are unchanged; only test-owned fixtures invoke clone/worktree/GC. The three new regressions exercise real Git behavior and visible refusal/no-write effects. No known implementation blocker remains for the accepted refusal rule. This is not the controller's independent review.

Actual Git 2.29 execution, physical durability/power loss, x86_64, full/atomic live capture, source comparison, SQL/CLI, resource/throughput/extreme-depth limits, cancellation/TK-29, hostile same-UID mutation, multi-worktree product semantics, repository-wide fsck and historical harness reruns remain the accepted deferrals. No merge, push, GitHub/Engram write, acceptance-ledger update or other-worktree mutation was performed by this fixer. Scoped final re-review and all task/epic/roadmap acceptance gates remain pending with the controller.
