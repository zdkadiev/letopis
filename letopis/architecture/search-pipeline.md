<!-- letopis: {"created_at":1793278527,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
# Search pipeline

A `search` request contains a query, a positive result limit and optional filters.
The service refreshes the document projection, validates requested source revisions,
observes embedding configuration and opens a scoped storage read view.

The query is split into Unicode alphanumeric tokens. Tokens are quoted and joined
with `AND` for FTS; a query with no tokens returns no candidates. The current
implementation obtains vector candidates first, when configuration and compatible
vectors are available, and then obtains FTS candidates. An unavailable query vector
leaves the full-text channel available.

FTS uses SQLite FTS5/BM25. Vector lookup uses cosine distance and joins vector keys
to knowledge by `search_hash`. The service collects metadata for the candidate
union, checks source bindings and live knowledge observations, and applies filters
before ranking. The read view carries the publication revision used in returned
metadata links; the links are not the knowledge text.

For each eligible channel, smaller BM25 or distance ranks first, with knowledge ID
as a tie-break. The channels are fused by adding `1 / rank` for each occurrence;
only then is the requested limit applied. A tie in the combined score is ordered
by ID. This is rank fusion, not arithmetic on raw BM25 and vector distances.

[Search implementation](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")

[FTS query](../../src/storage/fts.rs "letopis:type=external;path=src/storage/fts.rs")

[Vector query](../../src/storage/vectors.rs "letopis:type=external;path=src/storage/vectors.rs")

[Embedding execution](embedding-execution.md "letopis:type=related_to;path=letopis/architecture/embedding-execution.md")
