# Issue #65 Performance Diagnosis Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Produce reproducible model-free measurements, samply profiles and a ranked, causally supported bottleneck report for the current Letopis CLI.

**Architecture:** An external Python runner operates only on generated standalone repositories and invokes an unchanged release binary. Measurement, scenarios, profiling and analysis own separate responsibilities; product instrumentation is conditional on a demonstrated evidence gap. The campaign surveys 1k/10k first, profiles selected cases, then assesses 100k and controlled contention.

**Tech Stack:** Python 3.11+ standard library, existing deterministic corpus generator, Rust 1.95.0 locked/offline release build with debug symbols, Git, SQLite read-only metadata observations, samply 0.13.1 and optional macOS sample.

**Spec:** [Approved specification](../specs/2026-10-30-issue-65-performance-design.md).

Status: proposed 2026-10-30. Branch `issue-65`, worktree `/Users/zdkadiev/.codex/worktrees/issue-65/letopis`. Initial product commit `f7e0c6c90a7e4d4aa49e0a0c788c89ef7317c43b`. No implementation or benchmark execution is authorized by this document alone.

## Global Constraints

- Model-free initially: no real provider/model requests, model downloads or paid sessions. Bind the fake server to numeric loopback; never reuse ambient configuration.
- Python must be >= 3.11. On this host, bare `python3` resolves to Apple's 3.9.6; use `/opt/homebrew/bin/python3.12` (verify its version) for every `python3` command shown below. This notation means the selected interpreter, not permission to use the older system executable or install another interpreter.
- Generate only new fixture/evidence directories; historical receipts, user repositories and main's pending edits remain untouched. Reject existing output paths, symlinks and input/output overlap before writes.
- Release, locked/offline build; record full commit, build-input hashes, binary hash and harness identity separately. A changed product build gets a new baseline.
- 1k/10k/100k knowledge nodes; 2,000-byte fragments; 30 elements per file; fixed seed 65. Record actual counts, bytes and vector coverage.
- Two conditioning calls plus 20 measured calls for inexpensive repeatable scenarios; three initial observations for expensive refresh/reindex and 100k. p95 only at n >= 100, p99 only at n >= 1,000; no confidence claim from those thresholds.
- Default invocation watchdog 120 seconds, stage budget 20 minutes; explicit recorded full-audit/100k override at most 600 seconds. Timeouts are censored, not successful samples.
- CLI invocations remain fresh processes. Uncontrolled OS page cache is never labeled cold. Sampled RSS is not an exact peak; process scopes and missing metrics are explicit.
- Synchronous product I/O, DDD/hexagonal boundaries and default correctness remain intact. No optimization, new runtime, permanent tracing stack or public command.
- New functions/methods aim at 40 lines, maximum 60; files remain focused. Given/When/Then tests use independent expectations and hand mutation checks.
- Task commits include only listed paths. No merge/push or GitHub publication in this plan without applicable user authorization.

## Review Focus

1. Timeout/interrupt while a descendant retains a pipe: bounded collection and cleanup, with a failed receipt rather than a hanging runner (Task 2).
2. A benchmark reused after its first mutation: each measured edit starts from equivalent pre-edit projection and files (Task 1).
3. An externally supplied or rebuilt binary mismatched to its source receipt: reject before measurements and preserve old evidence (Task 1).
4. A search silently falls back to FTS or an automatic detached drain survives: reject vector-participation claims and account for every owned process (Task 5).
5. Short/no-sample or truncated samply profile, or CPU stacks containing no wait attribution: reject profile success or mark attribution unavailable (Task 4).

## File map and dependencies

Create only files with functionality in the task that needs them:

| File | Responsibility |
| --- | --- |
| `tests/performance/run.py` | CLI entry, stages, budgets, immutable evidence publication |
| `tests/performance/scenarios.py` | Corpus generation, release build receipts, deterministic requests/edits/reset |
| `tests/performance/measure.py` | Owned process lifecycle, bounded output, timing/resource records |
| `tests/performance/report.py` | Sample validation, distributions, coverage and ranked evidence summaries |
| `tests/performance/profile.py` | samply runs, profile validation, separate Git-count experiments |
| `tests/performance/git_wrapper.py` | Diagnostic executable selected through PATH; forwards argv/stdin and records operation/timing |
| `tests/performance/provider.py` | Owned deterministic loopback fake, request/barrier counters |
| `tests/performance/contention.py` | Owned drain/writer-reader schedules and metadata observations |
| `tests/performance/test_{scenarios,measure,report,profile,provider,contention}.py` | Tests beside the responsibility they exercise |
| `tests/performance/README.md` | Reproduction, metrics/scope, budgets, isolation and limitations |
| `docs/zkit/letopis/checks/issue-65/report.md` | Durable findings, coverage and follow-up issue drafts |

