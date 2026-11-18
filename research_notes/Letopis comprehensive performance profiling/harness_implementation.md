# Research harness implementation handoff

2026-11-17. Implemented, unit-tested; **no product benchmarks or samply captures executed by this worker**. No builds, installs, staging, commits, production Rust/Cargo/Makefile changes, or existing performance-tool edits.

## Files and reuse

- `tools/profile.py`: research driver, finite scenario matrix, bounded disposable fixtures, localhost OpenAI-protocol mock, scenario-owned assertions, raw receipts and analysis.
- `tools/profile_test.py`: 18 lightweight Given/When/Then tests.
- Existing `tests/performance/measure.py` remains process/wait4/timeout/group-cleanup owner. Driver calls `measure_process`; temporarily wraps its resource formatter to retain raw per-child user/system/fault/block/context-switch counters, then restores it in `finally`. Serial-only; this hook is not thread-safe for concurrent measurement calls.
- Existing `tests/performance/profile.py` validates samply files and computes `threadCPUDelta`-weighted inclusive stacks when the `.syms.json` sidecar exists. No raw-stack-count CPU percentages.
- Existing `scenarios.build_inspection_oracle` freezes fragment byte/range assertions; specialized old clean-case validators are deliberately not applied to faults, empty search, batches or held locks.
- Optional 100k inputs use existing `tests/target_corpus/generate.py`, not a second canonical generator.

## Interface

Use `/opt/homebrew/bin/python3.12 -B`; existing imported infrastructure requires modern Python.

```bash
PY=/opt/homebrew/bin/python3.12
DRIVER="research_notes/Letopis comprehensive performance profiling/tools/profile.py"
EVIDENCE="research_notes/Letopis comprehensive performance profiling/evidence/NEW-RUN-ID"

# Creates inputs/manifest only. No product invocation or build.
"$PY" -B "$DRIVER" prepare --evidence "$EVIDENCE" --include-real --stress-100k

# Review cases.json, then select smoke cases explicitly. Binary must already exist.
"$PY" -B "$DRIVER" validate --evidence "$EVIDENCE" --binary /ABSOLUTE/STOCK/letopis \
  --case sparse-100.index-noop --case sparse-100.search-selective --case canary-2.vector-only

# Uninstrumented timing, serial. Exact --case selections are repeatable.
"$PY" -B "$DRIVER" measure --evidence "$EVIDENCE" --binary /ABSOLUTE/STOCK/letopis

# Optional separate 100k stage: seven bounded canonical cases, no automatic retry.
"$PY" -B "$DRIVER" measure --evidence "$EVIDENCE" --binary /ABSOLUTE/STOCK/letopis \
  --stress-only --budget 1200

# Profile explicit representative cases using matching symbolized executable.
"$PY" -B "$DRIVER" profile --evidence "$EVIDENCE" --binary /ABSOLUTE/SYMBOLIZED/letopis \
  --case sparse-1000.index-full --budget 900
"$PY" -B "$DRIVER" analyze --evidence "$EVIDENCE"
```

Aliases: `baseline=measure`, `sample=profile`, `summarize/audit=analyze`. `prepare` refuses an existing evidence directory. Later stages append uniquely identified artifacts and reject harness/infrastructure drift. They do **not** resume completed repeats automatically: a new invocation is a separately identified campaign. Use exact case selections rather than rerunning a whole campaign accidentally.

Compilation receipts remain separate owner’s evidence. Driver archives supplied executable and adjacent `.dSYM`, records SHA-256; it does not validate compiler flags or build metadata by itself.

## State and measurements

Disposable inputs/copies/executable live strictly below `/private/var/folders/sw/f89n9_pj3mvfhr6bfn_7dnbm0000gp/T/opencode`. Evidence may live in the research tree. Scratch manifest records ownership; symlink/noncanonical/outside-root repository paths are rejected. Every measured scenario gets a fresh fixture-input copy; initialization/publication/edit setup occurs outside measured interval. Copies contain no active SQLite state.

