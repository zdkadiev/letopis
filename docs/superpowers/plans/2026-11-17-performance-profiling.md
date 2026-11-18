# Comprehensive Letopis Performance Profiling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. This worker does not spawn agents.

**Goal:** Reuse existing performance tooling for complete command coverage, OS measurements and separate samply evidence, without product edits.

**Architecture:** Thin campaign adapter composes existing generator/validator, measurement/reset, sampling/window, ownership/native observer, and build-provenance tooling. Only uncovered cases and OpenAI-protocol fixture are new research code.

**Tech Stack:** Python>=3.11, existing tests/performance modules, Git fixture/oracle CLI, pinned Cargo/Rust, macOS wait4/time, samply.

**Spec:** [2026-11-17-performance-profiling-design.md](../specs/2026-11-17-performance-profiling-design.md), including authoritative reuse-first amendment.

## Global Constraints

- No production/Cargo/dependency changes; no staging or commits.
- Local deterministic OpenAI-protocol mock only; no real model/external requests.
- Serial baselines; sampling separate; no OS-cold claims or conflation of maxRSS with allocations. Native heap/malloc_history/leaks diagnostics permitted as separately scoped evidence without product instrumentation.
- Canonical seed65: empty0 separately, smoke60/1k/10k baseline;100k gated stress.
- Budget: baseline30min, sampling15min/900s, total90min excluding build/install; <=20 captures, default10s windows with one authorized complete100k full-index exception <=600s product call if disk/budget permit; ordinary120s, gated100k initialization/full600s, stage1200s. Soft pre/post combined scratch+evidence2GiB guard; observed post-sample1GiB maxRSS stops further work. No hard instantaneous ceiling/live monitor claim.

## Review Focus

- Existing builder debug=1 must not become stock receipt. Canonical disabled Ollama config retained without override hook: local FTS, no provider measurement; supplementary active mock OpenAI only.
- Successful fallback is not hybrid participation; native observer coverage and query canary required.
- Copy/reset changes inodes; refresh outside timing, never copy active WAL state.
- Existing wait4 accounting/cleanup scope must remain explicit; detached RSS is not parent peak.
- Large setup/full can exceed defaults; retain censored evidence and distinguish initial versus normal timeout.

## Files and existing interfaces

Research prefix: `research_notes/Letopis comprehensive performance profiling/`.
Actual supplementary files: `tools/profile.py`, `tools/profile_test.py`. No separate campaign/openai/test_campaign modules required. Stabilized implementation defines CLI/reproduction; canonical old run.py separate, sparse supplement never replacement.
Update STATUS/final report. New immutable run root `evidence/<run-id>/`, raw `profiles/`.

Reuse:
- `tests/target_corpus/generate.py`: `CorpusOptions`, `generate_corpus(output, options)`.
- `tests/target_corpus/validate.py`: `validate_corpus(manifest_path)` canonical oracle.
- `tests/performance/scenarios.py`: fixture/reset/initialization, sanitation, hashes, build receipts, expected-result oracles.
- `tests/performance/measure.py`: `measure_process(argv, repository, data, options)`.
- `tests/performance/profile.py`: `record_profile(scenario, build, evidence)` and usable sample/symbol checks; `window.py` censored CLI-PID windows.
- `tests/performance/report.py`: validation/statistics; inspect exact API before composing.
- `tests/performance/compare.py`: frozen executable/source/symbol provenance, not unnecessary optimization comparison.
- `tests/performance/ownership.py`, `contention.py`: exact owned PID/drain/lock/barrier and native observer helpers.
- `tests/performance/controls.py`, `historical.py`: applicable packing/external/source controls.
- `tests/performance/provider.py`: loopback barrier/canary reference only; current route native Ollama.

Do not duplicate existing engines in research-local modules; actual profile.py is supplementary composition. Preserve raw schemas and campaign/build/fixture identity in summaries/audit.

Entry point (candidate interpreter, verify first):
```bash
/opt/homebrew/bin/python3.12 -B "research_notes/Letopis comprehensive performance profiling/tools/profile.py" --help
```
Record actual supported phase commands after fix-round1 stabilizes; no invented stage interface. Preserve immutable evidence/resume identities.
Test command:
```bash
/opt/homebrew/bin/python3.12 -B -m unittest discover -s "research_notes/Letopis comprehensive performance profiling/tools" -p 'profile_test.py' -v
```

