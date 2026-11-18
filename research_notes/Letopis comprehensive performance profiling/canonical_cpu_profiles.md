# Canonical CPU profiles — 2026-11-17

## Scope and measurement identity

Completed one exclusively owned sampler stage in **374.18 seconds**, within **900-second budget**, with **12/12 validated captures** and ten complete ordinary/profile pairs. Conservative total-evidence **2 GiB soft guard** stayed clear. No concurrent heap instrumentation, compilation, other benchmarks, provider workload, product edits, or agents. Network traffic was not independently traced.

All sampled commands and new ordinary controls use archived **symbolized debug1 release**:

```text
research_notes/Letopis comprehensive performance profiling/evidence/build-archives-20261117-0046a6d-v1/symbolized_release/product/letopis
```

- Source revision `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`.
- Executable SHA-256 `b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`.
- Matching adjacent `letopis.dSYM`, executable/dSYM UUID `FFEBF8EC-A110-37A6-AA8B-747F67DAEA21`.
- Samply 0.13.1, 1,000 Hz, existing owner’s `--save-only --unstable-presymbolicate`; no browser/server.
- Existing source/build/artifact verifier runs before collection. Each profile must have positive usable PID-filtered CPU deltas, observed Letopis library frames, exact archived executable path and matching code ID. Nonempty wrapper/Python stacks alone are not valid product evidence.
- **New debug1 ordinary controls are not pooled with stock-release baseline.** Earlier stock full-index 178.50 s and 10k median 4.63 s motivated this investigation but are not new same-build/same-state overhead controls.

## Existing owners and planned capture coverage

One-off [launcher source](evidence/canonical-cpu-launch-20261117-v1.py) composes existing `record_profile`, `record_profile_window`, `run_profile_pair`, `summarize_stacks`, fixture validators/resetters, correctness oracles and immutable JSON writers. Shared `tests/performance` code untouched.

Exact invocation from repository root:

```sh
/opt/homebrew/bin/python3.12 -B \
  'research_notes/Letopis comprehensive performance profiling/evidence/canonical-cpu-launch-20261117-v1.py'
```

- 100k: independently validate existing canonical stress fixture; ten-second **read-only status window** first; then **one complete `index --full` sampled invocation**, capped at 600 seconds. No new 135-second first publication. Status window may be deliberately censored; read-only projection counts are rechecked before full-index capture. No later 100k product calls.
- Complete 100k full capture replaces originally planned ten-second full-index window, per explicit authorization. No additional 100k debug1 ordinary control: do not call comparison against prior stock binary “profiler overhead.”
- 10k: one ordinary/profile pair for full index, status, inspect, search, traverse, one-file refresh and bulk refresh; check has **three alternating pairs**. Total ten 10k profiles plus two 100k captures = twelve target captures.
- Existing pair owner uses independent equivalent copied/reset fixtures, with preparation outside timers; case file/config hashes are compared afterward. Pair ordering is ordinary/profile or profile/ordinary according to pair index. No automatic Git-counter or supplementary macOS sample runs were requested or added.
- `refresh_one` and `refresh_bulk` execute search after deterministic edits; historical `reindex` scenario executes `index --full`.
- Reused 10k fixture retains disabled Ollama configuration; reused 100k fixture has disabled OpenAI configuration. Requests are intended to be absent through disabled configuration, **not independently network-verified**. Provider performance is not measured.

## Attribution rules and limits

- PID-filtered `threadCPUDelta` weights, not raw stack-count percentages or wall duration. Denominator is observed CPU delta of usable stacks for actual product PID, summed over its sampled threads.
- Function and source-file/source-location attribution are **inclusive**: names/files/locations deduplicated within a stack, then assigned that stack’s CPU delta. Caller/callee and inline costs overlap; do not sum into an additive pie or equate to elapsed time.
- Exact source paths/lines come from captured presymbolicated debug frames; symbol names alone are not proof of source-line coverage. Native-function count uses resolved names without `::` (excluding unknown/hex), a disclosed naming heuristic.
- Sampled wrapper elapsed time includes profiler launch, collection and save/symbolization. Pair differences/ratios quantify **wrapper-inclusive end-to-end overhead on those 10k cases**, not intrinsic slowing of product CPU and not 100k overhead.
- Status prefix is deliberately partial and never full-runtime latency. Existing window helper writes generic `first_publication_window`; new state-qualification evidence explicitly says actual state is an initialized status prefix.
- Tiny commands remain queued for later. No lack of samples interpreted as zero cost, and no attribution claim from a failed/empty profile.