Provider environment is an allowlist with dummy nonsecret key, no ambient provider credentials/proxies/Git configuration. Mock binds `127.0.0.1` only, never forwards, returns deterministic finite nonzero indexed vectors. A two-document canary gives a known unique vector-only cosine match absent from FTS. Reordered response behavior is unit-tested. Enabled foreground commands hold interoperable flock with actual holder PID; setup explicitly releases it only for owned synchronous drain. No untracked detached worker is deliberately launched. Disabled commands do not hold the lock.

Ordinary measurement uses existing monotonic/wait4 process owner, explicit JSON+EOF, captured pipe output (not terminal I/O), stream byte counts/full hashes plus bounded retained bytes, raw usage and normalized macOS RSS bytes. Timer includes existing owner’s collection/cleanup endpoint; not pure application compute. MaxRSS remains child lifetime maximum with OS-accounted reaped descendants, **not simultaneous process-tree peak or allocations**. Local mock request timings/bytes are separate; mock CPU/RSS unavailable. Stream truncation makes semantic validation unavailable, never success.

Default baseline: separately labeled first observation; two conditioning calls for cheap cases; 20 cheap repeats or three expensive repeats. Expensive conditioning omitted to preserve budget. Every call resets equivalent application state; OS cache remains uncontrolled. `--repeats N` is a recorded explicit reduction/override, without extra conditioning/first series. Successful-repeat summaries exclude first/conditioning/profile records. Failures stop that case’s series rather than automatically retrying; censored receipts persist.

30-minute ordinary / 15-minute sample caps; optional stress stage 1200 seconds. Per-call ordinary default60 seconds; canonical100k120 seconds and fresh publication600 seconds (initial setup publication also600). Profile wrapper independently capped10 seconds and at most20 selected cases per invocation. At 1000Hz, fewer than100 nonnull stacks are labeled insufficient; symbolization not inferred from sample counts. Profile resource totals are sampler-wrapper scope and never baseline CLI statistics.

Disk guard checks2GiB scratch occupancy between samples; it is **not** a hard instantaneous disk/RSS limit. Stage deadline bounds product setup subprocesses and measured calls; Python copy/assertion/analysis work is not interruptibly preempted. No live1GiB process RSS watchdog implemented.

## Implemented matrix versus remaining coverage

Implemented cases are declarations, **not exercised product evidence yet**:

- Sparse0/100/1k/10k; fresh/repeated init; absent/disabled/enabled config observation; initialized/published/backlog status.
- Index fresh/no-op/verify/full/scoped full/scoped file/scoped directory; one-element edit, removed owner and retained-invalid owner; both combined Full flag orders.
- Full/path-filtered check; one/100-element inspect with frozen byte/range/hash oracle; selective/common/no-match/empty FTS; needs-recheck filters.
- Nine traversal mode/direction combinations at100 nodes; representative depth0/2/8, node/edge bounds, absent edge output/type filter; generic bounds/start-exclusion assertions.
- 1k chain/star/eight-neighbor dense/long-text/shared-source/dangling/includes-cycle variants; source-only stale bytes. Optional bounded tracked real snapshot copies text in `letopis/src/docs/skills`, preserves old bases and labels missing provenance rather than repairing it.
- Disabled drain, direct100-node mock backlog drain, fully populated mock hybrid query, two-node vector-only canary, global/eight command help/version, unknown argv.
- Optional100k canonical first publication, no-op index, status/check/inspect/search/traverse, using existing generator. Ordinary driver excludes stress cases unless `--stress-only`.

Explicit remaining gaps:

