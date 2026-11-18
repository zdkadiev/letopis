# Canonical stock-release survey — 2026-11-17

## Scope and fixed rulings

Baseline-only sequential existing-infrastructure surveys requested at 60, 1,000, and 10,000 synthetic knowledge elements. No 100k run, sampled profile matrix, heap instrumentation, real provider, or optimization comparison.

- Revision: `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`.
- Stock executable SHA-256: `77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`.
- [Archived runner receipt](evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json); [archive validation](evidence/build-archives-20261117-0046a6d-v1/archive_validation.json).
- Existing `tests/performance/run.py --help` supports `--stage survey`, `--nodes`, `--baseline`, `--initial-timeout`, `--timeout`, `--stage-seconds`, and `--evidence`. No fixture-configuration override before first product call exists in this CLI. **Ruling:** retain generated valid `embedding_model.enabled=false` Ollama configuration; this is **disabled-config/local FTS measurement**, not an Ollama or OpenAI provider measurement. No remote/provider calls are intended or authorized.
- No `--repetitions` override. Canonical cheap scenarios start with two conditioning calls and 20 measured calls; if an observed cheap invocation exceeds two seconds, existing runner reduces its target to three measured calls. Mutation/full-index cases start with three measured calls and independent fixture reset.
- First publication cap 600 s; each ordinary call cap 120 s; stage cap 1,200 s including setup. New evidence directory per scale; no automatic retries.
- Supervisor records exact argv and console log, stops stage on any printed `valid:false`, and will not start next scale unless successful completion and every retained result passes its independent oracle.
- Timings include fresh CLI spawn, I/O capture and owned-process cleanup; fixture/setup/oracle time excluded from per-call latency. OS page cache and shared-host load uncontrolled.
- CPU and peak RSS use existing per-child `wait4` accounting of OS-accounted reaped descendants. RSS is lifetime maximum, not allocated bytes or simultaneous process-tree peak. macOS RSS values are bytes. No background/provider RAM attribution or allocation chronology is inferred.

## Results

**All three scales completed successfully, sequentially. 352/352 attempts passed independent correctness checks; 319 measured, 30 conditioning, three first-use. No timeouts, censored results, failures, or retries.**

| Elements | First-use / conditioning / measured | Stage wrapper elapsed | Adaptive change |
| --- | --- | --- | --- |
| 60 | 1 / 10 / 112 | 14.287 s | None |
| 1,000 | 1 / 10 / 112 | 22.820 s | None |
| 10,000 | 1 / 10 / 95 | 117.764 s | Inspect reduced to three measured calls |

At 10k, inspect's first conditioning invocation took **4.553 s**, exceeding the two-second adaptive threshold. Its second conditioning invocation took 0.662 s; three measured invocations followed. This is existing runner policy, not manual repetition reduction or a timeout. No other cheap scenario triggered reduction.

Stage wrapper durations include setup/oracles/fixture copying, not just CLI occupied time. Per-call tables below exclude conditioning. `first_use` is one search that initializes the projection, **not** proof of a cold filesystem cache. No p95 or p99 reported: sample counts are insufficient.

### 60 elements

| Scenario / phase | n | Wall median s | Wall min–max s | CPU median s | Peak RSS median / maximum MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| search / first use | 1 | 0.601554 | 0.601554–0.601554 | 0.045657 | 14.02 / 14.02 |
| search | 20 | 0.040933 | 0.034341–0.047880 | 0.021771 | 13.22 / 13.39 |
| inspect | 20 | 0.050729 | 0.047509–0.058857 | 0.023880 | 13.09 / 13.17 |
| traverse | 20 | 0.046525 | 0.043047–0.061170 | 0.026271 | 13.30 / 13.61 |
| check | 20 | 0.044191 | 0.039172–0.061249 | 0.025288 | 12.95 / 13.09 |
| status | 20 | 0.025701 | 0.024198–0.027488 | 0.013374 | 11.80 / 11.89 |
| refresh_one | 3 | 0.066519 | 0.064844–0.067503 | 0.031074 | 14.08 / 14.19 |
| refresh_several | 3 | 0.075636 | 0.073780–0.078905 | 0.039580 | 14.33 / 14.50 |
| refresh_bulk | 3 | 0.075964 | 0.071775–0.076923 | 0.039453 | 14.16 / 14.17 |
| reindex | 3 | 0.073136 | 0.072432–0.101762 | 0.038579 | 14.39 / 14.50 |

