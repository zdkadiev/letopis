# Bounded Hybrid Search Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Return only SQL-selected search candidates to Rust, with omitted limit 100 and explicit limit capped at 100.

**Architecture:** Keep current synchronous services and SQLite FTS5/vec1. Each channel returns at most the effective limit after SQL matching, joins and deterministic ordering; service freshness and fusion operate only on their bounded union. Preserve existing physical vector storage and identity.

**Tech Stack:** pinned Rust 1.95, rusqlite/SeaQuery, existing bundled SQLite and vec1; no dependency changes.

**Spec:** [bounded search design](../specs/2026-11-07-bounded-search-design.md)

## Global Constraints

- Effective positive limit L is 100 when omitted, otherwise min(requested, 100); zero, null and invalid numeric forms remain invalid.
- Each channel returns at most L knowledge hits; Rust candidate union is at most 2L; final output at most L.
- Matching, vector distances, ordering and candidate limiting execute in SQLite, before Rust result decoding. No unbounded refill.
- Filters/freshness apply within this selected pool; no complete-corpus ranking or tail-error audit guarantee.
- Measured corpus is 100k knowledge / 100k vectors. Preserve existing search_hash storage, including physical sharing supported by current tests; do not migrate it to another ownership model.
- Preserve all root edits and historical evidence; no root commit, push, merge or release is authorized by this plan.

## Review Focus

- Explicit huge u64 limit must clamp before usize conversion; direct service requests also cannot bypass the 100 cap.
- SQL bound is after node joins; shared hashes cannot expand returned rows beyond L.
- Orphan native vectors cannot consume knowledge slots; equal distances retain URI tie order.
- Corrupt metadata outside the SQL-selected pool must not be decoded as candidate metadata; corruption inside it remains an error.
- Restrictive filters may produce fewer than L results; no hidden full-corpus refill restores old latency.

### Task 1: Effective limit and CLI contract

**Files:** src/cli/input.rs, src/cli/help.rs, src/service/types.rs, tests/cli_runtime.rs and inline CLI input tests.

**Interfaces:** retain SearchRequest.limit: NonZeroUsize. Add documented shared MAX_SEARCH_LIMIT=100 in service/types.rs with required reexport; parser defaults only an absent field, validates positive integers, clamps before platform conversion. Service also normalizes direct requests.

- [x] Add Given/When/Then parser tests with literal expectations: omitted→100, 1→1, 5→5, 100→100, 101→100, u64::MAX→100; reject zero/null/negative/fraction/overflow-u64.
- [x] Run targeted parser tests and record expected failures before implementation.
- [x] Implement default/clamp and update help to optional positive limit, default/max100. Do not change traversal limits.
- [x] Run targeted tests and hand-mutate default and clamp separately; require failures and restore exact source.

### Task 2: SQL-limited storage channels

**Files:** src/service/mod.rs, src/storage/cache.rs, src/storage/vector.rs, src/service/utils.rs; storage trait doubles in service/{status,traversal,document_refresher}.rs; existing StorageRead callers under src/ and tests/.

**Interfaces:** append limit: NonZeroUsize to StorageRead::text_index_get_by_expression and ::vector_index_get_by_vector, matching implementations and forwarding doubles. Add the same argument to private get_nearest_vectors and its SQL helper. Existing unrelated test oracles choose an explicit bound sufficient for their fixture.

- [x] Add storage tests with literal ordered IDs: FTS more than L and tied scores; vector L=1 with nearest orphan; more than L equal-distance vectors inserted in reverse URI order; one physically shared vector joining more than L nodes; fewer than L current knowledge rows; selected versus unselected malformed URI.
- [x] Run these tests RED. Keep existing sharing, identity, legacy query-only and exact-distance coverage.
- [x] FTS: bound ORDER BY score,nodes.uri LIMIT with a SQL parameter before row decoding.
- [x] Vector: bind final ORDER BY distance,n.uri LIMIT after joins. Retain native K=count(vector_keys) internally for exact current-node coverage and URI tie handling: native K=L alone can discard eligible nodes or URI winners. SQLite-internal native workspace is distinct from Rust result collections; no full vector/match collection is returned to services. Native heap optimization is outside this change.
- [x] Verify raw SQL directly returns at most L rows, with known literal IDs. Inspect native/SQL plans; do not equate a row cap with bounded internal scanning.
- [x] Run storage tests GREEN; remove each SQL LIMIT in separate hand mutations and require its bound test to fail before restoring.

