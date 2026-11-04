<!-- letopis: {"created_at":1793727646,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
# SQLite storage and read views

Markdown and source files remain authoritative. `.letopis/cache.db` is the
rebuildable graph and search projection; `.letopis/vectors.db` holds vector state
separately so rebuilding the cache does not inherently discard those vectors.

The cache schema contains a publication revision, `nodes`, `edges`, the contentless
FTS5 `text_index` and per-file observations. A node has a text identity and an
integer row key. An edge stores its source, target and type; its target has no
foreign key, so a missing target can remain recorded for diagnostics. The FTS row
is associated with the node row key. Invalid-file state belongs to the file record.

The vector database tracks configuration identity, `search_hash` to vector keys,
vec1 cosine-search data, failed attempts and the latest query error. Its index
uses `search_hash` to join vectors to current knowledge nodes; identical search
text can therefore share a vector. The schema is independent of the cache schema.

`SqliteStorage` owns separate cache writer, vector writer and read connections.
The read connection attaches the vector database and opens a transaction that
captures the cache revision and vector configuration identity. Cache publication
uses an immediate transaction and advances the revision at commit. Vector writes
check the expected configuration identity; their writer has a bounded busy wait.
Do not infer atomicity of arbitrary operations across the two databases from their
being available to the same read connection.

[Cache schema](../../src/storage/cache.sql "letopis:type=external;path=src/storage/cache.sql")

[Vector schema](../../src/storage/vectors.sql "letopis:type=external;path=src/storage/vectors.sql")

[SQLite connections](../../src/storage/sqlite.rs "letopis:type=external;path=src/storage/sqlite.rs")

[Read transaction](../../src/storage/read.rs "letopis:type=external;path=src/storage/read.rs")

[Cache publication](../../src/storage/cache_write.rs "letopis:type=external;path=src/storage/cache_write.rs")