Reuse `tests/target_corpus/generate.py` through an explicit import by path with its existing `CorpusOptions` and `generate_corpus(output, options)`; do not call its model-specific `run.py`. Existing generator and product files are read-only by default. Existing `/.artifacts/` ignore rule already covers raw evidence. `src/` changes are a conditional decision in Task 6, not a planned refactor.

Dependency order: 1 → 2 → 3 → 4 → 5 → 6. Each task ends with one reviewable deliverable and a scoped commit. Keep each test run to its relevant suite until the final integration check.

## Task 1: Reproducible fixtures, requests and release identity

**Files:** create `scenarios.py`, `test_scenarios.py`, initial `run.py` and README in `tests/performance/`.

**Interfaces:** `prepare_fixture(output: Path, nodes: int) -> dict`; `build_release(source: Path, target: Path, evidence: Path) -> dict`; `prepare_scenario(fixture: dict, name: str, output: Path) -> dict`. Returned scenario keys: `repository`, `argv`, `stdin`, `expected`, `setup_seconds`, `state_hashes`, `cache_label`, `embedding_mode`. Paths are absolute; bytes stay private and become hashes/bounded synthetic results in receipts. Build receipt includes executable path and SHA-256, product commit/build-input hashes and compiler/profile settings.

- [ ] **1. Write failing fixture/identity tests.** Use a 60-node fixture; independently expect 2 Markdown files, 60 knowledge nodes, 60 sources, 300 edges. Assert one-file refresh changes one file, several-file refresh changes exactly two, and bulk refresh changes every knowledge file, without changing marked fragment validity. Assert two prepared mutation samples have equivalent pre-edit states. Existing output/symlink/overlap or mismatched binary/source pin must fail before product execution.
- [ ] **2. Run RED.** `python3 -B -m unittest discover -s tests/performance -p test_scenarios.py -q`; expect missing implementation failures.
- [ ] **3. Implement fixture/build/scenario setup and runner CLI.** `python3 -B tests/performance/run.py --stage survey|profile|scale|contention --nodes N --evidence NEW_DIR [--baseline MANIFEST]`. Add `--list-scenarios` with no execution; default execution is disabled-embedding FTS. Build via Cargo JSON artifact discovery with `CARGO_PROFILE_RELEASE_DEBUG=1`, an explicitly recorded target directory, `cargo build --release --locked --offline --message-format=json`. Read/hashes before and after build must match. Store the actual tree state independently of commit cleanliness; reject product-input dirtiness unless producing a separately identified diagnostic build. Initialize and validate projection outside timers; configure `.letopis/config.json` before any product call.
- [ ] **4. Verify GREEN and mutate.** Re-run the suite; remove the identity guard and then the reset guard separately and confirm their tests fail. Restore before continuing. README records all created paths and excludes compilation/setup from timed latency.
- [ ] **5. Commit scoped deliverable.** `git add tests/performance/scenarios.py tests/performance/test_scenarios.py tests/performance/run.py tests/performance/README.md`; `git commit -m 'test: prepare isolated performance scenarios and build receipts'`.

## Task 2: Bounded process measurement and ownership

**Files:** create `measure.py`, `test_measure.py`; modify `run.py`.

**Interfaces:** `measure_process(argv: list[str], repository: Path, data: bytes, options: dict) -> dict`; options contain deadline, explicit sanitized environment and capture limit. Result keys: `started_monotonic`, `duration_seconds`, `pid`, `outcome`, `exit_code`, `stdout`, `stderr`, `resources`. Outcomes distinguish success/nonzero/timeout/interrupted/spawn_failed. Each stream records byte count/hash, bounded capture and truncation. Resource fields carry value/unit/scope/method or an unavailable reason.