- No product execution/semantic validation gate yet. `validate` collects validation receipts but does not currently block later `measure` for missing validation.
- No dedicated detached-background lifecycle/worker accounting; no busy-drain/readiness-reset/contention/cancellation drill.
- No config update command exists. Direct malformed config and invalid JSON/field/limit cases not included beyond unknown argv.
- No A/B scoped-index-versus-whole-refresh differential oracle; no multi-file/add/rename/ignored/discovery stress, corrupt snapshot closure, missing-range/mixed-revision binding cases.
- Traversal bounds are asserted, but arbitrary topology/BFS exact identity sets are not independently certified. Inspect does not independently read returned snapshot’s Git bytes (only compares frozen live fragment range/hash).
- Real snapshot expected graph counts are unavailable; findings are diagnostic only, not a clean-corpus certification. Real inspect/traverse not included.
- Provider supports delay/failure/malformed/reordered modes internally, but only success cases are selected in current matrix; no failure/fallback/exhaustion/identity-reset/drain-stale-text cases. Empty enabled search requires explicit inference opt-out API but no matrix case yet.
- No duplicate-text/vector reuse/dimension scaling,100k enabled provider run, large-output/64KiB query/high-degree10k memory stress, vmmap/leaks or allocation tracing.
- No short-command samply aggregation, sample fallback, paired overhead series, or automated symbol-quality gate. Short profiles may be insufficient.
- Analyze audits observed IDs/streams/units and reports unobserved cases; it is not a comprehensive independent coverage/build/config/profile-identity audit. No unsupported tail percentiles are produced.

## Verification and mutation evidence

Commands:

```text
/opt/homebrew/bin/python3.12 -B -m unittest discover -s "research_notes/Letopis comprehensive performance profiling/tools" -p '*_test.py' -v
/opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py'
```

Initial test-first run failed on absent harness APIs. Second red batch: vector canary, reordered responses, inference opt-out, traversal bounds, throughput counters and100k matrix failed before implementation. Harness subsequently passed18 tests.

Manual mutation check temporarily changed four implementation decisions together; each had its own distinct failing test:

1. Remove missing-query-HTTP rejection → response/fallback test failed.
2. Multiply Darwin RSS by1024 → RSS unit test failed (`2097152 != 2048`).
3. Replace median with minimum → statistics test failed (`1 != 2`).
4. Force timeout flag false → actual sleeping-helper timeout test failed.

All mutations restored. Existing performance suite:94 tests passed,8 skipped,20.738s. Its output includes intentional fake failure/censor fixtures; unittest suite itself passed. These lightweight helper tests are not current product workload benchmarks. Product Rust checks were not run: no Rust/product/dependency edits and compilation belongs to separate worker.

## Review round1 fixes — supersedes affected v1 statements above

Full [review](harness_review.md) was read and checked against the reviewed source before editing. Original tools and this handoff were frozen in [`evidence/harness-review-v1/`](evidence/harness-review-v1/). No product benchmarking, build, installation, production edit, staging or commit performed during fixes. Scoped before/after artifacts and patch are stored in that snapshot directory.

### Verified findings and rulings

Review findings about case-only pooling, optional validation, unchecked fixture hashes, early unaccounted skips, ten-second sampler termination, unfiltered symbol attribution, raw-record loss, blocking lease acquisition, request-count drain oracle and nested export exclusions were confirmed in code.

Two cross-task decisions override the original proposed remedies:

- Sparse/chain/star/long/source fixtures are explicitly **supplementary**, not canonical replacements. Separate canonical-survey owner runs established seed65 mixed60/1k/10k via `tests/performance/run.py`. This driver does not duplicate that baseline. Optional100k now uses seed65, captures the existing generator's full manifest and calls independent `validate_corpus`; the canonical manifest is checked again before execution.
- No live RSS polling framework. Soft disk guards inspect **scratch plus this run's evidence**, including pre/post sample growth; recorded wait4 peak>=1GiB stops later samples. Setup process peaks also trigger a post-sample stop. These are not instantaneous/hard memory or disk ceilings. Python fixture copying/analysis is not interruptibly preempted. Per-call deadlines, bounded lease acquisition, existing owned-group cleanup and stage stop accounting remain explicit.

### Integrity changes

