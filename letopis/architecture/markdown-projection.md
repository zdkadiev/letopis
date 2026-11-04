<!-- letopis: {"created_at":1793278527,"source_snapshot":"fe8c6d1c0a1a8dcef5745c7939daecf43050c3a7"} -->
# Markdown projection and publication

Marked Markdown in the Git worktree is the authored knowledge; SQLite is its
searchable projection. `GitWorktreePaths::worktree_paths_get` returns raw indexed
and non-ignored untracked paths without running the Git executable; deleted tracked
paths remain included. Application `CandidateSelection` filters Markdown extensions
and `.git`/`.letopis` components as bytes. For structurally safe paths it asks the
filesystem to observe final symlinks without following components and skips them
before UTF-8 decoding and `RepoPath` validation. Unsafe parents, replacement and
I/O failures remain errors; only stable nonregular document leaves are excluded.
Only marked Markdown produces nodes.

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

[Raw Git worktree paths](../../src/git/worktree.rs "letopis:type=external;path=src/git/worktree.rs")

[Application candidate selection](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

[Markdown parser](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Parsing Markdown documents](markdown-parsing.md "letopis:type=related_to;path=letopis/architecture/markdown-parsing.md")

[Cache transaction](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Storage model](storage.md "letopis:type=related_to;path=letopis/architecture/storage.md")
