<!-- letopis: {"created_at":1793728313,"source_snapshot":"20757d2dc4bdd8f2f2e45843fed128c7cc780bb7"} -->
# Parsing Markdown documents

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

Invalid UTF-8, metadata, layout or declared links produce diagnostics, not a
partial parsed projection. The refresh service records an invalid file while
retaining its last valid published projection; it owns publication and transactions,
not the parser.

[Parser entry](../../src/markdown/parser.rs "letopis:type=external;path=src/markdown/parser.rs")

[Element layout](../../src/markdown/scan.rs "letopis:type=external;path=src/markdown/scan.rs")

[Metadata parsing](../../src/markdown/metadata.rs "letopis:type=external;path=src/markdown/metadata.rs")

[Link collection](../../src/markdown/links.rs "letopis:type=external;path=src/markdown/links.rs")

[Search text rendering](../../src/markdown/search.rs "letopis:type=external;path=src/markdown/search.rs")

[Markdown contract](../../docs/zkit/letopis/contracts/X-4-markdown.md "letopis:type=external;path=docs/zkit/letopis/contracts/X-4-markdown.md")

[Projection and publication](markdown-projection.md "letopis:type=related_to;path=letopis/architecture/markdown-projection.md")
