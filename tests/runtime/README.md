# TK-4: process locks, detached drain and own-process SIGINT

Standalone scratch proof for MC-5/NFR-2/NFR-5/FR-31/FR-32. Reuses the actual TK-2 publication and TK-3 work-query/drain helpers. This is a bounded native-process experiment, not the production CLI (TK-11/TK-29).

```sh
python3 -B tests/runtime/run.py --evidence /tmp/tk4-macos-new

docker build -t letopis-tk2:rust-1.95.0 -f tests/publication/Dockerfile .
docker run --rm --init \
  -v "$PWD:/repo" \
  -v "$HOME/.cargo/registry:/task-cargo/registry:ro" \
  -e CARGO_HOME=/task-cargo -e CARGO_NET_OFFLINE=true \
  -e RUSTUP_TOOLCHAIN=1.95.0-aarch64-unknown-linux-gnu \
  -w /repo letopis-tk2:rust-1.95.0 \
  python3 -B tests/runtime/run.py --evidence tests/runtime/.build/linux-new
```

Use a **new** evidence directory each time. The Rust registry and pinned vec1 source must already be cached for offline Docker; an initial native run downloads the checksum-verified source. The runtime target is separated by OS/architecture. Linux `--init` reaps adopted detached children. Negative controls modify a disposable source copy with separate build output, then restore and verify that copy; the reviewed worktree stays immutable even if interrupted. Nested mutation commands inherit the outer owned command group.

The runner preserves Rust 1.95.0, rusqlite 0.40.2, libsqlite3-sys 0.38.2 / SQLite 3.53.2 and vec1 0.8. It invokes TK-1's source checksum verifier, resolves matching SQLite headers and compiles a fresh extension. Receipts hash this crate and all TK-1/TK-2/TK-3 input files, record commands/environment/output, and include ten Rust tests, one outer-interruption regression, seven compiling negative controls, fmt, Clippy with warnings denied, complete TK-3/TK-2/TK-1 regressions and roadmap validation. The TK-3 process-group command runner provides outer timeouts/interruption cleanup.

## Observable schedules

- Ten publishers stop after detecting one shared corpus delta. Each admitted owner is held while an independent open-file description attempts the kernel lock. Locked re-detection makes only the first process publish: exact nodes, edges, fingerprints, FTS and revision are checked; corpus bytes remain unchanged.
- Ten live callers are held **after** each has independently released a successful free probe; then all may spawn. The first drain holds its own embed lease until nine candidates reject it. All ten launchers finish before the owner resumes. Native vector bytes and an empty eligible-work query verify completion.
- Both persistent lock files survive owner SIGKILL with the same inode/PID bytes. New processes acquire without repairing them; invalid and owned-live-decoy PIDs cannot govern ownership. An independent observer cannot acquire a held lease. Modes are 0700/0600.
- A detached child has its own session ID and `/dev/null` descriptors 0/1/2, does not hold index.lock, survives launcher death and releases embed.lock on death. Busy probes do not spawn; no-work callers return without a child.
- Publish and full reindex take real SIGINT at projections, FTS, fingerprints and before_commit (eight pre-commit cuts), retaining the exact old cache. Two after_commit cuts retain the complete new cache. Signal exit statuses are asserted for direct children. Cancelling a blocked waiter leaves a publisher and drain alive; cancelling an owner leaves its paused waiter alive and able to finish.
- Publish and reindex contenders also resume past the `waiting` hook into blocking flock. Before SIGINT, macOS `/usr/bin/sample` must show a counted native `flock` frame in libsystem_kernel; Linux `/proc/PID/syscall` must report blocking `SYS_flock(LOCK_EX)` on the descriptor whose device/inode matches index.lock. The pre-flock socket pause must fail that same observation. The real owner keeps the lock throughout observation/cancellation; publisher and drain survive, exact old cache/corpus stay unchanged, and both owners finish normally. A seventh compiling control deliberately leaves the waiter paused before flock and must fail the native-wait assertion.
- Failed spawn keeps the committed publication and eligible work with no spent attempts/vectors. SIGKILL after probe/before spawn is recoverable by a later no-change call. SIGKILL after a detached child's confirmed start does not prevent its completion.

## Fixture control and cleanup

`runtime ROOT publish|reindex|drain` is a scratch executable. Fixture config is model `three`; text `bravo` produces `[0,1,0]`, other fixture tokens produce `[1,0,0]`. Stored bytes are inspected through native vec1. No real provider call occurs.

`TK4_CONTROL` and role-specific `TK4_COMMAND_CUTS`/`TK4_DRAIN_CUTS` select optional Unix-socket handshakes. Each process has one connection lasting until exit; JSON events contain its PID, checkpoint, session/group and descriptor metadata. An acknowledged event releases exactly that process. This socket belongs solely to the test controller; the design still has no production IPC, daemon or process registry.

Checkpoint reads and parent event/exit waits are bounded at 20 seconds. Polling bounds waits; sleeps do not arrange races. Direct Child guards kill/reap abandoned children. Dropping an unacknowledged event aborts its socket. Controller teardown closes its listener and half-closes every established peer, then waits for EOF; later connecting children fail promptly. Detached children are not waitpid children of the harness: a connection instruction makes the connected child send SIGKILL to itself; no numeric PID from an Event is ever signalled. A disconnected event cannot target a new process, even if its PID metadata names a live owned decoy. A direct-child regression verifies actual self-SIGKILL status; detached kernel release is observed without claiming a grandchild wait status. PID text in lock files is never signalled. A group-signal negative control puts only fixture-owned processes in the addressed group, separate from the harness and users.

The Python regression interrupts the actual full runner after a mutant is written in a disposable workspace. It requires unchanged original source hashes, EOF from the still-connected nested command, nonzero runner exit and a shared mutation/command group. The RED reproducer uses an outer disposable copy too, protecting real untracked sources. Compile/startup readiness allows 120 seconds; active-peer/exit waits remain bounded at 20 seconds. The full runner provides the mutation group's 600-second command timeout.

The native-wait observer is fail-closed: sample/proc inspection must be available for these owned child processes, otherwise the check fails. No sleep establishes kernel waiting. A bounded five-second observation loop observes the actual call; captured native reports and their hashes are retained in each receipt under `flock-observations/`. The macOS sample executable checksum and Linux kernel/environment are recorded.

## Limits

These checks establish the bounded schedules above on macOS ARM64 and Linux ARM64. They do not establish fairness, scheduling-independent stress guarantees, physical-power durability, concurrent orphan collection, arbitrary parser/file-edit behavior, production CLI cancellation, provider transport, capacity, stat-10k, RAM or x86-64 behavior. Existing TK-3 cleanup and fixture/config limitations carry forward. G-0 remains open until TK-6.

See the [task card](../../docs/zkit/letopis/roadmap/epics/EP-8-proofs/tasks/TK-4-runtime-harness.md) and [execution report](../../docs/zkit/letopis/checks/TK-4/report.md).