- `manifest.json` is schema2. Supplementary family/canonical ownership are explicit; entire immutable input trees (including local Git config/objects) and exact expanded cases are hashed. Added/missing/edited files or symlinks reject execution before setup. Fixture copies are checked before product work. Old v1 evidence is historical, not reusable under the changed harness hash.
- A fixed owned loopback port is pinned per prepared run, so validation and measurement use the same exact rendered provider configuration. Bind failure stops execution rather than connecting to another listener. Configuration identity additionally includes dimensions, delay/mode, provider implementation and observer executable hash.
- `--build-metadata PATH` is required for execution. Supplied and archived executable hashes must match an actual successful locked/offline release receipt. Native arm64 UUID is checked with `xcrun dwarfdump --uuid`; profile runs additionally require matching dSYM UUID and every recorded dSYM file hash. Recorded build input hashes and revision are checked. No build is initiated.
- Passing validation is mandatory for exact case hash, binary/build receipt identity, fixture bytes and provider/observer configuration. Stock and symbolized executables require separate validation. Missing/failed validation produces explicit planned-slot skips and no setup/product launch. This is unit-tested without invoking product.
- Before executing any selected sample, a unique immutable `plan-<campaign>.json` enumerates all first/conditioning/repeat slots, including slots beyond profile-count limits. Every normal stop produces one durable outcome per slot: attempted result, setup failure, budget/resource skip, validation skip or stopped-after-failure skip. Interrupted/absent receipts remain independently detectable as missing, not reclassified as skips.
- Analysis partitions repeat statistics by campaign, stage, case, binary, build receipt, fixture and configuration. No cross-campaign/build/config pooling. First/conditioning/profile records remain excluded from ordinary-repeat distributions. Attempted/successful/invalid/censored/skipped/setup-failed/missing states are distinct. No throughput claim remains. Duplicate/unexpected/missing plan IDs, planned identity mismatches, stream corruption or RSS-unit mismatches suppress statistical output with explicit integrity findings.
- Raw ordinary measurements are written before result parsing; malformed JSON envelopes/nonobjects and later observation/assertion failures preserve wall/user/system/RSS/stream evidence. Profile postprocessing catches malformed structure/EOF/type/key/index errors and preserves raw wrapper plus child records.

### Sampling and drain completion

Sampling now launches the existing `tests/performance/window.py` mechanism as a thin adapter. Product window<=10s; samply gets a separate additional30s to finalize/save. The window's actual owned child PID is retained (an `env` exec applies the same allowlist without changing PID). Censored windows are non-baseline evidence, not successful end-to-end command timings.

Saved library code IDs and symbol sidecar IDs must match the pinned native build UUID. Existing `validate_profile` checks recording structure; existing `summarize_stacks(..., pid_filter=actual_product_pid)` supplies `threadCPUDelta`-weighted attribution. Wrapper stacks cannot satisfy the product sample threshold. No resolved Letopis weighted CPU function => unavailable; fewer than100 product nonnull stacks => insufficient. Raw sampler resource metrics remain wrapper-scope, never product baseline RSS.

Drain correctness counts indexed **items**, not HTTP requests; a valid batch of two inputs in one request is accepted. Completion additionally requires native current-config vector coverage (`covered_nodes`, missing/eligible/invalid counts, dimensions and populated verdict), not `vector_keys` alone.

Execution therefore accepts `--observer /ABSOLUTE/EXISTING/NATIVE-TEST-EXECUTABLE` for enabled drain validation. It invokes existing `observe_existing_corpus --ignored --exact` read-only observer interface with fixed request metadata outside measured drain time, recording observer hash/process/output separately. Missing observer is a validation gap, not a request to compile or a successful vector-completion claim. Existing hardcoded dimension3 checker is not reused unchanged; this driver checks dimension32.

Lease reacquisition is nonblocking with a deadline (at most2s and bounded by remaining stage time). Native observation/parse failures do not erase the completed drain record. Real snapshot import is narrowed to product `.rs`/`.sql`, knowledge `.md` and one explicit development document; hidden, sensitive and generated/evidence components are excluded at any depth before file reads. This is not a secret-content scanner; only the deliberately allowlisted public tracked subset is exported.

### Updated usage and execution ownership

Preparation still does not invoke product:

```bash
/opt/homebrew/bin/python3.12 -B \
  "research_notes/Letopis comprehensive performance profiling/tools/profile.py" prepare \
  --evidence /ABSOLUTE/NEW/EVIDENCE --include-real --stress-100k
```

Every validation/measure/profile invocation now also supplies actual compilation owner's `--build-metadata "research_notes/Letopis comprehensive performance profiling/evidence/build_metadata.json"`. For example, after canonical owner releases execution:

