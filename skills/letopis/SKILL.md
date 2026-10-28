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

## Configure, observe, and reindex

Interactive commands use an empty request without waiting for stdin. An open pipe
with no data after 250 ms also supplies an empty request. For commands that need
fields, pipe a JSON object; once data arrives, it is read until EOF.

`config` observes `.letopis/config.json` without changing it:

```sh
printf '%s\n' '{}' | letopis config
```

Before setup it succeeds with `{"ok":true,"result":null}` and creates no service
files. Otherwise `result` is the stored configuration value. There is no config
setter: edit the file yourself, then observe it again. A valid example is:

```json
{"provider":"ollama","model":"code-model","parameters":{"base_url":"http://127.0.0.1:11434/api"},"max_retries":2}
```

`max_retries` is required, and `0` means one attempt. Malformed content makes
`config` fail with exit 1 and `error.code:"embedding_failed"`; it never echoes the
file. Search stays on FTS while the configuration is invalid. `status` reports the
current `invalid_embedding_config` problem; fix the file and the next command sees
it. `config` does not reset vectors, perform inference, launch a drain, or create
service state.

`status` reads the last published revision and its current diagnostics without
refreshing:

```sh
printf '%s\n' '{}' | letopis status
```

`result` has `revision` (null before the first publication), `embedding_backlog`,
`embedding_failed`, held `locks` and `problems`. Problems are current observations,
not history: an invalid file, a dangling link, an includes cycle, a missing Git
`source_snapshot` or invalid configuration disappears when the condition is
repaired and the next command observes it. Exhausted embedding attempts clear only
with a changed configuration identity or a vector reset, not merely on the next
publication. `status` does not rebuild a missing or stale cache and does not make a
subsequent live-file read atomic.

`reindex` is explicit maintenance, not the normal edit path:

```sh
printf '%s\n' '{}' | letopis reindex
```

It rebuilds graph and FTS from the corpus under `index.lock` and returns
`{"ok":true,"result":{"revision":N}}`. It does not wait for vectors, does not
confirm changed sources, and preserves author-declared `source_snapshot`. Ordinary
edits need only the next refreshing command; use `reindex` after cache loss or
a deliberate full rebuild. A physically corrupt cache is a separate recovery
decision, not something this command silently repairs.

## Cancel your own foreground call

SIGINT is handled for your own `search`, `traverse`, `inspect`, `check` and
`reindex` invocation. The process writes exactly one response and exits 1:

```json
{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}
```

If a normal result was already printed, a later SIGINT adds no second JSON. If the
cancellation response won, the process cannot also exit 0 as a successful call.
An uncommitted SQLite transaction rolls back as after a crash and the kernel
releases `index.lock`. A publication that had already committed is not rolled
back, so cancellation promises your caller terminates, not that no publication
survived. Treat the outcome as uncertain, read `status` or retry the same
idempotent command, and do not claim that SIGINT edited authored knowledge.

There is no cancel subcommand and no PID-based cancel API. Signal only a process
you started; never signal another writer. A detached `embed --drain` is an
independent process in its own session. It has no cancellation response, keeps
its durable attempt reservation, may finish after your cancelled caller, and never
edits authored Markdown. Leave it alone; the next foreground command observes its
work.

## Current boundaries

The CLI has `check`, `config`, `search`, `status`, `traverse`, mixed `inspect`,
and `reindex`. No Letopis `read`, `snapshot`, `apply` or `cancel` command exists;
use Git and edit Markdown. Cancellation is a signal to your own foreground
process, not a command. Invalid files retain their previous valid projections
with current diagnostics; a fresh cache cannot recover a previous projection of
currently invalid Markdown. Reindex preserves author-declared `source_snapshot`
and never confirms changed sources for the agent. It also preserves existing
vectors keyed by unchanged search hashes when rebuilding a deleted cache. It does
not repair physical cache corruption or provide selective paths.
Embeddings are optional; FTS remains usable without a configured provider.
Packaging, installation and operational procedures remain TK-43/TK-44 work.
