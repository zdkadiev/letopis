<!-- letopis: {"created_at":1793278527,"source_snapshot":"637231a8227b17b60de15af5f6a3e7622e054bd4"} -->
# Search pipeline

Search refreshes the document projection, validates source-revision filters, reads embedding configuration, and opens a storage read view. It obtains text and available vector candidates, checks candidate eligibility and source provenance, then ranks the eligible results. Results are metadata links rather than the knowledge text; source text can be read from an inspect snapshot.

[Source: src/service/search.rs](../../src/service/search.rs "letopis:type=external;path=src/service/search.rs")

[Source: src/cli/help.rs](../../src/cli/help.rs "letopis:type=external;path=src/cli/help.rs")

FTS joins `text_index.rowid` to the integer `nodes.id`, while returned identities
and tied result ordering use `nodes.uri`. Neighbor pagination and embedding-work
selection also order by textual addresses, independently of numeric insertion
order. Public internal `row_id` names retain their existing meanings.
Neighbor pages join resolved integer endpoints but compare/order by `(type, URI)`.
The pinned native SQLite VM proves a LIMIT-bounded top-K working set (at most 32
retained records plus constant working-record overhead), without OFFSET or an
unbounded sorter. Filter lookup retains only supplied filter constants. A page
may scan adjacency rows; repeated-page latency can grow with degree. This memory
bound does not establish the original text-index streaming performance.

The separate version-1 vector database keeps its own row keys. Current cache
membership joins vectors through `search_hash`, never through node integer keys.
A cache-only schema-4 rebuild reuses vectors for unchanged text without document
inference, preserving vector identity, attempts/failures and query diagnostics.
The transition itself neither resets nor prunes vectors; later configuration
changes and ordinary drain pruning keep their existing rules.

[Source: src/storage/fts.rs](../../src/storage/fts.rs "letopis:type=external;path=src/storage/fts.rs")

[Source: src/storage/neighbors.rs](../../src/storage/neighbors.rs "letopis:type=external;path=src/storage/neighbors.rs")

[Source: src/storage/work.rs](../../src/storage/work.rs "letopis:type=external;path=src/storage/work.rs")

[Source: src/storage/vectors.rs](../../src/storage/vectors.rs "letopis:type=external;path=src/storage/vectors.rs")

[Source: src/storage/vectors.sql](../../src/storage/vectors.sql "letopis:type=external;path=src/storage/vectors.sql")
