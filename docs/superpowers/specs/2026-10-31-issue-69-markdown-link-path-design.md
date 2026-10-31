# Explicit graph paths in Markdown links — issue #69

Date: 2026-10-31. Status: accepted for implementation planning on 2026-10-31 after the user's “ок продолжай по задаче”; implementation, verification and all independent reviews are complete; integration awaits user choice.
Task: [issue #69](https://github.com/zaurkadiev/letopis/issues/69).
Base: `main@a82565ee8f590b959d2fd1cdf3e50cbe5d800187`.
Branch: `issue-69`, in a separate managed worktree.

## Intent

Give a Markdown link two independent purposes. Its ordinary destination is authored
for navigation; its `path` title field is the only Letopis graph/source target.
Letopis derives graph targets only from `path` and compares the local navigation
file path with that canonical target. It does not inspect file existence, compare
fragments, validate Markdown viewers, or maintain a compatibility mode for the old
destination-based graph mechanism.

This is a deliberate breaking change to marked links, not a change to canonical
knowledge/source identities. The agent continues to choose directories, create
files and maintain ordinary Markdown links. Letopis reads and indexes the corpus;
it does not author or repair it.

## Accepted decisions and superseded proposals

The following are user decisions, carried forward from the issue and conversation:

- Name the metadata field **`path`**.
- **Latest correction:** do not support existing destination-based marked links.
  There is one mechanism, with mandatory `path`; no fallback or compatibility flag.
- **Latest accepted refinement:** compare the local Markdown destination, resolved
  relative to its containing document, with the file part of `path`. Do not check
  file existence or fragments. Record a mismatch and expose it through `status`.
- Markdown-viewer behavior remains outside Letopis's concern; no browser, renderer
  or heading-anchor validation is introduced.
- Preserve the existing DDD/hexagonal boundaries and KISS, DRY and YAGNI rules in
  [AGENTS.md](../../../AGENTS.md) and the
  [program structure](../../zkit/letopis/architecture.md#program-structure).
- Prepare the specification first. Its approval precedes a separately prepared
  and approved implementation plan; neither code nor that plan is part of this step.

The earlier proposal preserving legacy links is **superseded**: `path` is mandatory.
The later proposal to ignore the destination entirely is also **superseded** by
the user's path-only comparison refinement. Viewer/fragment validation and legacy
compatibility remain outside the design.
The exact grammar and upgrade procedure below were proposed by the author and
carried into planning after the user accepted continuing with the refined scope.
Implementation remains gated on separate plan approval.

## Current behavior and affected owners

[Markdown contract X-4](../../zkit/letopis/contracts/X-4-markdown.md) currently defines
`letopis:type=TYPE`; [links.rs](../../../src/markdown/links.rs) passes `dest_url`
directly to `KnowledgeId::parse` or `SourceUri::parse`. Only inline links inside a
marked element declare edges. Images, links in image alt text, reference-style
links and links outside marked elements retain their current behavior.

[MarkdownParser](../../../src/markdown/parser.rs) already receives the document's
root-relative path and exact bytes. The path identifies the owning element; it
is the base only for checking the ordinary navigation destination, never for
resolving the graph target. Existing
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
are author choices checked only for lexical agreement of their local file paths
with `path`; their fragments and target existence are not checked.

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
4. For a canonical local target, compare its file path with the lexically resolved
   Markdown destination as defined below. A mismatch invalidates the marked file;
   the destination never supplies or replaces the graph target.
5. Construct the existing edge triple from the containing element, parsed target
   and type. Keep duplicate-triple deduplication, self-loop rejection, dangling
   knowledge handling and includes-cycle validation unchanged. A syntactically
   broken Markdown link that produces no inline-link event remains subject to
   existing Markdown parsing, not a new raw-text scanner.

A canonical local `path` is repository-root-relative, with the existing lexical
normalization. It is never relative to the containing Markdown file, current
working directory or viewer. Local source ranges remain inclusive `#Lx-Ly`;
knowledge anchors retain their existing grammar. Absolute HTTP/HTTPS sources
remain opaque under the existing SourceUri contract. `path` does not widen the
supported graph-address grammar or filesystem access rules.

### Local navigation path comparison

This is a pure lexical check, performed only for marked inline links whose parsed
`path` is a local knowledge or source address. It does not access the filesystem,
Git, a network, or a renderer. Remote HTTP/HTTPS source targets are outside this
local-file comparison: preserve their opaque IDs and existing validation without
adding URL-equivalence rules.

1. Take `dest_url` from the same decoded pulldown-cmark event. Discard the first
   literal `#` and everything after it **before** decoding the path. Do not parse,
   normalize or compare that fragment, even if it resembles a line range.
2. Accept a relative local reference. Reject root-absolute paths, network-path
   references (`//...`), a URI scheme before the first `/`, literal backslashes,
   literal controls, and a literal `?` in the remaining path as a path mismatch.
   A filename containing `?` can use `%3F`; a filename with `:` in its first
   component can use `./` to remain an unambiguous relative reference.
3. Percent-decode the navigation path exactly once as UTF-8 (unlike the title's
   literal `path`). Reject malformed percent escapes, invalid UTF-8, decoded
   controls, backslashes, or encoded `/` separators rather than guessing their
   navigation meaning. A literal percent filename uses `%25` in the navigation
   destination; the title keeps that filename verbatim.
4. If the navigation path is empty, it refers to the containing Markdown file.
   Otherwise join it to that file's parent directory. Normalize `.`/`..` and
   repeated `/` using the existing root-confined lexical rules; a root escape or
   an empty normalized file path fails. No symlinks or case aliases are resolved.
5. Compare the resulting repository-relative file path, preserving case and
   Unicode spelling, with the canonical file part of `path`. Exclude the latter's
   optional knowledge anchor or source line range from this comparison. Report
   `LinkPathMismatch` if they differ or steps 2–4 reject the navigation path.

For a document at `letopis/cache/cache-key.md`, `../../src/cache.rs#anything` and
`path=src/cache.rs#L2-L4` agree. `../../src/other.rs` does not. A matching but missing
file passes this new check; existing graph dangling-target/provenance diagnostics
remain independent. Different, empty or nonsensical navigation fragments do not
produce a mismatch. `path` itself still obeys existing domain anchor/range syntax;
this task adds no test that the fragment exists or that its range is present.

The author owns navigation beyond this lexical agreement. Letopis does not
promise viewer compatibility or fetch/inspect link destinations.

## Diagnostics and publication guarantees

Keep `InvalidLinkTitle` for malformed title structure or invalid type,
`InvalidTarget` for an address rejected by the domain parser, and `SelfLoop` for
an edge to the containing element. Add `DiagnosticKind::LinkPathMismatch` with
reason `link_path_mismatch` and fixed message
`Markdown link path does not match letopis path`. Preserve existing fixed safe
messages and byte-range coordinates; do not add raw title, path or URL payloads
to public diagnostics. A malformed title is never silently treated as navigation.

The same parser is used by publication and embedding's live-text validation.
Invalid marked files follow the existing per-file policy: a previously valid
projection remains available with its current invalid-file diagnostic; a fresh
cache publishes no partial projection from an invalid file. Old retained edges
are last-known valid data, not successful interpretation of old markup by the new
parser. Unrelated valid files continue to publish.

A path mismatch follows that same invalid-file policy and is stored in the
existing `files.error` during a successful refresh/reindex publication. Do not
introduce an error-history table or independent write from the parser. Extend
[GraphDiagnosticsService](../../../src/service/graph_diagnostics.rs)'s cached-message
classification so `check` and `status` expose the same safe reason:

```json
{"code":"validation_failed","message":"Markdown link path does not match letopis path","details":{"path":"letopis/cache/cache-key.md","reason":"link_path_mismatch"}}
```

This object is an item in `result.problems` from `status`, not a top-level command
failure. `details.path` identifies the containing Markdown file; it must not echo
the conflicting navigation/metadata addresses. Use the existing parser diagnostic
byte range where available; no new persisted range field is required.

`status` observes the last published state and does not parse edited files,
refresh, write or clear errors. Consequently, a newly introduced mismatch becomes
visible there after a refreshing command has published the finding. An author fix
alone does not clear it: the next successful refresh/reindex of that file replaces
its invalid state, after which `status` no longer reports the mismatch. Other
current findings remain subject to their existing rules. If SQL publication fails,
the old state remains; the invoking command reports that failure rather than
pretending the finding was durably recorded.

Changing only link destinations/titles changes exact element bytes and therefore
the element hash. For unchanged visible text/labels, existing search rendering
must produce the same search text and search_hash, allowing existing vectors to be
reused. No snapshot bytes, source_snapshot basis, graph ID representation, vector
configuration identity or retention rule changes.

Moving an author document can require repairs to its visible relative links. The
source element's own path-based ID changes as today, but unchanged `path` values
continue to address the same targets. If the move makes the visible relative path
disagree, the moved file receives a path-mismatch diagnostic until the author
repairs that link. The product neither repairs navigation nor updates incoming
references automatically.

## Upgrade: explicit reindex, no dual parser

This is a breaking markup upgrade. Migrate authored marked links by supplying
`path` with their intended old canonical graph/source address; author a
document-relative destination with the same local file path. Its fragment remains
independent. There is no automatic Markdown rewrite, migration CLI,
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
  private helpers if needed. Obtain the containing file from the already validated
  element ID (the part before its optional anchor), without extra parser arguments;
  `src/markdown/parser.rs` remains the shared codec. No
  generic metadata framework, new dependency or adapter/service is required.
- Reuse domain target validation, current application-owned MarkdownCodec,
  diagnostics and publication interfaces. Extend the existing DiagnosticKind and
  cached-message classification for the mismatch reason; no new service trait is
  required. No I/O in domain/Markdown and no new storage/Git dependency.
- Keep `lib.rs` composition, service orchestration, CLI request schemas, SQLite
  schemas and source verification unchanged unless implementation exposes a real
  necessary correction; surface any such scope change before adopting it.
- Update the active X-4 contract, affected current contract/design descriptions,
  authoring skill, operational upgrade documentation, active examples and relevant
  fixtures. Add a dated decision for the breaking change when implementing.
  Document one target mechanism: mandatory `path`, with a separate pure local
  navigation-path comparison and stored status diagnostic.
- Migrate the repository's active `letopis/` corpus and executable examples as
  needed for clean validation. Preserve `created_at` and honestly verified source
  bases. Do not invent new bases or change claims merely to update link syntax.
- Preserve historical approved specifications, reports, old snapshot fixtures and
  raw evidence against their original revisions. Older behavior may appear in
  explicitly historical or negative-test material; that is not a legacy runtime
  path. Classify active versus historical examples before bulk replacement.

## Acceptance and verification

| Area | Required evidence |
| --- | --- |
| One mechanism | A valid `type;path` title produces the expected literal triple; legacy `type` alone fails with InvalidLinkTitle even with a valid destination. |
| Path comparison | Relative spellings resolving to the same local file pass; different files, root escapes and unsupported local-reference forms produce LinkPathMismatch. Fragment changes do not affect comparison; empty/fragment-only destinations refer to the containing file. Graph targets still come only from path. |
| Navigation encoding | A single percent decode handles spaces, Unicode and literal percent/question-mark filenames; malformed escapes, invalid UTF-8, controls, backslashes and encoded separators fail. Percent escapes in metadata remain literal. |
| No existence/fragment lookup | Matching nonexistent files and differing/unresolvable navigation fragments pass this lexical check; no new filesystem, Git, network or renderer access occurs. Existing graph/provenance checks are unaffected. |
| Stored status finding | Refresh publishes a mismatch to files.error; status returns validation_failed with reason link_path_mismatch and the containing path. An edit alone leaves it visible; a successful refreshing repair clears it. Prior valid data and unrelated valid publications follow existing policy. Status itself performs no refresh/write. |
| Grammar | Missing/empty path, invalid type, wrong order, unknown/duplicate fields before path fail; semicolons, equals and `;path=` within the terminal address remain literal address content. |
| Canonical targets | Existing root-relative normalization, anchors, valid/invalid ranges, root escapes, spaces/Unicode and literal percent filenames use independent literal expectations. |
| Opaque URLs | Exact source ID survives percent sequences, query, fragment, semicolons and a literal `;path=` in the URL. Remote targets skip local-file comparison; no URL fetch, equivalence normalization or metadata percent decoding. |
| Element boundaries | Only current eligible inline links create edges; code examples, reference links, images/alt links and unmarked content retain current behavior. |
| Graph rules | Duplicate triples collapse; arbitrary valid relations remain supported; self-loop and includes/dangling checks retain their current outcomes. |
| Hashes and provenance | Syntax-only migration preserves search text/hash and vectors; exact-byte element hash changes; source basis and addressed bytes remain unchanged. |
| Refresh and upgrade | An old declaration cannot publish in a fresh cache; changed invalid files retain last valid data with a finding; explicit reindex reparses unchanged old/previously invalid new titles; repaired files clear their findings. |
| Author movement | Moving the containing file never rebases the canonical target. Repairing its relative destination restores lexical agreement; until then the file reports a mismatch. Its own source element ID changes as before. |
| Boundaries | Physical modules, imports, visibility, trait implementations and composition comply with AGENTS.md; no new process/runtime/dependency or viewer logic. |

Use focused Rust parser/link/search-rendering and real CLI publication/reindex
integration tests with Given/When/Then. Mutation-check the new meaningful tests
(e.g. replace path with dest_url, remove path comparison or cached-reason mapping,
accept a missing path, or percent-decode an opaque metadata address) and restore
the actual tree before running required root `make check` and `make verify`. These are future implementation checks, not checks claimed by
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

At specification-writing time, the issue and code were read on the pinned base
and the worktree was clean. That document-only stage ran reference/diff checks and
independent builder review, without product builds or model/viewer runs. The
subsequently approved implementation plan has now been executed and each task
reviewed; current validation evidence and limitations are in the issue-69 report.
The final whole-branch review approved the implementation without actionable findings.

### Design review receipts — 2026-10-31

The initial independent Superpowers builder check reported “nothing important
left” for the earlier destination-independent draft. A fresh independent builder
check of the later path-comparison refinement also reported “nothing important
left”. Local verification passed for nine document references/explicit anchors,
three example navigation paths, fenced blocks, superseded-decision/scope checks
and `git diff --check`. Product suites were not rerun for this document-only
revision; no product code, schema, dependency or active contract is changed. The
user subsequently approved the implementation plan and Subagent-driven execution.
Implementation, verification and all independent reviews are complete; no integration was performed.
