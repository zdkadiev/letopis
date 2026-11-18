# Letopis performance profiling results — November 17, 2026

**A full index of 100,000 knowledge nodes took 178.499 seconds and peaked at 443.61 MiB RSS in one stock-release run.** Status and check each took about 11 seconds and 370 MiB RSS. CPU profiles identify source observation/filesystem verification and graph publication as the main investigation targets—not proven optimization savings.

This publication contains final results and source-linked knowledge only. Intermediate notes, plans, scripts, binaries and the approximately 3.4 GB raw archive remain local. Artifact paths below identify retained evidence; they are not downloadable repository files.

## Scope and environment

The study covered `init`, `config`, `status`, `index`, `check`, `inspect`, `search` and `traverse`, plus internal `embed --drain`, help, version and an intentionally invalid argument. The 159-case supplementary catalog exercised index flags/scopes, graph shapes, search/traversal variants and mocked embedding states. This is bounded coverage, not every possible input or execution state.

- Source revision: [`0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`](https://github.com/zdkadiev/letopis/commit/0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8).
- Host: macOS 26.6.2, arm64 Mac16,7, 14 physical/logical cores, 48 GiB RAM.
- Rust 1.95.0; Letopis CLI 0.3.1; Samply 0.13.1.
- Stock release supplied baseline timings; a separate optimized debug1 release supplied symbols. Neither was the shipping LTO distribution profile.
- No affinity, isolated host, OS-cache control or thermal control was established. First application use is not a cold-cache experiment.
- Product source, Cargo manifest and lockfile were unchanged. Synthetic repositories contain their own fixture commits.

## Methodology and workload accounting

Foreground wall time includes process launch, captured I/O and owned-process cleanup. Fixture preparation, reset and independent correctness oracles are outside that timer. Darwin `wait4` supplies user/system CPU and lifetime peak RSS for the child and OS-accounted reaped descendants—not simultaneous process-family memory. Some legacy rows retain only combined CPU. Supplementary rows retain separate user/system, faults and block operations; block operations are not byte counts.

CPU medians are computed from each invocation's **user + system**, not by adding separate component medians. Wall, CPU and RSS are marginal summaries and need not describe the same invocation.

| Campaign | Observations |
|---|---|
| Canonical 60/1k/10k | 352 valid rows: 319 measured, 30 conditioning, three first use |
| Canonical 100k | 20 valid rows: 19 measured, one first publication |
| Supplementary catalog | 159 validations and 740 baseline rows; all 159 cases completed |
| Separate drain sweep | Three validations and nine baseline rows |

Total stock baseline observations: **1,087 = 319 + 19 + 740 + 9**, including one expected invalid-argv exit 1, not 1,087 successful operational commands. Validation, conditioning, first use and profiles are excluded. Catalog repetitions were 13 cases × 1, 129 × 3 and 17 × 20. Its 899 row identities and 1,798 foreground stream hashes reconciled without missing or duplicate slots. Do not pool case/configuration/build identities or infer tail percentiles from these sample counts.

| Workload family | Input and interpretation |
|---|---|
| Canonical, seed 65 | 60/1k/10k/100k knowledge nodes; 2,000-byte fragments; 30 per Markdown file; equal source-node counts; five edges per knowledge node |
| Canonical 100k | 100k knowledge nodes, 100k source nodes, 500k edges, 73,333 includes and 3,334 knowledge files |
| Supplementary, seed 20261117 | Sparse 0/100/1k/10k; 1k chain, star, dense, long-text, source, stale, dangling and cycle variants |
| Sanitized repository export | Actual projection: 45 knowledge nodes, 204 edges; source-node count unavailable |
| Loopback OpenAI-protocol mock | Deterministic 32-dimensional vectors; protocol/vector work, not actual model/provider performance |

The real export retained missing history/paths: every check/status repeat reported 89 findings—44 unavailable snapshots and 45 missing sources. Full index updated 13 files with zero invalid files updated; its 0.0803 s median describes that bounded 45-node projection, not a clean production knowledge base. Source-node addresses are not distinct source-file counts.

Canonical 60/1k/10k used disabled Ollama configuration; 100k used disabled OpenAI configuration. No provider calls were intended, but network traffic was not independently traced. Differing configurations, fixture preparations and host conditions prevent treating cross-scale ratios as a controlled complexity experiment.

## Per-command results

Wall and CPU are seconds; RSS is median lifetime peak in MiB. Values are medians except n=1.

| Command and state | Knowledge nodes / workload | n | Wall median [min–max] | CPU median | RSS median |
|---|---|---:|---|---:|---:|
| `init`, fresh | Sparse 0 | 20 | .066172 [.058114–.170055] | .0152745 | 11.125 |
| `config` | Sparse 100 | 20 | .016592 [.0114–.0673] | .0047775 | 8.578 |
| `status` | Canonical 100k | 3 | 10.679151 [10.551076–10.892807] | 10.161579 | 369.58 |
| `index --full` | Canonical 100k | 1 | 178.498726 [single observation] | 174.625770 | 443.61 |
| `index`, unchanged | Canonical 100k | 3 | .680731 [.676878–.686114] | .655125 | 17.17 |
| `check` | Canonical 100k | 3 | 10.859230 [10.753415–10.943573] | 10.464636 | 371.39 |
| `inspect` | Canonical 100k | 3 | 7.878149 [7.147968–47.384810] | 5.444781 | 60.08 |
| `search`, unchanged | Canonical 100k | 3 | .727827 [.702877–.729675] | .680672 | 17.88 |
| `traverse`, unchanged | Canonical 100k | 3 | .682390 [.680101–.691355] | .655148 | 17.84 |
| `embed --drain` | Separate 100-text mock | 3 | .997387 [.932611–1.013247] | .175816 | 13.656 |

The 100k inspect calls ran in order **47.384810 → 7.147968 → 7.878149 s**; the median hides substantial variation. No causal cache/wait explanation was established. First-publication search separately took 135.650655 s, CPU 132.357087 s and 363.67 MiB RSS once; it is not unchanged-query latency.

### Small and large workloads

Each cell is **n / median wall seconds / median combined CPU seconds / median peak RSS MiB**. Startup rows use sparse inputs; other rows use source-rich canonical inputs. These workload families are not interchangeable.

| Command and scenario | Small cohort | Large cohort |
|---|---|---|
| `init`, fresh, sparse 0 → 10k | 20 / .066172 / .0152745 / 11.125 | 20 / .067181 / .015353 / 11.102 |
| `config`, sparse 100 → 10k | 20 / .016592 / .0047775 / 8.578 | 20 / .016421 / .004917 / 8.562 |
| `status`, canonical 60 → 100k | 20 / .025701 / .013374 / 11.80 | 3 / 10.679151 / 10.161579 / 369.58 |
| `index --full`, canonical 60 → 100k | 3 / .073136 / .038579 / 14.39 | 1 / 178.498726 / 174.625770 / 443.61 |
| `check`, canonical 60 → 100k | 20 / .044191 / .025288 / 12.95 | 3 / 10.859230 / 10.464636 / 371.39 |
| `inspect`, canonical 60 → 100k | 20 / .050729 / .023880 / 13.09 | 3 / 7.878149 / 5.444781 / 60.08 |
| `search`, canonical 60 → 100k | 20 / .040933 / .021771 / 13.22 | 3 / .727827 / .680672 / 17.88 |
| `traverse`, canonical 60 → 100k | 20 / .046525 / .026271 / 13.30 | 3 / .682390 / .655148 / 17.84 |

| Intermediate canonical workload | n | Wall median [min–max], s | CPU median, s | RSS median / maximum, MiB |
|---|---:|---|---:|---|
| 1k full index | 3 | .365304 [.362799–.366241] | .322583 | 22.08 / 22.42 |
| 10k full index | 3 | 4.627904 [4.484957–4.966385] | 4.471358 | 77.23 / 77.42 |
| 10k check | 20 | 1.047962 [1.019874–1.182814] | 1.019178 | 62.47 / 64.91 |
| 10k status | 20 | .993107 [.959551–1.103140] | .974611 | 59.40 / 62.84 |
| 10k unchanged search | 20 | .105040 [.101475–.108385] | .085430 | 14.57 / 14.77 |
| 10k unchanged traverse | 20 | .108481 [.105017–.119115] | .088889 | 14.38 / 14.55 |

At 10k, search after one changed file took .456478 s, after two files .441105 s and after bulk changes 4.827821 s, n=3 each. These refresh scenarios execute search after edits, not separate index commands. At 60 nodes only two knowledge files exist, so two-file and bulk refresh are not distinct edit counts.

Inspect's 10k sample count reduced under the existing adaptive policy after a 4.553 s conditioning call; its second conditioning call was .662 s. The 100k study had no conditioning and used one stable repository sequentially. These are not identically warmed/cold independent trials.

## Sampled bottlenecks and code locations

The complete 100k full-index profile had **183,971 usable stacks**, **177.888334 s observed sampled CPU** and **185.133 s sampler-wrapper elapsed**. Product/result/count validation passed: revision 3, 3,334 updated files and zero invalid files.

| Inclusive sampled scope | 10k full index | 100k full index | Code |
|---|---:|---:|---|
| `EffectiveGraph::apply` | 19.81% | 53.35% | [Graph application](../../src/service/document_refresher.rs#L947-L973) |
| `DocumentPublication::reconcile_sources` | 9.72% | 23.92% | [Source reconciliation](../../src/service/document_refresher.rs#L731-L762) |
| `EffectiveGraph::collect_source_references` | 4.84% | 22.63% | [Reference collection](../../src/service/document_refresher.rs#L983-L1007) |
| SQLite source-file presence | 33.58% | 11.05% | Inclusive attribution, not exclusive SQLite wall time |

**These shares overlap and cannot be added.** Weighted thread CPU is not wall-time attribution. The publication loop calls graph application per changed file; `edges.retain` scans effective edges each time. Code suggests approximately O(FE) edge scanning for F files and E edges. Reference collection uses `knowledge.keys().any(...)` per source edge, suggesting worst-case O(SN) comparisons for S references and N knowledge IDs. Actual iterations and counterfactual savings were not measured. Native `_platform_memcmp` appeared in 27.63% and `sqlite3_step` in 6.96% of the 100k profile, overlapping Rust scopes.

| Command / capture | Observed hot scopes | Qualification |
|---|---|---|
| Complete 10k `check`, three captures | `observe_sources_batch` 81.99%, 81.38%, 84.55%; root verification 55.73%, 54.72%, 57.19% | 1,114 / 1,134 / 1,349 usable stacks; nested scopes |
| Complete 10k `status` | Source observation 85.92%; `file_get_by_path` 79.17% | 1,062 stacks |
| 100k `status` prefix | Provenance-file attribution 84.31%; filesystem-file attribution 76.41%; `__open` 49.13% | Stopped at 10.013 s, exit -15; not completed status latency |
| Complete 10k `inspect` | Git object write 62.40%; object sync 24.64%; `__fcntl` 22.17% | Does not measure blocked time |
| Unchanged 10k `search` / `traverse` | Change detection 72.96% / 66.69% | Only 111 / 109 stacks; coarse evidence |
| 10k one-file refresh | Source-reference collection 56.11% | 426 stacks; search-after-edit scenario |

Shared status/check ownership is [diagnostic batching](../../src/service/utils.rs#L279-L360), [source provenance](../../src/service/provenance.rs), [confined capture](../../src/filesystem/local.rs#L557-L703) and [root identity verification](../../src/filesystem/local.rs#L1220-L1227).

### Startup and capture quality

Canonical sampling produced 12 validated captures: ten complete 10k captures, a complete 100k index and a deliberately stopped 100k status prefix. Ten ordinary debug1/profile pairs were separate from stock data. Check's three sampler-wrapper/ordinary ratios were 2.1521, 2.3802 and 1.8415, median 2.15×; median difference 1.302544 s. These include launch/save/symbolization, not intrinsic product slowdown. No matched 100k ordinary debug1 control exists; stock 178.499 s versus sampled-wrapper 185.133 s is not an overhead estimate.

Four later window captures lacked an attributable product PID/library and remain unattributable, not zero-cost. Direct-root sampling then attempted eight categories in 15 captures: eight at 1 kHz and seven finite 10 kHz diagnostics. Six were usable and nine insufficient under at least 100 product-library stacks and positive resolved Letopis CPU. All requested outputs validated; sampler exits were 0. All eight public commands and internal drain have usable evidence across campaigns; help/version do not.

Config/help/version each used 64 read-only outputs with EOF. Config had 61 sampled PIDs at 10 kHz, not 64 independently traced executions. Help had only 38 product stacks and version eight. Individual child exits were not independently observed; PIDs derive from exact private-library path/UUID matches.

Config's `open_with_cleanup` carried 390,922 of 473,365 observed CPU µs; nested Git open-from-paths carried 315,898 and configuration-cache init 121,405. Fresh init's Git discovery/open scope carried 15,106 of 30,227 µs. These inclusive diagnostic scopes are not exclusive savings or per-call latency. Product-only counters were kept separate from all-library counters.

Long-1k traversal actually returned 16 elements and 16 edges; link lookup carried 23,286 / 53,442 observed CPU µs. Verified sparse-10k index showed change detection at 42,209 / 68,848 µs. Populated mock search showed `wire_embedding_executor_at_root` at 23,660 / 39,704 µs; broad entry scopes do not identify exclusive leaf causes.

## Mock embedding drain results

Each size had one validation and three stock baseline repeats using deterministic localhost 32-dimensional vectors. User/system columns are separate medians, not a combined-CPU median.

| Unique texts | Wall median [min–max], s | User / system median, s | RSS median, MiB |
|---:|---|---|---:|
| 2 | .051378 [.047056–.054496] | .004962 / .008008 | 12.313 |
| 100 | .997387 [.932611–1.013247] | .067246 / .107150 | 13.656 |
| 1,000 | 9.040224 [9.013907–9.132058] | .855949 / .758505 | 14.672 |

All 12 rows verified N unique hashes, queue N→0, N provider items, no recorded failures and native coverage N/zero missing at 32 dimensions. HTTP request counts happened to equal N; item coverage is the correctness condition. The 100-text combined CPU median is .175816 s. The catalog's separate 100-text cohort had .9948 s median wall and different identities; do not pool it.

Within one 100-text drain's `drain_ready` stacks, `sqlite3_step` accounted for 69,487 µs, `sqlite3WalFrames` 67,118 and `unixSync`/`full_fsync` 59,883 against 108,189 µs all-library observed CPU. These overlapping scopes motivate investigating SQLite commits/synchronization, not a measured wait or savings estimate.

Setup, observers and mock-server resources are outside foreground drain accounting. Launcher elapsed was 54.06 s including setup/validation. Per-product cap was 120 s; 1,800 s applied per driver stage, not globally. Disk and post-observation 1 GiB RSS stops were soft. Populated-query native checks separately verified the same DB before/after, 100 covered/zero missing/32 dimensions. No real model performance or adjunct search-scale sweep was measured.

## Memory results

Four valid 10k live snapshots describe allocation prefixes, not full-command allocation peaks or churn. Early full index at about 26 ms observed a 5,767,168-byte node vector and 1,192,464 bytes of SQLite page-cache allocations. Logged heap-plus-VM high-water was 12.5 MB and cumulative allocations 52.9 MB through that prefix only.

| Later snapshot | Approximate launch-to-snapshot time | Prefix heap+VM high-water / live, MB | Instrumented product lifetime RSS, bytes |
|---|---:|---|---:|
| check | .187 s | 26.9 / 6.5 | 71,991,296 |
| inspect | .133 s | 4.4 / 4.0 | 24,002,560 |
| index --full | .738 s | 30.7 / 30.6 | 85,082,112 |

Check showed 7,864,320 bytes in an edge vector and 5,767,168 in a node vector under graph diagnostics/storage reads. Later full index showed 8,047,680 bytes/9,960 allocations attributed to `BasisPreflight::add_file` BTree building and 5,565,776 bytes/4,166 allocations under `EffectiveGraph::from_storage`. Inspect showed 2,146,304 bytes on thread creation, 559,104 SQLite and 379,968 compression initialization. Reserved VM is not necessarily resident heap; allocation origin does not establish full-peak live ownership.

Offline scans reported zero leaks at each snapshot, not absence of leaks, reachable retention or later peaks. At-exit full-history capture failed because logging was lite; working live logging used `MallocStackLoggingNoCompact=1` and no-content tools. Early runs lacked wait4 peak RSS, so their RSS guard is unverified; later captures retained actual exit/RSS and oracles. No valid status/search/traverse heap capture or full allocation chronology exists.

Separate helper and `/usr/bin/time -l` runs calibrated CPU/sleep/64 MiB touch: wait4 CPU .513/.0125/.0179 s and peak RSS 12,550,144/12,517,376/79,593,472 bytes. These were independent invocations, not exact same-process accounting comparisons.

## Limitations and validation

No actual embedding model/provider, independent network trace, Linux/minimum-hardware run, multiagent contention, high-dimensional vectors, 100k hybrid search, exact byte I/O, lock waits or latency-tail guarantee was measured. Foreground response is not whole detached-worker completion. Soft resource checks were not hard live ceilings.

The vector-only canary returned its expected ID and one local HTTP request; frozen text lacked the query token. Its disposable cache was removed, so an independent raw FTS `MATCH` query is unavailable, not a passed check.

Study closeout recorded 94 performance-helper tests in 21.287 s, eight skipped, and 49 research-tool tests in 2.372 s. Integration prerequisites were unset; these are not Rust root/native integration checks. An earlier fake initial-publication timeout caused a helper error; the later pass does not explain or prove that delay fixed. No optimization was implemented; these are not before/after speedup results.

## Reproduction and retained evidence

Stock executable SHA-256:
`77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`.
Symbolized debug1 SHA-256:
`b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`.
Executable/dSYM UUID: `FFEBF8EC-A110-37A6-AA8B-747F67DAEA21`.
Build receipts pin 116 product inputs; the native observer receipt covers 130.

Use a disposable checkout, pinned Rust, Python 3.12, Git and locked dependencies; fetch missing dependencies before offline builds. The tracked [runner](../../tests/performance/run.py) and [fixture/build helpers](../../tests/performance/scenarios.py) provide a symptom reproducer:

```sh
OUT="$(mktemp -d "${TMPDIR:-/tmp}/letopis-results-XXXXXX")"
python3.12 -B tests/performance/run.py \
  --stage survey --nodes 10000 --scenarios check status reindex \
  --initial-timeout 600 --timeout 120 --stage-seconds 1200 \
  --evidence "$OUT/10k"
```

The harness scenario `reindex` executes `index --full` with `{}`. Without `--baseline`, the runner builds debug1, not stock; genuine matching stock receipts can be supplied with `--baseline`. Do not relabel builds. The generic runner copies/reset-refreshes fixtures; the original 100k campaign instead used one stable repository and disabled OpenAI configuration.

At 100k use explicit repetitions, disk capacity and product/stage budgets. Independently validate node/edge counts, findings and each response; preserve failed/censored/skipped attempts. For profiles, launch the actual private symbolized binary directly through Samply with JSON stdin/EOF and matching symbols. Verify PID/library identity and positive CPU samples. Never overwrite old evidence or execute archived hardcoded launchers unchanged.

Retained local root: `research_notes/Letopis comprehensive performance profiling/evidence/`. It is deliberately excluded from publication. Artifact locations:

- `build-archives-20261117-0046a6d-v1/`: binaries, symbols and complete build receipts; `native-observer-20261117-v1/`: observer.
- `canonical-survey-{60,1000,10000}-20261117-v1/`: raw samples, manifests and summaries.
- `canonical-stress-100000-20261117-v1/`: `samples.jsonl`, `resource_summary.json`, corpus validation and invocation order.
- `canonical-cpu-20261117-v1/`: raw profiles, symbol sidecars, attribution and paired controls; `100k-full-complete/`: complete large-index capture.
- `supplementary-prepare-20261117-v1/`: 159-case catalog, measurements, `campaign-control/case-results.json`, stream/native receipts.
- `direct-root-cpu-20261117-v1/`: 15 attempts, identities, attribution and output/native validation.
- `native-memory-20261117-v1/` and `native-memory-extension-20261117-v1/`: graphs, allocation reports and tool failures.
- `drain-scale-20261117-run-v2/`: 12 actual measurements and native receipts.

Obtain the retained archive or a separately reviewed evidence subset for independent recomputation. Intermediate notes and preparation scripts are not required to read the published conclusions and are not included in the PR.

## Optimization follow-ups

1. [#111: status/check source verification](https://github.com/zdkadiev/letopis/issues/111). Isolate live capture, root/path verification and graph materialization while preserving fresh per-binding observations, first-error order, confinement and pinned-view correctness.
2. [#112: full-index graph publication](https://github.com/zdkadiev/letopis/issues/112). Isolate per-file edge scans and source-ID membership with independently varied file/edge/reference counts. Preserve conflict rejection, source timestamps, last-valid projections and atomic rollback.
3. Investigate inspect's variable write/synchronization path separately; its 47-second call remains unexplained.

Use matched build/configuration/input identities, one-variable experiments, at least three alternating 10k/100k pairs and small/incremental controls. Report wall/CPU/RSS, all attempts and memory tradeoffs. No latency SLO or speedup is agreed. Verification, filesystem identity and snapshot durability must not be weakened; no new runtime, schema or cache architecture is implied.
