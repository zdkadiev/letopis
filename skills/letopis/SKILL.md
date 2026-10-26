---
name: letopis
description: Use when saving, checking, searching, traversing, or reading source-linked coding knowledge in a Git repository with Letopis.
---

# Letopis knowledge

Letopis indexes marked Markdown. The agent edits Markdown and code; CLI commands read,
check, and refresh the projection. Run from the Git root. In examples, `letopis`
means the supplied executable. Requests are JSON on stdin; require exit 0 and
`ok:true` before using a result. A failure has `ok:false` and `error`.

## Find, check, and read

Search before creating another record. Search and traverse return metadata, never
file contents; an empty search is not proof that a claim is true.

```sh
printf '%s\n' '{"query":"cache","limit":10}' | letopis search
printf '%s\n' '{}' | letopis check
printf '%s\n' '{"element_ids":["knowledge/decisions.md#cache"],"mode":"both","direction":"both","depth":2,"limit":20,"edge_limit":40}' | letopis traverse
printf '%s\n' '{"element_ids":["knowledge/decisions.md#cache","src/cache.rs#L2-L4"]}' | letopis inspect
```

`check` succeeds with `{"ok":true,"result":{"errors":[]}}` only when it finds
no current validation errors. Fix reported markup, dangling links, or `includes`
cycles in Markdown, then run it again. `traverse` supports `relations`,
`containment`, or `both`; `outgoing`, `incoming`, or `both`; optional `edge_types`.
It excludes the starting IDs and includes `edges` only if `edge_limit` is sent.
`search` accepts optional filters: `needs_recheck`, `sources`, `source_revisions`.
Treat a recheck flag as a prompt to examine current source bytes and provenance.

One `inspect` call returns `result.snapshot` (a retained Git tree OID) and
`result.elements` keyed by normalized requested IDs. Each value has `path`,
optional inclusive `lines`, and `hash` (SHA-256 of addressed bytes or null).
Inspect returns no text, flags, edges, or provenance basis. Read **both** knowledge
and local source from that same tree, using the returned path and lines:

```sh
git show <inspect-snapshot>:knowledge/decisions.md
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

Choose useful folders, filenames, anchors, and relation types for the repository.
IDs derive from Git-root-relative path and optional anchor; do not write a UUID.
Keep `created_at` when editing or renaming an existing element. A whole-file
element starts with metadata on line one:

```markdown
<!-- letopis: {"created_at":1792800000,"source_snapshot":"<verified-tree-oid>"} -->
Cache choice follows the checked code.
[code](src/cache.rs#L2-L4 "letopis:type=external")
```

For separate elements, place metadata immediately after each explicit heading:

```markdown
## Cache choice {#cache}
<!-- letopis: {"created_at":1792800000,"source_snapshot":"<verified-tree-oid>"} -->
The checked implementation uses the cache.
[code](src/cache.rs#L2-L4 "letopis:type=external")
[related](knowledge/other.md#decision "letopis:type=depends_on")
```

Do not mix a whole-file element with marked sections or nest marked elements.
Knowledge anchors begin with a letter and contain letters, digits, `_` or `-`;
source `#Lstart-Lend` is an inclusive line address. Link paths remain Git-root
relative even inside `knowledge/`. `external` binds a source and `includes`
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

## Current boundaries

The CLI has `check`, `search`, `traverse`, and mixed `inspect`. No Letopis
`read`, `snapshot`, or `apply` command exists; use Git and edit Markdown.
`status`, `reindex`, `config`, and handled cancellation scenarios remain later
TK-25–29 work. Portable installation and operations remain TK-44 work. Do not
claim handled `cancelled` JSON from the current executable. Embeddings are
optional; FTS remains usable without a configured provider.
