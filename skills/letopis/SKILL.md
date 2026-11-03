---
name: letopis
description: Use when saving, checking, searching, traversing, or reading source-linked coding knowledge in a Git repository with Letopis.
---

# Letopis knowledge

Letopis indexes marked Markdown. The agent edits Markdown and code; CLI commands read,
check, refresh, observe and rebuild the projection. Run from the Git root. In examples,
`letopis` means the supplied executable. Requests are JSON on stdin; require exit 0 and
`ok:true` before using a result. A failure has `ok:false` and `error`.
`letopis --help` and `letopis <command> --help` print the commands and each
command's request fields as plain text.

## Organize concept files

Store knowledge in a `letopis/` folder at the repository root. No corpus
registration is required. Reuse the repository's existing organization.
The `letopis/` knowledge folder is distinct from the private `.letopis/`
service directory, which is excluded from indexing.

Letopis discovers `.md` and `.markdown` files from
`git ls-files --cached --others --exclude-standard`: tracked files and untracked
files allowed by Git's ignore rules. Ignored untracked files are not indexed;
directories named `.git` or `.letopis` are excluded. Only Letopis markup outside
code blocks declares knowledge; ordinary Markdown alone creates no elements.
A clean `check` does not prove an ignored file was indexed. If a new concept is
missing, check its eligibility before changing its content or rebuilding the cache.

Group closely related knowledge in a file named after its common concept. A
file may hold one whole-file element or several section elements about that
concept. Keep each element's explanation, rationale, limits, examples, pitfalls
and sources together rather than creating an element for each fact. Search
first and update a suitable existing concept rather than creating a duplicate
or a narrowly fragmented file.

Organize knowledge by the subject it explains, rather than by its origin or
form. Keep a concept's domain explanation, implementation details, external
integrations and pitfalls together as separate marked sections when they serve
the same purpose. Before creating a file, search across these perspectives for
an existing concept that should contain the knowledge.

During consolidation, group files by their shared subject before considering
their size. Keep independently useful concepts separate even when they share
packages or sources. File-size targets guide organization; they do not
determine conceptual boundaries.

Aim for roughly 30–40 readable lines per knowledge element. Shorter elements
are fine when the concept needs less space; do not add filler. Avoid hiding a
large amount of text in very long lines. If a concept grows beyond this range,
split it along meaningful subtopics into separate knowledge elements. Prefer
marked sections in the same concept file when the subtopics belong together;
use separate concept-named files when that makes the organization clearer.
Link related elements with `related_to`, or use `includes` when an overview
contains them. Keep each element's claims, qualifications and sources together.

Aim for concept files of roughly 200–800 lines when enough related knowledge
exists. Consider merging subconcept files shorter than 200 lines into their
shared concept file, keeping their knowledge as separate marked elements.
When a concept file grows beyond 1,000 lines, consider splitting it into two
files named for meaningful subconcepts. These file-size guidelines do not
override conceptual cohesion: a file may be shorter, and unrelated material
should not be added to reach a target. Update element IDs and incoming links
after moving them; these file sizes are separate from element-size guidance.

Name the file after its common concept so its purpose is clear from the path.
Follow the repository's naming convention; otherwise use descriptive lowercase
hyphenated names with `.md`, such as `cache-key.md` or `http-retry-budget.md`.
Keep names stable as the explanation grows. Avoid generic buckets such as
`notes.md` or `decisions.md` for unrelated concepts.

A folder represents a collection of related concepts. The agent chooses the
grouping and creates a collection folder when the current concepts need one;
ordinary grouping needs no separate user instruction. Reuse suitable existing
collections and keep the hierarchy shallow, without scaffolding future topics.
Avoid accumulating hundreds of files in one folder: as a collection grows,
organize related concepts into clearly named, meaningful subfolders. Choose
groups by their conceptual relationships, not by arbitrary file counts.
For example:

```text
letopis/
  cache/
    cache-key.md
    cache-invalidation.md
  http-retry-budget.md
```

Folders organize files; they do not create graph elements or containment.
When a collection needs to be searched or traversed as knowledge, represent
the collection itself in a marked Markdown file and link to its members with
`letopis:type=includes`. Create that overview only when it serves a current
need. A concept may belong to several such collections through explicit links
without duplicating its file. Keep `includes` acyclic.