## Results

### Main finding: complete 100k full-index capture

The **complete** sampled `index --full` invocation passed the product/result/count oracle: revision 3, **3,334 files updated**, zero invalid files, 100k knowledge/source nodes and 500k edges. Sampler-wrapper elapsed **185.133 s**, product PID **94235**, **183,971 usable stacks**, **177.888334 s observed CPU**, 939 resolved Rust and 601 resolved native-name functions. This is not a ten-second prefix or censored full-index result.

| Inclusive sampled CPU presence | 10k complete full index | 100k complete full index |
| --- | ---: | ---: |
| `EffectiveGraph::apply` | 19.81% | **53.35%** |
| `EffectiveGraph::collect_source_references` | 4.84% | **22.63%** |
| `DocumentPublication::reconcile_sources` | 9.72% | **23.92%** |
| SQLite source file, all resolved locations | 33.58% | 11.05% |

All percentages overlap; **do not add** rows. Full 100k application source attribution points to:

```text
/Users/zdkadiev/Repos/letopis/src/service/document_refresher.rs
```

- [`publish_changes`, lines 384–395](../../src/service/document_refresher.rs): iterates updated files and applies each prepared change before reconciling sources.
- [`EffectiveGraph::apply`, entry line 947](../../src/service/document_refresher.rs): `self.edges.retain(...)` scans the effective edge set per applied file. Sampled optimized source location 948 carries 53.35% inclusive CPU; inline source attribution is not proof that that single textual line alone executes all the work.
- [`collect_source_references`, entry line 983, scan at 989](../../src/service/document_refresher.rs): for a source-target edge, `self.knowledge.keys().any(...)` scans knowledge IDs for address conflict detection. Optimized captured location 1004 carries the enclosing sampled scope; do not mistake mapped line for exact exclusive instruction cost.
- Captured native `_platform_memcmp` presence is **27.63%**, overlapping enclosing Rust paths. Native `sqlite3_step` presence is **6.96%**.

**Inference, not a controlled optimization result:** repeated whole-edge scans per updated file and knowledge-key scans per source reference are concrete scaling candidates, consistent with the stronger concentration at 100k. Exact scan/call counts were not instrumented; no counterfactual experiment established how much wall time removing either operation would save. The single 10k and 100k captures use different-sized/staged fixtures and disabled provider configurations; this is not a complete causal isolation of all differences.

Evidence: [100k raw profile](evidence/canonical-cpu-20261117-v1/100k-full-complete/profile.json.gz), [symbol sidecar](evidence/canonical-cpu-20261117-v1/100k-full-complete/profile.json.syms.json), [PID/source/function attribution](evidence/canonical-cpu-20261117-v1/100k-full-complete/product-attribution.json), [result validation](evidence/canonical-cpu-20261117-v1/100k-full-complete/product-result-validation.json). Comparison capture: [10k raw](evidence/canonical-cpu-20261117-v1/capture-reindex-0/profile.json.gz), [10k attribution](evidence/canonical-cpu-20261117-v1/capture-reindex-0/product-attribution.json).

### All captures and symbol/PID validation

Observed CPU below is **PID-filtered usable `threadCPUDelta`**, not full wall time. Rust/native counts count distinct resolved names present in sampled stacks, using classification rule above; “native” also contains resolved entry wrappers such as `main`/`start`, not only useful leaf functions. Every row matched executable path, hash and code ID stated above.

