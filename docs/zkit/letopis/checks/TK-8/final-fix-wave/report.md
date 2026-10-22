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