```bash
# First validate an exact supplementary case with stock binary.
/opt/homebrew/bin/python3.12 -B \
  "research_notes/Letopis comprehensive performance profiling/tools/profile.py" validate \
  --evidence /ABSOLUTE/NEW/EVIDENCE --binary /ABSOLUTE/STOCK/letopis \
  --build-metadata "research_notes/Letopis comprehensive performance profiling/evidence/build_metadata.json" \
  --case sparse-100.search-selective
```

Then measure only passing identities. To sample a case, first validate it with the **symbolized** binary and matching build receipt/configuration, then run `profile` for that same case. Explicit case selection is essential: framework breadth is not a directive to run all cases. Canonical60/1k/10k remains separate owner's work.

Remaining execution priorities: bounded unique-text drains2/100/1000 at32 dimensions with120s deadline, then selected50ms-delay/failure/malformed-provider cases; optional384/768 dimensions as one-variable controls. Current matrix still does not automatically implement all those follow-ups. Do not expand Cartesian regression scope. Short-command repeated launch captures and paired overhead controls can remain procedural after integrity gate. Native heap evidence and safely owned detached lifecycle remain separate unimplemented scope. No current product/profile validity is claimed by helper tests.

### Round1 test and mutation evidence

Commands:

```text
/opt/homebrew/bin/python3.12 -B -m unittest discover -s "research_notes/Letopis comprehensive performance profiling/tools" -p '*_test.py' -v
/opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py'
```

Test-first red run:28 tests,2 failures/9 errors for absent new APIs, wrong pooling, request-count drain rejection and throughput. A later focused red test rejected valid nonobject JSON via an unexpected `AttributeError`, exposing a raw-evidence loss path; fixed to normal validation rejection.

Focused restored tree initially:34 tests passed,1.993s; output [`round1-tests.txt`](evidence/harness-review-v1/round1-tests.txt). A final red regression exposed nonobject child JSON escaping before the profile parsing guard; moved receipt-shape handling inside that guard. **Final tree:35 tests passed,2.051s**, output [`round1-final-tests.txt`](evidence/harness-review-v1/round1-final-tests.txt). New checks independently exercise complete planned/skipped accounting and an exact-gate execution with no product launch, byte drift/additions, canonical seed65 validator at60 nodes, symbol/build mismatch, malformed profile/child preservation, native coverage despite batched HTTP, bounded actual competing-process flock, separate sampler/child deadlines/PIDs and disk root accounting.

Manual mutation batch: five distinct failures, each corresponding to one changed integrity rule:

1. Group by case only => identity-partition test failed (`1 != 8`).
2. Omit configuration from gate => exact-configuration rejection failed.
3. Disable immutable-input comparison => changed/additional input rejection failed.
4. Include wrapper stacks in sample threshold => PID test failed (`1002 != 2`).
5. Omit native covered-node check => native drain completion rejection failed.

Mutations restored; [`round1-mutations.txt`](evidence/harness-review-v1/round1-mutations.txt) contains actual failure output.

Existing suite rerun: **94 tests,8 skipped,1 error,82.333s**. Failure: `test_compare.ComparisonTests.test_failed_required_counter_marks_profile_campaign_incomplete`, `StopIteration` selecting a measured series at `tests/performance/test_compare.py:98`. Existing suite files were not modified; cause has **not** been established. Full output [`round1-existing-tests.txt`](evidence/harness-review-v1/round1-existing-tests.txt). This supersedes any implication above that the latest whole-suite run is green; do not treat earlier94-test pass as current evidence. No unrelated test repair or repeated benchmark work attempted.

No Rust/product/dependency checks run; no such files changed. Final scoped diff/snapshots allow re-review without rereading unrelated canonical owner artifacts.

## Review round2 handoff — ready for static review, not execution approval

The four findings in [round1 re-review](harness_rereview_round1.md) and the positive weighted CPU check have targeted changes. The partial implementation left by the previous author was preserved before recovery in [`recovery-before/`](evidence/harness-review-v2/recovery-before/). Final tool snapshots and the scoped diff against round1 are in [`after/`](evidence/harness-review-v2/after/) and [`round2.diff`](evidence/harness-review-v2/round2.diff). Recovery added only focused tests; no product code was changed.