| Capture | Product PID | Usable stacks | Observed CPU s | Rust / native names | Raw / attribution |
| --- | ---: | ---: | ---: | ---: | --- |
| 100k status prefix | 94135 | 9,572 | 8.763359 | 511 / 143 | [raw](evidence/canonical-cpu-20261117-v1/100k-window-status/sampler/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/100k-window-status/product-attribution.json) |
| 100k full index, complete | 94235 | 183,971 | 177.888334 | 939 / 601 | [raw](evidence/canonical-cpu-20261117-v1/100k-full-complete/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/100k-full-complete/product-attribution.json) |
| 10k full index | 98853 | 5,049 | 4.817918 | 711 / 505 | [raw](evidence/canonical-cpu-20261117-v1/capture-reindex-0/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-reindex-0/product-attribution.json) |
| 10k check, pair 0 | 99689 | 1,114 | 1.073285 | 460 / 118 | [raw](evidence/canonical-cpu-20261117-v1/capture-check-0/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-check-0/product-attribution.json) |
| 10k check, pair 1 | 99731 | 1,134 | 1.089548 | 473 / 137 | [raw](evidence/canonical-cpu-20261117-v1/capture-check-1/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-check-1/product-attribution.json) |
| 10k check, pair 2 | 634 | 1,349 | 1.259962 | 530 / 146 | [raw](evidence/canonical-cpu-20261117-v1/capture-check-2/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-check-2/product-attribution.json) |
| 10k status | 1204 | 1,062 | 1.028953 | 373 / 127 | [raw](evidence/canonical-cpu-20261117-v1/capture-status-1/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-status-1/product-attribution.json) |
| 10k inspect | 2103 | 4,007 | 1.212492 | 590 / 96 | [raw](evidence/canonical-cpu-20261117-v1/capture-inspect-0/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-inspect-0/product-attribution.json) |
| 10k search | 2837 | 111 | 0.104096 | 237 / 75 | [raw](evidence/canonical-cpu-20261117-v1/capture-search-1/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-search-1/product-attribution.json) |
| 10k traverse | 3743 | 109 | 0.107274 | 243 / 112 | [raw](evidence/canonical-cpu-20261117-v1/capture-traverse-0/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-traverse-0/product-attribution.json) |
| 10k one-file refresh | 4006 | 426 | 0.424144 | 429 / 176 | [raw](evidence/canonical-cpu-20261117-v1/capture-refresh_one-1/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-refresh_one-1/product-attribution.json) |
| 10k bulk refresh | 4753 | 5,202 | 4.976041 | 772 / 492 | [raw](evidence/canonical-cpu-20261117-v1/capture-refresh_bulk-0/profile.json.gz) / [attribution](evidence/canonical-cpu-20261117-v1/capture-refresh_bulk-0/product-attribution.json) |

Each raw profile has adjacent `profile.json.syms.json`, helper `receipt.json`/`stacks.json`, and new `product-attribution.json` with exact hashes, PID, binary/debug identity, weighted functions, source files and line locations. Window puts sampler files under `sampler/` and separately retains `child.json`.

The 100k status child deliberately timed out at **10.013 s**, exit -15, while the window wrapper finished and saved; its source/CPU attribution is **prefix-only**. It is not a successful status latency sample. No failed/empty CPU captures or automatic retries occurred.

### Other observed hot scopes

These are inclusive CPU observations, not exclusive wall shares:

- **10k check:** `observe_sources_batch` **81.99%, 81.38%, 84.55%** across three captures; filesystem `directory_verify_root` **55.73%, 54.72%, 57.19%**. Captured source files `src/service/provenance.rs` and `src/filesystem/local.rs` carry most of this work, nested within status/check diagnostics.
- **10k status:** `observe_sources_batch` **85.92%**, filesystem `file_get_by_path` **79.17%**. The 100k status prefix likewise shows provenance **84.31%** and filesystem source-file presence **76.41%**, with native `__open` **49.13%**. Overlap forbids adding these.
- **10k inspect:** `git::repository::object_write` **62.40%**, `git::durability::object_sync` **24.64%**, native `__fcntl` **22.17%**. Source files `src/git/repository.rs`, `src/git/durability.rs`, `src/git/cleanup.rs`. Observed product CPU only 1.212 s versus wrapper elapsed 6.513 s; stacks suggest file/object synchronization paths, but do not measure exact I/O wait or justify assigning remaining elapsed time to a single wait.
- **10k unchanged search/traverse:** `detect_changes` **72.96% / 66.69%** and confined filesystem discovery/root checks dominate sampled stacks. Only **111 / 109 observations**, one capture each: coarse evidence, not robust rankings or confidence/tail estimates.
- **10k one-file refresh:** `collect_source_references` **56.11%**; **426** observations. **Bulk refresh:** provenance **20.87%**, graph apply **19.22%**, projection publication **19.21%**, overlapping with SQLite/native work. Neither is a separate index command.

Exact principal symbol source paths retained in sidecars:

