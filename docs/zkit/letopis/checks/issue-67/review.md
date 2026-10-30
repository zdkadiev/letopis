# Issue 67 review status

Final whole-branch independent review is **pending**. Task 8's measurement author
has not dispatched a reviewer or certified the branch. The controller owns the
Task 8 review and final base/head review, including final findings and rulings.
The accepted Task 1–7 reviews, fixes and receipts are preserved in the local
hash-indexed SDD archive referenced by [the evidence index](evidence-index.json).

## Task 8 self-review

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
3. The pre-existing direct-drain caller-directory versus resolved-root lock
   inconsistency remains unverified and unfixed. The root-based matrix does not
   resolve it, and Task 8 introduces no change to that path.
4. Performance results concern this synthetic 60/1k corpus and host. Profiles are
   sampled CPU evidence; CLI lifetime overlap is not SQLite/flock wait accounting;
   sampled direct RSS and wait4 lifetime accounting are not whole-stack peak RAM.
5. Host/Linux platform evidence is scoped as recorded in
   [compatibility.md](compatibility.md); no x86_64, NFS, power-loss or real-provider
   performance guarantee is inferred.

The final report records the exact successful/censored counts and command paths.
Independent review findings and their resolutions must be appended by the
controller before final branch acceptance.