- [ ] **1. Write failing lifecycle tests.** Tiny child programs cover valid output, nonzero, output above 64 KiB, invalid UTF-8, interrupted input, timeout, and a same-group grandchild retaining stdout after its parent exits. Assert deadlines cover pipe EOF and cleanup, partial receipts survive, and an unrelated sentinel process remains alive. Resource fields never claim exact sampled peaks or derive per-run CPU by subtracting global cumulative child counters.
- [ ] **2. Run RED.** `python3 -B -m unittest discover -s tests/performance -p test_measure.py -q`.
- [ ] **3. Implement direct owned sessions and bounded pipe drain.** Use argv, `start_new_session=True`, concurrent bounded readers and monotonic deadlines; terminate/escalate/reap only owned children/groups, even if the direct child already exited while pipes remain open. Prefer per-child `wait4` resource accounting where supported; retain documented OS semantics and units, label combined child accounting honestly. Unsupported metrics are unavailable. Never sum unrelated lifetime peaks or subtract them. On terminal error publish a failure receipt without raw credentials. Runner writes a partial manifest early and final JSONL records before stopping at the stage deadline.
- [ ] **4. Verify GREEN and mutate.** Remove group cleanup and change timeout classification in separate temporary mutations; both must fail. Restore. Test malformed CLI output yields a correctness failure even at exit zero.
- [ ] **5. Commit scoped deliverable.** Add only `measure.py`, `test_measure.py`, `run.py`; commit `test: measure owned CLI processes with bounded cleanup`.

## Task 3: Survey, correctness oracles and summaries

**Files:** create `report.py`, `test_report.py`; modify `scenarios.py`, `test_scenarios.py`, `run.py`, README.

**Interfaces:** `validate_result(scenario: dict, measurement: dict) -> dict`; `summarize_samples(records: list[dict]) -> dict`; `write_report(evidence: Path) -> Path`. Validation returns observed IDs/counts/snapshot checks and `valid`; only valid successful observations enter latency quantiles. Failures and timeouts remain in totals and throughput accounting. Summary schema version 1 fixes nearest-rank quantiles and stores n, min/max, median, optional p95/p99 and unavailable reasons.

- [ ] **1. Write failing correctness/statistics tests.** Literal values `[1,2,3,4]` yield median 2.5, min 1, max 4, no p95/p99; `range(1,101)` yields p95 95 with nearest rank. Assert timeout durations are excluded from success quantiles but counted as attempted calls. Wrong IDs, false `ok`, nonempty check errors, invalid inspect hashes and missing traversal edges invalidate samples. Assert first-use/conditioning/measured and FTS/fake modes cannot be pooled.
- [ ] **2. Run RED.** `python3 -B -m unittest discover -s tests/performance -p test_report.py -q`.
- [ ] **3. Implement deterministic requests and survey orchestration.** Use the generator's marked IDs (first node `knowledge/part-0000.md#node-000000`) and independent expected graph edges from fixed small fixtures. Read captured inspect bytes from its returned Git tree and verify SHA-256; check actual requested coverage rather than assuming exit zero proves it. Survey unchanged FTS search, inspect, depth-2/limit-20/edge-limit-40 traverse, unscoped check, status, three edit patterns and reindex. Edit an existing ordinary body token outside metadata/links; preserve exact mutation recipe. First-use/conditioning are retained separately. Measure 20 cheap or 3 expensive repetitions under budgets and write an explicit pending/skipped row for every remaining scenario/mode/scale.
- [ ] **4. Verify GREEN and mutate.** Temporarily include timeout as a latency success and bypass one correctness check; tests must fail. Run all performance tests quietly. Keep a validation failure visible even if later successful samples exist.
- [ ] **5. Commit scoped deliverable.** Commit only touched performance files: `test: add validated CLI survey and performance summaries`.

## Task 4: Samply profiles and Git attribution

**Files:** create `profile.py`, `git_wrapper.py`, `test_profile.py`; modify `run.py`, report and README.

**Interfaces:** `record_profile(scenario: dict, build: dict, evidence: Path) -> dict`; `validate_profile(path: Path) -> dict`; `record_git_calls(scenario: dict, build: dict, evidence: Path) -> dict`. Profile result includes sampler version/settings, elapsed wrapper duration, profiled process scope, file hash, thread/sample counts, paired baseline references and unsupported-attribution fields.

