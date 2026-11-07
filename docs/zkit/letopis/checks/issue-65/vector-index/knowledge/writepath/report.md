# Write-path finding published

Added four source-based sentences to the existing vec1 findings section in
`letopis/architecture/storage.md`: singleton insert rewrites a whole small packed
block, 16 KiB reuse threshold differs from approximately 1 MiB rebuilt blocks,
incremental BLOB writes serve delete tombstones, and rebuilt-layout read timing
does not establish fresh singleton-drain layout, write/WAL cost or model-drain
throughput. No new concept file or element; existing created_at preserved.

Reviewed the finalized native `write-path-note.md` and reused the already-declared
pinned `vendor/vec1/vec1.c` source binding. All 19 source bytes were recaptured
through a separate temporary index and compared; the unchanged source set gives
the same verified author basis `846d0684054c989a798b06b46ddbfbabcd99c8e9`.
The user's Git index was untouched; no production sources changed.

Compatible installed Letopis0.2.0 scoped check returned exit0 and `errors:[]`.
Inspect of the knowledge element and native C returned exit0 and retained tree
`d99064e7c7a1e063c62fe3362998d236f9659df0`. Read both paths from that one tree,
verified returned hashes and current bytes, and compared all 19 declared source
bindings to author-basis hashes. `git diff --check` passed.

Exact requests/responses: `check.json`, `inspect.json`; source hashes: `basis.json`;
same-tree readback and process observation: `readback.json`. No search/provider
query, build, benchmark or timing was run. Final process-identity observation
found no actual `letopis embed --drain` process; no process was killed. This task
has no continuing CLI or owned CPU/background work.
