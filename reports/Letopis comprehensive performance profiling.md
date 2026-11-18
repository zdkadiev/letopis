# Letopis profiling exposes large-graph publication costs

> **Publication scope:** this repository includes the report and supporting Markdown
> notes, not the multi-gigabyte raw capture archive or research tools. Links into
> `evidence/` and `tools/` refer to that local archive, not downloadable GitHub files.
> See [reading order and evidence availability](../research_notes/Letopis%20comprehensive%20performance%20profiling/README.md).

**A complete 100,000-node full index took 178.499 seconds and peaked at 443.61 MiB RSS in one stock-release run; its sampled counterpart concentrated CPU in graph application and source reconciliation.** The 100k corpus contained 100,000 knowledge nodes, 100,000 source nodes and 500,000 edges. Check and status each took about 11 seconds, while unchanged search, traverse and no-op index took about 0.7 seconds in their respective three-run series. These are synthetic, single-host results, not production capacity guarantees. Across the campaign, all eight public commands have usable CPU evidence, but help and version do not; memory allocation evidence consists of four live snapshots, not full-lifetime profiles. Local deterministic OpenAI-protocol mocks exercised embedding paths, not an actual model or provider. The strongest next investigation is to isolate the graph scans observed during publication, while preserving filesystem confinement, identity checks, snapshot durability and source-current correctness. ([100k baseline](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress.md); [CPU profiles](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_cpu_profiles.md))

## Scope and environment

This report covers `init`, `config`, `status`, `index`, `check`, `inspect`, `search` and `traverse`, plus internal `embed --drain`, help, version and an intentionally invalid argument. The supplementary catalog includes index flags and scopes, graph shapes, search and traversal variants, and mocked embedding states. It is a bounded survey of these paths, not proof that every possible combination, corpus or execution state has been profiled. Foreground response is not the entire detached-worker workflow. ([Command inventory](../research_notes/Letopis%20comprehensive%20performance%20profiling/command_inventory.md); [159-case inventory](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/supplementary-prepare-20261117-v1/case-inventory.md))

Measurements ran on macOS 26.6.2, arm64 Mac16,7, 14 physical and logical cores, 48 GiB RAM, Rust 1.95.0, Letopis CLI 0.3.1, and samply 0.13.1. Source revision was `0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`. Stock release and optimized debug-information release were separately archived; the latter supplied symbols for profiles and is not the shipping distribution profile with LTO. No CPU affinity, OS-cache control or isolated host was established. First application use is not a cold-cache experiment. ([Build validation](../research_notes/Letopis%20comprehensive%20performance%20profiling/build_validation.md); [methodology](../research_notes/Letopis%20comprehensive%20performance%20profiling/profiling_methodology.md))

All actual research agents used OpenAI models, Sol or Terra. Provider-region failures were recovered using already completed data; no blind canonical rerun was used. Product source, Cargo manifest and lockfile were not changed for this research. Synthetic repositories have their own fixture commits; this statement does not mean no commits existed anywhere.

## Methodology

Stock foreground wall time includes process launch, captured I/O and owned-process cleanup; fixture preparation and independent result checks are outside the timed interval. Darwin `wait4` records user and system CPU and a lifetime peak RSS for the direct child and OS-accounted reaped descendants, not a simultaneous process-family peak. Some legacy canonical records retain only combined CPU; supplementary rows retain separate user, system, faults and block-operation counters. Block operations are not byte counts. Neither total descendants, bytes of I/O, exact SQL counts nor lock-wait time were independently measured. Calibration used separate helper and `/usr/bin/time` invocations, not identical processes. ([Memory audit and calibration](../research_notes/Letopis%20comprehensive%20performance%20profiling/memory_evidence_audit.md))

Samply profiles use weighted sampled thread CPU and symbol/PID validation, not wall-time attribution. Inclusive caller and callee shares overlap. Ten 10k debug1 ordinary/profile pairs were separate from stock data; wrapper-inclusive paired times include sampler launch and saving. There was no matched 100k debug1 ordinary control, so stock 178.499 seconds and sampler-wrapper 185.133 seconds cannot yield an overhead estimate. The short-command direct-root captures likewise have no same-build ordinary controls. ([CPU audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_cpu_audit.md); [direct-root results](../research_notes/Letopis%20comprehensive%20performance%20profiling/direct_root_cpu_results.md))

