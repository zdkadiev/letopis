# Bounded SQL hybrid search

2026-11-07. Implemented the user-approved bounded retrieval contract. On the
existing synthetic 100k corpus with real Ollama vectors, complete CLI search
became substantially faster. These are three fixed ordinary pairs per limit,
not the separate screening/profile calls. No ordinary sample was dropped.

| Limit | Before wall median, s | After wall median, s | Speedup | Lower latency | Maximum observed CLI RSS, MiB, before → after |
| --- | ---: | ---: | ---: | ---: | ---: |
| 5 | 66.622050 | 1.343517 | 49.59× | 97.98% | 158.656 → 34.016 |
| 100 | 70.915011 | 1.866692 | 37.99× | 97.37% | 151.297 → 35.141 |

[Raw summary](comparison-summary.json), [all calls and pins](pairs/manifest.json),
[independent final audit](review/final-data-review.md), [commands](commands.md).

For each limit all six ordinary stdout byte hashes match, including result order,
paths, inclusive lines, revision6 and empty flags. The separate omitted-limit and
limit1000 calls match explicit100 output. This equivalence is demonstrated for
this one query/workload; bounded retrieval does not preserve full-corpus RRF for
every query or filter. Source, binary, config, native model/dimension, vector
identity and count checks passed before/after. Independent SQL oracles and byte
freshness evidence validate the expected results rather than trusting exit0.

## What changed

L defaults to100 if omitted, otherwise min(positive requested limit,100). Zero,
null, fractional/exponential, negative and u64-overflow forms remain invalid.
Direct SearchService callers cannot bypass the cap. The same L reaches both SQL
channels and final ranking. FTS uses BM25/URI ordering and a bound SQL parameter;
vec1 uses distance/URI ordering and a bound final SQL limit after joins. Only
selected knowledge rows are decoded into Rust. The unique union is at most2L;
fresh knowledge/provenance checks, filters and RRF run inside that pool.

No refill is performed after filters; fewer than L results can be returned even
if eligible knowledge exists outside the pool. Candidate metadata/provenance
outside the selected pool is not observed. Global refresh, requested-revision
preflight and SQL/native errors retain their existing entry requirements. Full
corpus source auditing remains a check/index responsibility.

Native K remains count(vector_keys) internally so orphan vectors cannot consume
knowledge slots and equal distances retain global URI tie order after joins.
SQLite/vec1 may scan/work on the full vector set. The bound applies to SQL output
and application collections, not all internal SQLite working memory. No ANN,
new dependency/runtime, embedding identity reset, document reembedding or physical
vector ownership migration was introduced. The measured corpus has100k knowledge
and100k vectors; existing physical search_hash sharing remains supported.

[Design](../../../../../superpowers/specs/2026-11-07-bounded-search-design.md),
[plan](../../../../../superpowers/plans/2026-11-07-bounded-search.md), D-108 in
[decisions](../../../decisions.md).

## Corpus, model and measurement boundaries

The owned online-backup copy contains100000 knowledge nodes,100000 source nodes,
500000 edges,3334 marked files,1000 source files and100000 real768-dimensional
embeddinggemma vectors. Query:`case 000000`. Model:
`embeddinggemma:300m-qat-q4_0`; actual Ollama version0.40.15 and model digest are
pinned in [provider evidence](provider.json). The mirrored truncate:false warmup
request reproduces the independent oracle's f32 query-vector hash. Population
throughput was not remeasured.

Every call uses a fresh CLI process; the fixture is already initialized and
unchanged. SQL/state/correctness checks and provider warmup are outside CLI timers
and read data, so no call is described as cold. The first owned-copy conditioning
call took144.235049 s, including copied-file observation refresh; it is excluded.
Prespecified after-only screening calls took3.459733/1.451485 s for L5/L100 and
are retained separately. OS/model caches and other shared-host activity are
uncontrolled. No owned build/test/profile/drain ran during ordinary pairs; the
driver reserves output/fixture ownership and checks relevant active tools before
calls. Pair order is before/after, after/before, before/after for each limit.

The old binary is the previously captured flat-index release SHA
`ccf4a70e8f0ae54f036ec0dabbd4d01ce01e074de3e23cfae33559153d4497b9`.
This new campaign is not pooled with earlier111/61 or65/65-second campaigns.
Only wall medians/small-sample ranges are supported; no p95/p99, steady-state,
production relevance-quality, Linux/x86 or4GB/full-stack acceptance is claimed.
CLI wait4 CPU/RSS excludes the Ollama daemon/model and is not combined-stack RAM.

## Independent result and preservation checks

