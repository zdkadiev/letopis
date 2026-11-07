# Search SQL and read amplification experiment — rejected as the primary optimization

Baseline: `3d820de41ee12385bc734ac4ceeb56b78ba379f9`. The isolated `source/` clone remains clean. No product patch, new API, schema/runtime change, or new release build was produced. No full search timing was run; the coordinator owns whole-search timings. All writes are under this experiment directory.

The scoped evidence supports retaining the existing point-read structure for this optimization round. Two tiny reductions are possible, but neither addresses the 111.853 s full-hybrid baseline: the revision-query owner costs 0.237 sampled CPU s; FTS/vector overlap causes **one** duplicate metadata read for this actual query. Reusing prepared statements may save roughly 1 s of SQL-only work, but is a separate small follow-up after the parser/eligibility bottleneck, not a substitute for that work.

## Inputs and isolation

- Read repository AGENTS.md; storage/cache/vector/query/transaction code; application-owned StorageRead/CacheDatabase contracts; CLI filter/read-view contracts; mechanics MC-2/MC-3 and search rules.
- Fixture: `/Users/zdkadiev/Repos/letopis/.artifacts/issue-65/2026-11-06-native-followup/ollama-100k/repository/.letopis/cache.db`.
- Opened original DBs through SQLite URI `mode=ro` plus `pragma query_only=on`. Copied cache through SQLite backup to `cache-copy.db`; all benchmark SQL uses **only the copy**, opened SQLITE_OPEN_READONLY. The original corpus, Git state and DBs were not modified.
- Direct, read-only fixture counts: 100,000 knowledge nodes; 100,000 source nodes; 500,000 resolved edges; zero unresolved edges; 100,000 vector keys; all 100,000 knowledge hashes covered. `"case" AND "000000"` has exactly **one** FTS match. Counts: [fixture-counts.json](fixture-counts.json).
- Existing native profile source: `2026-11-06-native-followup/ollama-100k/full-native-stacks.json`; sampled total is 111.901117 CPU s. CPU samples overlap and must not be summed as independent costs. These are **existing profile observations**, not new end-to-end measurements.

## Exact amplification in the full-covered hybrid request

Source locations refer to `source/`.

| Work | Baseline count | Safe upper reduction |
| --- | ---: | ---: |
| `CandidateReads::get_knowledge` / `nodes_get_by_id` | FTS 1 + vectors 100,000 = 100,001 | 1 duplicate point read |
| `SearchEligibility::get_source_bindings` / outgoing-edge query | 100,000 | Cannot skip without changing demanded edge validation |
| `project_get_by_id` during eligibility | 100,000 for this unfiltered request | 99,999 with operation-local, first-demand revision reuse |
| vector identity/config capture and checks | constant, not per-candidate | Constant only |
| live file observation and Markdown parse | 100,000 | Outside this SQL experiment |

`get_candidate_metadata` consumes FTS first, verifies each FTS row ID, then consumes every vector and verifies the current metadata search_hash. It populates the union before eligibility and ranking. Search hash/config checks are not accidental overhead and must remain.

`get_eligible_links` iterates IDs in BTreeMap order: edge retrieval/decoding, source and revision filters, revision lookup, live knowledge read/parse, provenance observation, flags, then eligible-set fusion/limit. A revision cached eagerly before this loop would reorder errors; a correct lazy value must be initialized only after the first candidate passes source/revision filters, at the original revision lookup position. Its lifetime must be confined to that pinned operation. Do not cache live bytes, flags, or provenance as part of this change.

`with_read_tx` pins both cache project and attached vector metadata before invoking the operation. Same-owner nested transactions join and only the outer owner finalizes. Identity comparisons before vector retrieval observe the pinned vector view. The public contract still requires source-revision preflight before inference and candidate-dependent empty exits.

## Query plans and scoped cost

The standalone diagnostic executable [native-probe.c](native-probe.c) uses the exact SQLite 3.53.2 amalgamation from pinned `libsqlite3-sys 0.38.2`. It was compiled with `cc -O2 -DSQLITE_ENABLE_FTS5 -DSQLITE_THREADSAFE=1` and that package's header. This is a **synthetic SQL-only probe**, with simplified compile flags, not the product executable. It excludes SeaQuery construction, Rust/domain decode/validation, parsing, Git, provider calls and filesystem work. It materializes SQL columns and executes the baseline query shapes against all 100,000 copied knowledge IDs in a read transaction. Three repetitions per strategy are recorded in [native-probe.tsv](native-probe.tsv) and summarized in [native-probe-summary.json](native-probe-summary.json).