The canonical 60/1k/10k survey produced **352 valid rows: 319 measured, 30 conditioning and three first-use**. The separate 100k extension produced **20 valid rows: 19 measured and one first publication**. The supplementary catalog produced **159 validation and 740 baseline rows**, with 13 cases at n=1, 129 at n=3 and 17 at n=20; all 1,798 saved foreground streams were rehashed. A separate drain sweep produced three validations and nine baseline rows. Thus **1,087 stock baseline observations** comprise 319 + 19 + 740 + 9; one catalog observation is an expected invalid-argv exit 1, not a successful operational command. Validation, conditioning, first-use and sampled runs are not included in that total. Do not pool cases, fixture identities or tails. ([Canonical audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_evidence_audit.md); [catalog audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_evidence_audit.md); [drain audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/drain_scale_audit.md))

## Workloads

| Family | Input and state | Purpose and boundary |
|---|---|---|
| Canonical synthetic, seed 65 | 60, 1k, 10k and 100k knowledge elements; 2 kB fragments, 30 per file, five edges per node; equal knowledge/source-node counts | Mixed graph and source work, first publication, changed and unchanged scenarios. At 100k: 500k edges and 73,333 includes. |
| Supplementary synthetic, seed 20261117 | Sparse 0/100/1k/10k; 1k chain, star, dense, long-text, source, stale, dangling and cycle variants | Broader command, flag, topology and failure coverage; not interchangeable with canonical series. |
| Sanitized own-repository export | Actual projection: 45 knowledge nodes, 204 edges; source-node count unavailable | Bounded real-content example, not a clean production database or full repository knowledge base. |
| Loopback OpenAI-protocol mock | Deterministic 32-dimensional vectors, populated and queued states | Protocol and local vector-path exercise, not real provider latency, model CPU, RAM or quality. |

The real export retained unavailable historical bases and missing paths: each check and status baseline reported **89 findings**, including 44 unavailable snapshots and 45 missing sources. Its full-index median of 0.0803 seconds describes that 45-node projection only. The manifest's placeholder knowledge count does not supersede observed database counts. Actual source files and source nodes are distinct quantities. ([Catalog evidence audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_evidence_audit.md); [stress validation](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress_audit.md))

## Command coverage

Each row identifies the strongest observed explanation, not a guarantee for all states. Times and memory below are stock, while attribution is from separate symbolized captures. CPU is the median of each invocation's **user + system seconds**, not the sum of separately computed component medians; RSS is median lifetime peak.

| Command | Representative stock result; n, wall median [min–max], combined CPU median, RSS median | Sampled bottleneck and evidence limit |
|---|---|---|
| `init` | Empty sparse fresh: 20, 0.0662 [0.0581–0.1701] s, CPU 0.0152745 s, 11.12 MiB | Git `open_with_cleanup` was present in about 50% of sampled CPU in one usable 10 kHz capture; inclusive scope, not leaf self-cost. |
| `config` | Sparse 100: 20, 0.0166 [0.0114–0.0673] s, CPU 0.0047775 s, 8.58 MiB | Same Git-open scope about 82.6% of aggregate sampled CPU. Sixty-four outputs, 61 sampled PIDs; no per-call latency or child-exit trace from the aggregate. |
| `status` | Canonical 100k: 3, 10.679 [10.551–10.893] s, CPU 10.161579 s, 369.58 MiB | Provenance/source observation and filesystem verification dominate sampled CPU; 100k capture is a censored prefix, not completed status latency. |
| `index` | Canonical 100k full: 1, 178.499 s, CPU 174.625770 s, 443.61 MiB; no-op: 3, 0.681 [0.677–0.686] s, CPU 0.655125 s, 17.17 MiB | Full publication shows graph application and source reconciliation; 10k verified index capture shows change detection. Different states are not interchangeable. |
| `check` | Canonical 100k: 3, 10.859 [10.753–10.944] s, CPU 10.464636 s, 371.39 MiB | Three 10k captures show source observation and root verification; separate 10k heap snapshot shows graph-vector allocations. |
| `inspect` | Canonical 100k: 3, 7.878 [7.148–47.385] s, CPU 5.444781 s, 60.08 MiB | 10k sample contains Git object writing and synchronization; wall-time remainder is not a measured I/O-wait breakdown. |
| `search` | Canonical 100k unchanged: 3, 0.728 [0.703–0.730] s, CPU 0.680672 s, 17.88 MiB | 10k unchanged capture points to change detection, with only 111 usable stacks. Mock hybrid is a separate path. |
| `traverse` | Canonical 100k unchanged: 3, 0.682 [0.680–0.691] s, CPU 0.655148 s, 17.84 MiB | 10k unchanged change detection has 109 stacks; a separate long-text traversal capture observes link lookup. |
| Internal `embed --drain` | Separate 100-text sweep: 3, 0.997 [0.933–1.013] s, CPU 0.175816 s, 13.656 MiB | One direct capture shows SQLite commit and synchronization scopes inside `drain_ready`; sampled CPU is not measured I/O wait. |
| Help, version, invalid argv | Catalog coverage cases, n=1 each | Help/version remained insufficient even at 10 kHz; invalid argv intentionally exits 1. No zero-cost claim or detailed CPU ranking. |