### Task 3: Bounded service orchestration and public contracts

**Files:** src/service/search.rs; tests/{knowledge_search,search_consistency,search_filters,search_filter_fallback,markdown_search}.rs; docs/zkit/letopis/contracts/{X-1-cli.md,schemas/X-1-cli.yaml}; docs/zkit/letopis/{mechanics,decisions}.md.

**Interfaces:** QueryCandidates::find_vector_candidates receives effective limit and forwards it to storage; SearchService passes the same effective limit to FTS and final ranking. Existing rank fusion and per-selected-candidate observation ordering remain in their owners.

- [x] Add tests recording channel limits and candidate observations for explicit5/default100/direct-service oversized requests; a candidate outside the SQL pool must not trigger provenance errors, while an inside candidate error still aborts.
- [x] Add selective-filter expectations for bounded pools and no refill; preserve pinned views, fallback and requested-revision preflight behavior.
- [x] Run RED, implement effective-limit propagation, then run GREEN and mutate dropped limit propagation to confirm failure.
- [x] Update obsolete full-corpus/beyond-limit tests to assert the newly agreed candidate boundary, retaining independent selected-candidate corruption checks.
- [x] Update current CLI schema/help/mechanics and add a new decision explaining bounded ranking; retain historical proof reports unchanged.

### Task 4: Verification, performance and source-linked findings

**Files:** new evidence under .artifacts/issue-65/bounded-search-<fresh>/ and docs/zkit/letopis/checks/issue-65/bounded-search/; update existing letopis/architecture/search-pipeline.md with verified source basis.

- [x] Capture dirty source and baseline binary hashes; use an isolated local capture for changes and tests, preserving root/index/other worktrees. Integrate only intended reviewed paths after baseline byte checks.
- [x] Run make verify on the actual integrated tree. Run host storage runner into a fresh evidence directory; report Linux and any unavailable checks explicitly.
- [x] Use a separate online-backup copy of the 100k corpus and the already captured baseline release; preserve original fixture and provider identity. Freeze candidate binary/source hashes.
- [x] Screening: one ordinary search each at limits5 and100, plus direct SQL row counts. Compare selected IDs/order/flags to independent SQL candidate oracles; record bounded-vs-full ranking differences as accepted semantic changes, not bit-identical correctness requirements.
- [x] If valid, run three alternating ordinary pairs per limit without owned build/test/drain overlap; preserve every sample, validate <=L channel rows and <=2L observed candidates. Include omitted/above100 CLI smoke calls.
- [x] Record genuine Samply separately from timing, validate process/binary/symbol identity and analyze remaining cost. Do not replace Samply with manual timings.
- [x] Independent final review of architecture, SQL boundary, tests, mutation receipts and measurement arithmetic; fix findings and rerun affected checks.
- [x] Publish factual findings in existing Letopis concept with a verified complete source tree; scoped check, search and inspect/readback. No Codex-memory write, global install, commit or publication.

## Execution status

User approved the plan with go. Native execution completed in an isolated capture,
then guarded integration into root. All implementation tasks, eight hand mutations,
actual-root make verify and the16-command host storage gate passed. Three ordinary
pairs per limit5/100 and the separate genuine Samply recording are validated.
Independent code/method findings were fixed and the final data audit passed.
See [report](../../zkit/letopis/checks/issue-65/bounded-search/report.md) and its
source/method pins. Root remains dirty/uncommitted; earlier work is preserved.

Ruling: retain complete native K internally and limit current-node rows in SQL;
this preserves orphan/tie behavior while bounding Rust collections. Ruling: direct
L5 propagation coverage was treated as a verification requirement and proved by
separate FTS/vector widening mutants. Legacy required-limit/refill tests were
updated to the explicitly approved contract. No additional runtime/dependency or
physical vector ownership change was introduced.