```text
/Users/zdkadiev/Repos/letopis/src/service/document_refresher.rs
/Users/zdkadiev/Repos/letopis/src/service/provenance.rs
/Users/zdkadiev/Repos/letopis/src/service/utils.rs
/Users/zdkadiev/Repos/letopis/src/filesystem/local.rs
/Users/zdkadiev/Repos/letopis/src/git/repository.rs
/Users/zdkadiev/Repos/letopis/src/git/durability.rs
/Users/zdkadiev/Repos/letopis/src/git/cleanup.rs
/Users/zdkadiev/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/libsqlite3-sys-0.38.2/sqlite3/sqlite3.c
/Users/zdkadiev/Repos/letopis/vendor/vec1/vec1.c
```

SQLite C source locations were observed at **11.05% inclusive CPU in complete 100k full index**, versus **33.58% in 10k full index**. Vec1 C source appeared at **0.021%** in 10k full index; this is not vector-query/provider performance. System-native names such as `_platform_memcmp`/`__open` resolved, but no corresponding system source-line claim is made.

### Matched debug1 controls and observed wrapper overhead

All ten ordinary/profile pairs passed correctness and actual case-file/config hash equality. Three check pairs alternate ordinary→profile, profile→ordinary, ordinary→profile. Per-case times below are independent of stock survey series.

| 10k scenario / pair | Order | Ordinary debug1 s | Sampler wrapper s | Difference s | Ratio |
| --- | --- | ---: | ---: | ---: | ---: |
| check / 0 | ordinary → profile | 1.130625 | 2.433169 | +1.302544 | 2.15× |
| check / 1 | profile → ordinary | 1.109249 | 2.640225 | +1.530976 | 2.38× |
| check / 2 | ordinary → profile | 1.306290 | 2.405530 | +1.099240 | 1.84× |
| full index / 0 | ordinary → profile | 4.670793 | 6.691465 | +2.020673 | 1.43× |
| status / 1 | profile → ordinary | 1.076165 | 2.444522 | +1.368357 | 2.27× |
| inspect / 0 | ordinary → profile | 5.846568 | 6.513277 | +0.666709 | 1.11× |
| search / 1 | profile → ordinary | 0.108672 | 1.572849 | +1.464177 | 14.47× |
| traverse / 0 | ordinary → profile | 0.124885 | 1.417130 | +1.292245 | 11.35× |
| one-file refresh / 1 | profile → ordinary | 0.428369 | 1.877086 | +1.448717 | 4.38× |
| bulk refresh / 0 | ordinary → profile | 5.482134 | 6.519559 | +1.037425 | 1.19× |

Check median paired difference **+1.302544 s**, median paired ratio **2.15×** (three pairs). Other cases have **one pair**: no distribution or general overhead guarantee. High short-call ratios reflect wrapper-inclusive startup/save/symbolization and host variation, not proof that product instructions became 11–14 times slower.

No same-build 100k ordinary control was run, as instructed. **No 100k overhead estimate** is inferred from 185.13 s sampled-wrapper versus 178.50 s old stock result, nor transferred from 10k controls.

## Evidence and handoff

Primary directory: `evidence/canonical-cpu-20261117-v1/`.

- [Manifest, exact source/harness hashes and UUID verification](evidence/canonical-cpu-20261117-v1/manifest.json)
- [Archived symbolized build receipt](evidence/canonical-cpu-20261117-v1/build.json)
- [Independent reused 100k corpus validation](evidence/canonical-cpu-20261117-v1/reused-100k-validation.json)
- [Readonly status post-window projection-count check](evidence/canonical-cpu-20261117-v1/post-status-readonly-state.json)
- [Raw 10k paired records](evidence/canonical-cpu-20261117-v1/samples.jsonl)
- [Combined derived capture/overhead analysis](evidence/canonical-cpu-20261117-v1/analysis-summary.json)
- [Completion: twelve validated captures, ten complete pairs](evidence/canonical-cpu-20261117-v1/completion.json)
- [Final offline raw-profile/PID/CPU/hash/debug-ID/pair-input verification](evidence/canonical-cpu-20261117-v1/final-validation.json)
- Each `pair-<scenario>-<index>.json` retains order, both raw records and matched actual input/config hashes.

Maximum guarded allocated total evidence: **1,910,501,376 bytes (~1.78 GiB)**; below 2 GiB. No profiling permission failure, install, security change, counter/wall-sampler extra capture, source mutation, automatic retry or heap instrumentation. Full 100k capture completed, so fixture was not abandoned in a censored write state.

**Exclusive sampler/measurement ownership released.** No profiler, product command, build or launcher remains running under this worker. Tiny-command profiles and supplementary work are not started automatically.