The 100k inspect calls occurred in order **47.384810, 7.147968, 7.878149 seconds**. Reporting the median without the first observation would hide material variation; no cause, including cache state, is established. Sampled `init` and `config` are direct-root diagnostic captures, while the representative times above are distinct stock scenarios. ([100k raw and summaries](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress.md); [selected catalog CPU/RSS](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_evidence_audit.md); [direct-root attribution](../research_notes/Letopis%20comprehensive%20performance%20profiling/direct_root_cpu_results.md))

The following compact comparison gives **n / median wall seconds / median combined CPU seconds / median peak RSS MiB** for each public command. Canonical columns are source-rich seed-65 fixtures; startup rows instead use sparse seed-20261117 fixtures, explicitly labeled in the row. They are separate workload families, and startup at sparse 10k is not a 100k result. Different provider configurations and execution states prevent treating the columns as a controlled scaling experiment.

| Command and scenario | Small cohort: n / wall / CPU / RSS | Large cohort: n / wall / CPU / RSS |
|---|---|---|
| `init`, fresh, sparse 0 → 10k | 20 / 0.066172 / 0.0152745 / 11.125 | 20 / 0.067181 / 0.015353 / 11.102 |
| `config`, sparse 100 → 10k | 20 / 0.016592 / 0.0047775 / 8.578 | 20 / 0.016421 / 0.004917 / 8.562 |
| `status`, canonical 60 → 100k | 20 / 0.025701 / 0.013374 / 11.80 | 3 / 10.679151 / 10.161579 / 369.58 |
| `index --full`, canonical 60 → 100k | 3 / 0.073136 / 0.038579 / 14.39 | 1 / 178.498726 / 174.625770 / 443.61 |
| `check`, canonical 60 → 100k | 20 / 0.044191 / 0.025288 / 12.95 | 3 / 10.859230 / 10.464636 / 371.39 |
| `inspect`, canonical 60 → 100k | 20 / 0.050729 / 0.023880 / 13.09 | 3 / 7.878149 / 5.444781 / 60.08 |
| `search`, canonical 60 → 100k | 20 / 0.040933 / 0.021771 / 13.22 | 3 / 0.727827 / 0.680672 / 17.88 |
| `traverse`, canonical 60 → 100k | 20 / 0.046525 / 0.026271 / 13.30 | 3 / 0.682390 / 0.655148 / 17.84 |

These medians summarize separate marginal distributions, not necessarily the same invocation. The 100k no-op index has no matched 60-node no-op series in the canonical survey. ([Canonical survey](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_survey.md); [raw sparse rows](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/supplementary-prepare-20261117-v1/measurements.jsonl))

## Timing and CPU results

The following canonical comparisons preserve per-series identities. CPU is median user plus system seconds; RSS is median lifetime peak. n=20 and n=3 characterize only these observations; they do not establish p95, p99 or a formal capacity gate.

| Workload | n | Wall median [min–max], s | CPU median, s | RSS median / max, MiB |
|---|---:|---:|---:|---:|
| 60 full index | 3 | 0.073 [0.072–0.102] | 0.039 | 14.39 / 14.50 |
| 1k full index | 3 | 0.365 [0.363–0.366] | 0.323 | 22.08 / 22.42 |
| 10k full index | 3 | 4.628 [4.485–4.966] | 4.471 | 77.23 / 77.42 |
| 100k full index | 1 | 178.499 [178.499–178.499] | 174.626 | 443.61 / 443.61 |
| 10k check | 20 | 1.048 [1.020–1.183] | 1.019 | 62.47 / 64.91 |
| 10k status | 20 | 0.993 [0.960–1.103] | 0.975 | 59.40 / 62.84 |
| 10k unchanged search | 20 | 0.105 [0.101–0.108] | 0.085 | 14.57 / 14.77 |
| 10k unchanged traverse | 20 | 0.108 [0.105–0.119] | 0.089 | 14.38 / 14.55 |
| 100k first-publication search | 1 | 135.651 [135.651–135.651] | 132.357 | 363.67 / 363.67 |

