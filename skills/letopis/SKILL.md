---
name: letopis
description: Use when setting up Letopis or authoring source-linked Markdown with Letopis element IDs, graph links, and Git provenance.
---

# Letopis authoring

Letopis indexes marked Markdown. Edit the documents to change knowledge. Run the CLI
from the Git root with a JSON request on stdin. Require exit 0 and `ok:true`
before using a response. Use `letopis <command> --help` for current request fields.

Use [Search](../letopis-search/SKILL.md) to find and read knowledge,
[Capture](../letopis-capture/SKILL.md) to retain findings from prior work, and
[Dream](../letopis-dream/SKILL.md) to consolidate or repair existing knowledge.
For setup, discovery, embedding configuration, index maintenance, and cancellation,
read [operations](references/operations.md).

## Knowledge files

Follow the project's knowledge layout, defaulting to `letopis/` at the Git root.
Marked `.md` and `.markdown` files can live anywhere allowed by
[discovery settings](references/operations.md#discovery-settings). Letopis keeps generated
service state in `.letopis/`. Only Letopis markup outside code blocks declares knowledge.

A folder is a parent concept that can contain both concept files and subfolders.
To make a collection searchable and traversable, give it a marked overview and
connect its file elements and child overviews with acyclic `includes` edges.
A concept can belong to several collections through links to the same element.

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
Use plain Markdown links for navigation and marked links for graph edges.

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

Keep the recorded `source_snapshot` as long as the evidence in that tree supports
the claim and its bindings. Treat `source_changed` as a cue to review the claim.
Record a different tree only when the claim or its evidence requires a different
version. Verify every local binding against that replacement before saving it.

## Check edits

```sh
printf '%s\n' '{}' | letopis check
```

Read `result.problems` even with `ok:true`. Fix relevant findings in the Markdown and
rerun the check. For `source_changed`, review every address in `details.targets`
before updating the element's source basis.
See the [check and status reporting rules](references/operations.md#review-check-and-status-findings).
Confirm the saved element and its evidence using
[a single inspect snapshot](../letopis-search/SKILL.md#read-snapshot-evidence).
If the saved element is missing, check discovery before editing its markup or
rebuilding the index. Use [Dream's repair workflow](../letopis-dream/references/workflow.md#plan-changes-and-their-links)
when moving or renaming elements that may have incoming links.