- **Source identity:** `--build-receipt` is mandatory in addition to `--build-metadata`. `verify_product_source` uses the existing `scenarios.verify_build` owner to compare executable and complete product inventory, rejects dirty/untracked inputs, and checks current HEAD against the receipt. It rechecks before every planned sample and binds the receipt hash into build identity. Both archived stock and symbolized receipts contain 116 inputs and passed a read-only current-source check: [`source-inventory-check.txt`](evidence/harness-review-v2/source-inventory-check.txt). This does not run either binary.
- **Disk:** post-copy guard receives the full evidence root, counts it with scratch and stops before setup; the resource-stop reason is retained.
- **RSS:** setup wait4 peak is checked and persisted before response parsing, including failed/timed-out setup. Profile decisions use the product child receipt, not sampler-wrapper RSS; unavailable child peaks remain explicitly unavailable. A resource stop prevents future planned slots, including after setup failure or a usable censored profile. These remain soft post-observation checks, not live hard limits.
- **Coverage:** per-campaign/stage/case/build/fixture/configuration and observation-phase coverage delegates to `account_plan`. Completed, censored, invalid attempted, skipped, setup-failed and missing slots remain distinguishable; usable censored captures are not completed commands.
- **CPU:** a resolved `letopis::` function itself must have positive `inclusive_cpu_delta`; positive CPU elsewhere is insufficient.

Updated invocation, after independent review and release of exclusive measurement ownership:

```bash
PY=/opt/homebrew/bin/python3.12
ROOT="research_notes/Letopis comprehensive performance profiling"
"$PY" -B "$ROOT/tools/profile.py" validate \
  --evidence /ABSOLUTE/NEW/EVIDENCE --binary /ABSOLUTE/STOCK/letopis \
  --build-metadata "$ROOT/evidence/build_metadata.json" \
  --build-receipt "$ROOT/evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json" \
  --case sparse-100.search-selective
```

All execution stages require matching `--build-receipt`; profile must use the symbolized executable and `symbolized_release/build.json`. The earlier round1 examples omit this newly mandatory argument and are superseded. `prepare` and `analyze` do not require it.

### Verification

- Prior author's test-first red evidence: [`round2-red.txt`](evidence/harness-review-v2/round2-red.txt), 40 tests, one failure and seven errors before the partial implementation. Missing-API errors establish absence of the intended paths, not independently proven behavior for every case.
- Final command: `/opt/homebrew/bin/python3.12 -B -m unittest discover -s "research_notes/Letopis comprehensive performance profiling/tools" -p '*_test.py' -v` — **41 tests passed in 2.230 s**. Full output: [`round2-verified-tests.txt`](evidence/harness-review-v2/round2-verified-tests.txt).
- Single-condition in-memory mutations, using Python 3.12, were each detected: source owner bypass, wrong disk root, setup RSS bypass, absent profile child RSS, censored-as-completed count, and zero-weight Letopis acceptance. Output: [`mutations-round2.txt`](evidence/harness-review-v2/mutations-round2.txt). The initial disk mutation reached an incomplete fake provider instead of setup; the test was strengthened, and [`mutation-disk-confirmed.txt`](evidence/harness-review-v2/mutation-disk-confirmed.txt) shows the wrong root actually launched mocked setup. Setup RSS bypass produced the expected malformed-JSON error instead of the resource-stop error.
- Disabling stage-wide resource-stop propagation made both setup-failure and censored-profile cases fail: [`mutation-stage-stop.txt`](evidence/harness-review-v2/mutation-stage-stop.txt). Mutations were in-memory only; `cmp` confirmed `tools/profile.py` is byte-identical to `recovery-before/profile.py`. The test file was intentionally strengthened.
- The earlier [`recovery-mutations.txt`](evidence/harness-review-v2/recovery-mutations.txt) used incompatible system Python 3.9 and stopped on an ambiguous anchor. It is preserved as failed verification, not evidence of a harness defect.

No product benchmark, build, native observer, sampler or heavy existing suite was run during recovery. Last full existing suite remains 94 tests, eight skipped and one error; the separately preserved diagnosis established the fake initialization timeout path, not the cause of its delay. Static re-review and bounded runtime validation remain necessary before supplementary measurements.