The 60/1k/10k fixtures used disabled Ollama configuration and the 100k fixture disabled OpenAI configuration; both intended no provider calls, but network traffic was not independently traced. Cross-scale movement is an association across different runs and configurations, not a controlled complexity experiment. `refresh_one`, `refresh_several` and `refresh_bulk` are searches after edits, not index commands. At 10k their wall medians were 0.456, 0.441 and 4.828 seconds respectively, n=3 each. ([Canonical series](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_survey.md); [100k series](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress.md))

Explicit drain scales separately from these disabled-embedding cases:

| Unique texts and vectors | n | Wall median [min–max], s | User / system median, s | RSS median, MiB |
|---|---:|---:|---:|---:|
| 2 | 3 | 0.051 [0.047–0.054] | 0.005 / 0.008 | 12.313 |
| 100 | 3 | 0.997 [0.933–1.013] | 0.067 / 0.107 | 13.656 |
| 1,000 | 3 | 9.040 [9.014–9.132] | 0.856 / 0.759 | 14.672 |

Every drain sweep validation and baseline had N unique hashes, queue N→0, N provider items and native confirmation of N covered vectors at 32 dimensions. The separate catalog 100-text drain has different fixture/configuration identity and must not be pooled with this 100-text cohort. Mock server resources and setup are outside foreground product accounting. ([Drain sweep](../research_notes/Letopis%20comprehensive%20performance%20profiling/drain_scale_results.md); [independent audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/drain_scale_audit.md))

## Sampling profiles and bottlenecks

The complete 100k full-index sampler captured **183,971 usable stacks**, 177.888334 seconds of sampled CPU and a 185.133-second sampler-wrapper interval. Product completion and graph counts passed validation. The highest consequential inclusive scopes in that capture are:

| Inclusive sampled CPU | Scope and source | Interpretation |
|---:|---|---|
| 53.35% | `EffectiveGraph::apply`, [document_refresher.rs](../src/service/document_refresher.rs) | Applies changed files; source contains edge `retain` over the effective edge set per changed file. |
| 23.92% | `DocumentPublication::reconcile_sources`, [document_refresher.rs](../src/service/document_refresher.rs) | Enclosing source-reconciliation work; overlaps other scopes. |
| 22.63% | `EffectiveGraph::collect_source_references`, [document_refresher.rs](../src/service/document_refresher.rs) | Source-reference conflict path includes `knowledge.keys().any(...)` scan. |
| 11.05% | SQLite source locations in the same capture | Inclusive source-file presence, not SQLite's exclusive wall-time contribution. |

These figures **overlap and cannot be added**, and the sampled source line is not an exclusive instruction-cost measurement. The code structure suggests O(FE) edge scanning for F changed files and E effective edges, and O(SN) knowledge-key scanning for S source references and N knowledge IDs. Actual scan counts were not measured; observed scale association does not establish a counterfactual speedup or wall-time promise. A 10k full-index capture showed 19.81%, 9.72% and 4.84% respectively for apply, reconciliation and collection, on a different fixture and state. ([CPU attribution and raw profile](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_cpu_profiles.md))

Other useful scopes are in [provenance.rs](../src/service/provenance.rs), [local.rs](../src/filesystem/local.rs), [repository.rs](../src/git/repository.rs) and [durability.rs](../src/git/durability.rs). Three 10k check samples placed `observe_sources_batch` at 81.38–84.55% and root verification at 54.72–57.19%, inclusive. The 10k status sample placed source observation at 85.92%; the 100k status prefix showed 84.31%. Inspect's 10k profile showed object writing at 62.40% and object synchronization at 24.64%. Unchanged 10k search and traverse showed change detection at 72.96% and 66.69%; those short captures support only coarse interpretation. These nested shares are not additive and do not measure blocked time. ([CPU profiles](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_cpu_profiles.md))

