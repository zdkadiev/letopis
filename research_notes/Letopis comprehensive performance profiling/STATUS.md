# Profiling task status

## Final synthesis — supersedes earlier progress statements

All measurement phases are complete and frozen. Canonical survey has 352 rows (319 measured, 30 conditioning, three first-use); 100k has 20 (19 measured, one first publication); supplementary catalog has 159 validations and 740 baseline rows; drain sweep has three validations and nine baseline rows. The 1,087 stock baseline rows include one expected invalid-argv exit 1. Twelve canonical CPU captures and 15 direct-root captures are complete; direct-root independent audit is qualified PASS. Four valid live memory snapshots are retained with their documented limits.

Fresh final verification: performance helper suite 94 tests in 21.287 seconds, OK with eight intentional integration-environment skips; research tools 49 tests in 2.372 seconds, OK. See `evidence/final-verification-20261117-v1/`. These are not Rust root or native product integration tests. The earlier full-suite error and unresolved underlying timeout cause remain historical evidence.

Final report written at `reports/Letopis comprehensive performance profiling.md`. Independent review identified a presentation gap; the report now adds per-command combined CPU and small/large comparisons plus reproduction prerequisites. Figure derivation and link checks are recorded in `report_writing_check.md`. Targeted re-review remains pending; do not mark the report task fully complete before review. No further measurements are authorized by this status.

## Recovery ledger

- Previous Sol author failed with provider error `Country, region, or territory not supported`. Recovery made no credential, network or security changes.
- Canonical CPU stage completed before that error: `completion.json` records 12 validated captures and 10 complete pairs. The full 100k capture has a 185.133 s sampler-wrapper duration, 183,971 usable stacks and 177.888334 s sampled CPU. Inclusive weighted hotspots: `EffectiveGraph::apply` 53.35%, `collect_source_references` 22.63%, `reconcile_sources` 23.92%; these overlap and are not wall-time shares.
- Native-memory Terra agent owns measurement; recovery ran no product benchmarks or builds.
- Existing-suite diagnosis remains preserved: targeted fake unit test passed in 1.7 s; the last full 94-test run failed after fake initialization timed out. The underlying greater-than-five-second delay is unproved. Diagnosis was not rerun.
- Round2 recovery is **not ready for review**. Current partial tools were snapshotted in `evidence/harness-review-v2/recovery-before/`; focused Python 3.12 suite passed 40 tests in 2.267 s. Mutation run `evidence/harness-review-v2/recovery-mutations.txt` failed: it used system Python 3.9 and stopped at an ambiguous replacement; it does not establish mutation coverage. Scoped final diff and handoff remain unfinished. No product or tool source was edited during recovery.

### Recovery continuation — supersedes the preceding readiness statement

