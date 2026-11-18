# Performance measurements and bottlenecks

## Check and status before historical range reuse {#check-status}
<!-- letopis: {"created_at":1794000000,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

On 2026-11-06, check and status on the synthetic 100k corpus were sampled
after tree-validation reuse. Historical snapshot hashing accounted for 66.11%
and 66.68% of observed CPU, current source hashing for 28.02% and 29.32%, and
Git object verification for 63.60% and 64.01%, respectively. Inclusive CPU
scopes overlap. The profiles had 36,423 and 37,262 usable stacks. Before reuse,
structural validation accounted for 23.76% and 25.10% of observed CPU. The
later profiles captured no samples in that scope.

Earlier separate temporary Rust instrumentation, not Samply, measured historical hashing about 30.5s CPU, object decoding 13.5–13.6s, tree validation 9.7–9.8s, object hashing 5.8s, live captures 8.1–8.2s and graph diagnostics 0.6–0.7s. Nested scopes overlap. Its 100k source bindings caused about 400k decodes and 4.8 GB decoded content. Measurement commits, fixture and shared-host conditions are recorded in the reports.

In the measured revision, `service/utils.rs` owned diagnostic batches,
`provenance.rs` observed bindings, and `git/reader.rs` verified historical
objects. The operation-local cache reused hashes for identical tree and source
addresses. Distinct ranges of one file were read separately and live captures
stayed fresh. Issue 108 later implemented and measured historical reads shared
across ranges. Parallel batches remain a separate proposal with corruption
and failure-order constraints.

[Samply report](../../docs/zkit/letopis/checks/issue-65/native-followup/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/native-followup/report.md")
[Samply data](../../docs/zkit/letopis/checks/issue-65/native-followup/native-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/native-followup/native-summary.json")
[Instrumented scopes](../../docs/zkit/letopis/checks/issue-65/check-status-profiling/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/check-status-profiling/report.md")
[Batch orchestration](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")
[Source provenance](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")
[Git reader](../../src/git/reader.rs "letopis:type=external;path=src/git/reader.rs")

[Issue 108 measurements](#historical-range-reuse-2026-11-10 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#historical-range-reuse-2026-11-10")

## Search bottlenecks {#search}
<!-- letopis: {"created_at":1794000000,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

For the 2026-11-07 bounded hybrid search on 100k elements, Samply observed
3,323 usable stacks and 1.537790 s CPU. Document refresh accounted for 44.846%,
vector retrieval 37.263%, and eligibility 7.626% of sampled CPU. Inclusive scopes
overlap. The measured code refreshed in `service/search.rs` and retrieved vec1
results in `storage/vector.rs`. SQLite could scan the full vector collection.

An earlier flat/cos experiment found a native query speedup and near-parity on
the complete CLI search. A different pre-bounding campaign measured
111.886→61.102 s (1.831x) after Markdown cursor and bounded successful parse
reuse. Each comparison applies to its own campaign.

[Bounded search report](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/report.md")
[Vector index experiment](../../docs/zkit/letopis/checks/issue-65/vector-index/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/report.md")
[Markdown search experiment](../../docs/zkit/letopis/checks/issue-65/search-optimization/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/search-optimization/report.md")
[Search service](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")
[Vector storage](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

[Native and whole-CLI vec1 measurements](../architecture/storage.md#vec1-measurements "letopis:type=related_to;path=letopis/architecture/storage.md#vec1-measurements")

[Current search behavior](../architecture/search-pipeline.md#request-bounds "letopis:type=related_to;path=letopis/architecture/search-pipeline.md#request-bounds")

## Measured latencies and speedups {#measurements}
<!-- letopis: {"created_at":1794000000,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

Repeated inspect: 100k 40.957→7.963s (5.14x, three pairs); 10k 3.774622→0.760819s (4.96x, ten per side). Structural-validation reuse at 100k: check 42.452→32.038s (1.325x), status 42.218→31.566s (1.337x), three pairs each. Earlier 100k first publication took 162.648s and one full index 207.402s; neither is a speedup claim.

Bounded hybrid search with real Ollama vectors, query case 000000: at 100k, L5 66.622→1.344s (49.59x), L100 70.915→1.867s (37.99x); at 10k, L5 5.294→0.182s (29.15x), L100 5.261→0.291s (18.06x). Each uses three alternating pairs, initialized synthetic corpora and warm model; OS cache and shared-host load are uncontrolled. 10k and 100k campaigns differ and do not prove linear scaling or general relevance equivalence. The 10k corpus reused 9,999 matching real vectors and generated one with Ollama.

Earlier 10k embeddings-disabled pre-optimization medians: traverse 0.106s, check 2.008s, status 1.956s, refresh one file 0.285s, several 0.301s, bulk 5.904s, and index --full 5.406s. These are not current-version timings. Small samples do not justify p95/p99 or production/minimum-hardware acceptance.

[Inspect and publication](../../docs/zkit/letopis/checks/issue-65/2026-11-06-100k/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/2026-11-06-100k/report.md")
[1k and 10k survey](../../docs/zkit/letopis/checks/issue-65/current/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/current/report.md")
[Check and status](../../docs/zkit/letopis/checks/issue-65/native-followup/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/native-followup/report.md")
[100k bounded hybrid](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/report.md")
[10k bounded hybrid](../../docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md")

## Optimization mechanisms {#optimizations}
<!-- letopis: {"created_at":1794155240,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Five changes produced the measurements above; their campaigns are separate and
must not be pooled.

**Git loose objects.** The writer computes the OID, looks for an existing loose
object, verifies its kind and identity, syncs the object file and parent chain
and rechecks write activity before reuse. Packed or absent objects and any
integrity or I/O failure keep the ordinary staging path.
[reuse](../../src/git/durability.rs#L183-L222 "letopis:type=external;path=src/git/durability.rs#L183-L222")
[call site](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

**Tree validation.** After a fresh read verifies an object's type and identity,
`tree_bytes_get_by_oid` skips structural validation for an OID already retained
in a bounded reader-owned set of 4096 identities. Full snapshot audits still
validate tree structure directly; overflow stops retaining rather than skipping
fresh checks.
[reader](../../src/git/reader.rs#L150-L165 "letopis:type=external;path=src/git/reader.rs#L150-L165")

**Markdown scanning.** Element bounds advance the line offset incrementally and
count line breaks only over the new byte range. Search independently reuses one
bounded successful `ParsedFile` when a binding's fresh capture has the same path
and SHA-256; missing, invalid, unmarked or oversized observations clear it.
[layout](../../src/markdown/parser.rs#L204-L237 "letopis:type=external;path=src/markdown/parser.rs#L204-L237")
[parse reuse](../../src/service/search.rs#L325-L361 "letopis:type=external;path=src/service/search.rs#L325-L361")

## Retrieval optimizations {#retrieval-optimizations}
<!-- letopis: {"created_at":1794155240,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

**Vector index.** New databases build a packed exact flat/cos index. Preparation
rebuilds any non-flat model in one immediate transaction, rechecks after taking
the writer and leaves read-only or query-only handles unchanged.
[schema](../../src/storage/vectors.sql#L24-L25 "letopis:type=external;path=src/storage/vectors.sql#L24-L25")
[preparation](../../src/storage/schema.rs "letopis:type=external;path=src/storage/schema.rs")

**Bounded search.** The per-channel and final limit defaults to 100 and is capped
at 100. Both FTS and vector SQL apply the bound after joins and deterministic
ordering, before Rust decoding; filtering and fusion use only that union, with
no refill.
[limit](../../src/service/types.rs#L21-L24 "letopis:type=external;path=src/service/types.rs#L21-L24")
[service cap](../../src/service/search.rs#L105-L105 "letopis:type=external;path=src/service/search.rs#L105-L105")
[text SQL](../../src/storage/cache.rs#L549-L575 "letopis:type=external;path=src/storage/cache.rs#L549-L575")
[vector SQL](../../src/storage/vector.rs#L673-L712 "letopis:type=external;path=src/storage/vector.rs#L673-L712")

[measurements](#measurements "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#measurements")

## Ignore-directory pruning {#ignore-pruning}
<!-- letopis: {"created_at":1794251269,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Discovery passes the `.letopis/ignore` bytes to the `gix_dir::walk` exclude stack,
so a matching directory is pruned before its entries are read; the post-walk
filter still removes excluded paths as a safety net. With the default
`respect_gitignore=true` the stack also carries the repository's Git exclusion
sources; `respect_gitignore=false` uses only the Letopis bytes.

A single-host 10k-element measurement with `respect_gitignore=false` and an
ignore file keeping only `/letopis/` and `/src/` changed discovery from 13.2 s to
0.43–0.49 s and search from 26.6–32.5 s to 0.26–0.31 s. These are local wall-time
samples, not the controlled campaigns above, and are not pooled with them.

[Worktree walk and exclusion stack](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

[Discovery measurements and search](performance-bottlenecks.md#search "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#search")

## November 10 command profiles and refresh candidate {#command-profiles-2026-11-10}
<!-- letopis: {"created_at":1794260240,"source_snapshot":"d427b2746a0502450f64ef2caf12ca49f9476e27"} -->

On 2026-11-10, an optimized build of commit `cc9a021` was measured on macOS
with a synthetic 10,000-element corpus and embeddings disabled. Unprofiled
medians of three runs were: search 0.100 s, traverse 0.108 s, inspect 0.792 s,
check 1.920 s, status 1.883 s, and `index --full` 5.286 s. A no-change `index`
took 0.113 s in one run. Search after changing one file took 0.421 s; after
changing all fixture files it took 5.820 s, each median of three. OS page cache
and shared-host load were uncontrolled; these are not production measurements.

One Samply profile per workload attributed 57.3% and 55.0% of sampled CPU for
one- and two-file refreshes respectively to `collect_source_references`.
For each source edge it scans the knowledge IDs to detect a conflict. Replacing
that scan with an equivalent indexed lookup is a candidate, not a measured
speedup: preserve conflict detection and validate behavior and scaling first.
The refresh scenarios execute search after editing the fixture, not `index`.

Search, traverse and unchanged index profiles mainly showed refresh and file
change detection. Inspect showed Git blob writing, staging and synchronization.
Check and status profiles attributed 50.3% and 51.5% of sampled CPU to the
historical source hash lookup; source provenance as a whole accounted for 90.6%
and 92.3%. Full index and bulk refresh also spent sampled CPU in provenance.
An init profile had only 60 samples and a config profile only one, insufficient
to rank their bottlenecks. Inclusive profile shares overlap and are not wall-time
shares. Real-provider search and production workloads were not assessed.

Raw captures and receipts are local ignored artifacts under
`.artifacts/profile-20261110-*`; they are not portable repository evidence.

[Source reference reconciliation](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")
[Source provenance](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")
[Git operations](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")
[Scenario definitions](../../tests/performance/scenarios.py "letopis:type=external;path=tests/performance/scenarios.py")
[Earlier check and status findings](performance-bottlenecks.md#check-status "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#check-status")

## Historical range reuse after issue 108 {#historical-range-reuse-2026-11-10}
<!-- letopis: {"created_at":1794274714,"source_snapshot":"cd688426bf88773c9d3fc3fb3f658a0a31302cd9"} -->

The issue 108 candidate `557d8ef` was measured on synthetic embeddings-disabled
shared-source corpora. Three alternating pairs measured check/status medians
of 1.201→0.973 / 1.153→0.925 s at 10k and
11.729→9.423 / 11.312→8.966 s at 100k (1.234–1.262x). OS page cache and
shared-host load were uncontrolled; these are not production measurements.

Reuse belongs to one accepted batch and historical tree/path group. Diagnostics
chunk knowledge at 256 nodes: the measured 100k shared status executed 391
grouped reads and 391 actual blob decodes, not one decode for the entire CLI.
Its 23,851 historical SHA calls covered 2,815,200 bytes; 100,000 fresh live SHA
calls covered 32,343,750 bytes. Hashes for identical source addresses were already reused;
no measured baseline SHA-call reduction is claimed. Live capture remains fresh.

The candidate probe measured about 7.992 s CPU in live capture versus
0.008 / 0.071 s historical/live SHA-256 for that status case. Thread CPU/wall
intervals include timer overhead; inclusive scopes overlap. sha2 0.10.9 uses
its software backend in the checked feature configuration. CPU SHA256 support
does not imply that this backend uses it, or make hashing the next priority.

Unique-path controls stayed near parity, with observed check wall regressions
of 1.15% at 10k flat and 1.97% at 100k sharded (CPU +0.16%). Whole-child RSS
rose about 3.6–3.8% in the 100k sharded control; no significance or full allocation
cause is established. Plan/result maxima were 62 entries for shared and 257
for unique paths. CLI chunks did not reach the 4096-entry / 1 MiB fallback caps.

Original flat 100k publication timed out at 600 s; its observed candidate was
intentionally interrupted. The approved sharded control uses 100 files per leaf
and different paths/tree shape, so its success does not erase that limitation.

[Measured results](../../docs/zkit/letopis/checks/issue-108/report.md#L1-L77 "letopis:type=external;path=docs/zkit/letopis/checks/issue-108/report.md#L1-L77")
[Batch provenance](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")
[Diagnostic batch size](../../src/service/mod.rs#L24-L24 "letopis:type=external;path=src/service/mod.rs#L24-L24")
[Batch ownership](../../src/service/utils.rs#L310-L335 "letopis:type=external;path=src/service/utils.rs#L310-L335")
[Earlier findings](performance-bottlenecks.md#check-status "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#check-status")

## November 17 profiling method and build identity {#profiling-method-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

The 2026-11-17 study measured revision
`0046a6d83709fcfb2a610a3789d8ef6d9ba2bcd8`, Letopis 0.3.1, Rust 1.95.0,
on macOS 26.6.2 arm64, Mac16,7, 14 cores and 48 GiB RAM.
All eight public commands, internal `embed --drain`, help, version and one
intentionally invalid argument were covered. At this revision the public
publication command is `index`, including `index --full`; the harness scenario
called `reindex` executes `index --full`, not a separate public subcommand.

Stock release supplied ordinary timings. A separate optimized debug1 release
supplied Samply 0.13.1 symbols; neither was the shipping LTO distribution profile.
Stock executable SHA-256 was
`77343af98921fcb0c1342488702c1bc5fc302a56476d46790e8aa6439e9443db`;
debug1 SHA-256 was
`b16196a645ad011150bad2e3c65884e935b6d575add68fb3392bc0a117ee05b5`.
Debug1 executable/dSYM UUID was `FFEBF8EC-A110-37A6-AA8B-747F67DAEA21`;
complete build receipts pinned 116 product inputs.

Foreground wall includes CLI launch, captured I/O and owned-process cleanup;
fixture preparation and independent correctness oracles are outside that timer.
Darwin `wait4` CPU is user plus system for the child and OS-accounted reaped
descendants. RSS is an individual lifetime maximum, not simultaneous family
memory or allocation volume. Supplementary rows preserve user/system, faults and
block-operation counters; some legacy rows preserve only combined CPU.
Block operations are not byte counts. No host isolation, affinity or OS-cache
control was established; first application use is not proof of a cold cache.

Samply percentages use PID-filtered weighted thread CPU, not raw stack counts.
Inclusive caller/callee scopes overlap and cannot be summed as wall-time shares.
All research agents were OpenAI Sol/Terra; provider-region interruptions reused
finished evidence rather than blindly repeating canonical benchmarks.
The research changed no production source, Cargo manifest or lockfile.

[Final report](../../reports/Letopis%20comprehensive%20performance%20profiling.md "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Workload coverage](#profiling-corpus-2026-11-17 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#profiling-corpus-2026-11-17")

## November 17 workloads and observation accounting {#profiling-corpus-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Canonical seed-65 corpora had 60/1k/10k/100k knowledge nodes, 2,000-byte
fragments, 30 elements per Markdown file, equal source-node counts and five
edges per knowledge node. At 100k: 500k edges, 73,333 includes and 3,334
knowledge files. A source address is not necessarily a distinct source file.
Small canonical fixtures used disabled Ollama configuration; 100k used disabled
OpenAI configuration. No provider requests were intended, but network traffic
was not independently traced.

Supplementary seed-20261117 fixtures covered sparse 0/100/1k/10k inputs and 1k
chain, star, dense, long-text, source, stale, dangling and cycle variants.
Deterministic loopback OpenAI-protocol mocks supplied 32-dimensional vectors.
Those paths measure protocol/vector work, not an actual embedding model.
Canonical and sparse topologies/configurations are different workload families.

The sanitized own-repository export actually published 45 knowledge nodes,
204 edges and 13 updated files, with zero invalid files updated. Its source-node
count is unavailable, not zero. Every check/status repeat reported 89 findings:
44 unavailable snapshots and 45 missing sources. Its 0.0803 s full-index median
describes that bounded projection, not a clean production corpus or all repo data.

Canonical 60/1k/10k: 352 valid rows = 319 measured + 30 conditioning + 3 first use.
100k: 20 valid rows = 19 measured + 1 first publication.
Catalog: 159 validations + 740 baseline rows, all 159 cases completed;
13 cases at n=1, 129 at n=3 and 17 at n=20. All 899 row identities and
1,798 stdout/stderr hashes reconciled, without missing/duplicate/skipped slots.
The invalid-argv case intentionally exits 1 in validation and baseline.
Separate drain scaling: 3 validations + 9 baseline rows.
Total stock baseline observations: **1,087 = 319 + 19 + 740 + 9**, including
one expected invalid-argv probe, not 1,087 successful operational commands.
Do not pool validation, conditioning, first use, profile or different-case rows.

[Workloads and observation accounting](../../reports/Letopis%20comprehensive%20performance%20profiling.md#methodology-and-workload-accounting "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")

## November 17 per-command stock measurements {#profiling-latency-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Values below are medians except n=1. Wall and combined CPU are seconds;
RSS is median individual lifetime peak in MiB. CPU is the median of each
invocation's user + system, not the sum of separate component medians.
Columns are marginal summaries and need not describe the same invocation.

| Command / actual workload | n | Wall | CPU | RSS |
|---|---:|---:|---:|---:|
| init, fresh sparse 0 | 20 | 0.066172 | 0.0152745 | 11.125 |
| config, sparse 100 | 20 | 0.016592 | 0.0047775 | 8.578 |
| status, canonical 100k | 3 | 10.679151 | 10.161579 | 369.58 |
| index --full, canonical 100k | 1 | 178.498726 | 174.625770 | 443.61 |
| index unchanged, canonical 100k | 3 | 0.680731 | 0.655125 | 17.17 |
| check, canonical 100k | 3 | 10.859230 | 10.464636 | 371.39 |
| inspect, canonical 100k | 3 | 7.878149 | 5.444781 | 60.08 |
| search unchanged, canonical 100k | 3 | 0.727827 | 0.680672 | 17.88 |
| traverse unchanged, canonical 100k | 3 | 0.682390 | 0.655148 | 17.84 |
| embed --drain, separate 100-text mock | 3 | 0.997387 | 0.175816 | 13.656 |

At sparse 10k, init remained 0.067181 s wall / 0.015353 CPU / 11.102 MiB;
config was 0.016421 / 0.004917 / 8.562, n=20 each. These startup rows are not
100k results. Their measured low absolute times do not negate Git-open startup
scopes observed in separate diagnostic profiles.

100k wall ranges: status 10.551076–10.892807, check 10.753415–10.943573,
unchanged index 0.676878–0.686114, search 0.702877–0.729675,
traverse 0.680101–0.691355. Inspect ran in order
**47.384810 → 7.147968 → 7.878149 s**; the median hides substantial variation.
No causal cache/wait explanation was established for inspect's first call.
100k first-publication search separately took 135.650655 s, CPU 132.357087 s,
363.67 MiB RSS, n=1; it is not unchanged-query latency.

[Command table and ranges](../../reports/Letopis%20comprehensive%20performance%20profiling.md "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Scaling](#profiling-scale-2026-11-17 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#profiling-scale-2026-11-17")

## November 17 scale-specific measurements {#profiling-scale-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Each entry is **n / median wall seconds / median combined CPU seconds /
median peak RSS MiB**, from source-rich canonical fixtures. Do not substitute
the supplementary sparse graph's lower full-index times for these observations.

| Command | 60 knowledge nodes | 1,000 | 10,000 | 100,000 |
|---|---|---|---|---|
| index --full | 3 / .073136 / .038579 / 14.39 | 3 / .365304 / .322583 / 22.08 | 3 / 4.627904 / 4.471358 / 77.23 | 1 / 178.498726 / 174.625770 / 443.61 |
| status | 20 / .025701 / .013374 / 11.80 | 20 / .122336 / .111313 / 16.75 | 20 / .993107 / .974611 / 59.40 | 3 / 10.679151 / 10.161579 / 369.58 |
| check | 20 / .044191 / .025288 / 12.95 | 20 / .136799 / .116742 / 17.76 | 20 / 1.047962 / 1.019178 / 62.47 | 3 / 10.859230 / 10.464636 / 371.39 |
| inspect | 20 / .050729 / .023880 / 13.09 | 20 / .094162 / .067027 / 13.80 | 3 / 1.372925 / .614514 / 19.09 | 3 / 7.878149 / 5.444781 / 60.08 |
| search unchanged | 20 / .040933 / .021771 / 13.22 | 20 / .060200 / .040050 / 13.49 | 20 / .105040 / .085430 / 14.57 | 3 / .727827 / .680672 / 17.88 |
| traverse unchanged | 20 / .046525 / .026271 / 13.30 | 20 / .051889 / .032120 / 13.43 | 20 / .108481 / .088889 / 14.38 | 3 / .682390 / .655148 / 17.84 |

Canonical full-index wall ranges at 60/1k/10k were .072432–.101762,
.362799–.366241 and 4.484957–4.966385 s. The 100k full-index value is one
sample, not a distribution. Status/check 100k maximum observed RSS was
373.75/373.22 MiB; full index maximum was 443.61 MiB.

At 10k, search after one changed file took .456478 s, after two files .441105 s,
and after bulk changes 4.827821 s, n=3 each. These `refresh_*` scenarios execute
search after edits, not separate index commands. With only two files at 60
nodes, `refresh_several` and `refresh_bulk` are not different edit counts.

Inspect at 10k reduced to n=3 under the existing runner's adaptive policy after
a 4.553 s first conditioning invocation; the second conditioning call was .662 s.
100k had no conditioning and used one stable repository sequentially; earlier
scales used prepared scenario copies. Configurations also differ across scales.
Consequently ratios are observed associations, not controlled complexity proofs,
cold/warm equivalence, tail-percentile estimates or production capacity limits.

[Scale-specific results](../../reports/Letopis%20comprehensive%20performance%20profiling.md#small-and-large-workloads "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Publication hypothesis](#graph-publication-2026-11-17 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#graph-publication-2026-11-17")

## Remaining status and check bottleneck at 100k {#check-status-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Status/check consume approximately ten CPU-seconds and 370 MiB RSS at 100k,
not merely ten seconds waiting on disk. Three complete 10k check profiles
placed `observe_sources_batch` at 81.99%, 81.38%, 84.55% and
`directory_verify_root` at 55.73%, 54.72%, 57.19% of observed CPU.
They contained 1,114 / 1,134 / 1,349 usable stacks.
Complete 10k status had 1,062 stacks: source observation 85.92% and
`file_get_by_path` 79.17%. Nested percentages overlap.

The 100k status capture covered only an approximately ten-second prefix:
provenance source-file attribution 84.31%, filesystem source-file attribution
76.41%, native `__open` 49.13%. Deliberate timeout at 10.013 s, exit -15,
is not a completed status measurement. Stock latency samples are separate.
No complete 100k check CPU capture was collected.

`StatusService::get_status` calls `CheckObservations::collect_check_problems`
inside a status-read transaction. Unscoped `CheckService::check` first runs
verified refresh, pins the revision, then collects findings from one read view.
Shared diagnostics retrieve nodes and source bindings, materialize graph data
and call provenance in 256-knowledge-node batches. Historical-file range reuse
already exists; fresh current-source capture remains per binding.

The current-file path traverses and verifies ancestors, observes entries,
opens/reads/stats files, rechecks identities and verifies the named root.
`directory_verify_root` reopens the root and compares identities. Several
checks on one call chain may protect different race windows; sampled presence
does not prove that removing or caching any check is safe.
Measure actual opens, reads, root checks, bytes and allocations first.
Preserve fresh per-binding observations, first-error order, typed failures,
confinement, no-follow checks, concurrent replacement detection and pinned views.

[Focused optimization issue #111](https://github.com/zdkadiev/letopis/issues/111)
is open; it records baselines, reproduction, code links and acceptance criteria.
It is a scoped follow-up to #100, not another historical-read reuse proposal.

[CPU findings](../../reports/Letopis%20comprehensive%20performance%20profiling.md#sampled-bottlenecks-and-code-locations "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Shared diagnostics](../../src/service/utils.rs#L279-L360 "letopis:type=external;path=src/service/utils.rs#L279-L360")
[Provenance contract](../../src/service/mod.rs#L747-L778 "letopis:type=external;path=src/service/mod.rs#L747-L778")
[Confined capture](../../src/filesystem/local.rs#L557-L703 "letopis:type=external;path=src/filesystem/local.rs#L557-L703")
[Root identity check](../../src/filesystem/local.rs#L1220-L1227 "letopis:type=external;path=src/filesystem/local.rs#L1220-L1227")
[Prior historical reuse](#historical-range-reuse-2026-11-10 "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#historical-range-reuse-2026-11-10")

## Full-index graph publication bottleneck {#graph-publication-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

The complete 100k symbolized full-index capture contained 183,971 usable stacks
and 177.888334 s observed CPU; sampler-wrapper elapsed was 185.133 s.
Product/result/count validation passed: revision 3, 3,334 updated files,
zero invalid files, 100k knowledge/source nodes and 500k edges.
This is not a truncated index profile or the separate 178.498726 s stock run.

| Inclusive sampled scope | 10k full index | 100k full index |
|---|---:|---:|
| EffectiveGraph::apply | 19.81% | 53.35% |
| DocumentPublication::reconcile_sources | 9.72% | 23.92% |
| EffectiveGraph::collect_source_references | 4.84% | 22.63% |
| SQLite source-file presence | 33.58% | 11.05% |

These percentages overlap. Native `_platform_memcmp` appeared in 27.63% and
`sqlite3_step` in 6.96% at 100k, overlapping their enclosing paths.
Source locations describe enclosing/optimized scopes, not exclusive line cost.

The publication loop applies each prepared changed file before final source
reconciliation. `EffectiveGraph::apply` calls `edges.retain` across effective
edges per file. Work can grow with the sum of edge-set sizes across F changed
files, approximately O(FE) for an E-sized graph. `collect_source_references`
uses `knowledge.keys().any(...)` per source edge to reject source/knowledge
address conflicts, suggesting worst-case O(SN) comparisons. Neither actual
iteration counts nor counterfactual savings were measured.

`reconcile_sources` has another knowledge-key scan for unreferenced touched
sources. It already groups local source paths for one capture per path.
Keep exact conflict rejection, owner replacement/deletion, touched sources,
earliest referring `created_at`, final edges, last-valid invalid-file behavior,
atomic rollback and source/discovery integrity when testing simpler lookups.
Do not transfer provenance's fresh-per-binding contract blindly to publication.

[Focused optimization issue #112](https://github.com/zdkadiev/letopis/issues/112)
is open. It covers repeated graph scans and memory, separate from #111.
No optimization, numeric speedup or new representation has been accepted.

[Publication profile results](../../reports/Letopis%20comprehensive%20performance%20profiling.md#sampled-bottlenecks-and-code-locations "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Publication loop](../../src/service/document_refresher.rs#L365-L406 "letopis:type=external;path=src/service/document_refresher.rs#L365-L406")
[Graph scans](../../src/service/document_refresher.rs#L947-L1007 "letopis:type=external;path=src/service/document_refresher.rs#L947-L1007")
[Source publication](../../src/service/document_refresher.rs#L731-L811 "letopis:type=external;path=src/service/document_refresher.rs#L731-L811")

## Inspect writes and unchanged-query overhead {#inspect-change-detection-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

A complete 10k inspect capture placed Git object writing at 62.40%,
object synchronization at 24.64% and native `__fcntl` at 22.17% inclusive
sampled CPU. Owners are `src/git/repository.rs`, `durability.rs` and cleanup.
The 100k stock calls varied 47.384810 → 7.147968 → 7.878149 s.
Profiles motivate investigating writes/synchronization, but do not measure
blocked time or prove that the first call's extra wall time was disk/cache wait.
Durability and integrity checks must not be removed as a shortcut.

Unchanged 10k search/traverse captures placed change detection at
72.96% / 66.69% observed CPU, but had only 111 / 109 usable stacks.
They support a coarse path-level conclusion, not robust function rankings.
At 100k unchanged search/traverse/no-op index took .727827/.682390/.680731 s
median, n=3 each. Those are not the cost of FTS/traversal/index mutation alone.

Search after one-file refresh at 10k placed source-reference collection at
56.11% observed CPU, 426 stacks. Bulk refresh showed provenance 20.87%,
graph apply 19.22% and projection publication 19.21%, overlapping.
Refresh scenarios are searches after edits, not index commands.

The later direct-root long-1k traversal capture actually returned 16 elements
and 16 edges, not 1,000 nodes. Its `get_knowledge_link_by_element` scope carried
23,286 of 53,442 observed CPU microseconds; this is not baseline latency.
Verified sparse-10k index showed change detection at 42,209 / 68,848 µs.
Populated mock search showed `wire_embedding_executor_at_root` at
23,660 / 39,704 µs; a broad constructor scope is not an exclusive leaf cause.
No 100k hybrid-query or real-provider inference measurement was made here.

[Sampled CPU results and qualifications](../../reports/Letopis%20comprehensive%20performance%20profiling.md#sampled-bottlenecks-and-code-locations "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Git operations](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")
[Object synchronization](../../src/git/durability.rs "letopis:type=external;path=src/git/durability.rs")

## Startup profiles and sampling coverage {#profiling-startup-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Canonical profiling produced 12 validated captures: ten complete 10k captures,
one complete 100k index and one deliberately censored 100k status prefix.
Ten 10k ordinary debug1/profile pairs used equivalent prepared inputs.
Check's three wrapper/ordinary ratios were 2.1521, 2.3802 and 1.8415,
median 2.15×, with median difference 1.302544 s. Wrapper includes profiler
startup, collection and saving; this is not intrinsic product slowdown.
There was no ordinary debug1 100k control for estimating profile overhead.

Four later nested/window captures lacked the product PID/library and remain
unattributable. A direct Samply-to-Rust launch then attempted eight categories
in 15 captures: eight at 1 kHz, seven finite 10 kHz diagnostic attempts.
Six were usable, nine insufficient under at least 100 product-library stacks
and positive resolved Letopis CPU. Every requested output validated and every
sampler exited 0. Help had only 38 product stacks at 10 kHz; version had eight.
Neither supports detailed attribution or a zero-cost conclusion.

Config/help/version used 64 read-only outputs with EOF per aggregate.
Config's usable 10 kHz aggregate had 61 sampled product PIDs, not 64
independently observed executions. PIDs derive from exact private-library
path/UUID matches in profiles, not an independent OS execution trace.
Per-child exit codes were not independently observed.

Config `open_with_cleanup` carried 390,922 of 473,365 observed CPU µs;
nested Git open-from-paths carried 315,898 and config-cache init 121,405.
Fresh init's Git-open/discovery scope carried 15,106 of 30,227 observed µs.
These inclusive aggregates/short diagnostics do not establish per-call
latency or exclusive savings. Product-only counters were saved separately
from all-library stacks for eligible PIDs. All eight public commands and
internal drain have usable evidence across campaigns; help/version do not.

[Startup results and sampling coverage](../../reports/Letopis%20comprehensive%20performance%20profiling.md#startup-and-capture-quality "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")

## Mock embedding drain scale and commit scopes {#embedding-drain-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

The separate completed v2 sweep used deterministic localhost OpenAI-protocol
vectors at 32 dimensions, N unique texts/hashes and three baseline repeats
per size after one validation. All 12 rows passed, no skips/timeouts/failures.
Wall/user/system columns are independent medians; do not add separate CPU
medians and label that sum as the median combined CPU.

| Unique texts | Wall median [min–max], s | User / system medians, s | Median peak RSS, MiB |
|---:|---|---|---:|
| 2 | .051378 [.047056–.054496] | .004962 / .008008 | 12.313 |
| 100 | .997387 [.932611–1.013247] | .067246 / .107150 | 13.656 |
| 1,000 | 9.040224 [9.013907–9.132058] | .855949 / .758505 | 14.672 |

Every row verified queue N→0, N distinct hashes/provider items, no recorded
failures and native current-configuration coverage N/0 missing at 32 dimensions.
Request counts happened to equal N; item coverage, not one-request-per-item,
is the correctness rule. The 100-text combined CPU median is .175816 s.
Do not pool this cohort with catalog `sparse-100.mock-drain`, whose median
wall was .9948 s and whose fixture/configuration identities differ.

In a separate usable 100-text CPU capture, stacks containing `drain_ready`
included `sqlite3_step` 69,487 µs, `sqlite3WalFrames` 67,118 and
`unixSync`/`full_fsync` 59,883 out of 108,189 observed all-library CPU µs.
Nested scopes overlap; this is a commit/synchronization lead, not measured
I/O wait or evidence that model inference dominates.

Sweep launcher elapsed was 54.06 s including setup, validation and observers.
Per-product cap was 120 s; 1,800 s applied per driver stage, not globally.
Disk and post-observation 1 GiB RSS stops were soft, not instantaneous limits.
Earlier unstarted v1/v2 preflight records remain historical, not failed drains.
Real model/network latency, model RAM, semantics and the adjunct search sweep
were not measured. Native populated search separately verified the same DB
before/after with 100 covered, zero missing and 32 dimensions.

[Drain scale and CPU results](../../reports/Letopis%20comprehensive%20performance%20profiling.md#mock-embedding-drain-results "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")

## Live allocation snapshots and memory limits {#profiling-memory-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Four valid 10k live snapshots were collected: early full index plus later
check, inspect and full index. Full history means recorded history only
through that snapshot, not the complete invocation.

Early index snapshot at approximately 26 ms observed a 5,767,168-byte
node-vector allocation and 1,192,464 bytes/273 SQLite page-cache allocations.
Logged prefix heap-plus-VM high-water was 12.5 MB and cumulative allocations
52.9 MB; neither is full-command peak/churn. Heap analysis showed 11,453,856
bytes in 50,987 malloc nodes. VM reservations are not resident heap or RSS.

| Later snapshot | Approximate time after launch | Prefix heap+VM high-water / live, MB | Instrumented product lifetime RSS, bytes |
|---|---:|---|---:|
| check | .187 s | 26.9 / 6.5 | 71,991,296 |
| inspect | .133 s | 4.4 / 4.0 | 24,002,560 |
| index --full | .738 s | 30.7 / 30.6 | 85,082,112 |

Check allocation stacks showed 7,864,320 bytes in an edge vector and 5,767,168
in a node vector under graph diagnostics/storage reads. Later index showed
8,047,680 bytes/9,960 allocations in `BasisPreflight::add_file` BTree building
and 5,565,776 bytes/4,166 allocations under `EffectiveGraph::from_storage`.
Inspect showed 2,146,304 bytes on signal-thread creation, 559,104 SQLite
and 379,968 compression initialization; thread VM reservation is not necessarily
resident heap. Stack origin does not establish full-peak live ownership.

All four offline scans reported zero leaks at the snapshot, not leak absence,
absence of reachable retention or later peaks. At-exit full-history capture
failed as lite logging; original early runs lacked wait4 peak RSS and cannot
claim a passed RSS guard. Later captures retained actual exit/RSS and oracles.
No valid status/search/traverse heap capture or allocation chronology exists.

Working native logging was `MallocStackLoggingNoCompact=1` with no-content
`leaks --outputGraph=PATH`; normal `MallocStackLogging=YES` refused full history.
Separate helper and `/usr/bin/time -l` runs calibrated CPU/sleep/64 MiB touch:
wait4 CPU .513/.0125/.0179 s and RSS 12,550,144/12,517,376/79,593,472 bytes.
They were independent invocations, not exact paired-accounting equivalence.

[Allocation results and limitations](../../reports/Letopis%20comprehensive%20performance%20profiling.md#memory-results "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Graph materialization](../../src/service/graph_diagnostics.rs "letopis:type=external;path=src/service/graph_diagnostics.rs")

## Profiling reproduction and retained evidence {#profiling-reproduction-2026-11-17}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

The final report is `reports/Letopis comprehensive performance profiling.md`;
intermediate notes and raw receipts remain local under
`research_notes/Letopis comprehensive performance profiling/`.
Only final results are versioned; local archive paths are not GitHub downloads.
No multi-gigabyte evidence upload/commit is implied.

Key evidence directories within that notes root:

- `evidence/build-archives-20261117-0046a6d-v1/`: stock/debug1 receipts,
  private executables, symbols and 116-input provenance.
- `evidence/canonical-survey-{60,1000,10000}-20261117-v1/`: samples, manifests,
  correctness results and separate measured/conditioning/first-use records.
- `evidence/canonical-stress-100000-20261117-v1/`: samples, resource summary,
  corpus validation and ordered invocations on one stable fixture.
- `evidence/canonical-cpu-20261117-v1/`: raw compressed profiles, symbol
  sidecars, PID attribution, result validation and matched 10k controls.
- `evidence/supplementary-prepare-20261117-v1/`: 159-case catalog,
  measurements, per-case identities/statistics and final audit.
- `evidence/direct-root-cpu-20261117-v1/`: 15 capture attempts, exact
  identities, output/native checks and usable/insufficient classification.
- `evidence/native-memory-20261117-v1/` and
  `evidence/native-memory-extension-20261117-v1/`: snapshot graphs/errors.
- `evidence/drain-scale-20261117-run-v2/`: 12 actual rows and native receipts.
- `evidence/final-verification-20261117-v1/`: final helper-test logs.

Use a disposable checkout, pinned toolchain, Python 3.12 and locked dependencies;
fetch missing dependencies before offline runs. The tracked runner accepts
`--stage survey --nodes 10000 --scenarios check status` or `--scenarios reindex`,
`--initial-timeout 600 --timeout 120 --stage-seconds 1200 --evidence NEW_DIR`.
Without `--baseline`, it builds debug1, not stock. Genuine matching stock
receipts can be supplied with `--baseline`; never forge that identity.
At 100k use explicit repetitions/time/disk budgets; the generic runner copies
fixtures unlike the old stable-fixture launcher. Never rerun old hardcoded
launchers unchanged or overwrite saved evidence; review/update output paths.
Validate source/binary/config/fixture identity and command results before timing.

[Reproduction and retained evidence](../../reports/Letopis%20comprehensive%20performance%20profiling.md#reproduction-and-retained-evidence "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
[Tracked runner](../../tests/performance/run.py "letopis:type=external;path=tests/performance/run.py")
[Fixture and build contracts](../../tests/performance/scenarios.py "letopis:type=external;path=tests/performance/scenarios.py")

## Profiling verification, limitations and optimization follow-ups {#profiling-followups-2026-11-18}
<!-- letopis: {"created_at":1794952486,"source_snapshot":"ba696b852786a2b6fd7534a76474944d19e17637"} -->

Independent CPU, memory, catalog, drain and direct-root audits passed with
documented qualifications. Final report recheck returned READY TO DELIVER.
Final historical helper runs: 94 performance tests, eight skipped, 21.287 s;
49 research-tool tests, 2.372 s. Integration environment variables were unset;
these are not Rust root/native integration suites or fresh tests of an optimization.
An earlier helper run errored after a fake initial-publication timeout;
the later pass does not explain or prove the underlying launch delay fixed.

Missing measurements include production-clean corpora, Linux/minimum hardware,
multiagent contention, high-dimensional vectors, 100k hybrid search, actual
embedding-model CPU/RAM/quality, independent network tracing, lock waits,
byte I/O, whole detached-worker completion and full allocation chronology.
The vector-only canary returned its expected ID and one local HTTP request,
and the frozen text lacked its query token. Its disposable cache was removed,
so an independent raw FTS MATCH check is unavailable, not a passed test.

Do not derive p95/p99 from n=3/20, infer zero cost from missing samples,
pool stock and debug1, or promise a counterfactual speedup from inclusive shares.
Soft pre/post disk/RSS checks were not instantaneous hard resource limits.
Historical unexecuted Python-wrapper preparation and unstarted drain preflights
are not additional measurements; later completed receipts supersede them.

Interactive priority is shared status/check source observation:
[issue #111](https://github.com/zdkadiev/letopis/issues/111).
Next focused priority is full-index graph publication:
[issue #112](https://github.com/zdkadiev/letopis/issues/112).
Both include pinned code, baselines, reproduction, invariants and acceptance.
Inspect's variable write/synchronization path remains a separate investigation.
These are open work items, not implemented fixes or accepted architecture changes.

For each optimization, count actual hot-path operations, change one mechanism,
use matched inputs/builds and at least three alternating 10k/100k pairs plus
small/incremental controls, report CPU/wall/RSS and all failures/regressions,
preserve confinement/identity/audit/transaction/durability guarantees, add
mutation-checked correctness tests and run `make verify` for Rust changes.

[Validation results and limits](../../reports/Letopis%20comprehensive%20performance%20profiling.md#limitations-and-validation "letopis:type=external;path=reports/Letopis comprehensive performance profiling.md")