Independent direct-root reanalysis found, within the 100-item drain's `drain_ready` stacks, `sqlite3_step` at 69,487 µs, `sqlite3WalFrames` at 67,118 µs and `unixSync`/`full_fsync` at 59,883 µs, against 108,189 µs all-library observed CPU. These overlapping scopes point to a SQLite commit/synchronization investigation, not a measured wait or savings estimate. In config's aggregate, `gix::open::repository::open_from_paths` appeared for 315,898 µs and configuration-cache initialization for 121,405 µs of 473,365 µs observed; fresh init showed Git discovery for 15,106 µs of 30,227 µs. These figures are inclusive and capture-specific. ([Direct-root independent audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/direct_root_cpu_audit.md))

Coverage consists of **12 validated canonical captures**, including the complete 100k index and a deliberately terminated 100k status prefix, plus **15 direct-root captures in eight categories**, six usable and nine insufficient. Direct attempts used 1 kHz, followed when needed by one finite 10 kHz diagnostic. All eight public commands have usable evidence across the two campaigns, and the internal drain was sampled. Four earlier window captures lacked an attributable product PID/library and are **unattributable**, not zero-cost. Help and version remained insufficient. Native iteration-group output counts and sampler status do not provide individual child exit traces. ([Direct capture inventory](../research_notes/Letopis%20comprehensive%20performance%20profiling/direct_root_cpu_results.md); [additional window limitations](../research_notes/Letopis%20comprehensive%20performance%20profiling/additional_cpu_results.md))

## Memory results

Four valid 10k live snapshots describe allocation prefixes: early full index, later full index, check and inspect. The early full-index snapshot observed a **5,767,168-byte node-vector allocation** and **1,192,464 bytes of SQLite page-cache allocations** in the sampled prefix. The later full-index prefix included 8,047,680 bytes attributed to `BasisPreflight::add_file` BTree construction and 5,565,776 bytes under `EffectiveGraph::from_storage`; stacks identify allocation origins, not definitive live ownership. Check showed 7,864,320 bytes in an edge vector and 5,767,168 in a node vector. Inspect showed a thread-creation VM allocation and smaller SQLite and compression allocations; reserved VM is not necessarily resident heap. ([Native memory results](../research_notes/Letopis%20comprehensive%20performance%20profiling/memory_results.md))

Instrumented later check, inspect and full-index product lifetime RSS were 71,991,296, 24,002,560 and 85,082,112 bytes. These are not stock baselines or full-process allocation peaks. Offline leak scans reported zero leaks at each snapshot, which does not establish absence of leaks, retained reachable memory, or later peaks. The original early live launcher did not retain product wait4 peak RSS, so its post-run RSS guard is unverified. At-exit full-history capture failed because logging was lite; tool errors and unsuccessful attempts remain in the evidence. No full-lifetime allocation churn or peak ownership is available. ([Memory audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/memory_evidence_audit.md))

## Limitations

The campaign separates local FTS, mock hybrid, explicit drain and normal foreground commands. Detached-worker lifetime and resources, blocked-worker behavior and whole-workflow completion remain unmeasured. The vector canary returned the expected unique ID, the frozen input lacked the query token, and receipts recorded local HTTP and native vector coverage. However, its disposable cache was deleted; an independent raw FTS `MATCH` query is unavailable. This supports the bounded vector-path interpretation, not a direct proof of the raw FTS result. ([Catalog audit](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_evidence_audit.md))

No actual embedding model or provider, independent network trace, Linux run, multiagent contention, high-dimensional vector workload, 100k hybrid search, exact I/O bytes, lock timing, allocation chronology or latency-tail guarantee was measured. Shared-host scheduling, filesystem cache and thermal conditions were uncontrolled. Memory and disk stops were observed soft guards, not instantaneous hard limits. Results do not constitute a before/after comparison or establish production capacity.

Fresh final verification passed: **94 performance helper tests in 21.287 seconds, eight skipped**, and **49 research-tool tests in 2.372 seconds** (41 driver and eight direct-adapter tests). Integration environment variables were intentionally unset; these are not Rust root or native product integration tests. An earlier 94-test run had one error following a fake first-publication timeout; the later pass and a targeted fake-unit pass do not explain or prove a fix for the original delay. ([Final performance log](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/final-verification-20261117-v1/existing-performance-tests.log); [research-tool log](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/final-verification-20261117-v1/research-tool-tests.log); [historical diagnosis](../research_notes/Letopis%20comprehensive%20performance%20profiling/existing_suite_diagnosis.md))