| Query / 100,000 calls | Prepare/finalize each call, median s | Retain/reset one prepared statement, median s | Difference s |
| --- | ---: | ---: | ---: |
| Node by URI | 0.217145 | 0.064811 | 0.152334 |
| Project revision | 0.064561 | 0.013127 | 0.051434 |
| Outgoing declarations | 1.099431 | 0.213720 | 0.885711 |

Every variant returns the independently counted literal row totals (nodes/revision 100,000; outgoing edges 500,000); all report **zero SQLITE_STMTSTATUS_FULLSCAN_STEP**. There is no missing index/full-table-scan defect:

- nodes: unique URI index.
- revision: INTEGER PRIMARY KEY.
- resolved edges: covering URI index -> covering `edges_outgoing` -> target rowid; a small per-owner temp sort for the final two ordering terms.
- unresolved edges: covering URI index -> PRIMARY KEY source lookup.
- UNION ALL merges ordered branches; full query plans are retained in the TSV.

This suggests ~1.089 s of raw SQL preparation savings across these three shapes under this diagnostic setup. It is not a predicted or accepted full-search speedup. The native production profile reports all `sqlite3LockAndPrepare` at 1.741148 CPU s (1.56% of sampled total), an approximate ceiling for eliminating **all** prepare work, including unrelated queries. Keeping the same per-call SeaQuery construction while using `prepare_cached` would not remove the whole outgoing-query owner.

Relevant observed production owners:

| Owner | Inclusive sampled CPU s | Fraction of sampled total |
| --- | ---: | ---: |
| `CandidateReads::get_candidate_metadata` | 0.495577 | 0.443% |
| `StorageRead::nodes_get_by_id` | 0.465977 | 0.416% |
| `StorageRead::project_get_by_id` | 0.237025 | 0.212% |
| `get_edges_by_source_id` | 2.710971 | 2.423% |
| all `sqlite3LockAndPrepare` | 1.741148 | 1.556% |

The source and subset hashes are recorded in [native-profile-subset.json](native-profile-subset.json). These inclusive owners overlap with child SQLite/SeaQuery frames. None is added to another to claim a total.

## Options considered and decision

1. **Lazy operation-local revision reuse:** semantically plausible if first-demand position is preserved and pinned scope is unchanged. Reject for this primary experiment: <0.3% existing sampled owner, ~0.06 s SQL-only constant query cost. No evidence it would move the end-to-end target materially. An eager capture or persistent cache is not an acceptable replacement.
2. **Reuse FTS metadata for overlapping vector IDs:** preserve FTS row checks and vector hash checks using the validated pinned metadata, but actual overlap is one. Reject as immaterial for this fixture. A different all-matching lexical query would need its own evidence.
3. **Prepared-statement cache for hot point reads:** smallest infrastructure-only follow-up; it retains each execution and therefore the normal value/error demand order and active-scope checks. Existing native preparation cost and the scoped probe suggest around one second, not tens of seconds. Defer rather than broadening a parser-focused optimization. A product patch would need tests for refreshed values after writes and across scopes, corrupt second demanded row, statement reset after failure, and schema invalidation; this probe does not establish those guarantees.
4. **Call `nodes_get()` once, or materialize all edges up front:** reject. Decoding unrelated/noncandidate rows can surface malformed storage before the first originally demanded candidate, consumes extra memory, and can alter failure precedence. Bulk error behavior is not equivalent to ordered demand.
5. **Filter outgoing SQL to only external edges:** reject. Current retrieval decodes all declared relations before source extraction. Pushing filtering below it could hide errors in other currently demanded persisted edges. Adding a new public batch/source-only API solely to share a cache is outside this small experiment.
6. **Reduce vectors/eligibility using `limit`, top-k, or oversampling:** reject. Full eligible channels must be formed before recomputed ranks/fusion/limit. No candidate truncation was attempted.

## Reproduction and validation limits

Run `./native-probe cache-copy.db > native-probe.tsv` and `python3 -B summarize-probe.py` in this directory to repeat the scoped diagnostic. The summary validates independent known row totals and zero full scans and pins source, executable and copy SHA-256 hashes.

`git -C source status --short --branch` reports clean detached HEAD. No Rust product code/tests changed, so no product `make verify`, RED/GREEN/mutation claims, or release binary were generated. No full100k whole-search run was performed. SQLite 3.42.0 from system Python could backup/read ordinary tables but cannot read the fixture's contentless-delete FTS schema; the FTS count and query timing were therefore obtained with the pinned 3.53.2 C probe, not attributed to the system engine.