### 1,000 elements

| Scenario / phase | n | Wall median s | Wall min–max s | CPU median s | Peak RSS median / maximum MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| search / first use | 1 | 0.379902 | 0.379902–0.379902 | 0.299782 | 20.31 / 20.31 |
| search | 20 | 0.060200 | 0.056586–0.074723 | 0.040050 | 13.49 / 13.67 |
| inspect | 20 | 0.094162 | 0.087387–0.118481 | 0.067027 | 13.80 / 14.09 |
| traverse | 20 | 0.051889 | 0.047333–0.072409 | 0.032120 | 13.43 / 13.64 |
| check | 20 | 0.136799 | 0.132091–0.148078 | 0.116742 | 17.76 / 19.27 |
| status | 20 | 0.122336 | 0.110865–0.151302 | 0.111313 | 16.75 / 18.09 |
| refresh_one | 3 | 0.076067 | 0.075978–0.076966 | 0.045960 | 17.80 / 18.08 |
| refresh_several | 3 | 0.093030 | 0.092948–0.097233 | 0.054925 | 18.05 / 18.09 |
| refresh_bulk | 3 | 0.385428 | 0.381632–0.385860 | 0.341603 | 22.69 / 22.72 |
| reindex | 3 | 0.365304 | 0.362799–0.366241 | 0.322583 | 22.08 / 22.42 |

### 10,000 elements

| Scenario / phase | n | Wall median s | Wall min–max s | CPU median s | Peak RSS median / maximum MiB |
| --- | ---: | ---: | ---: | ---: | ---: |
| search / first use | 1 | 3.405045 | 3.405045–3.405045 | 3.223853 | 57.53 / 57.53 |
| search | 20 | 0.105040 | 0.101475–0.108385 | 0.085430 | 14.57 / 14.77 |
| inspect | 3 | 1.372925 | 0.733593–1.413920 | 0.614514 | 19.09 / 19.11 |
| traverse | 20 | 0.108481 | 0.105017–0.119115 | 0.088889 | 14.38 / 14.55 |
| check | 20 | 1.047962 | 1.019874–1.182814 | 1.019178 | 62.47 / 64.91 |
| status | 20 | 0.993107 | 0.959551–1.103140 | 0.974611 | 59.40 / 62.84 |
| refresh_one | 3 | 0.456478 | 0.434827–0.468071 | 0.412188 | 51.73 / 52.05 |
| refresh_several | 3 | 0.441105 | 0.437757–0.444107 | 0.403375 | 49.61 / 50.36 |
| refresh_bulk | 3 | 4.827821 | 4.731903–5.825978 | 4.675226 | 77.94 / 78.53 |
| reindex | 3 | 4.627904 | 4.484957–4.966385 | 4.471358 | 77.23 / 77.42 |

CPU is user+system, not sampler CPU; no sampler ran during these surveys. RSS maximum column is maximum of **individual lifetime peaks** within that series, not simultaneous family peak or maximum across unreported conditioning. Raw conditioning resources remain in JSONL.

## Exact execution and evidence

Executed from repository root with existing Python 3.12; no repetition/scenario override:

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/run.py \
  --stage survey --nodes <60|1000|10000> \
  --baseline "$PWD/research_notes/Letopis comprehensive performance profiling/evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json" \
  --initial-timeout 600 --timeout 120 --stage-seconds 1200 \
  --evidence "$PWD/research_notes/Letopis comprehensive performance profiling/evidence/canonical-survey-<scale>-20261117-v1"
