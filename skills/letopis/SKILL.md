---
name: letopis
description: Use when saving or retrieving source-linked coding knowledge between agent sessions in a Git repository using Letopis.
---

# Use Letopis Knowledge

Letopis reads marked Markdown; agents edit the files. Ordinary Markdown without
Letopis metadata is **not knowledge**. Run commands from the Git root. In examples,
`letopis` means the supplied or built executable; use its absolute path if needed.
This initial version has no `help`/`--help`; use the request shapes below.

## Find before writing

Send JSON on stdin, not positional search text:

```sh
printf '%s\n' '{"query":"cache","limit":10}' | letopis search
printf '%s\n' '{"element_ids":["knowledge/cache.md"]}' | letopis inspect
```

Require exit 0 and `ok:true`. Search returns links, lines, revision and flags;
inspect returns `result.snapshot` and `result.elements` metadata, **not text**.
Read exact knowledge bytes from the returned tree:

```sh
git show <snapshot>:knowledge/cache.md
```

Use the returned lines to select a section. Direct live reads are allowed, but
separate files may be edited between reads; one inspect tree preserves captured
knowledge bytes. Do not treat an empty search result as proof of correctness.

## Save a decision linked to code

Read the actual source first; record only what it supports. Prefer updating an
existing relevant knowledge. Preserve its `created_at` on edits/renames; change
`source_snapshot` only after rechecking the source.

For **new** knowledge about an already committed `src/cache.rs`, this example
verifies its live bytes against the basis tree and creates a whole-file element:

```python
import json, subprocess, time
from pathlib import Path

tree = subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"]).decode().strip()
source = subprocess.check_output(["git", "show", f"{tree}:src/cache.rs"])
assert Path("src/cache.rs").read_bytes() == source, "Source differs from basis"
metadata = {"created_at": int(time.time()), "source_snapshot": tree}
Path("knowledge").mkdir(exist_ok=True)
with Path("knowledge/cache.md").open("x") as output:
    output.write("<!-- letopis: " + json.dumps(metadata) + " -->\n")
    output.write("Cache decision: use_cache unconditionally returns true.\n")
    output.write('[code](src/cache.rs "letopis:type=external")\n')
```

The observation must match the source you actually read; this example is for the
fixture function. If live source differs from the tree, do not declare that tree
as its basis. Do not stage or commit somebody else's changes to make it match.

Whole-file metadata must be the **first line**; its ID is `knowledge/cache.md`.
For multiple independent elements use explicit headings and adjacent metadata:

```markdown
## Cache choice {#cache-choice}
<!-- letopis: {"created_at":1792800000} -->
A self-contained observation, with its evidence when available.
```

Its ID is `knowledge/decisions.md#cache-choice` when stored in that file. Assign
creation time when creating an element. Add the verified `source_snapshot` when
adding external links. Never nest marked elements or mix a whole-file element
with section elements. Express grouping with `includes` links instead.

All link paths are **Git-root relative**, even inside `knowledge/`:

```markdown
[code](src/cache.rs#L2-L2 "letopis:type=external")
[related](knowledge/cache.md "letopis:type=depends_on")
[member](knowledge/cache.md "letopis:type=includes")
```

An ordinary link without `letopis:type=...` is navigation, not a graph edge.
Anchors must start with a letter, then letters/digits/underscore/hyphen; source
line fragments `#Lstart-Lend` are inclusive and are not knowledge anchors.
The files must be tracked or unignored Git candidates (`.md`/`.markdown`).

## Confirm in a new invocation

Search for the saved observation, inspect its knowledge ID, and read the returned
Git bytes. Each search/knowledge inspect refreshes valid changed files before
answering. Inspect's **knowledge tree is not `source_snapshot`**: the latter names
the verified source basis, read separately with `git show <basis>:src/cache.rs`.
Inspect cannot yet retrieve source IDs or a mixed knowledge/source package.

Embeddings are optional. Successful foreground calls may launch a short-lived
background process; FTS stays usable while it runs or the provider fails.
For an explicit attempt, send EOF (or `{}`) to:

```sh
letopis embed --drain </dev/null
```

Drain processes missing vectors sequentially and stops when no eligible work
remains. It reuses current content hashes; attempts persist and are bounded.
Do not loop forever on exhausted work, alter databases, or invent reset commands.
Report provider/configuration problems; do not install a model or change settings
without a task requiring it.

## Current command boundaries and corrections

| Available now | Deferred |
|---|---|
| `search`, knowledge-only `inspect`, `embed --drain` | Full `check`/rename: TK-19 |
| FTS fallback and basic source projection | Source lifecycle/provenance: TK-20/21 |
| Knowledge tree reads through Git | Source filters/traverse/mixed inspect: TK-22/23/24 |
| Safe command errors | `status`/diagnostics: TK-28 |

Never report a successful `check` in this slice. The future check contract covers
markup, dangling links and includes cycles; until implemented, do not claim a
complete validation from search alone. Correct the owning Markdown references
when files/anchors change, then refresh and inspect the affected knowledge.
Invalid files retain their previous valid index projection, while current inspect
may reject them with `validation_failed`: fix the file, not the database. Inspect
an unknown ID rejects the whole requested package; surface that error honestly.
