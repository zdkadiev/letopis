# Canonical 100k stress — 2026-11-17

## Scope and bounded method

Baseline-only stock release at `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. Existing canonical generator and validator, seed 65, 100,000 knowledge elements, 2,000 bytes per fragment, 30 elements per file. No historical shape rewrite, compiler invocation, product edit, provider workload, profiling matrix, or heap tooling.

Pre-run disk inspection:

- Existing report evidence: **784,592 KiB allocated** (766.20 MiB), using `du -sk`.
- Existing 10k canonical survey alone: **620,716 KiB allocated**.
- Filesystem free space: **67,240,672 KiB**. Free capacity is not the retained-artifact budget.
- `run.py --help` confirms scale uses same survey path, selectable read scenarios and repetition override, but not a stable shared fixture option. `prepare_scenario` copies the corpus and refreshes copied projection. Avoiding five or more duplicate 200 MB corpora is more important than calling this entry point unchanged.
- Existing historical runner rewrites canonical source shape and requires debug1 frozen-comparison receipts; it is unsuitable for this stock/canonical seed65 requirement.

**Selected approach:** one-off composition of existing owners around **one stable disposable repository**. [Launcher source](evidence/canonical-stress-launch-20261117-v1.py) is captured evidence, not a new shared/reusable harness. Imports existing generator, independent corpus validator, build verifier, request/oracle owners, measurement process controller, JSONL persistence and summary owners; no changes to shared `tests/performance`.

Exact command from repository root:

```sh
/opt/homebrew/bin/python3.12 -B \
  'research_notes/Letopis comprehensive performance profiling/evidence/canonical-stress-launch-20261117-v1.py'
```

## Time, disk, memory and stop rules

- One stage, **1,200 seconds**, including setup and validation.
- First publication: one search, **up to 600 seconds**.
- Check/status/inspect/search/traverse/no-op index: **three measured calls each**, **120 seconds per call**. No conditioning, matching existing 100k scale policy; first and subsequent inspect calls retain their individual order and must not be silently described as identically warmed.
- Full index last: **one measured call**, **up to 600 seconds**, subject to remaining stage time. Three 600-second full-index allowances alone exceed the entire stage budget; explicitly reduced to one attempt, not silently reduced after observing performance.
- Setup, corpus validation, disk probes and correctness checks outside CLI timers. All product calls use archived stock binary after source/hash validation.
- **Conservative 2 GiB total evidence soft guard**, including preexisting evidence, checked before/after calls and after fixture generation/validation. This is stricter than a 2 GiB budget on new artifacts alone. No mid-call hard disk cap, file deletion, fixture reset or automatic retry.
- **Product lifetime peak RSS >1 GiB** triggers stop after retaining completed sample. Not a hard memory watchdog. No aggregation of provider/background or Python validator RAM into this metric.
- First invalid result, nonzero exit, timeout, watchdog/cancellation, or soft-guard violation stops all later series. Completed/failed raw receipt persists; remaining invocations receive explicit skip reasons. No reuse of timed-out state.

## Provider ruling

One-off fixture preparation permits explicit configuration before any product call. After canonical generation, launcher writes **valid disabled OpenAI** configuration with loopback `http://127.0.0.1:1/v1`, float encoding, zero retries, and no credentials. Corpus bytes/source shape remain unchanged and are independently validated.

This is local FTS/disabled-config measurement, **not OpenAI provider performance**. Requests are intended to be absent because embeddings are disabled; network traffic is **not independently traced**. Earlier 60/1k/10k CLI surveys retained disabled Ollama because their CLI offered no configuration hook; therefore configuration bytes differ, explicitly. This is not a controlled cross-scale provider experiment.

Stress receipts record the argv/request supplied by the one-off launcher to the existing measurement owner; this is application-side invocation evidence, not operating-system exec/network tracing. Earlier canonical survey records retain their separate [audit qualification](evidence/canonical-survey-audit-qualification-20261117-v1.json); original receipts remain unchanged.

## Results and evidence

**Complete: 20/20 attempts succeeded and passed independent result oracles; no timeouts, guard violations, skipped calls, or retries.** One first publication and 19 measured calls. Full stage elapsed **469.165 s**, including **9.780 s** fixture generation and independent corpus validation, disk probes, source/build verification, and result oracles.

