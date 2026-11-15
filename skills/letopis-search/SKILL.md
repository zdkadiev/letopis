---
name: letopis-search
description: Use when a user asks to find information or answer a question from a project's Letopis knowledge base, including requests beginning with /search.
---

# Search Letopis knowledge

Treat the text after `/search` as the question to answer. Ask for a query if it is empty. Work from the project's Git root and use its documented knowledge directory if it specifies one. The default is `<repo-root>/letopis/`.

## Search the index

- Use `letopis search` from the Git root when the CLI and a published index are available. If index availability is unclear, `letopis status` with `{}` on stdin reports `result.revision`, which is `null` before publication. Search, traverse, and inspect refresh the index from Markdown as part of the lookup.
- Try the user's wording or its distinguishing terms first. If results are weak, try a few shorter variants with exact identifiers, synonyms, or terminology found in the corpus. Full-text search requires every query token, so try variants individually. Preserve the user's original question when interpreting results.
- Serialize requests as JSON on stdin. Require exit 0 and `ok: true`. For a question about vector scoring, start with a query such as `vector ranking`.

  ```sh
  jq -cn --arg query 'vector ranking' '{query:$query,limit:10}' | letopis search
  ```

- Use optional `filters.needs_recheck` to select links with flags (`true`) or without them (`false`). Use `filters.sources` for exact source addresses and `filters.source_revisions` for complete Git tree OIDs. Search returns a bounded set. Try more targeted queries or inspect the Markdown when results are sparse.
- Use result paths and line ranges to open passages. When a hit names related concepts or evidence, use its exact `element_id` in `traverse`. Choose `relations`, `containment`, or `both` and set the direction to `outgoing`, `incoming`, or `both`. Set `edge_limit` to include the returned edge triples. Starting IDs are excluded from the returned elements. Narrow with `edge_types` when the relationship is known.

```sh
printf '%s\n' '{"element_ids":["letopis/cache/cache-key.md"],"mode":"both","direction":"both","depth":1,"limit":10,"edge_limit":20}' | letopis traverse
```

## Read snapshot evidence

Search, traverse, and inspect return metadata. Read the underlying passages before citing them. Request relevant knowledge and declared local sources together with `inspect` to read them from one Git tree.

```sh
printf '%s\n' '{"element_ids":["letopis/cache/cache-key.md","src/cache.rs#L2-L4"]}' | letopis inspect
```

`result.snapshot` is a retained Git tree OID. `result.elements` is keyed by normalized IDs and provides `path`, optional inclusive `lines`, and a SHA-256 `hash` or `null`. Use `git show "${snapshot}:${element_path}"` with the same returned tree for every path. Select the returned ranges and preserve exact bytes and line endings when checking hashes. Use live reads for discovery and the returned tree when consistency across files matters. The author-declared [source basis](../letopis/SKILL.md#record-an-honest-source-basis) remains separate from the reading tree.

Treat a local `hash:null` as unavailable bytes or an unavailable range. HTTP(S) sources remain opaque to Letopis. An unknown ID fails the whole batch with `not_found`, including a removed element or a source whose last `external` declaration was removed. Retry with confirmed IDs to recover the available evidence and report what could not be read. For search or traversal flags such as `source_changed`, verify the relevant source before presenting the claim as current.

## Search Markdown directly

If the CLI is unavailable, the index is unpublished, or CLI results are insufficient, search filenames and headings in the knowledge directory and use `rg` on `*.md` and `*.markdown` with the same query variants. Read matching sections and follow relevant local links, including plain Markdown links. Use this route without initializing or rebuilding the index. If an expected marked file is missing from the index, check Git eligibility and [Letopis discovery settings](../letopis/references/operations.md#discovery-settings). If the directory is absent or the search yields no support, report the gap.

Answer the original question with links to the supporting passages, using file paths and line numbers or section anchors. Distinguish documented claims from inferences and identify material gaps or conflicts.
