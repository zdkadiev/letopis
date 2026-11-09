# Performance measurements and bottlenecks

## Check and status bottlenecks {#check-status}
<!-- letopis: {"created_at":1794000000,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

For check and status on the synthetic 100k corpus, Samply after tree-validation reuse recorded historical snapshot hashing at 66.11% and 66.68% of observed CPU, current source hashing at 28.02% and 29.32%, and Git object verification at 63.60% and 64.01%, respectively. These are inclusive sampled scopes that overlap; do not add them or treat them as wall-time shares. The profiles had 36,423 and 37,262 usable stacks. Before reuse, structural validation had 23.76% and 25.10% observed CPU; afterward no samples landed there, which does not mean no validations occurred.

Earlier separate temporary Rust instrumentation, not Samply, measured historical hashing about 30.5s CPU, object decoding 13.5–13.6s, tree validation 9.7–9.8s, object hashing 5.8s, live captures 8.1–8.2s and graph diagnostics 0.6–0.7s. Nested scopes overlap. Its 100k source bindings caused about 400k decodes and 4.8 GB decoded content. Measurement commits, fixture and shared-host conditions are recorded in the reports.

The source-diagnostic batch owner is service/utils.rs; provenance.rs observes each binding and live file. git/reader.rs decodes and verifies historical trees. The existing operation-local cache retains successful hashes keyed by tree and complete source URI; distinct ranges in the same file remain distinct. Live captures are never cached. Reuse of verified full historical file bytes across ranges, or parallel bounded batches, remains an unmeasured proposal subject to corruption and failure-order guarantees.

[Samply report](../../docs/zkit/letopis/checks/issue-65/native-followup/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/native-followup/report.md")
[Samply data](../../docs/zkit/letopis/checks/issue-65/native-followup/native-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/native-followup/native-summary.json")
[Instrumented scopes](../../docs/zkit/letopis/checks/issue-65/check-status-profiling/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/check-status-profiling/report.md")
[Batch orchestration](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")
[Source provenance](../../src/service/provenance.rs "letopis:type=external;path=src/service/provenance.rs")
[Git reader](../../src/git/reader.rs "letopis:type=external;path=src/git/reader.rs")

## Search bottlenecks {#search}
<!-- letopis: {"created_at":1794000000,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

For the bounded hybrid-search 100k recording, Samply observed 3,323 usable stacks and 1.537790s CPU: document refresh 44.846%, vector retrieval owner 37.263%, and eligibility 7.626%. These inclusive CPU shares can overlap and are not latency shares. Search refreshes in service/search.rs; vec1 retrieval runs in storage/vector.rs. SQLite may scan the full vector collection despite the SQL output limit. FTS and vector channels each return at most L candidates, L defaults to 100 and is capped at 100; no refill follows filtering.

A flat/cos native full-K SQL probe improved 2.145223→0.107106s CPU (20.03x), but complete search changed 65.593→65.830s in that separate experiment: no end-to-end gain was established. Before bounded retrieval, Markdown cursor and bounded successful parse reuse changed a different full hybrid campaign 111.886→61.102s (1.831x). These campaigns must not be added or pooled.

[Bounded search report](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/report.md")
[Vector index experiment](../../docs/zkit/letopis/checks/issue-65/vector-index/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/vector-index/report.md")
[Markdown search experiment](../../docs/zkit/letopis/checks/issue-65/search-optimization/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/search-optimization/report.md")
[Search service](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")
[Vector storage](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

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
