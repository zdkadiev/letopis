# Issue 67 review status

Final whole-branch review at `8af440a` identified F-I1 (sparse-index membership)
and F-M1 (copied internal command notes). The consolidated fixes are committed
as `b488aeb` (code) and `ae9e568` (report/evidence). **Independent scoped
re-review accepted both findings as addressed, with no new findings.**
The branch is technically ready within the stated evidence limits. No merge,
push, PR, issue update or publication is implied. The full original review
and scoped verdict are preserved under `final_acceptance` in the evidence index.
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
subagent was spawned by this fix implementer; the controller supplied the
independent scoped review and recorded acceptance.

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
Independent review and scoped re-review are complete. F-I1 and F-M1 are
closed; M3 remains the explicitly nonblocking test-strengthening limit above.

## Rulings I made

The complete ruling audit below is ordered by execution phase. Original
ledger positions and verbatim text are retained in the hash-indexed
`final_acceptance.rulings` artifact, alongside the full final execution
ledger. Costs describe what would need revisiting if a ruling were wrong.
The per-object/delta buffer revision was explicitly approved by the user.

| Phase | Ruling | Cost if wrong |
| --- | --- | --- |
| Preflight | Treat the explicit Subagent-driven selection as execution approval. | Rework if that contextual authorization were wrong; no broader publication authority. |
| Preflight | Gate the reader privately before product adoption. | Rework the compatibility driver if it does not represent the adopted implementation. |
| Preflight | Include collision fixtures in Linux build inputs. | Repair missing build context; no runtime change. |
| Task 1 | Resolve the dangling schedule reference with the approved bounded campaign. | Rework orchestration or disclose censored coverage if the schedule is wrong. |
| Task 1 | Add an optional verification callback to the existing contention helper. | Check ownership/receipt compatibility for existing callers. |
| Task 2 | Keep durability probes and reusable publication helpers in their own private owner. | Rework the helper boundary if adoption cannot reuse it safely. |
| Task 2 | Use necessary exact tempfile 3.27.0 dependency resolution. | A reversible dependency update; locked/MSRV checks remain necessary. |
| Task 2 | Judge reachable helper execution rather than unconditional command-crate presence. | A false reachability judgment could hide a helper; source audit and negative-control tracing are required. |
| Task 2 | Keep durable direct private refs without automatic reflogs or identity emulation. | Automatic private-ref history would require an explicit later feature; existing logs are preserved. |
| Task 2 | Copy the required collision fixture into all root-test Docker contexts. | A small build-context correction, not runtime behavior. |
| Task 2 | Reuse a controlled uncapped local ODB with empty replacements. | Rework the reader if pinned library allocation/replacement behavior differs. |
| Before adoption | Require actual command-owned cancellation resources before replacing the transport. | Private ownership API rework if active writers cannot be cleaned safely. |
| Verification | One unchanged make verify covers make check; use focused iterations. | Run again when changed inputs or a real failure invalidates prior evidence. |
| Task 4 | Expose locked gix-pathspec directly instead of enabling a broader feature bundle. | A reversible manifest edge; no new package version or runtime feature. |
| Task 4 | Migrate obsolete Git-process checkpoints while preserving behavioral assertions. | Rework test observations if they are weaker than the replaced checkpoints. |
| Task 4 | Construct the non-Send service on its worker before explicit barriers. | Preserve setup-before-lock ordering; no new public Send guarantee. |
| Task 4 | Use an owned test checkpoint and real dirfd namespace model. | The model is not an exhaustive paused-kernel proof; rework a missing boundary if found. |
| Task 5 | Leave already-single-open direct drain behavior unchanged. | Avoid an unrelated normalization change; later source tracing found no wrong-root lock. |
| Task 6 | Batch preflight/check/status; preserve search/traversal demand order. | No cross-binding historical reuse for those two read callers in this change. |
| Task 6 | Recompute all BasisUnavailable results rather than cache negative roots. | Cheap absent-basis work repeats; repaired roots are not hidden. |
| Task 7 | Whitelist exact Git collision inputs in Docker context. | A small packaging correction; other exclusions stay intact. |
| Task 7 | Copy the required config corpus fixture into both Linux contexts. | A small fixture-only context addition; failed attempts remain evidence. |
| Task 7 | Accept either evidenced optimized corpus frame with all other readiness predicates. | Overbroad matching could falsely acknowledge readiness; negative fixtures retain main-thread/Git/native requirements. |
| Task 7 | Pin upstream Git 2.50.1 as the independent strict Linux oracle. | Historical weaker malformed-tree acceptance is not promised; no runtime Git dependency. |
| Task 7 | Use held-open untouched stdin for argv-rejection tests. | Keep exact error/exit intent; do not hide generic BrokenPipe failures. |
| Task 8 | Keep one aggregate initialization budget and skip infeasible repeats. | Some baseline maintenance remains unassessed instead of overrunning or inventing medians. |
| Task 8 | Update the stale Markdown pointer to one canonical JSON inventory. | Documentation repair if links or inventory scope drift. |
| Task 8 | Document the corrected timeout-side scheduling behavior. | Keep documentation and tested implementation consistent. |
| Final scope | Do not infer physical-power-loss or NFS durability from local sync tests. | Separate hardware/fault validation is required for those guarantees. |
| Final scope | Limit platform claims to macOS arm64 and Linux aarch64 evidence. | Run another platform gate before promising it. |
| Final scope | Do not infer larger-scale or whole-stack NFR acceptance. | A separate capacity/resource assessment remains necessary. |
| Final scope | Keep real-provider/model performance outside this provider-free campaign. | A separately authorized provider experiment is required. |
| Final scope | Claim concrete cancellation schedules rather than exhaustive fault/SIGKILL cleanup. | Further fault models or drills are needed for broader guarantees. |
| Final scope | Do not promise adversarial same-mtime detection on an open index handle. | A stronger freshness contract would need separate design and evidence. |
| Final scope | Preserve historical proof receipts without claiming every standalone harness reran. | Run a relevant historical manifest separately when needed. |
| Final scope | Resolve the drain suspicion through the actual root-bound executor call path. | No new dynamic subdirectory contention experiment was performed. |
| Integration | Keep publication and integration subject to explicit user choice. | The branch stays local until authorized. |
| Final fix | Refresh affected corrected-pin evidence and retain unchanged baseline observations explicitly. | Mis-scoped pins could misattribute results; fresh identities/oracles and historical labels are required. |
