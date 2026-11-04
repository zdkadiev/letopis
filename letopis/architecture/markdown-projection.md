<!-- letopis: {"created_at":1793278527,"source_snapshot":"c10d57c3676b3c54105d3c896e9f7f19e7cfe3d7"} -->
# Markdown projection and publication

Marked Markdown is authored knowledge; the document refresh service discovers candidate files, captures and parses them, and publishes a derived graph and search projection through storage. The refresh service owns the index lock and rechecks changes under it before publication. Routine refresh retains source roots; verified refresh and reindex perform closure verification. Reindex requests a full projection while ordinary refresh is incremental.

[Source: src/service/document_refresher.rs](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Source: src/markdown/mod.rs](../../src/markdown/mod.rs "letopis:type=external;path=src/markdown/mod.rs")

The cache schema is version 4: `nodes.id` is an internal integer key and
`nodes.uri` is the unique textual knowledge/source address. Knowledge input
`id` and `uri` must match; unequal writes poison the unit. URI UPSERT preserves
the key while the row exists; deletion/rebuild promises no key/revision stability.

Both resolved edge endpoints are integer foreign keys to `nodes(id)`.
`unresolved_edges` retains missing-target declarations with a numeric source FK
and textual target URI, without placeholder nodes. Logical reads return the ordered
union; diagnostics see missing targets, while traversal reads only resolved edges.
Deleting a target demotes incoming declarations before removing its row; commit
promotes declarations whose target exists, including unchanged owners' links.
Deleting/editing an owner removes both representations. All mutations roll back
together on failure, including nodes, FTS, files and one revision increment.

Publication first writes changed nodes/FTS/files and reconciles derived sources,
then inserts only changed owners' declarations from the effective metadata graph.
Unchanged owners are not reparsed/reinserted; captured Markdown/search text lives per file.
Invalid Markdown retains its last-valid projection. Missing source bytes preserve
the source row with a null hash rather than becoming a missing knowledge target.

Incompatible schema 1/2/3 or malformed caches are rejected before vector access.
Direct open/reindex recognize the exact schema read-only before writable cache
open; status stays read-only. Rejection preserves main/WAL bytes and identities,
committed state, vectors/config/locks, including DELETE and uncheckpointed WAL.
All three entries may create an empty reader WAL/SHM; existing SHM reader marks
may change while its identity and safe lifetime survive. Readers observe live WAL
and never unlink sidecars; SHM byte stability is not guaranteed.
An operator must stop calls/drain, close handles, preserve vectors/config/locks/
Markdown/Git, remove only cache files, and reindex. A fresh cache cannot recover
discarded last-valid projections for currently invalid or missing Markdown.

[Source: src/storage/graph.rs](../../src/storage/graph.rs "letopis:type=external;path=src/storage/graph.rs")

[Source: src/storage/cache.sql](../../src/storage/cache.sql "letopis:type=external;path=src/storage/cache.sql")

[Source: src/storage/schema.rs](../../src/storage/schema.rs "letopis:type=external;path=src/storage/schema.rs")

[Source: src/storage/cache_write.rs](../../src/storage/cache_write.rs "letopis:type=external;path=src/storage/cache_write.rs")

[Source: src/storage/edges.rs](../../src/storage/edges.rs "letopis:type=external;path=src/storage/edges.rs")

[Source: src/storage/sqlite.rs](../../src/storage/sqlite.rs "letopis:type=external;path=src/storage/sqlite.rs")

[Source: src/storage/connection.rs](../../src/storage/connection.rs "letopis:type=external;path=src/storage/connection.rs")
