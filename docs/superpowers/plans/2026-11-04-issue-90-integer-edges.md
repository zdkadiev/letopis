# Issue #90 Integer Edge References Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist resolved edges through integer node foreign keys using nodes-first publication, preserving public addresses and missing-target diagnostics.

**Architecture:** Keep the existing cache transaction and storage/service boundaries. Storage persists resolved edges and unresolved declarations; document refresh publishes nodes before changed owners' edges. Readers expose textual identities, diagnostics see all declarations, and bounded traversal sees resolved edges only.

**Tech Stack:** Existing pinned synchronous Rust, SeaQuery, rusqlite and bundled SQLite/FTS5/vec1; no new dependencies.

**Spec:** [Revised #90 specification](../specs/2026-11-04-issue-90-node-identifiers-design.md).

**Status:** Complete. Revised specification and follow-up plan approved; Tasks 1–4 passed independent specification and quality gates. Whole-branch review returned With fixes for I1/M1/M2; one consolidated correction at `c1f2810`, fresh host/Linux acceptance, exact input reconciliation and clean scoped rereview completed. Final code/evidence readiness is approved; see the [integer-edge report](2026-11-04-issue-90-integer-edges-report.md). Original PASS receipts retain their pre-review-fix scope. Primary checkout and `rafactor` remain untouched.

## Global Constraints

- `edges.source_id` and `edges.target_id` are integer foreign keys to `nodes(id)`. Public IDs, URI sorting and cursor semantics remain textual.
- `unresolved_edges(source_id, target_uri, type)` retains absent-target declarations; its source is an integer foreign key. No placeholder nodes or duplicate logical declarations across tables.
- Cache schema becomes **4**, rejecting prior layouts intact; vector schema **1** and its DDL remain unchanged. Preserve vectors by `search_hash`, never cache integer ID.
- One transaction covers both passes, source reconciliation, FTS, unresolved-link reconciliation, files and one revision increment. Any failed mutation poisons publication.
- Existing low-level storage clients may insert declarations before targets. Commit resolves targets that appeared later; missing targets remain reportable.
- Invalid Markdown retains its last-valid projection. Missing local source bytes keep existing source-row/null-hash behavior. Includes and arbitrary relation names retain current rules.
- Preserve URI paging with index streaming or a proven page-bounded SQLite top-K working set (at most 32 records, no OFFSET), never a degree-sized sort. Disclose possible adjacency scan cost; do not claim original index-only performance.
- Keep one-file captured/search text lifetime; reuse effective graph metadata and retain only changed owner IDs. Do not rebuild every unchanged edge or load graph data inside bounded neighbor reads.
- Preserve explicit boxed transaction interfaces and current wiring. Use SeaQuery and bound values for ordinary queries, lowercase new executable SQL, named constants, documented APIs and Given/When/Then tests.
- Freeze historical fixtures/evidence. New evidence goes to new directories; no live user cache deletion or real/paid provider calls.
- Existing Subagent-driven choice carries forward. Each task has implementation/spec/quality gates; independent final review follows final verification. No merge, push, PR update or issue closure is part of these implementation steps without authorization.

## Review Focus

1. Link owner sorts before a newly created target or external source: one publication must resolve it independently of file order (Task 2).
2. An unchanged owner points at a deleted/recreated target: preserve the diagnostic and restore traversal using the new integer ID (Tasks 1–2).
3. A pending declaration's target is inserted before a duplicate declaration: duplicate detection must span both tables and poison the unit (Task 1).
4. Invalid owner Markdown retains old declarations; editing/deleting a valid owner clears both resolved edges and pending findings (Task 2).
5. A schema-3 cache already exists with useful vector state: reject it untouched and prove cache-only rebuild keeps vectors with different numeric keys (Task 3).

## File responsibilities

- `src/storage/cache.sql`, `schema.rs`, `query.rs`, `mod.rs`: schema 4, typed unresolved-table identifiers and the private persistence module declaration.
- Create `src/storage/edges.rs`: resolved/unresolved declaration writes, ordered reads, demotion and promotion. Extract existing edge operations from `graph.rs`; keep node encoding/decoding there. This is a concrete persistence responsibility, not a new public component.
- `src/storage/graph.rs`, `cache_write.rs`, `read.rs`: delegate edge operations, preserve node deletion semantics and reconcile unresolved targets before commit. Keep error poisoning in `SqliteCacheWrite`.
- `src/storage/neighbors.rs`: integer joins, textual decoded endpoints and URI cursor/order.
- `src/service/document_refresher.rs`: keep per-file preparation and node/text publication; defer changed owners' edges until sources/nodes are ready.
- `src/service/mod.rs`: clarify atomic transaction and declaration lifecycle contracts; no new public trait or domain type.
- `tests/storage_node_identity.rs`, `tests/storage_cache.rs`, `tests/node_identity_lifecycle.rs`, `tests/graph_read.rs`: direct SQLite and public storage lifecycle coverage.
- Create `tests/integer_edge_publication.rs`: actual refresh/check/status/traverse scenarios using existing temporary repository fixtures.
- `tests/storage_cache_upgrade.rs`, create `tests/fixtures/storage/cache-v3.sql`: prior schema-3 rejection and vector-preserving rebuild; keep cache-v2 and legacy fixtures unchanged.
- Active docs/schema/knowledge and a new `docs/superpowers/plans/2026-11-04-issue-90-integer-edges-report.md`: current design, upgrade procedure, results and limits. Preserve the original report as historical evidence.

## Task 1: Integer edge persistence and logical declaration lifecycle

**Interfaces:** Preserve `CacheWrite::{upsert_node, insert_edge, delete_outgoing, delete_node, commit}` and `StorageRead::get_edges_by_source_id`. Private `edges.rs` functions consume `&Connection` and existing domain values, returning `StorageResult`; use `insert_edge`, `get_edges_by_source_id`, `delete_outgoing`, `demote_incoming_edges(db: &Connection, target_id: i64, target_uri: &str) -> StorageResult<()>` and `resolve_edges(db: &Connection) -> StorageResult<()>`. Keep source lookup/duplicate validation and typed errors within storage. No builder type escapes storage.

- [x] Freeze the current `src/storage/cache.sql` as `tests/fixtures/storage/cache-v3.sql`, recording its SHA-256 and captured revision. Preserve schema-1/2 fixtures byte-for-byte.
- [x] Write `resolved_edges_reference_integer_node_keys` in `storage_node_identity`: PRAGMA column types/FK targets, independent literal integer edge values and rejected absent numeric endpoints; assert `pragma foreign_key_check` is empty.
- [x] Add direct storage tests for edge-before-target then commit, absent target with no fake node, cross-table duplicate after target UPSERT, target deletion/reappearance, deletion blocked by pending outgoing declarations, and owner cleanup. Compare ordered public triples and raw rows independently.
- [x] Run `cargo test --locked --offline --test storage_node_identity --test storage_cache --test node_identity_lifecycle`; confirm new assertions fail on the current textual-edge schema.
- [x] Implement schema 4, unresolved table/index and the persistence module. Extract the existing edge functions from `graph.rs`. Keep insert duplicate/missing-source failures typed `InvalidRequest` and declaration reads deterministic by source URI, target URI, type.
- [x] Implement node deletion preflight before demotion/FTS deletion: outgoing declarations in either table reject deletion; incoming edges demote before restrictive target FK deletion. `delete_outgoing` clears both representations. A nonexistent node remains a no-op.
- [x] Reconcile resolvable declarations before revision update in `commit`, using set-based bound queries without copying all declarations into Rust. Keep duplicates impossible across representations. A reconciliation failure returns an error and drops/rolls back the transaction.
- [x] Update bounded neighbor joins to integer keys but select endpoint URIs and sort/page by `(type, neighbor URI)`. Test incoming/outgoing pages with numeric allocation opposite lexical URI order; unresolved edges never enter pages.
- [x] Replace the old blanket no-`TEMP B-TREE` neighbor-plan assertion with actual native query-plan/VM evidence for index streaming or LIMIT-controlled top-K retention. Exercise every existing direction/mode/filter/cursor combination and a high-degree node. Reject unbounded sorters, automatic indexes and other degree-sized materialization; verify the top-K collection is capped at the requested limit (at most 32 retained records, plus constant working-record overhead; zero OFFSET). Use the pinned bundled SQLite limit/delete/insert control flow as the oracle for the emitted bytecode, not just presence of a LIMIT clause. Keep the source-snapshot query's existing no-sort assertion. If the bound cannot be established, stop and revise the plan instead of weakening the memory contract.
- [x] Update affected current-schema raw fixtures/oracles across source/tests/examples, including direct edge DML. Preserve historical fixtures. Run the focused tests and relevant existing neighbor target.
- [x] Mutation-check integer/FK assertions, cross-table duplicate detection, deletion/reappearance and removal of the bounded SQL limit; restore each mutation and rerun the focused suite.
- [x] Commit intended persistence/tests paths after spec and quality review.

## Task 2: Publish nodes before edges and preserve user diagnostics

**Interfaces:** Keep `Refresher`, `DocumentPublication`, `PreparedProjection` and `CacheWrite` boundaries. `EffectiveGraph` tracks changed owner IDs alongside its existing metadata/edges; per-file `publish` no longer inserts edges. Add `DocumentPublication::publish_edges(&self, graph: &EffectiveGraph, unit: &mut dyn CacheWrite) -> DocumentResult<()>` after source reconciliation, followed by existing validation/commit. Transaction handles remain explicit operation capabilities.

- [x] Add `new_targets_resolve_independently_of_file_order` covering reindex and incremental refresh: `a.md` points to newly introduced `z.md#target`; include local and opaque HTTP source declarations. Literal assertions check integer raw endpoints, public URI edges and absence of dangling findings after one successful refresh.
- [x] Add `unchanged_owner_tracks_target_deletion_and_return`: source bytes unchanged, target missing/created/deleted/recreated. Assert exact `check` and observation-only `status` problem details, no nonexistent node, traversal visibility, distinct recreated numeric ID and no stale endpoint.
- [x] Add valid-owner edit/delete cleanup and invalid-owner last-valid cases. Test that adding a target alone resolves retained declarations without reparsing the unchanged owner.
- [x] Add transaction fault regressions after nodes, demotion and promotion, using existing private test checkpoints/injection patterns. A new read sees old revision/files/FTS/node keys/declarations; `foreign_key_check` remains clean.
- [x] Add an inline refresher test with a forwarding transaction observer that records node/edge operations. For targets known to exist in the final fixture, assert every node/source UPSERT precedes every `insert_edge`; include a genuinely absent target to ensure it remains a diagnostic rather than failing the publication. This must fail before the ordering change, even though Task 1 promotion already makes final-state integration tests pass.
- [x] Run `cargo test --locked --offline --test integer_edge_publication` and the new inline ordering test plus existing refresher/diagnostics tests. Record the ordering test's expected red result before orchestration changes; do not claim already-passing final-state tests prove a missing second pass.
- [x] Remove edge insertion from per-file projection publication. Track changed owners through effective graph application; after all node/removal/source operations, insert only those owners' final edges and commit. Do not retain all files' search text or reinsert unaffected owners' edges.
- [x] Clarify service contracts and run new tests plus existing check/status/traversal/source lifecycle suites. Ensure unrelated error formatting and source availability behavior remain unchanged.
- [x] Mutation-check second-pass ordering, unchanged-owner promotion, cleanup and rollback assertions; restore mutations and rerun affected tests.
- [x] Commit intended orchestration/tests paths after spec and quality review.

## Task 3: Prior-cache compatibility and persistent vector safety

**Interfaces:** Preserve public `open_storage`, status and reindex entry points. Schema 4 recognition must reject cache schemas 1, 2 and 3 before vector access. Operator rebuild remains explicit and quiescent; tests use only temporary databases.

- [x] Extend frozen-cache rejection tests with the exact schema-3 fixture, both absent and populated vectors, and direct open/CLI status/reindex. Assert existing closed bytes/inodes and vector state remain unchanged per entry point, with no vector-file creation when absent. Explicitly check the existing read-only status WAL-reader artifacts (empty cache WAL and shared-memory index); do not hide them through a fixture journal-mode change or unsafe cleanup.
- [x] Update malformed-current-schema cases to independently break either endpoint foreign key/type or unresolved table constraints. Exact schema recognition rejects each before vector access.
- [x] Extend cache-only rebuild oracle to schema 4: existing vectors/failures/config/query-error sentinels survive; cache/node and vector keys intentionally differ; literal float bytes and `search_hash` membership match, and no fake-provider inference occurs.
- [x] Run `cargo test --locked --offline --test storage_cache_upgrade`; confirm new prior-schema/DDL expectations fail before fixture adaptations, then implement the minimum required fixture/test changes without modifying frozen history.
- [x] Mutation-check schema-3 rejection and vector preservation; restore changes, rerun focused tests and `python3 -B tests/storage/check_schema.py .` with prepared Python 3.11+.
- [x] Commit intended upgrade/fixture paths after spec and quality review.

## Task 4: Active contracts, platform acceptance and final review

**Interfaces:** No new product interface. Final receipts describe the exact revised tree; original report/receipts retain their original scope.

- [x] Update executable/design schema parity, mechanics MC-2, D-108, I-8 and development upgrade guidance for schema 4 and both integer endpoint references. Preserve old historical report links with an explanatory historical heading/anchor where needed.
- [x] Update affected source-linked projection/search knowledge with verified exact source bytes and retain separate snapshots for original evidence. Check affected local links/anchors.
- [x] Run `make verify` via the required host acceptance runner, writing `.artifacts/issue-90/integer-edges-host-<new timestamp>`, and Linux runner to `.artifacts/issue-90/integer-edges-linux-<new timestamp>`. Do not duplicate root runs already included by successful runners. Use `/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host|linux --evidence <new directory>` and isolated build/evidence paths.
- [x] Correct the native-run-discovered malformed-argv test-helper BrokenPipe race after deterministic reproduction: only tolerate closed-stdin BrokenPipe, preserve other errors and response assertions, then obtain new native receipts for the corrected helper. Product parsing remains unchanged.
- [x] Check receipt input hashes against the actual final tree, native/standalone/linkage results and every command exit. Record exact test results, ignored tests, platform limitations and restored mutation outcomes in the new report. Preserve vectors DDL and frozen fixture hashes.
- [x] Request independent final review of the follow-up range and report, covering FKs, two-pass ordering, retained diagnostics, target/owner lifecycle, lexical paging, transaction rollback, upgrade/vector safety and AGENTS/module boundaries. Resolve findings and rerun affected checks.
- [x] Commit only intended docs/report changes; report readiness for the existing PR without claiming it is updated, merged or reverified beyond captured evidence.

## Plan self-review

The identity contract maps to Task 1; publication and visible diagnostics to Task 2;
old-cache/vector guarantees to Task 3; active docs and native acceptance to Task 4.
The five Review Focus cases each have explicit regressions. Public signatures stay
unchanged; private reconciliation is owned by storage and document scheduling by
the existing service. No extra service, runtime, shared ownership or full-corpus
text buffer is introduced.

## Design review

A fresh builder check found one material query-plan conflict: numeric endpoint
indexes do not directly satisfy URI ordering. The revised bounded top-K decision
above resolves it, conditional on native VM proof and the explicit stop condition.
The reviewer found no other important gaps in duplicate detection, source/target
lifecycle, unchanged-owner reconciliation, last-valid projections or rollback.
This is design review, not implementation or platform acceptance.

## Final-review correction closeout

- I1: exact read-only cache recognition before writable opening for direct and
  confined paths; existing concurrent bootstrap and writable revalidation remain.
- M1: main/WAL bytes and identities, vectors/config/locks preserved; all refusal
  entries may create empty reader sidecars or update existing SHM reader marks.
- M2: MC-1 refers to storage promotion/demotion from MC-2; unchanged owners are
  not reparsed/reinserted.
- Four journal regression tests cover 24 independent entry/state/vector cases;
  preflight and WAL visibility mutations fail behaviorally and are restored.
- Fresh actual-tree host/Linux acceptance and input reconciliation passed: per
  debug/release profile host 1052/0/11 and Linux 1046/0/11 passed/failed/ignored.
  Scoped independent rereview approved I1/M1/M2 closure and final readiness.
- Original fixtures/evidence/source bases remain historical. All four execution
  rulings, reasons and costs are recorded in the public report closeout.

- Task 1–4 review reports each confirm specification compliance and quality
  approval. The original whole-branch **With fixes** verdict remains historical;
  the single `5872c9e..c1f2810` correction received a clean scoped rereview with
  no new breakage and no out-of-scope observations. No publication was performed.
