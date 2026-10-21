# TK-4 independent review and parent disposition

One fresh read-only whole-change review by `/root/review_tk4` (gpt-6-astra, high), after the initial macOS/Linux runs. It inspected every new runtime source, the consumed legacy helpers, both PASS manifests, all 59 then-current input hashes and mutation/TDD logs. Those receipts are retained in [before-review](before-review/). It did not independently rerun execution. Base and HEAD were both `c8f414b6e0f739015728b62cccbecbfc54deafd8`; new files were explicitly inventoried because the agreed branch remains uncommitted.

## Reviewer findings

**Critical:** none.

**Important 1 — detached-child signalling lacks durable ownership.** The old `kill_detached` sent SIGKILL to a numeric PID from a retained event. The child's 20-second read timeout could expire, allowing exit/reaping/PID reuse before the parent signalled. A live parent-side Event/socket did not pin process identity. The reviewer recommended a connection instruction that makes the actual peer signal itself, or a supervisor that prevents reuse; a prior liveness check alone would retain a race.

**Important 2 — interrupting the runner can leave a mutant and an escaped nested command.** The old mutation script changed actual untracked worktree sources and depended on `finally`. The outer runner could SIGKILL its group on timeout/Ctrl-C, bypassing restoration. Its nested Cargo commands started additional sessions, escaping that kill group. The reviewer recommended disposable sources/build output plus interruption-aware descendant cleanup and a regression that interrupts while a mutant is active.

**Minor 3 — “blocked waiter” boundary.** The waiter is paused in the fixture socket hook after a failed nonblocking probe, before entering blocking flock. The test proves cancellation/isolation of a contender that observed contention; it does not prove cancellation while executing the blocking syscall. Reviewer requested precise wording or an independently observed in-syscall schedule if required.

The reviewer praised actual TK-2/TK-3 reuse, ten-successful-probe scheduling, independent flock/state/native-byte oracles, persistent inodes/live decoys, session/null-descriptor checks and correctly scoped two-platform receipts. Verdict: **With fixes**, addressing both Important findings before completion. No second review is claimed.

## One fix pass and evidence

| Finding | Parent grade / result | RED → GREEN |
|---|---|---|
| Adopted PID identity | Important, fixed | `expired_or_mislabelled_event_never_signals_pid` first failed `disconnected event attempted numeric-PID signal` using only an owned live decoy as simulated reused PID. The connection now requests self-SIGKILL; claimed PID metadata is ignored. The disconnected case fails safely, decoy survives, and a directly owned child proves actual self-SIGKILL exit status. Complete Rust suite: 9/9. |
| Mutation interruption | Important, fixed | `test_outer_interrupt_keeps_sources_and_terminates_nested_command` interrupted the actual outer runner in a disposable reproducer. RED: `src/launch.rs` remained changed, nested EOF false, groups differed. GREEN: no original hashes changed, nested EOF true, same group, nonzero runner exit. Mutations now use a disposable copy of all four crates and independent build output; Cargo inherits the owned outer mutation group. |
| Waiter boundary | Minor, initially deferred; closed by user follow-up | New publish/reindex tests independently observe actual blocking flock before SIGINT on macOS/Linux; the pre-flock pause fails the same oracle. See in-flock-verification.json and the report follow-up. |

Logs: [identity RED](tdd/review-identity-red.txt), [identity GREEN](tdd/review-identity-green.txt), [interruption RED](tdd/review-interruption-red.txt), [interruption GREEN](tdd/review-interruption-green.txt). The first refreshed receipts reran nine Rust tests, the interruption regression, six compiling controls, fmt/Clippy and all legacy regressions after both fixes; these now live in before-flock-followup/. Current receipts include the subsequent 10/1/7 follow-up. The historical 59-file receipts are not used to claim verification of final code.

## Every boundary the reviewer declined to judge

Each row is a parent ruling, including its consequence if relied upon too broadly.

| Set aside | Parent ruling | Cost / limit |
|---|---|---|
| Independent execution | Accept read-only review of receipts/current hashes; author runs platforms. | No separate person's rerun is claimed. |
| Entire legacy/third-party re-audit | Reuse unchanged pins/helpers and complete regressions. | Unrelated latent upstream/legacy defects remain outside this proof. |
| Production CLI/transport/cancellation/status/deployment | Keep scratch scope; later TK-11/TK-29/TK-42 own delivery. | This branch is not a finished product runtime. |
| General concurrency/liveness, orphan collection, final-empty-query/exit handoff | Limit conclusions to selected exclusion/cancellation schedules. | No fairness/stress/general liveness guarantee; new work in the exit window may await another command. |
| Other architectures/OS/filesystems/mounts | Limit evidence to measured macOS ARM64/Linux ARM64 environments. | No Windows, x86-64 or network/other filesystem guarantee. |
| Physical durability/resources | Treat these as process-failure/functional checks. | Power-loss, exhaustion, capacity, stat-10k and RAM gates remain. |
| General inputs/provider/hostile local clients | Keep trusted bounded Markdown/config/provider/channel fixtures. | No full parser, live-edit, JCS, real transport/credentials or adversarial local-channel guarantee. |
| Exact adopted exit status/reaping | Observe detached connection/kernel release; direct child proves self-SIGKILL; rely on init for adopted reaping. | No exact waitpid/reaping proof for every adopted child. |
| Administrative completion | Parent updates local cards/report/Engram only after final checks. | Commit/merge/push/GitHub closure remain separate user actions. |

## Execution rulings

- Keep changes uncommitted as the approved plan requires; retain branch/worktree and ignored execution ledger. Integration still needs a later user instruction.
- The review-package helper rejected the empty commit range. The reviewer therefore read the explicit new-file inventory plus all receipt hashes. The risk of omitting untracked files was bounded by checking the complete input inventory.

The historical reviewer verdict remains unchanged as a record of that snapshot. The subsequent user-requested follow-up closes the Minor using native OS observation and a seventh compiling control, with refreshed 10/1/7 platform receipts. This is not a second independent review.
