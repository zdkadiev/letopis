# TK-4 execution report

Status: PASS on macOS ARM64 and Linux ARM64 after the two Important review fixes; TK-4 acceptance completed. The remaining Minor was closed by the user-requested in-syscall SIGINT follow-up.

## Scope

Scratch process proof for MC-5, NFR-2/NFR-5 and FR-31/FR-32, on top of unchanged TK-2 publication and TK-3 embedding helpers. Implementation and reproduction live in [tests/runtime](../../../../../tests/runtime/README.md). No production CLI, IPC service, permanent worker, PID registry or schema change was added. G-0 remains open until TK-6.

Worktree `/Users/zdkadiev/.codex/worktrees/tk-4/letopis`, branch `TK-4`, base `c8f414b6e0f739015728b62cccbecbfc54deafd8`. The proof and reporting receipts below were captured before integration. The user subsequently requested committing TK-4 and merging it into local `main`; the resulting commit hashes and final integration checks are recorded in Git and shared Engram. Push and issue closure are separate actions.

## Acceptance and independent oracles

| Criterion / command | Schedule and expected result | Actual |
|---|---|---|
| `cargo test --locked --test contention ten_commands_publish_one_delta -- --exact` | Ten commands detect the same delta. Hold each admitted publisher and independently try kernel flock. Exact nodes/edges/files/FTS, one revision increment, unchanged corpus; an already active drain stays the only drain while all ten commands finish. It then stores exact vectors for current texts. | PASS on both platforms |
| `... ten_probe_winners_admit_one_drain -- --exact` | Hold ten live callers after each successful probe has been released. Release all before child-side exclusion. Hold first owner until nine reject. All callers return; independent flock sees the owner; literal native vector bytes and empty work prove completion. | PASS on both platforms |
| `... dead_owners_and_stale_pids_do_not_block -- --exact` | SIGKILL owners of both locks; direct signal status is SIGKILL. Next owner acquires same persistent inode, including stale/invalid and unrelated owned-live PID contents. Decoy survives; modes 0700/0600. | PASS on both platforms |
| `... launcher_exit_does_not_wait_or_leak_locks -- --exact` | Caller returns while child is paused. Independent index probe is free, embed held. Session differs/is child's PID; descriptors 0/1/2 match /dev/null type/device. Busy caller does not spawn; after detached owner death a new owner acquires same inode. | PASS on both platforms |
| `cargo test --locked --test cancellation sigint_before_commit_keeps_exact_cache -- --exact` | Eight real SIGINT cuts: publish/reindex × projections, fts, fingerprints, before_commit. Assert actual SIGINT status and exact old snapshot (project/nodes/edges/files/FTS), unchanged corpus and successful retry. | PASS on both platforms |
| `... sigint_after_commit_keeps_new_cache -- --exact` | Two actual SIGINT cuts after commit. Independently expected complete fixture plus exact committed snapshot survives. | PASS on both platforms |
| `... sigint_waiter_leaves_owner_and_drain_alive -- --exact` | Cancel a waiter in an owned fixture group containing the publisher: publisher/drain remain alive and finish. Conversely cancel publisher while another waiter is paused: exact old cache is observed before waiter proceeds and publishes. | PASS on both platforms |
| `... failed_or_lost_spawn_keeps_work_recoverable -- --exact` | Missing executable: committed publication, no attempts/vectors, work still eligible. SIGKILL after probe: same. Later no-change command drains. SIGKILL after confirmed detached start: child completes independently. No-work caller spawns nothing. | PASS on both platforms |

The runner exports the checksum-verified native extension before each command. Direct Cargo commands require that same `LETOPIS_VEC1_EXTENSION`; prefer `python3 -B tests/runtime/run.py --evidence NEW_DIRECTORY` for reproduction. Exact commands, environment and logs are retained in the manifests.

## Stack and platforms

Rust 1.95.0, rusqlite 0.40.2, libsqlite3-sys 0.38.2 / SQLite 3.53.2, vec1 0.8. Source pin: SQLite `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`, SHA-256 `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`. TK-1 checks the source; the runner resolves matching SQLite headers and compiles a fresh extension.

- macOS ARM64 receipt: `macos-arm64/manifest.json` — PASS, macOS 26.6.2 ARM64.
- Linux ARM64 receipt: `linux-arm64/manifest.json` — PASS, Linux ARM64 Docker.
- Docker: `letopis-tk2:rust-1.95.0`, image ID `sha256:aebf49e1f99b1406d6db6e5c02ed44585f2174fde88fce56e985265e515c9bf7`. `--init` reaps adopted children; the host Cargo registry is mounted read-only with offline Linux toolchain selection. Host binaries are not used for Linux.