- [ ] **1. Write failing adapter tests.** Fake samply verifies exact argv uses `record --save-only --rate 1000 --output NEW_FILE`, no browser/server/upload. Reject exit-zero missing, truncated or zero-sample profiles; fixture gzip JSON has a known nonzero sample count. Missing profiler/permission failure becomes unavailable, never an empty successful profile. Git wrapper forwards binary stdin/stdout/stderr byte-for-byte and exit status, including paths with spaces; evidence records only command category, IDs and timings, not blob arguments/content. Resolve the real Git executable before adding the wrapper to PATH; reject recursive resolution.
- [ ] **2. Run RED.** `python3 -B -m unittest discover -s tests/performance -p test_profile.py -q`.
- [ ] **3. Implement the profiler and counter adapters.** Profile the product executable directly with JSON stdin; own sampler and descendants under Task 2 lifecycle rules. Pair three unprofiled and three profiled equivalent-state executions per selected scenario, in alternating order, recording overhead and correctness. Preserve profiles as a separate series. Use macOS `sample` only for supplementary wall-time stacks if necessary and record command/settings. Treat sandbox `Unknown(1100)` as an execution-permission failure: use authorized escalation without changing global security settings. Install nothing. Git wrapper is a separate instrumented experiment; `GitProcess` strips `GIT_*`, so do not rely on inherited Trace2 variables. No sum of overlapping Git intervals or claim that external Git children were captured without inspecting profile contents.
- [ ] **4. Verify GREEN and mutate.** Remove zero-sample rejection and then stdin forwarding; tests fail. Restore. Verify an actual tiny release CLI profile with samply outside sandbox; report if a short command has inadequate samples and select a longer valid scenario rather than inflating coverage.
- [ ] **5. Commit scoped deliverable.** Commit only touched performance files: `test: profile CLI scenarios with samply and separate Git counters`.

## Task 5: Fake-vector and controlled contention scenarios

**Files:** create `provider.py`, `contention.py`, `test_provider.py`, `test_contention.py`; modify scenarios, runner, report and README.

**Interfaces:** `start_provider(evidence: Path) -> FakeProvider` context manager, exposing numeric loopback URL, request counters, barrier controls and cleanup; `build_observer(source: Path, target: Path, evidence: Path) -> dict`; `observe_vectors(scenario: dict, observer: Path, evidence: Path) -> dict`; `run_contention(scenario: dict, build: dict, evidence: Path) -> dict`. Keep fake provider lifetime outside CLI latency and report its CPU/RSS separately where observable.

- [ ] **1. Write failing fake/ownership tests.** Fixed dimension-3 vectors and an explicit unique canary/query prove a node absent from FTS candidates is returned through the vector path. Reject missing/current-hash/config mismatches, unchanged fake request count on a supposed query embedding, and malformed responses. Tiny schedules assert one direct owned `embed --drain` process performs the work, a reader overlaps an observed writer interval, cancellation releases locks, and unrelated processes survive. Assert Python embed-lock coordination interoperates with the product before relying on it.
- [ ] **2. Run RED.** `python3 -B -m unittest discover -s tests/performance -p 'test_*provider.py' -q` and `python3 -B -m unittest discover -s tests/performance -p test_contention.py -q`.
- [ ] **3. Implement fake population and contention without orphan drains.** Configure nested `embedding_model`, model `performance-fixture`, retries 0 and a loopback URL owned by this runner. Never invoke model inventory. Before first enabled foreground refresh, hold `.letopis/embed.lock` through an owned interoperable lease to prevent automatic detach, release it after foreground completion, then run `embed --drain` directly as an owned child. Populate before isolated fake-query measurements. For drain contention, wait for a fake request barrier proving the direct drain owns the embed lock before readers start; stop/reap readers before releasing/stopping the writer so readers cannot launch detached successors. FTS reindex contention uses disabled embeddings. Detect and reject any unaccounted drain rather than killing by process name. Observe SQLite metadata read-only after writer completion against the documented schema; hash/config/dimension/current-key coverage is distinct from semantic relevance. Fake provider never follows redirects or forwards a request.
  For vector coverage, build the existing `target_corpus` integration-test executable with `cargo test --release --locked --offline --test target_corpus --no-run --message-format=json` and pin its source/binary identity outside measurement. Invoke only `observe_existing_corpus --ignored --exact` with `LETOPIS_CORPUS_REQUEST` naming a new request containing absolute `root`/`output` and `expected_knowledge`, `expected_sources`, `expected_edges`. This observer reads native vec1 through product storage, validates current config/hash/dimension coverage and makes no model request. Do not try loading vec1 through Python SQLite; Python handles only ordinary read-only metadata queries. Its observations are outside timed intervals. Vector participation still needs the separate canary assertion; neither observation establishes semantic relevance.

- [ ] **4. Verify GREEN and mutate.** Break vector coverage checking and omit reader-before-writer cleanup separately; tests fail. Restore. Verify post-run `check`, current node/edge counts and committed projection integrity on a tiny real CLI fixture. Count errors/timeouts alongside successful contention calls.
- [ ] **5. Commit scoped deliverable.** Commit only touched performance files: `test: assess fake-vector overhead and controlled CLI contention`.

