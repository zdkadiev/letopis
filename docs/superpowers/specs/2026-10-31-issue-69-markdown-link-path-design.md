# Explicit graph paths in Markdown links — issue #69

Date: 2026-10-31. Status: draft for user approval; implementation has not started.
Task: [issue #69](https://github.com/zaurkadiev/letopis/issues/69).
Base: `main@a82565ee8f590b959d2fd1cdf3e50cbe5d800187`.
Branch: `issue-69`, in a separate managed worktree.

## Intent

Give a Markdown link two independent purposes. Its ordinary destination is authored
for navigation; its `path` title field is the only Letopis graph/source target.
Letopis must not infer one from the other, validate Markdown viewers, or maintain
a compatibility mode for the old destination-based graph mechanism.

This is a deliberate breaking change to marked links, not a change to canonical
knowledge/source identities. The agent continues to choose directories, create
files and maintain ordinary Markdown links. Letopis reads and indexes the corpus;
it does not author or repair it.

## Accepted decisions and superseded proposals

The following are user decisions, carried forward from the issue and conversation:

- Name the metadata field **`path`**.
- **Latest correction:** do not support existing destination-based marked links.
  There is one mechanism, with mandatory `path`; no fallback or compatibility flag.
- **Latest correction:** Markdown-viewer behavior is outside Letopis's concern.
  Do not compare destination and `path`, including their files or fragments.
- Preserve the existing DDD/hexagonal boundaries and KISS, DRY and YAGNI rules in
  [AGENTS.md](../../../AGENTS.md) and the
  [program structure](../../zkit/letopis/architecture.md#program-structure).
- Prepare the specification first. Its approval precedes a separately prepared
  and approved implementation plan; neither code nor that plan is part of this step.

The earlier answers preserving legacy links and requiring same-file agreement are
**superseded** by the latest correction. The issue's initial compatibility and
viewer-validation proposals are not acceptance requirements for this design.
The exact grammar and upgrade procedure below are the author's proposed choices,
subject to approval of this specification.

## Current behavior and affected owners

[Markdown contract X-4](../../zkit/letopis/contracts/X-4-markdown.md) currently defines
`letopis:type=TYPE`; [links.rs](../../../src/markdown/links.rs) passes `dest_url`
directly to `KnowledgeId::parse` or `SourceUri::parse`. Only inline links inside a
marked element declare edges. Images, links in image alt text, reference-style
links and links outside marked elements retain their current behavior.

[MarkdownParser](../../../src/markdown/parser.rs) already receives the document's
root-relative path and exact bytes. The path identifies the owning element; it
must not become a base for resolving the graph target. Existing
[domain identity types](../../../src/domain/identity.rs) and
[SourceUri](../../../src/domain/source.rs) continue to validate and normalize the
canonical address.

[Incremental refresh](../../../src/service/document_refresher.rs) can skip
unchanged files, so replacing the binary does not reinterpret every stored edge.
This matters for the explicit cache-upgrade procedure below.

## Proposed format: one mandatory terminal path field

The following grammar applies to the **decoded link title supplied by the pinned
pulldown-cmark parser**, after ordinary Markdown escape/entity handling:

```text
graph-title = "letopis:type=" TYPE ";path=" ADDRESS
TYPE        = [a-z][a-z0-9_]*
ADDRESS     = the complete remaining nonempty title string
```

Use exact lowercase keys in this exact order. No extra spaces are inserted around
the keys or delimiter; do not trim or re-encode ADDRESS. `target` is not an alias.
A missing/empty `path`, invalid TYPE, unknown prefix field, wrong field order or
repeated field before the terminal address is a malformed Letopis title.

**`path` is terminal, not a generic parameter map.** Once `;path=` is consumed,
every remaining character belongs to ADDRESS, including `;`, `=`, quotes already
unescaped by Markdown, spaces or percent signs where the existing domain grammar
allows them. For example, `;path=` in an HTTP query is literal URL content, not a
second metadata field. Appending another setting after the address is unsupported:
it becomes address content and is validated as such, never ignored or treated as
an overriding field. This fixed grammar avoids a new escaping language and keeps
opaque URLs intact. Tests must pin this rule explicitly rather than assuming a
semicolon-separated key/value framework.

There is **no additional percent decoding, URL decoding, backslash unescaping,
case folding or whitespace trimming** in the title-field parser. Markdown's own
quote/escape/entity rules still apply. In particular, `%20` in `path` remains the
literal characters `%20`; a local filename containing a space is written with a
space inside the quoted title. Existing domain restrictions, including reserved
`#` delimiters and rejected root escapes, remain unchanged.

### Examples

Examples are complete inline-link snippets within a marked element. In
`letopis/cache/cache-key.md`:

```markdown
[implementation](../../src/cache.rs#L2-L4 "letopis:type=external;path=src/cache.rs#L2-L4")
[invalidation](cache-invalidation.md "letopis:type=related_to;path=letopis/cache/cache-invalidation.md")
[member](cache-invalidation.md "letopis:type=includes;path=letopis/cache/cache-invalidation.md")
```

The first edge addresses source `src/cache.rs#L2-L4`; the next two address knowledge
`letopis/cache/cache-invalidation.md` with distinct types. The ordinary destinations
are examples of author choices, not values that Letopis resolves or verifies.

A viewer-specific fragment and an encoded navigation path may be independent:

```markdown
[decision](cache-invalidation.md#viewer-heading "letopis:type=related_to;path=letopis/cache/cache-invalidation.md#expiry")
[file with spaces](../../src/cache%20key.rs "letopis:type=external;path=src/cache key.rs")
[remote](https://Example.com/a%20b?q=x;y#part "letopis:type=external;path=https://Example.com/a%20b?q=x;y#part")
```

The last source URL preserves its spelling, percent sequences, query, semicolon
and fragment. It is not fetched or translated into a local path.

## Parsing and graph behavior

1. Reuse the current pulldown-cmark events and element ownership checks. An
   unmarked link is ordinary Markdown. A link title beginning `letopis:` inside a
   marked element must match the new grammar or produce a file diagnostic.
2. Parse TYPE using the existing `EdgeType`. Agent-defined relation names remain
   allowed; `external` and `includes` keep their existing special semantics.
3. Read ADDRESS exclusively from `path`. For `external`, call `SourceUri::parse`;
   for every other type, call `KnowledgeId::parse`.
4. Construct the existing edge triple from the containing element, parsed target
   and type. Keep duplicate-triple deduplication, self-loop rejection, dangling
   knowledge handling and includes-cycle validation unchanged.
5. Do not read, resolve, normalize or compare the link's `dest_url` for graph
   semantics. Any destination accepted by the Markdown parser, including an empty
   destination or one pointing elsewhere, has the same graph outcome for a fixed
   valid title. A syntactically broken Markdown link that produces no inline-link
   event remains subject to existing Markdown parsing, not a new raw-text scanner.

A canonical local `path` is repository-root-relative, with the existing lexical
normalization. It is never relative to the containing Markdown file, current
working directory or viewer. Local source ranges remain inclusive `#Lx-Ly`;
knowledge anchors retain their existing grammar. Absolute HTTP/HTTPS sources
remain opaque under the existing SourceUri contract. `path` does not widen the
supported graph-address grammar or filesystem access rules.

There is no promise that a visible destination reaches the same file or fragment
as the graph target. The author owns navigation accuracy. Letopis does not promise
viewer compatibility, detect misleading navigation or fetch destinations.

## Diagnostics and publication guarantees

Use the existing reasons: `InvalidLinkTitle` for malformed title structure or
invalid type, `InvalidTarget` for an address rejected by the domain parser, and
`SelfLoop` for an edge to the containing element. Preserve existing fixed safe
messages and byte-range coordinates; do not add raw title, path or URL payloads
to public diagnostics. A malformed title is never silently treated as navigation.

The same parser is used by publication and embedding's live-text validation.
Invalid marked files follow the existing per-file policy: a previously valid
projection remains available with its current invalid-file diagnostic; a fresh
cache publishes no partial projection from an invalid file. Old retained edges
are last-known valid data, not successful interpretation of old markup by the new
parser. Unrelated valid files continue to publish.

Changing only link destinations/titles changes exact element bytes and therefore
the element hash. For unchanged visible text/labels, existing search rendering
must produce the same search text and search_hash, allowing existing vectors to be
reused. No snapshot bytes, source_snapshot basis, graph ID representation, vector
configuration identity or retention rule changes.

Moving an author document can require repairs to its visible relative links. The
source element's own path-based ID changes as today, but unchanged `path` values
continue to address the same targets. The product neither repairs navigation nor
updates incoming references automatically.

## Upgrade: explicit reindex, no dual parser

This is a breaking markup upgrade. Migrate authored marked links by supplying
`path` with their intended old canonical graph/source address; author the ordinary
destination independently. There is no automatic Markdown rewrite, migration CLI,
legacy parse mode, schema migration or new persistent parser-version mechanism.

Before relying on the upgraded projection, run the existing full `reindex` with
`{}` on stdin under the new binary. Reindex, rather than an ordinary refresh,
forces unchanged candidates through the new parser. The operator must wait for
old Letopis invocations/drains to finish and use the new binary consistently;
mixed-version use of a shared cache is outside this upgrade procedure.

The required order is: finish prior processes, update the binary, author the new
markup, run `reindex`, then run `check` and resolve all findings before trusting the
migration. Do not delete cache.db, vectors.db, Git snapshots, locks or author files
as part of this upgrade. Status remains read-only and does not upgrade the cache.

Because reindex retains previous valid projections of invalid files, reindex alone
is **not** proof that every old declaration has been migrated. `check` must report
no markup errors; non-markup findings such as missing source bases must also be
reviewed under their existing contracts. If obsolete files remain, their errors
must remain visible until fixed or removed. A fresh-cache test must show that old
marked links cannot create new edges. Tests must also show that an already-cached
invalid new-form title is reconsidered by explicit reindex after the upgrade.

No schema change is needed: parsed canonical triples and file diagnostics retain
the same storage representation. Existing vectors and attempts remain keyed by
their existing identities. Do not add an automatic reparse framework solely for
this breaking change.

## Implementation boundaries and documentation scope

- Keep title grammar and conversion in `src/markdown/links.rs`, with cohesive
  private helpers if needed. `src/markdown/parser.rs` remains the shared codec.
  No generic metadata framework, new dependency or adapter/service is required.
- Reuse domain target validation, current application-owned MarkdownCodec,
  diagnostics and publication interfaces. No I/O in domain/Markdown, and no new
  storage/Git dependency or public API just for link parsing.
- Keep `lib.rs` composition, service orchestration, CLI request schemas, SQLite
  schemas and source verification unchanged unless implementation exposes a real
  necessary correction; surface any such scope change before adopting it.
- Update the active X-4 contract, affected current contract/design descriptions,
  authoring skill, operational upgrade documentation, active examples and relevant
  fixtures. Add a dated decision for the breaking change when implementing.
  Document one mechanism: mandatory `path`, destination not inspected.
- Migrate the repository's active `letopis/` corpus and executable examples as
  needed for clean validation. Preserve `created_at` and honestly verified source
  bases. Do not invent new bases or change claims merely to update link syntax.
- Preserve historical approved specifications, reports, old snapshot fixtures and
  raw evidence against their original revisions. Older behavior may appear in
  explicitly historical or negative-test material; that is not a legacy runtime
  path. Classify active versus historical examples before bulk replacement.

## Acceptance and verification to plan after approval

| Area | Required evidence |
| --- | --- |
| One mechanism | A valid `type;path` title produces the expected literal triple; legacy `type` alone fails with InvalidLinkTitle even with a valid destination. |
| Destination independence | Change only destination across relative, absolute URL, mismatched file/fragment and empty Markdown destinations; canonical triples stay identical. No target lookup comes from destination. |
| Grammar | Missing/empty path, invalid type, wrong order, unknown/duplicate fields before path fail; semicolons, equals and `;path=` within the terminal address remain literal address content. |
| Canonical targets | Existing root-relative normalization, anchors, valid/invalid ranges, root escapes, spaces/Unicode and literal percent filenames use independent literal expectations. |
| Opaque URLs | Exact source ID survives percent sequences, query, fragment, semicolons and a literal `;path=` in the URL. No URL fetch or percent decoding. |
| Element boundaries | Only current eligible inline links create edges; code examples, reference links, images/alt links and unmarked content retain current behavior. |
| Graph rules | Duplicate triples collapse; arbitrary valid relations remain supported; self-loop and includes/dangling checks retain their current outcomes. |
| Hashes and provenance | Syntax-only migration preserves search text/hash and vectors; exact-byte element hash changes; source basis and addressed bytes remain unchanged. |
| Refresh and upgrade | An old declaration cannot publish in a fresh cache; changed invalid files retain last valid data with a finding; explicit reindex reparses unchanged old/previously invalid new titles; repaired files clear their findings. |
| Author movement | Moving the containing file does not rebase target path; only the source element ID changes. |
| Boundaries | Physical modules, imports, visibility, trait implementations and composition comply with AGENTS.md; no new process/runtime/dependency or viewer logic. |

Use focused Rust parser/link/search-rendering and real CLI publication/reindex
integration tests with Given/When/Then. Mutation-check the new meaningful tests
(e.g. replace path with dest_url, accept a missing path, or percent-decode an opaque
address) and restore the actual tree before running required root `make check`
and `make verify`. These are future implementation checks, not checks claimed by
this design-only step. Native storage/platform harnesses are required only if the
implementation actually changes their owned behavior or inputs.

A successful user-approved implementation of this specification does not require
opening a viewer, browser automation, renderer-specific anchor support, paid model
runs, or a navigation test matrix.

## External format reference and design-only verification

[CommonMark 0.31.2 links](https://spec.commonmark.org/0.31.2/#links) separates
inline-link destinations and titles and describes Markdown-level title escapes.
This design uses the already decoded title event and adds no second Markdown
parser. Viewer and rendering policies are outside this contract.

The issue and current code were read on the pinned base. This worktree initially
had no tracked or untracked edits. This specification is the only planned change
at this stage. Product builds, root tests, model runs and viewer checks are not run
for this document-only work; verify its local references, examples and diff, and
perform the independent builder check before handing it over for approval.

### Design review receipt — 2026-10-31

A fresh independent Superpowers builder check read this draft and reported
“nothing important left”; no material questions remain for planning. Local
verification checked eight document references/explicit anchors, fenced blocks
and the superseded-decision/scope statements. Product suites were not rerun:
there are no product-code, schema, dependency or active-contract changes in this
specification commit. Approval of the full specification is still pending.