[Platform audit](platform-audit.json) verifies both PASS manifests hash the same 60 current input files, including all reused helpers. Existing evidence directories are rejected without modification (retained under TDD).

Each final full run checks ten TK-4 Rust integration tests, one Python outer-interruption regression and seven compiling isolated negative controls, fmt/Clippy, the entire TK-3 runner (11 Rust + 2 Python), TK-2 (15 Rust / 25 cuts), TK-1 (10 Rust + 4 Python and rejected-capability builds), and roadmap consistency. This is functional evidence, not a throughput/resource measurement.

## RED / GREEN and negative controls

[TDD records](tdd/) retain the missing-wrapper RED (one reused publication primitive already passed, three new wrapper tests timed out at absent hooks) and four-test GREEN. Existing TK-2 transactional rollback and kernel signal handling already passed the new cancellation checks; they were not artificially changed. The independently compiled group-signal negative control failed the foreign-owner-survival assertion, then the unmutated complete eight-test suite passed.

Seven repeatable controls in `negative_controls.py` must compile first and fail a named behavioral assertion; a build failure or unrelated test failure is rejected. Each modified source lives only in a disposable copy with its own build output, is restored byte-for-byte in `finally`, and the complete green suite is compiled/run again afterward. The actual worktree is never mutated. Nested Cargo commands inherit the outer owned mutation command group, so outer cancellation terminates them too. Platform mutation receipts include original/mutated/restored hashes and exact commands/logs.

| Known-bad change | Required failing oracle |
|---|---|
| Remove drain-side kernel guard | Independent flock must observe a held embed lease |
| PID-file contents decide ownership | A free kernel lease must admit despite invalid/live stale PID text |
| Unlink a held lock file | Independent open must still refer to the locked inode; admission of a second active drain is also an exact exclusion violation |
| Signal whole fixture group | Cancelling waiter must leave publication owner alive |
| Omit setsid | Detached session must differ from caller's |
| Inherit caller stdout | Detached descriptors must refer to /dev/null |
| Keep waiter paused before blocking flock | Independent kernel-wait observation must reject the fixture pause |

An initial mutation-runner assertion selected the checkpoint name rather than its admission outcome; it rejected the PID mutant for the wrong reason. The oracle now accepts the ordinary rejection event and explicitly asserts free-lease admission. Only the later passing mutation receipts count as evidence.

### Pre-integration oracle follow-up

The fresh pre-commit macOS run passed all ten ordinary Rust tests, then rejected the `unlink-held-file` mutant for the wrong reason: the Rust test correctly failed `second active drain`, while the mutation runner accepted only `assertion failed: held`. Unlinking an owned inode allows either oracle to fire first, depending on scheduling. [Actual RED receipt](tdd/unlink-oracle-red/negative-controls/unlink-held-file.txt) and [runner failure](tdd/unlink-oracle-red/08-python3.txt) preserve the failure without retrying it away.

Only this mutation now accepts the two exact exclusion-violation signatures; every other case retains its single expected assertion. Compilation must still succeed, the behavioral test must fail, and unrelated failures are rejected. Mutation receipts record the accepted signatures and the one actually detected. Current platform receipts were refreshed for this final source; the previous 10/1/7 receipts are retained in `before-unlink-oracle-followup/`. This additional author finding is separate from the original three reviewer findings; no second independent review is claimed.

## Decisions and limits

- Reuse TK-2/TK-3 exact helpers; new code owns embed.lock only. Command probes close before spawn; child holds a separate real flock lease throughout drain. No nested index lock around TK-2 publish.
- To force the worst launch race, acquire/release probes in controlled order and hold all ten live callers before allowing any spawn. Simultaneous successful nonblocking probes cannot be assumed: short probes can legitimately reject each other. The publication contention schedule separately releases all ten detected commands together.
- SIGINT targets a directly owned unreaped Child. Group-signal mutation groups contain only fixture processes, never the harness/user's group. Lock-file PID contents are never used for signalling. Detached SIGKILL is requested through the peer connection and performed by that child itself. Numeric Event PID metadata is never a signal target; expired/disconnected events cannot affect a new process. A direct-child regression asserts actual self-SIGKILL status; detached kernel release is checked without claiming a grandchild waitpid status.
- Fixture channels have 20-second bounds. Direct children are killed/reaped on abandonment. Event/controller teardown closes channels to abort detached peers, and Linux init reaps them. The outer TK-3 command-group runner alone cannot kill a child that deliberately started a new session; inner fixture teardown/channel deadlines provide that boundary.
- Fake provider/model fixtures and existing TK-3 orphan-cleanup limitation carry forward. No real network/credentials, production runtime, full parser, general JCS, arbitrary in-place file snapshots, concurrent orphan reclamation, physical power loss, fairness/stress, x86-64, capacity, stat-10k or RAM claim. TK-11/TK-29/TK-42 retain their scope.
- Shared Engram is accessed through its CLI at `/Users/zdkadiev/Repos/letopis`; no second graph was initialized in this worktree. Proof completion was recorded at revision 15 (`970d7e45b8f687ce647e41003e4cfc6bd627300d`). The complete implementation/evidence/constraints/handoff snapshot was then saved at revision 16 (`703c8fc5a4645591233237a00e172a1fa66ad8ed`); those are historical recording points, not a claim about the latest graph HEAD.