Round2 is **ready for static re-review**, not approved for supplementary execution. Focused Python 3.12 suite passed 41 tests; targeted in-memory mutations detected the five review boundaries and stage-stop propagation. Original harness source remains byte-identical to the recovery snapshot; focused tests were strengthened. Read [round2 handoff](harness_implementation.md#review-round2-handoff--ready-for-static-review-not-execution-approval), [scoped diff](evidence/harness-review-v2/round2.diff), and preserved outputs before review. No benchmarks, builds, sampler runs, native observer runs, or heavy existing-suite reruns.

## Latest review ledger

Supplementary fix-round1 review **not approved**. Original blockers1 and3 addressed; four remaining areas: full source inventory, disk-root accounting, failed/profile-child RSS accounting, coverage counts; weighted Letopis CPU check also required. Fix-round2 in progress with original author. **No supplementary benchmark allowed yet.**

Canonical audits remain **PASS with qualifications**. CPU profiling in progress under exclusive owner; completion not claimed. Existing94-test suite has one `StopIteration` error under separate unresolved diagnosis: **not all green**. This ledger update ran no tests/builds/benchmarks and made no commits; no design expansion.

- [x] Inspect workspace (initial main clean per coordinator; inspected HEAD `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`, only requested report scaffold untracked at observation).
- [x] Inventory every command (implementation-backed command matrix in `command_inventory.md`).
- [x] Establish profiler/measurement methodology (design specified; OS/profiler capability and measurement validation still pending).
- [x] Define corpora/load matrix (reuse seed65 smoke60/1k/10k; empty0 separate;100k stress gated by actual costs; completed workload research incorporated).
- [x] Approve profiling design (2026-11-17: user chose Proceed autonomously, samply, local deterministic embeddings only; no further checkpoints).
- [ ] Build isolated profiling harness (supplementary adapters exist; fix-round1 underway, review gate not passed).
- [ ] Validate fixture semantics and measurements (canonical oracle352/352 valid; supplementary validation/re-review pending).
- [ ] Run serial uninstrumented matrix (canonical60/1k/10k complete319 measured;100k complete19 measured+1 publication; full supplementary command coverage pending).
- [ ] Record samply profiles (exclusive existing-infrastructure owner; counts/results pending, one complete100k full-index capture authorized).
- [ ] Analyze timing CPU memory and source hotspots.
- [ ] Independent evidence audit.
- [ ] Save final report + reproduction.

Design: `docs/superpowers/specs/2026-11-17-performance-profiling-design.md`.
Executable checklist: `docs/superpowers/plans/2026-11-17-performance-profiling.md`.

Research scaffolding/design only so far; no harness code, builds, installation, or benchmarks performed by this worker. Execution now authorized autonomously under recorded design. No production/Cargo edits, staging, or commits authorized.

Project execution update: `canonical_survey.md` complete352/352 valid attempts (319 measured/30 conditioning/3 first-use) at60/1k/10k; archived stock release build receipts linked there. These are nine canonical mixed graph/source scenarios, not all command/flag coverage. Supplementary sparse driver tools/profile.py + profile_test.py is separate; harness_review.md rejects current measurement readiness, fix-round1 underway, review gate not yet passed.

100k update: [canonical_stress.md](canonical_stress.md) complete20/20 valid,19 measured+1 publication,no failures; full178.498726s n=1,max443.61MiB lifetime RSS. [canonical_stress_audit.md](canonical_stress_audit.md) PASS with qualifications,4334 input hashes confirmed. Disabled100k OpenAI/earlier Ollama imply intended zero calls, not network trace. Soft disk/RSS guards retain honest scope.

Profile ruling: default10s window not hard user limit; one complete100k full-index samply capture allowed within900s sampler-stage/600s product cap if disk/budget permit. No extra178s ordinary control required; stock versus debug1 is not overhead control. Actual profile completion not claimed. Documentation-only update; no execution/staging/commits.

Ledger rulings: canonical old runner retains valid disabled Ollama config (no override hook), local FTS with zero actual provider calls intended, not Ollama measurement; network traffic not traced. Active mocks remain loopback OpenAI only. Memory budget is observed post-sample1GiB stop threshold, disk soft pre/post guard over combined scratch+evidence; deadlines retained, no hard instantaneous resource protection/new monitor claimed. This worker performed documentation updates only.

Reuse-first: existing tests/performance measurement/reset/profile/ownership/report/provenance and tests/target_corpus generator/validator. Only missing campaign cases and OpenAI loopback adapter under research tools; no duplicate harness.

Review decisions2026-11-17: retain existing two conditioning/20 cheap/3 expensive calls; reductions require visible actual n/skips. Native heap/malloc_history/leaks diagnostics permitted separately; availability needs attempted-tool or documented constraints, no product instrumentation. Stock/symbol sibling retained. Adapter filenames remain provisional pending actual harness; reproduction/spec amended to match implementation. No additional execution phase completed by this documentation update.

Native memory phase ended; exclusive measurement ownership released. [memory_results.md](memory_results.md) records calibration, one valid early 10k full-index live heap graph and allocation stacks, at-exit/lite failures, zero leaks in that one snapshot only, and missing full-run/check/inspect coverage. User ruling raised combined evidence soft cap 2→4 GiB with >20 GiB free; CPU captures preserved. Direct live launcher did not capture post-process peak RSS, so 1 GiB guard cannot be claimed verified. No further workload remains running under memory worker.

Native memory bounded extension complete: three 10k live graphs for check, inspect and later full index, each direct product exit 0 and independent oracle valid; instrumented wait4 peak RSS 71,991,296 / 24,002,560 / 85,082,112 B, below the post-observed 1 GiB stop. Prefix allocator high-water stacks and zero-leak snapshot limitations documented in [memory_results.md](memory_results.md). Coordinator ruling under user autonomy—not explicit user budget—allowed 4 GiB combined soft cap while free space ≥20 GiB; evidence retained. Original phase's missing RSS guard remains unverified. Exclusive measurement ownership released; no product workload remains running.

## Coordinator recovery map — supplied state, not a progress poll

- Static round 2 PASS; 41 focused tests, 15 P0 semantic validations and coordinator 30 stream-hash checks reported passed. Active exclusive catalog measurement owner: session `ses_e1521c1aeffeX4Ma2k9N2jpJWX`, evidence `supplementary-prepare-20261117-v1/`; 159 finite cases, validation remaining, then explicit n=3 ordinary / n=20 init-config / n=1 coverage baseline. Do not restart completed 372 canonical attempts, 12 CPU captures or four native heap snapshots.
- Drain adjunct: three cases prepared, not executed; [static review](drain_scale_review.md) PASS for bounded runtime validation after ownership transfer. Remaining four tiny CPU launcher cases under implementation, not executed; other profiles later through existing driver. CPU and memory audits qualified PASS.
- Latest full 94-test suite remains 8 skips and 1 error; targeted fake unit passed separately and does not erase the full-suite error. Original Sol agents failed due to region restrictions; replacements are Terra/OpenAI only. No product changes or user commits. Raw data unchanged by this ledger update.

## Completed measurements and active direct CPU owner

Coordinator handoff: supplementary catalog complete 159 validations + 740 baseline samples, alongside completed canonical/core evidence; drain-scale v2 complete 3 validations + 9 baseline samples. [Drain raw audit](drain_scale_audit.md) PASS qualified; retain all other raw audits and earlier unstarted preflights. Current exclusive benchmark owner is session `ses_e1521c1aeffeX4Ma2k9N2jpJWX` for direct-root eight CPU categories. The unused Python aggregate wrapper was never executed; four earlier window captures are preserved as unattributable. Do not restart completed work or run concurrent benchmarks.