SQL top-L channels are independently queried with the pinned native module; the
resulting pool has6 knowledge IDs for L5 and101 for L100. Exact rational reciprocal
rank fusion provides the bounded oracle; SQL window ranks provide the separate
old full-channel oracle. Both expected orders happen to match here. Native probe
uses SQLite CLI3.54.1; product bundles3.53.2. The same vendored vec1 source and
actual product SQL/parameter plans are captured in [SQL plans](sql-plans-v2/receipt.json).

[Freshness proof](freshness.json) independently checks all4334 canonical file
hashes;1000 historical Git-source files equal current source bytes; all101
candidate knowledge ranges/anchors match stored hashes and all101 source bindings
select present equal historical/current byte ranges. No product parser/hash/
provenance helper computes these expected values. This is a frozen-fixture proof,
not a guarantee against arbitrary concurrent edits.

[Vector preservation](vector-preservation/summary.json) confirms original and
owned fixtures still expose100000 native vectors and307200000 bytes, with the
same original ordered payload SHA3
`4ED319020B3ADFE2FD6FC375EE308B6266F2D9E74689A6DE1EF7E22488218EE4`.
The exact SELECT spelling is preserved because sha3_query includes SQL text.

## Behavioral verification and review

Actual-root [host runner](host-storage/manifest.json) passed all16 commands,
including complete make verify V=1, strict Rustdoc, real60-second deadline,
native capabilities and copied standalone executable/linkage. Its269 input
hashes remain pinned. Nested make verify logged170 successful result groups,
2460 successful test executions, zero failures and22 ignored executions; counts
include debug/release/examples/subprocess repetitions, not unique tests.
The explicit deadline test passed separately in60.01 s. Linux was not run.

New tests cover default/clamp/invalid limits, direct service cap, SQL cardinality,
orphan/tie/shared-hash selection, selected versus unselected corrupted URIs,
bounded observations and no refill. Eight valid-build hand mutations failed real
assertions and were restored exactly: parser default/clamp, FTS/vector SQL caps,
service cap, premature native K, and separate FTS/vector propagation widening.

[Code review](review/code-review.md) identified an obsolete refill test and a
coverage gap; both were fixed. Full tests additionally found an obsolete CLI
case rejecting an omitted limit, which was updated while retaining explicit new
default tests. The focused L5 hybrid observation test and both propagation
mutations cover the gap. [Method review](review/method-review.md) requested
freshness/reservation/dependency pins; those were added before measurements.
[Final independent data audit](review/final-data-review.md) verified raw outputs,
arithmetic,107 current product/269 host/107 baseline/27 method pins, corpus,
freshness and all profile function totals/identity. No Important finding remains.

## Genuine Samply

Separate save-only recording after ordinary pairs:3323 usable samples, two
threads, observed CPU delta1537790 microseconds. Binary, profile and symbol UUIDs
match519E4889-7285-3915-98A8-59BE598FA9D1 and the pinned new executable.

| Scope | Inclusive observed CPU, s | Share |
| --- | ---: | ---: |
| Document refresh |0.689639|44.846%|
| Vector retrieval owner |0.573023|37.263%|
| Search eligibility |0.117272|7.626%|

These are sampled CPU shares; scopes may overlap and cannot be added or treated
as wall percentages. Eligibility includes bindings, freshness and provenance.
The dominant remaining work is refresh and native/SQL retrieval rather than the
previous corpus-wide eligibility loop. No profiler overhead percentage is claimed.

[Recording](profile/search.json.gz), [symbols](profile/search.json.syms.json),
[validation](profile/validation.json), [identity](profile/identity.json),
[function totals](profile/stacks.json).

## Source and evidence

The new immutable executable SHA is
`117bee3d746af4946c0a3a52f2aa6e8bd972e06ba25d2e982b1fe27a73af53ba`.
[Product pin](product/pin.json) records all107 inputs, exact build flags, root
HEAD52ed491 and dirty/uncommitted status. The root was not committed or installed
globally. [Task diff](review/final-change.patch) excludes earlier dirty work.
Earlier archives and original fixture data are preserved; new evidence is separate.

Findings belong to the existing search/storage Letopis concepts. Publication and
same-tree readback receipts are recorded in the knowledge subdirectory after the
immutable measurement evidence has been captured. Earlier0.2 knowledge cache and
its configuration are preserved; no new-element vector readiness is inferred.

Publication completed at repository knowledge revision106. Both updated concepts
return empty flags; source bindings are9 for search and30 for storage. Author
basis is220d1ff2be3be8d5ba3ea387a8e155ce091defab; same-tree readback snapshot is
a0d6f03ac70a54eefdbaca0e7d84e4d0c780c36a. All37 unique requested knowledge/source
addresses were read back and byte/hash checked. [Publication receipts](knowledge/report.md).