## Find, check, and read

Search before creating another record. Search and traverse return metadata, never
file contents; an empty search is not proof that a claim is true.

```sh
printf '%s\n' '{"query":"cache","limit":10}' | letopis search
printf '%s\n' '{}' | letopis check
printf '%s\n' '{"element_ids":["letopis/cache/cache-key.md"],"mode":"both","direction":"both","depth":2,"limit":20,"edge_limit":40}' | letopis traverse
printf '%s\n' '{"element_ids":["letopis/cache/cache-key.md","src/cache.rs#L2-L4"]}' | letopis inspect
```

`check` succeeds with `{"ok":true,"result":{"errors":[]}}` only when it finds
no current validation errors. Fix reported markup, dangling links, or `includes`
cycles in Markdown, then run it again. `traverse` supports `relations`,
`containment`, or `both`; `outgoing`, `incoming`, or `both`; optional `edge_types`.
It excludes the starting IDs and includes `edges` only if `edge_limit` is sent.
`search` accepts optional filters: `needs_recheck`, `sources`, `source_revisions`.
Treat a recheck flag as a prompt to examine current source bytes and provenance.

Search/traverse/inspect automatically refresh the database without editing Markdown.
Routine reads verify addressed source objects; a full source-snapshot audit needs
unscoped `check` or `reindex`. For audit guarantees and rebuild behavior, read
[operations](references/operations.md#source-snapshot-audits).

One `inspect` call returns `result.snapshot` (a retained Git tree OID) and
`result.elements` keyed by normalized requested IDs. Each value has `path`,
optional inclusive `lines`, and `hash` (SHA-256 of addressed bytes or null).
Inspect returns no text, flags, edges, or provenance basis. Read **both** knowledge
and local source from that same tree, using the returned path and lines:

```sh
git show <inspect-snapshot>:letopis/cache/cache-key.md
git show <inspect-snapshot>:src/cache.rs
```

For `src/cache.rs#L2-L4`, select inclusive lines 2–4 from the captured source
bytes, preserving their original bytes for hash comparison. A declared source
with missing bytes or unavailable range can have `hash:null`; do not present
it as verified source text. A source whose last current `external` declaration
was removed, a deleted knowledge element, or any other unknown ID makes the
**whole** inspect request fail with `not_found`. HTTP/HTTPS source addresses stay
opaque and are not fetched. `cat` and ordinary live reads are useful for quick
inspection, but separate reads can cross concurrent edits and do not form one
snapshot. Search/traverse results likewise do not lock later filesystem reads.

## Write marked Markdown

IDs derive from Git-root-relative path and optional anchor; do not write a UUID.
Keep `created_at` when editing or renaming an existing element. For a file with
one element, use whole-file metadata on line one. Both examples below are
authored in `letopis/cache/cache-key.md`:

```markdown
<!-- letopis: {"created_at":1792800000,"source_snapshot":"<verified-tree-oid>"} -->
# Cache key

Cache keys use the source hash.
[code](../../src/cache.rs#L2-L4 "letopis:type=external;path=src/cache.rs#L2-L4")
```

For multiple elements in a concept file, give each section an explicit unique
anchor and place its metadata immediately after the heading. Link section
elements using their Git-root-relative file path and anchor:

```markdown
## Cache key {#cache-key}
<!-- letopis: {"created_at":1792800000,"source_snapshot":"<verified-tree-oid>"} -->
Cache keys use the source hash.
[code](../../src/cache.rs#L2-L4 "letopis:type=external;path=src/cache.rs#L2-L4")
[invalidation](cache-key.md#invalidation "letopis:type=related_to;path=letopis/cache/cache-key.md#invalidation")

## Cache invalidation {#invalidation}
<!-- letopis: {"created_at":1792800000} -->
Invalidate cached entries when their source changes.
```

Do not mix a whole-file element with marked sections or nest marked elements.
Knowledge anchors begin with a letter and contain letters, digits, `_` or `-`;
source `#Lstart-Lend` is an inclusive line address. The mandatory title is exactly
`letopis:type=TYPE;path=ADDRESS`, where TYPE
matches `[a-z][a-z0-9_]*`. `path` is terminal: its complete nonempty remainder is
the canonical target, including allowed semicolons, equals signs and spaces.
Keys/order are fixed; no extra field or `target` alias is accepted. Old marked
links without `path` are invalid, with no fallback. Markdown handles its own
escapes/entities; the title receives no extra percent decoding or trimming.
Canonical local paths remain Git-root relative even inside nested folders.
Author ordinary destinations relative to the containing document. Letopis compares
only their local file path, lexically normalizing `.`/`..` within the root after
one UTF-8 percent decode. The title keeps literal filenames: a space stays a
space, `%20` stays `%20`; use `%20`/`%25` in navigation for space/percent filenames.
Unsupported absolute/scheme/query paths, root escapes, malformed encoding, controls,
backslashes and encoded `/` produce a mismatch. Destination fragments are ignored;
no existence, fragment, symlink, viewer or network lookup is performed. Remote
HTTP/HTTPS source targets are exempt from this comparison and remain opaque.
See the [exact navigation rules](../../docs/zkit/letopis/contracts/X-4-markdown.md#link-path).
`external` binds a source and `includes`
defines containment; other relation names follow lowercase underscore grammar.
Plain Markdown links do not create graph edges. Check after every edit, then
search/inspect the saved ID and read its returned tree bytes.

## Record an honest source basis

`source_snapshot` is an **existing complete Git tree** whose addressed source
bytes you checked; it is not the inspect reading snapshot merely because inspect
returned it. Verify with `git show <tree>:<path>` and compare the exact addressed
bytes (including line endings and final newline) to the source you actually read.
For a source matching committed HEAD, `git rev-parse 'HEAD^{tree}'` gives a
candidate basis. Never use it without that byte comparison.

For uncommitted source, create a complete tree through a **separate temporary
index**. The real index and HEAD are untouched. This example assumes the path is
within the repository and uses argv-based Git commands:

```sh
tmp=$(mktemp -d)
GIT_INDEX_FILE="$tmp/index" git read-tree HEAD
GIT_INDEX_FILE="$tmp/index" git add -- src/cache.rs
tree=$(GIT_INDEX_FILE="$tmp/index" git write-tree)
git show "$tree:src/cache.rs"  # compare addressed bytes with the source just read
rm -rf -- "$tmp"
```

Use `tree` as `source_snapshot` only after comparison; for a line range compare
that exact range. A temporary index can omit unrelated staged changes because
it starts from HEAD; it must never point at `.git/index`. Do not stage or commit
someone else's files to make a basis. The tree must exist and have the source
bytes before recording it. If source changes after the comparison, recheck it.

After a code edit, run `check` and search with `{"query":"cache","limit":10,
"filters":{"needs_recheck":true}}`, inspect/read current bytes, compare them
with the old basis, update the knowledge claim if needed, make a new verified
source tree, then update `source_snapshot`. A changed source does not become
verified just by changing the OID. Repeat `check`, search, and inspect afterward.

For a file or anchor rename, update the element's path/heading and every incoming
knowledge link. Run `check` to expose dangling references and cycles, and use
`traverse` with `direction:"incoming"` on the old ID before the rename to locate
known referers. After repairing them, run `check` and inspect the new ID. Preserve
`created_at`; keep `source_snapshot` only while its checked source binding still
supports the statement.

## Operational guidance

For first-time setup, embedding configuration, status diagnostics, or a deliberate
rebuild, read [operations](references/operations.md). Ordinary Markdown edits need
the next refreshing command. The mandatory-path binary upgrade requires an
[explicit reindex](references/operations.md#mandatory-path-upgrade-issue-69), even
for unchanged files; status alone cannot perform it. Before cancelling a command,
read [foreground cancellation](references/operations.md#cancel-your-own-foreground-call).

The public CLI has `init`, `check`, `config`, `search`, `status`, `traverse`,
`inspect`, and `reindex`. Use Git to read captured bytes and edit Markdown to
change knowledge; there are no `read`, `snapshot`, `apply`, or `cancel` commands.
