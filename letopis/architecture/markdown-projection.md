# Markdown indexing

## Discovery and candidate selection {#discovery}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Marked Markdown in the Git worktree is the authored knowledge; SQLite is its
searchable projection. `GitWorktreeListing::worktree_paths_get_by_ignore_mode_and_ignore_bytes`
returns raw indexed and policy-selected untracked paths without running the Git executable;
deleted tracked paths remain candidates unless the ignore bytes exclude them. The Git
interface receives the standard-exclusion mode and root-relative ignore bytes, and passes
them to the worktree walk as an exclude stack: a matching directory is pruned before its
entries are read, while the final path filter remains as a safety net. Ancestor exclusions
block child negation. Application code safely reads the config and optional
`.letopis/ignore`; `letopis init` writes a commented default when the file is absent and
never replaces an existing one. Tracked owners can be filtered too. Application
`CandidateSelection` filters Markdown extensions
and `.git`/`.letopis` components as bytes. For structurally safe paths it asks the
filesystem to observe final symlinks without following components and skips them
before UTF-8 decoding and `RepoPath` validation. Unsafe parents, replacement and
I/O failures remain errors; only stable nonregular document leaves are excluded.
Only marked Markdown produces nodes.

[Raw Git worktree listing](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

[Application candidate selection](../../src/service/utils.rs "letopis:type=external;path=src/service/utils.rs")

## Parsing pipeline {#pipeline}
<!-- letopis: {"created_at":1793728313,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

`MarkdownParser` implements `MarkdownCodec::file_parse` as a pure codec: it receives a repository-relative path and
file bytes, and returns parsed data or diagnostics. It does not read the filesystem,
write Markdown, access Git or SQLite, or call an embedding provider.

First it validates UTF-8 and parses the whole document with `pulldown-cmark`,
including heading attributes and byte offsets. It scans the Markdown events for
Letopis metadata comments. A file without such comments returns `None`; examples
inside fenced or indented code blocks do not declare elements.

A metadata comment must occupy its own line and contain valid JSON with required
`created_at` and optional `source_snapshot`; unknown fields are rejected. At the
first line it declares a whole-file element, identified by path. Immediately after
an explicitly anchored heading it declares a section element, identified by
`path#anchor`. A section ends at the next heading of equal or higher level, or EOF.
Duplicate or invalid anchors, orphan metadata and nested elements are errors.

For each element the parser records metadata, byte span, inclusive line range and
SHA-256 of its exact source bytes. It then collects graph edges from inline links
inside the element whose title follows `letopis:type=TYPE;path=ADDRESS`. The
`external` type targets a source; other types target knowledge. Images and ordinary
or reference-style links do not declare edges. Local navigation paths are checked
against the declared target path; remote external addresses remain opaque.

Finally, the parser renders search text from the element's parsed events, excluding
metadata comments, and computes a separate SHA-256 `search_hash` of that text.
The result contains elements, their search text and hashes, and a set of edges.
The exact element hash and search hash serve different purposes: a markup change
can alter the former without changing the latter.

[Parser, element layout, metadata, links and search text](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Markdown contract](../../docs/zkit/letopis/contracts/X-4-markdown.md "letopis:type=external;path=docs/zkit/letopis/contracts/X-4-markdown.md")

## Diagnostics and publication boundary {#diagnostics}
<!-- letopis: {"created_at":1793728313,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

Invalid UTF-8, metadata, layout or declared links produce diagnostics, not a
partial parsed projection. The refresh service records an invalid file while
retaining its last valid published projection; it owns publication and transactions,
not the parser.

[Projection and publication](#publication "letopis:type=related_to;path=letopis/architecture/markdown-projection.md#publication")

## Refresh and publication {#publication}
<!-- letopis: {"created_at":1793278527,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->

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

Routine refresh retains source roots. An unscoped `check` and `index --verify`
also audit the full source-snapshot closures. `index --full` verifies closures and
uses the library `reindex` method to reparse the selected corpus. For `index`,
`paths` limits owner-file updates and removals. For `check`, it selects
reported findings while the refresh still covers the corpus.

[Refresh implementation](../../src/service/document_refresher.rs "letopis:type=external;path=src/service/document_refresher.rs")

[Markdown parser](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Parsing Markdown documents](#pipeline "letopis:type=related_to;path=letopis/architecture/markdown-projection.md#pipeline")

[Cache transaction](../../src/storage/cache.rs "letopis:type=external;path=src/storage/cache.rs")

[Storage model](storage.md#schemas "letopis:type=related_to;path=letopis/architecture/storage.md#schemas")

[Check scope](cli-observation.md#check-scope "letopis:type=related_to;path=letopis/architecture/cli-observation.md#check-scope")
