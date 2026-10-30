# Issue 67 review status

Final whole-branch independent review at `8af440a` found **0 Critical, 1 Important
(F-I1 sparse-index membership), 1 Minor (F-M1 copied command notes)** and was not
ready to merge. The consolidated fix is implemented; **scoped independent re-review
is pending**. The implementer has not certified acceptance. The controller owns
the review/rulings and plan progress. The original independent report is preserved
in the fresh hash-indexed final-fix archive.
The accepted Task 1–7 reviews, fixes and receipts are preserved in the local
hash-indexed SDD archive referenced by [the evidence index](evidence-index.json).

## Consolidated final fixes

F-I1 is fixed at `b488aeb4a7faa33f58e6e9c87a4f4d638631680b`: sparse-directory
entries expand from their index OID through the existing verified raw tree reader,
into an in-memory index used by both listing and ignores/walk. Ordinary indexes
stay borrowed. No persistent index/HEAD/worktree write, subprocess fallback,
service interface, runtime or dependency was added. SHA-1/SHA-256 tests assert a
real collapsed directory, staged membership distinct from HEAD, tracked ignored
restoration, deleted membership, nested index-backed ignores, complete unrequested
inspect capture and unchanged user state. The omission mutant compiles and fails.

Current `make verify` passes; focused Linux Git 2.50.1 passes 22 repository tests,
5 helper-denial tests and 41 traced intervals. Two failed Linux image setup/oracle
attempts are retained. Current performance after measurements use the corrected
freeze; original optimized `ba4a11d` values are historical. Fresh ordinary paired
controls and after-only publication/maintenance/contention/profile/counter/RSS
refreshes are distinguished from reused historical baseline evidence.

F-M1's four current notes now match their own receipts (18/14/four timeouts/exit1;
8/8; 8/8; 20/20). A new corrected task-8 archive preserves the old point-in-time
archive unchanged. The stale specification opening is corrected. No reviewer or
subagent was spawned by this fix implementer; scoped acceptance remains open.

## Historical Task 8 self-review (pre-fix `ba4a11d`)

- Product inputs are committed at `ba4a11db26705999018187e8301a51e9942fe1ba`;
  the frozen release uses the same locked compiler/profile/debug settings as the
  immutable baseline. Later changes affect only comparison scheduling and docs.
- Ordinary initialized measurements use alternating pairs, two excluded warmups,
  equivalent bytes/configuration and stable independent projections. First
  publication, audits, contention, counters, profiles and sampled RSS have separate
  receipts and scopes. Censored observations remain visible.
- A campaign defect was found before initialization: unconditional three-probe
  scheduling retried an infeasible side after timeout. The narrow correction
  skips only later initialization attempts for that side/scenario. Successful
  sides retain three; successful transports with invalid outputs remain visible.
  Focused RED, 22-test GREEN, skip-disable mutant RED and restored GREEN are archived.
- The supplemental direct-PID RSS sampler validates PID, parent, start identity
  and executable; missing evidence stays unavailable. Its controlled 32 MiB,
  wrong-executable, absent-PID and timeout tests pass. A byte-unit mutant fails.
  The original Python fixture's self-exec identity mismatch and its failed controls
  are preserved, rather than discarded as successful evidence.
- No Rust, public API, service interface, `lib.rs` wiring, storage schema, native
  input or dependency changes were made in Task 8. Its six-line scheduling change
  adds no product abstraction. The controller's Task 7 source/platform receipts
  remain applicable; unchanged product suites were not run redundantly.
- Unrelated issue 65 edits and the staged R-10 file are protected by byte/index
  guards. No push, merge, publication or GitHub update is part of this task.

## Limits carried into final review

1. Minor M3: the tracked-entry mutant detects initial eligibility. The maintained
   same-handle ordinary index/ignore edit regression passes; there is no dedicated
   stale-second-read mutant, and deliberately preserved mtimes are outside that
   regression's demonstrated scope.
2. Task 7's 39 traced foreground/direct-drain invocations exclude the eligible
   foreground-to-detached interval. Separate lifecycle tests and source audit
   cover that boundary; the 39 traces do not independently establish it.
3. Direct-drain wrong-root locking was not substantiated. `commands.rs:142–145`
   builds the background wrapper at the caller directory, but
   `BackgroundService::drain` (`src/service/background.rs:35–44`) only calls
   `tasks.drain()` and never uses that wrapper's filesystem/process.
   `wire_embedding_executor` (`src/lib.rs:329–350`) resolves the Git root;
   `EmbeddingService::drain` (`src/service/embedding.rs:102–113`) acquires
   `self.files.try_lock_embed()` on that root-bound executor. The unused
   collaborator is not evidence of a concurrency defect. No runtime change or
   dynamic subdirectory-lock test was performed or claimed.
4. Performance results concern this synthetic 60/1k corpus and host. Profiles are
   sampled CPU evidence; CLI lifetime overlap is not SQLite/flock wait accounting;
   sampled direct RSS and wait4 lifetime accounting are not whole-stack peak RAM.
5. Host/Linux platform evidence is scoped as recorded in
   [compatibility.md](compatibility.md); no x86_64, NFS, power-loss or real-provider
   performance guarantee is inferred.

The final report records the exact successful/censored counts and command paths.
Independent review findings and their resolutions must be appended by the
controller before final branch acceptance.