```

Each scale has `manifest.json` (host/settings and archived harness), `build.json`, `corpus.json`, `samples.jsonl`, `coverage.json`, `summary.json`, `completion.json`, and disposable independent fixture copies.

| Scale | Raw samples | Canonical summary | Completion | Exact launch / ruling |
| --- | --- | --- | --- | --- |
| 60 | [JSONL](evidence/canonical-survey-60-20261117-v1/samples.jsonl) | [JSON](evidence/canonical-survey-60-20261117-v1/summary.json) | [JSON](evidence/canonical-survey-60-20261117-v1/completion.json) | [JSON](evidence/canonical-survey-60-20261117-v1-launch.json) |
| 1k | [JSONL](evidence/canonical-survey-1000-20261117-v1/samples.jsonl) | [JSON](evidence/canonical-survey-1000-20261117-v1/summary.json) | [JSON](evidence/canonical-survey-1000-20261117-v1/completion.json) | [JSON](evidence/canonical-survey-1000-20261117-v1-launch.json) |
| 10k | [JSONL](evidence/canonical-survey-10000-20261117-v1/samples.jsonl) | [JSON](evidence/canonical-survey-10000-20261117-v1/summary.json) | [JSON](evidence/canonical-survey-10000-20261117-v1/completion.json) | [JSON](evidence/canonical-survey-10000-20261117-v1-launch.json) |

Derived CPU/RSS summaries with full precision, phase counts, raw-sample hashes, and fixture configurations: [canonical-survey-resource-summary-20261117-v1.json](evidence/canonical-survey-resource-summary-20261117-v1.json). Console logs are adjacent `canonical-survey-<scale>-20261117-v1.log`.

## Actual workload coverage and limits

- All **nine existing canonical scenarios** completed at each scale, in runner order: search, inspect, traverse, check, status, refresh_one, refresh_several, refresh_bulk, reindex.
- `refresh_*` invokes **search after edits**, not the `index` command: one file, two files, or all fixture knowledge files changed. At 60 elements there are only two knowledge files, so `refresh_several` and `refresh_bulk` change the same file count and are not distinct edit-scale evidence there.
- Historical scenario `reindex` executes **`index --full`**. This survey does not measure an obsolete reindex subcommand.
- Fixtures contain 60/1k/10k knowledge elements, respectively 300/5,000/50,000 edges and equal source-node counts to knowledge counts. Elements are 2,000 bytes, 30 per knowledge file, deterministic seed 65. These are synthetic graph/source shapes, not production evidence.
- No measured init/config/help/version/no-change-index/scoped-index/index-verify/drain scenario in this canonical runner; do not claim every public command/flag was benchmarked. Build validation separately ran help/version, and profiler smoke initialized its own unrelated fixture.
- No contention, detached-workflow, real/fake provider throughput, allocation chronology, heap leak evidence, 100k scale, Linux, or before/after comparison established here. Zero embedding requests are intended by explicit disabled configuration; network traffic itself was not traced.
- **Audit qualification:** original launch-receipt wording such as “zero provider calls” is intended-by-disabled-config, **not network-verified**. Canonical JSONL does not independently trace per-command argv; command mappings above come from exact runner/scenarios source archived with each run. Top-level runner argv is captured separately in launch receipts. [New qualification sidecar](evidence/canonical-survey-audit-qualification-20261117-v1.json) records these limits without rewriting immutable original evidence or changing any measured statistics.
- No causal bottleneck claim from timings alone; no cold-cache, confidence/tail guarantee, or extrapolation from missing scales.

## Handoff

Canonical 60/1k/10k measurement work complete. No build, sampler, provider, or benchmark remains running under this worker. Measurement ownership released for coordinator transfer; no 100k, heap, or sampled matrix starts automatically. Product/source untouched, no commits.