## Recommendations

First, investigate graph-publication scans with controlled instrumentation or matched one-variable experiments at multiple file, edge and source-reference counts. Measure actual iterations, CPU, elapsed time, allocations and correctness; compare equivalent build and fixture identities. Preserve behavior for changed and unchanged sources, invalid inputs and concurrent state. The observed profile motivates this investigation but does not justify a claimed speedup.

Second, study check/status provenance and inspect Git object synchronization using targeted traces that distinguish CPU, filesystem work and blocking. Preserve confinement, path and file identity checks, current-source verification, snapshot durability and recovery guarantees; disabling verification or synchronization is not an acceptable shortcut. Treat direct-root startup scopes as broad entry points, not proof that Git open itself is an exclusive leaf bottleneck.

Third, extend measurement only where decisions need it: realistic approved knowledge bases, explicit real-provider/model authorization, high-dimensional vectors, background completion and contention, and independently validated allocation timelines. Retain separate stock and diagnostic build identities, control or record host conditions, and increase repetitions before making tail or capacity claims. No schema change, async runtime or other product design change is implied by these observations.

## Reproduction and evidence

Reproduction is a **future new campaign**, not permission to overwrite saved evidence. Use the exact archived revision, pinned inputs and independent oracles; create new evidence directories and update any absolute paths in one-off launchers after reviewing them. For canonical 60/1k/10k, the recorded command template is:

Prerequisites include the matching revision and complete 116-input product-build verification: [stock build receipt](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json), [symbolized build receipt](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/build-archives-20261117-0046a6d-v1/symbolized_release/build.json), and [build metadata](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/build_metadata.json). Supplementary mock/vector cases also require the matching [native observer build receipt](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/native-observer-20261117-v1/archived-build.json) and [archived observer binary](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/native-observer-20261117-v1/target_corpus-observer); see [supplementary preparation](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_preparation.md) for its 130-input receipt and fixture setup. Archived absolute executable and temporary paths are host-specific, not portable: adapt them and verify the resulting identities. Validate each exact case, configuration and arguments before collecting its matching baseline; do not substitute a different validated case.

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/run.py \
  --stage survey --nodes <60|1000|10000> \
  --baseline "$PWD/research_notes/Letopis comprehensive performance profiling/evidence/build-archives-20261117-0046a6d-v1/stock_release/build.json" \
  --initial-timeout 600 --timeout 120 --stage-seconds 1200 \
  --evidence "$PWD/research_notes/Letopis comprehensive performance profiling/evidence/NEW-canonical-<scale>"
```

The 100k run used a one-off shared-fixture launcher rather than the unchanged survey entry point; inspect and adapt its [exact source and recorded invocation](../research_notes/Letopis%20comprehensive%20performance%20profiling/canonical_stress.md) to a new directory. The [supplementary campaign](../research_notes/Letopis%20comprehensive%20performance%20profiling/supplementary_results.md), [drain sweep](../research_notes/Letopis%20comprehensive%20performance%20profiling/drain_scale_results.md), [direct-root launcher](../research_notes/Letopis%20comprehensive%20performance%20profiling/direct_root_cpu_results.md), and [native-memory procedures](../research_notes/Letopis%20comprehensive%20performance%20profiling/memory_results.md) record exact commands, seeds, fixtures, inputs and resource limits; their archived scripts have fixed paths and must not be rerun against existing output.

Full per-case statistics, n, identities and ranges for all 159 supplementary cases are in [case-results.json](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/supplementary-prepare-20261117-v1/campaign-control/case-results.json); [raw catalog rows](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/supplementary-prepare-20261117-v1/measurements.jsonl), [canonical 100k rows](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/canonical-stress-100000-20261117-v1/samples.jsonl), [full 100k CPU profile](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/canonical-cpu-20261117-v1/100k-full-complete/profile.json.gz) and [drain rows](../research_notes/Letopis%20comprehensive%20performance%20profiling/evidence/drain-scale-20261117-run-v2/measurements.jsonl) retain the underlying evidence. Source-specific audits linked throughout qualify each result and supersede earlier preparatory notes.

## Conclusion

The central distinction is between **publication work that grows with graph and source relationships** and **unchanged foreground operations that still perform change detection or provenance checks**. The evidence prioritizes where to measure next; it does not license weakening correctness or translating inclusive sampled CPU directly into promised latency savings.
