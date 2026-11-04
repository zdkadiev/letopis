<!-- letopis: {"created_at":1793278527,"source_snapshot":"e6730713cdcf157bcb2294169c248f3b165a37f1"} -->
# Markdown projection and publication

Marked Markdown in the Git worktree is the authored knowledge; SQLite is its
searchable projection. The Git adapter enumerates indexed paths and non-ignored
untracked candidates without running the Git executable. It skips final symlinks;
file access does not follow unsafe components. Only marked Markdown produces nodes.

`DocumentRefreshService` compares candidate file state with cached observations.
A routine refresh can return early when nothing requires publication. Otherwise it
takes the index lock, detects changes again, captures bytes, and asks `MarkdownCodec`
to parse elements, metadata and links. A file may contain one whole-file element or
non-nested marked sections. Edges and source bindings come from declared links.

Publication applies file state, nodes, outgoing edges and FTS text in a cache write
transaction. An invalid file records the current error while retaining its last
valid projection. The transaction advances the publication revision only on commit;
a failed write is not published as a partial successful revision.

Routine refresh uses root retention for source snapshots. Verified refresh and
`reindex` request closure verification; `reindex` performs a full projection rather
than an incremental one. The full unscoped `check` also requests verified refresh.

[Refresh implementation](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Corpus discovery](../../src/git/corpus.rs "letopis:type=external;path=src/git/corpus.rs")

[Markdown parser](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Parsing Markdown documents](markdown-parsing.md "letopis:type=related_to;path=letopis/architecture/markdown-parsing.md")

[Cache transaction](../../src/storage/cache_write.rs "letopis:type=external;path=src/storage/cache_write.rs")

[Storage model](storage.md "letopis:type=related_to;path=letopis/architecture/storage.md")
