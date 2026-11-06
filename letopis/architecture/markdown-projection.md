<!-- letopis: {"created_at":1793278527,"source_snapshot":"b3d2f67dce3dfb7bddbcd5e90545b1a475d8e5fb"} -->
# Markdown projection and publication

Marked Markdown in the Git worktree is the authored knowledge; SQLite is its
searchable projection. `GitWorktreePaths::worktree_paths_get` returns raw indexed
and policy-selected untracked paths without running the Git executable; deleted
tracked paths remain candidates unless additional patterns exclude them. The Git
interface receives standard-exclusion mode and root-relative Gitignore bytes;
ancestor exclusions block child negation. Application code safely reads the config
and optional `.letopis/ignore`; tracked owners can be filtered too. Application `CandidateSelection` filters Markdown extensions
and `.git`/`.letopis` components as bytes. For structurally safe paths it asks the
filesystem to observe final symlinks without following components and skips them
before UTF-8 decoding and `RepoPath` validation. Unsafe parents, replacement and
I/O failures remain errors; only stable nonregular document leaves are excluded.
Only marked Markdown produces nodes.

`DocumentRefreshService` compares candidate file state with cached observations.
A routine refresh can return early when nothing requires publication and the
observed policy is unchanged. Otherwise it
takes the index lock, detects changes again, captures bytes, and asks `MarkdownCodec`
to parse elements, metadata and links. A file may contain one whole-file element or
non-nested marked sections. Edges and source bindings come from declared links.

Publication applies file state, nodes, outgoing edges and FTS text in a cache write
transaction. An invalid file records the current error while retaining its last
valid projection. The transaction advances the publication revision only on commit;
a failed write is not published as a partial successful revision. Policy bytes
and presence are rechecked before no-op returns and commits; a concurrent edit
aborts rather than mixing policies. A newly excluded owner is removed normally.

Routine refresh uses root retention for source snapshots. Verified refresh and
`index --full` request closure verification; full mode uses the library `reindex`
method to reparse the selected projection rather than apply an incremental one. The full unscoped `check` also requests verified refresh.

[Refresh implementation](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Raw Git worktree paths](../../src/git/worktree.rs "letopis:type=external;path=src/git/worktree.rs")

[Application candidate selection](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

[Markdown parser](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Parsing Markdown documents](markdown-parsing.md "letopis:type=related_to;path=letopis/architecture/markdown-parsing.md")

[Cache transaction](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Storage model](storage.md "letopis:type=related_to;path=letopis/architecture/storage.md")
