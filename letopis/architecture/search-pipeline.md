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

The synthetic 100k corpus with real 768-dimensional Gemma vectors, query `case 000000`,
measured three ordinary pairs per L: L5 wall median66.622→1.344s (49.59x), L100
70.915→1.867s (37.99x). All six outputs per L match byte-for-byte in this scenario;
that is not a general full-corpus ranking equivalence or relevance-quality proof.
Separate genuine Samply at L100 shows refresh44.846%, vector retrieval37.263%,
search eligibility7.626% of observed CPU; inclusive scopes overlap, not wall shares.
Actual-root make verify and host storage acceptance passed. Measurements are warm
local observations, not a Linux/minimum-hardware gate; no document drain was timed.

The separate 10k synthetic corpus measured L5 5.294→0.182s (29.15x) and L100
5.261→0.291s (18.06x), three paired runs each. This is one query with warm model;
these separate campaigns do not establish linear scaling or general relevance.

[10k measurements](../../docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/2026-11-07-10k-hybrid/report.md")
[Paired measurements](../../docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/comparison-summary.json")
[Genuine sampled CPU](../../docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/profile/stacks.json")
[Host verification](../../docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/host-storage/manifest.json")
[Full report](../../docs/zkit/letopis/checks/issue-65/bounded-search/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/bounded-search/report.md")
[Earlier research](../../docs/zkit/letopis/checks/issue-65/search-next/report.md "letopis:type=external;path=docs/zkit/letopis/checks/issue-65/search-next/report.md")
[Embedding execution](embedding-execution.md#service "letopis:type=related_to;path=letopis/architecture/embedding-execution.md#service")