## Design and issue reporting

The user requested reporting all findings into the design documents, [GitHub issue #4](https://github.com/zaurkadiev/letopis/issues/4) and shared Engram. [Design findings](findings.md) collects every review finding, its disposition, all evidence boundaries and the TK-11/TK-29/TK-42 handoff. MC-5, A-AR1 and resilience F-1/T-2 now link those findings. Two stale cross-process `--cancel` references were aligned with the existing D-95; no new product decision is introduced. The issue report explicitly distinguishes local, uncommitted proof from published source artifacts. Issue closure and Git integration remain separate actions.

The subsequent user-requested clarification explains that “issues” means the three harness review findings: a potentially wrong PID signal, unsafe interruption of mutation tests, and an unproven in-flock cancellation boundary. All three are resolved with macOS/Linux evidence. No separate GitHub issues were created; the existing issue #4 holds the report. The two corrected design references are documentation consistency fixes, distinct from the three reviewer findings.

## Independent review

[Review record](review.md): one fresh whole-change read-only review found two Important issues and one Minor, no Critical. Both Important issues were independently reproduced RED and fixed in one pass: safe connection-directed self-SIGKILL, and interruption-safe disposable mutation sources with nested group cleanup. Nine Rust tests plus the actual outer-runner interruption regression pass afterward. The initial Minor was closed by a follow-up: a separate test resumes the waiter and independently observes the actual blocking flock before SIGINT on both platforms. Every reviewer-set-aside boundary has a parent ruling/cost in the record. No second review is claimed.

The interruption fixture allows 120 seconds for native compilation/startup before its event; active-peer/exit waits remain bounded at 20 seconds. Final receipts supersede the historical 59-input-file results in before-review/.

## Follow-up: SIGINT inside blocking flock

The user requested closing the remaining review gap. `sigint_inside_blocking_flock_leaves_other_processes_alive` now covers both publish and full reindex. It first proves the paused `waiting` hook is **not** a kernel flock wait, resumes it, and requires an independent OS observation before signalling the directly owned child:

- macOS: a counted native `flock` frame in libsystem_kernel from `/usr/bin/sample` (its executable checksum and OS version are recorded).
- Linux: `/proc/PID/syscall` reports `SYS_flock` with `LOCK_EX`, and that descriptor's device/inode matches the held index.lock. Captured wchan is `locks_lock_inode_wait` in these runs.

The publisher holds index.lock continuously throughout observation and cancellation. Actual child exit status must be SIGINT; the publisher/drain must remain alive with their kernel leases. The complete old cache and corpus remain unchanged before releasing the publisher; afterward both original owners finish, exact committed fixture/vector bytes match and eligible work is empty. This holds in both modes on both platforms.

A seventh compiling control deliberately keeps the waiter at its pre-flock socket pause: the kernel-wait oracle must fail within its five-second observation bound. It is detected on both OSes. Observation availability is mandatory, with no skip/fallback to timing. [Native observation index and checksums](in-flock-verification.json) links all raw reports; the platform manifests hash every report too. [Observer RED](tdd/in-flock-observer-red.txt), [native GREEN](tdd/in-flock-observer-green.txt), [pre-hook negative control](tdd/in-flock-pre-hook-control.txt).

Final totals per platform: 10 Rust tests, one Python interruption regression and seven compiling negative controls; fmt/Clippy, TK-3/TK-2/TK-1 regressions and roadmap checks PASS. All 60 current input hashes match on both platforms. The previous 9/1/6 receipts are retained as historical evidence in before-flock-followup/. No additional independent review is claimed; the targeted OS observations and negative control close the previously deferred boundary.
