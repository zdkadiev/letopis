# Vector findings recorded in Letopis knowledge

Updated existing coherent owner and unchanged element ID:
`letopis/architecture/storage.md`. Preserved `created_at:1793727646`; no duplicate
report-bucket knowledge file or element was introduced. Added mode semantics,
full-K ANN limits, authoritative flat payload, proposed guarded populated-database
migration, native-only measurement scope and corrected raw-typed corruption findings.
The text explicitly says flat is an investigated candidate, not integrated product
state or accepted whole-CLI performance. No product/configuration edits were made.

Author source basis: `96680e54c8a39e774291ea0c471296cf1b6ad33f`. Created a complete
tree from HEAD through a separate temporary index, force-adding only the three
precisely linked ignored native experiment evidence files. The real Git index
was not modified. All 18 external source files were byte-compared against that
tree before recording the basis; exact hashes are in `basis.json`.

The initially requested experimental binary
`.artifacts/issue-65/search-optimization-2026-11-06/product/letopis` returned exit1
`internal_error: cannot open embedding service` for filtered search, scoped check
and existing-ID inspect, before publication. Coordinator's read-only diagnosis:
root cache schema v2, experimental product expects v4. No cache reset/migration,
identity change, configuration change or binary installation was attempted.

Used the existing compatible `/opt/homebrew/bin/letopis`, version `0.2.0`, after
checking actual help. Binary resolved path and SHA-256 are in
`captured-readback.json`. This installed-version check verifies the root knowledge
publication workflow; it is not acceptance of current product-source changes.

Final commands, all from repository root:

```sh
printf '%s\n' '{"paths":["letopis/architecture/storage.md"]}' | /opt/homebrew/bin/letopis check
printf '%s\n' '{"query":"vec1","limit":5,"filters":{"sources":["vendor/vec1/vec1.c"]}}' | /opt/homebrew/bin/letopis search
```

Both exited0/ok:true. Scoped check returned `errors:[]`; filtered search found
exactly `letopis/architecture/storage.md`, lines1–142, revision96, flags empty.
The check scopes findings and skips exhaustive source-closure audit; ordinary
refresh remains global. No full corpus audit is claimed.

One inspect request addressed the knowledge element, `vendor/vec1/vec1.c`, and
the native evidence `report.md`, `summary.json`, `control-v2-runs.json` under
`.artifacts/issue-65/vector-index-2026-11-07/native-probe/`. Exact request/response
and exit0 are preserved in `inspect-installed.json`. It returned retained snapshot
`d17b76446f0dcc21e910c1b6167823405d10e1b9` with non-null hashes for every item.

Read both knowledge and sources with `git show <that-snapshot>:<path>` from that
same immutable tree. Compared addressed bytes to all inspect hashes and current
files; additionally compared every one of the 18 declared sources in this
reading snapshot to the verified author-basis hashes. All matched. Detailed
receipt: `captured-readback.json`.

`git diff --check -- letopis/architecture/storage.md` passed. Product builds,
`make verify`, storage runner and whole-CLI performance experiments were not run
for this Markdown-only task. Future integration state and whole-command timings
remain the coordinator's follow-up.