## Task 6: Execute the bounded campaign, verify causes and report

**Files:** modify README and approved spec/plan status; create `docs/zkit/letopis/checks/issue-65/report.md` and supporting durable evidence index. Raw evidence stays under new ignored `.artifacts/issue-65/` directories.

**Interfaces consumed:** runner stages and manifest schema from Tasks 1–5. **Produced:** coverage table, raw-evidence hash index, confirmed/suspected/not-reproduced ranking and local follow-up issue drafts. No speedup claim without a separately measured correct implementation.

- [ ] **1. Verify tooling before campaign.** Run `python3 -B -m unittest discover -s tests/performance -p 'test_*.py' -q`, then the generator's `python3 -B -m unittest discover -s tests/target_corpus -p test_generate.py -q`. Record test counts and mutation receipts, verify no temporary mutations remain. Run a 60-node end-to-end fixture for every runner stage before 1k; smoke scale is not target-scale evidence.
- [ ] **2. Run Stage A then Stage B.** Use `python3 -B tests/performance/run.py --stage survey --nodes 1000 --evidence .artifacts/issue-65/survey-1k-<unique>` and the 10000 equivalent, preserving the build manifest for subsequent stages. Select the two highest absolute-cost or growth outliers and profile each with `--stage profile --baseline <manifest> --nodes N --evidence <new directory>`. Do not profile a known-cheap case merely to produce a file. Record machine load, equivalent input state, overhead and unknown attribution; keep progress updates during long runs and stop at budgets.
- [ ] **3. Run Stage C and causal controls.** Execute `--stage scale --nodes 100000` and `--stage contention` with pinned manifests in separate directories. Survey all requested scenarios or name each unassessed row; include sequential fake drain and fake query overhead, leave real hybrid/model-backed drain unassessed. Check #49 in the measured revision; add parallel cases only if actually implemented. For each candidate use one controlled variable (e.g. unchanged source tree expanded while knowledge/requests stay fixed, changed-file count, or writer presence), preserving all other inputs, and record actual controls. If external evidence cannot resolve SQL/lock/hash attribution, document the precise gap before adding minimal local diagnostic hooks. Any hooks require new instrumented build identity, paired overhead runs, focused mutation-tested tests, `make check` and `make verify`; do not introduce public interfaces or ship an optimization. If hooks are unnecessary, explicitly report no product changes.
- [ ] **4. Review report and evidence independently.** Audit every coverage row, correctness oracle, profile sample/symbol sufficiency, process cleanup, quantile denominator, cache label, code-owner attribution and causal claim. Use a fresh reviewer according to the chosen execution workflow. Record exact commands/results, failures, interrupted stages, unavailable metrics/platforms/models, and raw artifact hashes. Rank confirmed costs by observed impact; label all other candidates suspected/not reproduced. Draft separate optimization issues with baseline, improvement criterion and risks, but publish only when authorized. Check document links and `git diff --check`; rerun product/native harnesses only if their inputs actually changed, and name those not rerun.
- [ ] **5. Commit the diagnostic result.** Commit only intended documentation, final runner changes and any explicitly justified hooks/tests. Never stage raw corpora/profiles or unrelated files. Suggested message: `docs: report measured Letopis performance bottlenecks`. No automatic GitHub closure or formal gate verdict; hand off the ranked findings and remaining gaps for review.

## Self-review and execution handoff

Spec coverage: Task 1 owns source identity/isolated inputs/reset; Task 2 lifecycle/resources; Task 3 full scenario inventory/correctness/statistics; Task 4 actual samply profiles/overhead/Git; Task 5 vector-client scope/ownership/contention; Task 6 scale/causality/coverage/report. All five Review Focus items have explicit tests. Product hooks are conditional, not an excuse for speculative work. No paid/model execution or optimization is included.

Self-review checks: task interface names/keys align; fixture and build setup precede measurement; cleanup accounts for EOF and possible detach; quantiles separate timeouts and conditioning; final report cannot substitute fake vector behavior for true hybrid performance. Files and commands named above are planned additions, not claimed existing or executed commands.

Plan approval and execution method are pending. Native execution is recommended because the tasks share one evidence schema and lifecycle model, with a fresh whole-branch review at the end. Subagent-driven execution remains available with per-task implementation/review gates. Implement only after the user's plan review.
