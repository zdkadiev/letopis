# Independent bounded-search implementation review

Reviewed the 33-path current-task patch against the captured candidate tree, candidate AGENTS.md, bounded-search specification and approved plan. SHA-256 verification matched all 33 candidate after hashes and all root before hashes in `review/changes.json`. Earlier dirty root work was excluded. No code was edited and no build was started by this reviewer.

## Findings

### Critical

None found.

### Important

1. **Update the remaining full-corpus refill test to the new agreed behavior.** `tests/search_filters.rs:271-284` still defines `ineligible_fts_head_cannot_consume_the_final_limit`: with limit 1, an eligible `a.md` containing a source link and a shorter ineligible `b.md`, it expects `a.md`. This is the former eligibility-before-limit contract. The current SQL selection limits the channel before filtering, so its ineligible lexical head can leave an empty result. The untouched test contradicts the new `tests/knowledge_search.rs:37-58` no-refill scenario and is a likely full-suite failure. Update its name, described Given/When/Then and expected result, ideally asserting its lexical-head fixture independently. This is an affected regression test, not a request to broaden product scope. I have not executed this test; the parent already has the full candidate suite running.

### Minor

1. **Remove stale completeness claims in affected comments.** `src/service/search.rs:63` describes complete candidate channels, although the service now receives bounded channels. `tests/storage_cache.rs:815` says retrieval has no candidate limit although the immediately following call supplies 100. These comments preserve the old contract and can mislead future maintenance; update them to describe the selected pool / sufficient fixture bound.

2. **The planned per-channel service propagation test is only partially covered.** `src/service/search.rs:1363-1382` verifies a single FTS-selected observation; `tests/knowledge_search.rs:19-34` checks the final cap for a direct request. There is no new test asserting that an explicit smaller bound reaches both channels, nor a mutation removing vector-bound propagation. A regression replacing the vector argument with 100 could retain the final response bound while violating the per-channel <=L observation budget. Add a focused hybrid test with disjoint lexical/vector candidates and L=5, or a storage recorder asserting both received limits, then hand-mutate the vector propagation. The implementation itself correctly forwards the effective limit at lines 120 and 347.

## Correctness and architecture reviewed

- `src/cli/input.rs:269-270`: positive u64 input is clamped before platform-size conversion; explicit null and invalid numeric forms remain rejected. Parser tests cover omitted, 1, 5, 100, 101, u64::MAX, zero, null, negative, fractional/exponential and overflow forms.
- `src/service/search.rs:98,120-134`: direct requests are capped at 100; both channels and final ranking receive the same effective limit.
- `src/storage/cache.rs:532-547`: FTS matching, ordering and bound execute in SQL after the nodes join, before row parsing.
- `src/storage/vector.rs:639-697`: native K remains total key count and final bound follows current-node joins and distance/URI ordering. Shared hashes cannot expand the returned Rust rows past L; orphans consume no joined slots. This matches the explicitly approved SQLite-internal workspace exception.
- `src/service/search.rs:385-416`: candidate metadata reads are driven only by returned channel IDs; the union is bounded by 2L. Eligibility has no tail/refill path. Duplicate cross-channel IDs can cause two bounded metadata reads but one fresh eligibility observation.
- `tests/storage_vectors.rs:16-87` covers current knowledge, orphan exclusion, shared hashes, URI ties, underfilled pools, and selected versus unselected malformed URI. `tests/knowledge_search.rs:351-388` covers the corresponding lexical malformed-URI boundary.
- Application-owned traits remain in `service/mod.rs`, storage SQL remains in the existing concrete owners, and no technology dependency, async runtime, new wrapper or composition change was introduced.
- Contracts, schema, help and current mechanics describe the new bounded/no-refill behavior; historical proof reports were preserved.

## Evidence and limits

Inspected task1 mutation receipts and all four later mutation logs. Each failed a real test assertion rather than compilation: omitted default, parser clamp, FTS SQL cap, vector SQL cap, direct-service clamp, and premature native K. Candidate hashes match the restored source hashes recorded for the storage/service mutations. The FTS SQL-cap mutation is caught by the fresh-observation test; the command stops after the failing library test, before its storage integration target runs.

At review time `candidate-tests.log` only showed compilation. Full `make verify`, native storage runner, integrated-tree validation, timing arithmetic, SQL plans/oracles and genuine Samply evidence were not yet available and are not certified by this review. The parent will supply those separately; no performance claim follows from the SQL output bound alone.
