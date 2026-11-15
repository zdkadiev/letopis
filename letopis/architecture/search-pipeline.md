# Knowledge retrieval

## Graph model and traversal {#graph-and-traversal}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
Knowledge elements are identified by repository path or path plus anchor. A graph
edge is identified by the triple `(source, target, type)`; `external` targets a
source address and other types target knowledge. Self-loops and target-kind/type
mismatches are rejected. An element with an outgoing `includes` edge is a
metavertex; this kind is derived rather than authored as Markdown metadata.

The domain `Graph` contains selected identities and optional edges; it does not
load storage. The application `GraphService` receives the shared cache owner and enters a read
scope; nested calls join the caller's pinned view. It expands neighbors breadth-first,
tracks visited node keys, omits start
nodes from discovered elements, and stops at the requested depth and element limit.
The optional edge limit is separate from the element limit.

`TraversalService` refreshes documents, observes embedding configuration and opens
a storage read view before invoking graph traversal. It enriches selected elements
with locations, revision and current observation flags. Search and traversal return
links and metadata, not the Markdown body.

[Graph domain](../../src/domain/graph.rs "letopis:type=external;path=src/domain/graph.rs")

[Graph traversal](../../src/service/graph.rs "letopis:type=external;path=src/service/graph.rs")

[Traversal service](../../src/service/traversal.rs "letopis:type=external;path=src/service/traversal.rs")

## Request bounds and retrieval {#request-bounds}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

A search request requires a query. An omitted limit defaults to 100; an explicit
positive limit uses min(limit, 100). Zero/null/invalid numeric forms are errors.
The same effective L bounds each SQL channel and the final response.

Search refreshes the projection, validates all requested source revisions and
captures configuration before its pinned read. Query tokens form a bound literal
FTS AND expression; unavailable query embeddings retain the FTS fallback.
FTS5/BM25 and vec1 cosine matching, joins, ordering and top-L selection execute
inside SQLite. Rust decodes only selected knowledge IDs and ranking metadata:
at most L per channel, at most 2L unique candidates. It loads no complete vector
or FTS result collection. SQLite's own exact scan/workspace may still cover all
vectors; final SQL limiting follows current-node joins and URI tie ordering.

Only selected candidates receive fresh knowledge/provenance observations. Filters
apply inside that pool without refill; lower candidates cannot fill an excluded
head. Eligible ranks are recomputed per channel, fused by sum(1/rank), then limited.
Ties use knowledge ID. This replaces full-corpus eligible RRF and tail-candidate
error observation; global refresh/revision/SQL failures still apply. No new ANN,
runtime, embedding identity reset or physical vector ownership change is involved.

[Search service](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")
[CLI defaults and validation](../../src/cli/input.rs "letopis:type=external;path=src/cli/input.rs")
[Shared request limit](../../src/service/types.rs "letopis:type=external;path=src/service/types.rs")
[FTS SQL](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")
[Vector SQL](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")
[Agreed design](../../docs/superpowers/specs/2026-11-07-bounded-search-design.md "letopis:type=external;path=docs/superpowers/specs/2026-11-07-bounded-search-design.md")

## Measured search behavior {#measurements}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"41e999b9c873fc11d715bd30fe7e3aec2d203c70"} -->

The 2026-11-07 trials used separate synthetic 10k and 100k corpora with real
768-dimensional Gemma vectors and the warm-model query `case 000000`. At each
limit, all six 100k outputs matched byte-for-byte for that query. The agreement
is specific to the measured query and corpus. Host storage and `make verify`
passed for the measured change. The trials covered query execution on one host.

[Paired command timings](../development/performance-bottlenecks.md#measurements "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#measurements")
[Search CPU profiles](../development/performance-bottlenecks.md#search "letopis:type=related_to;path=letopis/development/performance-bottlenecks.md#search")

[10k measurements](../../docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md")
[Paired measurements](../../docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json")
[Genuine sampled CPU](../../docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json")
[Host verification](../../docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json")
[Full report](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/report.md")
[Earlier research](../../docs/zkit/letopis/checks/issue-65/search-next/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/search-next/report.md")
[Embedding execution](embedding-execution.md#service "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#service")