### Task 1: Compose reuse interfaces and local OpenAI fixture

**Files:** Actual profile.py/profile_test.py.
**Interfaces:** Reuse existing scenario dictionaries; local provider lives in actual adapter. Final commands/functions follow stabilized implementation, no separate speculative provider API.

- [ ] Read completed workload_design.md, existing README, report/ownership/native observer APIs and tests; record reused functions and genuine gaps.
- [ ] Add failing tests for `/v1/embeddings` indexed response, literal canary vectors/dimensions, loopback-only routing, supplementary configuration before first CLI, absent-output guard. Canonical disabled config unchanged, not mislabeled OpenAI.
- [ ] Run tests to intended failure; implement narrow OpenAI fixture using existing ownership/barrier pattern, finite deterministic32-dimensional vectors, dummy key, success/delay50ms/HTTP failure/malformed modes.
- [ ] Stabilize actual supplementary dispatcher/uncovered cases; separate sparse/empty fixtures from canonical generator/validator results, no duplicate engine.
- [ ] Run tests; mutate index/config/canary guard individually, confirm failures, restore and rerun.

### Task 2: Pin builds, validate measurement and semantics

**Files:** Campaign/test extensions; evidence artifacts.
**Interfaces:** prepare writes build/fixture/case receipts; validate marks case eligibility/expected outcomes for baseline.

- [ ] Record OS/compiler/Python/profiler/permission/disk preflight; pin stock locked/offline release and separate optimized release symbols sibling using existing provenance machinery. Sampling sibling uses only `CARGO_PROFILE_RELEASE_DEBUG=1`, no strip override; validated matching dSYM works with existing harness, so no gratuitous debug2 rebuild. Explicitly avoid debug=1 override for stock; no Cargo changes.
- [ ] Run relevant existing measure/reset/profile/ownership and generator-validator tests; preserve prerequisite integration skips. Cross-check wait4 CPU/RSS against independent `/usr/bin/time -l` helpers; retain documented cleanup/reaped-descendant scope and OS counters.
- [ ] Add failing adapter tests for fallback-not-hybrid, native observer canary/coverage, scoped index versus check paths, copied inode refresh, realistic-export missing basis and partial-failure receipts.
- [ ] Implement minimal validations, all eight commands/all index forms/internal drain/help/version/errors, expected IDs/flags/revision/hash rather than only ok:true.
- [ ] Validate empty/smoke60; canonical oracle only certifies canonical graphs. Own-repo content export uses independent Git/cache and preserves missing-basis diagnostics.
- [ ] Run tests/validate; mutation-check new assertions, restore and record eligibility/gaps before matrix.

### Task 3: Serial baseline and cost-gated scale

**Files:** Campaign adapters only if needed; raw evidence.
**Interfaces:** baseline composes measure_process stock-binary calls; native records gain case/sample/setup labels without losing fields.

- [ ] Test finite coverage/repeat IDs, budget skip/timeout receipt and duplicate detection; implement minimal adapter and verify failing controls.
- [ ] Run complete empty/smoke60 coverage, meaningful1k/10k operations serially. Preserve two conditioning plus20 cheap/3 expensive measured calls and existing >2s expensive switch/mutation/full/100k classification; first-observed separate. Record budget reductions with requested/actual n and skips; flags/help/errors coverage receipts need not multiply expensive aliases.
- [ ] Run representative1k sparse/chain/star/dense-ish/long-text/source variants with independent manifests/oracles and applicable existing controls; avoid full cross-product.
- [ ] Reuse owned embed.lock/drain ownership to isolate enabled vector/query cases, direct known-backlog drain and dedicated lifecycle overlap; label lease suppression and server-thread CPU/separate RSS unavailable.
- [ ] Gate100k on actual lower-scale setup/reset costs and remaining budget. Historical~163s setup/~207s full/~41s check/status justify caution, not predictions. Use1–3 expensive observations explicitly, <=600s call/1200s stage, stop after failed setup.
- [ ] Reuse stable private read fixtures with pre/post hashes/revision/coverage when copies expensive. Preserve failures/skips; no OS-cold label or hidden repetition reductions.