| Scenario / phase | n | Wall median s | Wall min–max s | CPU median s | Peak RSS median / maximum MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| search / first publication | 1 | 135.650655 | 135.650655–135.650655 | 132.357087 | 363.67 / 363.67 |
| check | 3 | 10.859230 | 10.753415–10.943573 | 10.464636 | 371.39 / 373.22 |
| status | 3 | 10.679151 | 10.551076–10.892807 | 10.161579 | 369.58 / 373.75 |
| inspect | 3 | 7.878149 | 7.147968–47.384810 | 5.444781 | 60.08 / 60.56 |
| search | 3 | 0.727827 | 0.702877–0.729675 | 0.680672 | 17.88 / 18.75 |
| traverse | 3 | 0.682390 | 0.680101–0.691355 | 0.655148 | 17.84 / 18.09 |
| no-op index | 3 | 0.680731 | 0.676878–0.686114 | 0.655125 | 17.17 / 17.28 |
| full index (`index --full`) | 1 | 178.498726 | 178.498726–178.498726 | 174.625770 | 443.61 / 443.61 |

Inspect's individual measured times in execution order: **47.384810, 7.147968, 7.878149 s**. First and subsequent captures are not interchangeable warmed observations; median alone hides this difference. No causal explanation or cache attribution established without profiling/controls. All no-op index responses explicitly had `published:false` in addition to passing ordinary index/count validation.

- Highest recorded product lifetime RSS: **465,158,144 bytes / 443.61 MiB**, during full index; below 1 GiB guard.
- Allocated total evidence at first guard: **803,778,560 bytes**. Maximum/last guarded total: **1,169,702,912 bytes** (about **1.09 GiB**), below conservative 2 GiB guard.
- Observed guarded increase: about **349 MiB**, not several copied 200 MB fixtures. Forty-two before/after/setup disk observations retained. These are `du` observations, not exact disk-write bytes or a continuous peak.
- Corpus validator confirmed **100,000 knowledge nodes, 100,000 source nodes, 500,000 edges, 73,333 includes**, with generator manifest/hash/config captured.
- CPU is per-child wait4 user+system with OS-accounted reaped descendants. RSS is maximum lifetime resident set, not allocation volume or simultaneous process-tree RAM. Python validator memory and provider/background RAM are not included.
- No tail quantiles: three observations per read and one full-index observation cannot establish p95/p99, confidence bounds, or production representativeness.
- One shared state means probes are sequential, not independent cold-start trials; only first-publication search begins without a projection. Raw `first_use` phase identifies that state; generic single-stable-fixture cache label does not establish an initialized or OS-cold start.
- This bounded extension is not an unchanged `run.py --stage scale` execution; explicit one-off composition avoids duplicated fixtures. It does not include mutation refreshes, contention, provider throughput, other command/flag families, or optimization comparisons.

Primary directory: `evidence/canonical-stress-100000-20261117-v1/`.

- [Manifest and exact owner/source identity](evidence/canonical-stress-100000-20261117-v1/manifest.json)
- [Archived stock build receipt](evidence/canonical-stress-100000-20261117-v1/build.json)
- [Canonical generator corpus manifest](evidence/canonical-stress-100000-20261117-v1/fixture/corpus.json)
- [Independent corpus validation and actual config](evidence/canonical-stress-100000-20261117-v1/corpus_validation.json)
- [Raw per-call JSONL, argv, request bytes and captured outputs](evidence/canonical-stress-100000-20261117-v1/samples.jsonl)
- [Existing-owner timing summary](evidence/canonical-stress-100000-20261117-v1/summary.json)
- [Derived CPU/RSS/ordered timing summary](evidence/canonical-stress-100000-20261117-v1/resource_summary.json)
- [Disk observations](evidence/canonical-stress-100000-20261117-v1/disk_observations.jsonl)
- [Completion and empty skip list](evidence/canonical-stress-100000-20261117-v1/completion.json)

Raw sample SHA-256: `7a1eea909bd64e11550d7a379e7399527605b6643e978aee8b66243526b22e3e`. Original 60/1k/10k evidence was not rewritten; new provider/argv [qualification sidecar](evidence/canonical-survey-audit-qualification-20261117-v1.json) is linked from its summary.

## Handoff

100k measurement complete. No build, sampler, provider, benchmark or launcher remains running under this worker. **Exclusive measurement ownership released for coordinator transfer.** No sampled matrix or heap work starts automatically; no product changes, APIs, commits, or agents spawned.
