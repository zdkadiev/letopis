<!-- letopis: {"created_at":1793727646,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
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

`CacheStorage` owns the cache writer and combined read connection; `VectorStorage`
owns the independent vector writer. Application-owned `CacheDatabase` and
`VectorDatabase` expose zero-argument closure scopes. Services share the same
`Rc` owner when nested calls must join: they use their injected fields instead
of passing transaction handles. Only the outer scope commits or rolls back.
A caught nested error, panic or failed write prevents publication. A read inside
a write sees its uncommitted changes; a write inside a read is rejected and
makes the outer scope fail, whether the caller attempts a nested write scope or
a direct write method and catches its error. A rejected read-only publication
revision accessor does not poison the read scope. Failed cleanup disables reuse
if SQLite stays active.

A cache read pins its revision and attached vector identity before the callback.
Publication advances revision once at the outer commit; the pending revision is
provisional. Vector writes verify expected identity and use a bounded busy wait.
Query diagnostics use the separate vector writer while search retains its view;
service-owned write scopes end before provider HTTP. Different owners and the
two databases do not join into a shared transaction.

Status uses an observation-only CacheStorage. Absent storage needs no connection,
creates no durable file and skips the callback; an unpublished cache also skips
observation. The observed owner cannot write. `LocalFileSystem` provides generic
file/directory identity, no-follow and alias checks. Composition selects database
filenames and brackets opening/attaching with those checks; SQLite connections,
schemas and attachments remain in storage. Filesystem has no database-role component.
Cache node, edge, file, FTS and neighbor queries live with `CacheStorage` in
`storage/cache.rs`. Vector, embedding work, pruning and status-supporting SQL
observations live in `storage/vector.rs`; unit tests stay inline. `StatusService`
owns configuration-mode policy, identity interpretation and backlog/failed arithmetic.

Storage also validates use-case entry requirements without opening a transaction.
An incompatible inherited scope becomes rollback-only. Query inference permits a
cache read with an inactive vector owner; drain requires both owners inactive so
work selection can observe each completed vector publication.

[Cache schema](../../src/storage/cache.sql "letopis:type=external;path=src/storage/cache.sql")

[Vector schema](../../src/storage/vectors.sql "letopis:type=external;path=src/storage/vectors.sql")

[SQLite connections](../../src/storage/connection.rs "letopis:type=external;path=src/storage/connection.rs")

[Cache scopes](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Transaction lifecycle](../../src/storage/transaction.rs "letopis:type=external;path=src/storage/transaction.rs")

[Vector scopes](../../src/storage/vector.rs "letopis:type=external;path=src/storage/vector.rs")

[Direct write rejection tests](../../tests/storage_read_scope_failures.rs "letopis:type=external;path=tests/storage_read_scope_failures.rs")

[Generic file checks](../../src/filesystem/file.rs "letopis:type=external;path=src/filesystem/file.rs")

[Confined path checks](../../src/filesystem/path.rs "letopis:type=external;path=src/filesystem/path.rs")

[Filesystem owner](../../src/filesystem/local.rs "letopis:type=external;path=src/filesystem/local.rs")

[Composition](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Status policy](../../src/service/status.rs "letopis:type=external;path=src/service/status.rs")

[Application interfaces](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")