### Task 4: Separate samply profiles and hotspots

**Files:** Campaign adapters; raw profiles.
**Interfaces:** sample composes record_profile/window on symbols sibling; no sampled latency in baseline.

- [ ] Select <=20 captures from observed expensive/growing operations; save-only1000Hz/presymbolication, no browser/upload.
- [ ] Reuse bounded censored-window wrapper with default10s capture, actual CLI-PID stacks/independent child outcome. One complete100k full-index capture authorized within900s sampler-stage/600s product-call cap if disk/budget permit. Tiny operations aggregate with wrapper attribution or insufficient-samples label.
- [ ] Run paired unprofiled/profiled controls where budget permits; no extra178s ordinary full-index control required. Stock baseline versus debug1 sampled sibling is not matched-build overhead control; state limitation. Verify usable symbols/stacks, preserve permission/symbol gaps.
- [ ] Attribute frames to exact source/lines; distinguish observed CPU, waits, hypotheses, mock versus model cost, RSS versus allocations.
- [ ] Incorporate completed memory-agent tool constraints; attempt supported native heap/malloc_history/leaks diagnostics separately from baseline without product instrumentation. Preserve command/environment/raw output and process/lifetime scope; mark unavailable only with attempted-tool or documented constraints.

### Task 5: Audit raw evidence and save report

**Files:** Campaign-only audit/tests; STATUS/report.
**Interfaces:** summarize reuses report statistics; audit independently reads native receipts/manifests versus expanded coverage and expected outcomes.

- [ ] Write failing literal-statistics/duplicate/missing sample/hash mismatch/nonloopback endpoint tests; implement minimal missing audit, mutation-check then restore/rerun.
- [ ] Run summarize/audit; reconcile every command with evidence or explicit unavailable/not-applicable reason. Verify counters/units/scopes, failed-sample exclusion, profile/build/fixture hashes.
- [ ] Fill report with exact data, first/warm states, n/censoring, mock/FTS/vector/background scope, source hotspots and limitations; link raw artifacts and reproduction commands.
- [ ] Check links/JSON/hashes and actual diff/status; production/Cargo unchanged, no staging/commits. Update only evidence-backed STATUS phases.

## Handoff

### Execution ledger / fix-round1

[Canonical100k stress](../../../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress.md) complete20/20 valid (19 measured+1 publication), no failures; full178.498726s n=1, maximum443.61MiB individual lifetime RSS. [Audit PASS with qualifications](../../../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress_audit.md):4334/4334 input hashes confirmed. Intended zero provider calls is disabled-config assertion, not network tracing; soft guards not instantaneous.100k disabled OpenAI versus earlier disabled Ollama explicitly separate.

Profiles exclusively owned by existing-infrastructure worker; counts/results not complete. Complete100k full-index sampling exception above approved based on observed178s bottleneck, no repeat ordinary control mandated. Supplementary review gate remains separate, not passed by canonical audit.

[Canonical survey](../../../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_survey.md): complete352/352 valid,319 measured/30 conditioning/3 first-use at60/1k/10k, stock archived build receipts linked there. Nine old-runner mixed graph/source scenarios complete; full all-command supplementary matrix not yet complete. Do not rerun/relabel canonical sparse.

[Harness review](../../../research_notes/Letopis%20comprehensive%20performance%20profiling/harness_review.md): **not approved**, fix-round1 underway. Before supplementary substantial measurement/profile claims, verify identity-partitioned summaries, planned/attempted/missing sample ledger, enforced validation, immutable inputs, CLI-PID/matching-symbol profiles, proper censored windows and bounded ownership cleanup. Re-review gate still open.

Rulings: separate canonical old runner from sparse supplement; retain canonical disabled Ollama configuration because no override hook and no intended provider calls; replace invented hard live RSS/disk ceilings with honest post-sample/pre-post soft guards above. No monitoring-framework expansion. Final report states scope/overshoot limitations.

Autonomous approval retained; no new design checkpoint. This worker updates documentation only, no harness/build/benchmark execution or commits. Existing20-cheap/3-expensive/two-conditioning defaults retained, native memory diagnostics separately allowed, actual adapters profile.py/profile_test.py. Incorporate completed findings explicitly; review approval never inferred from fixes underway.
