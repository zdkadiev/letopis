# Storage Query Builder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace ordinary SQL strings with readable SeaQuery composition while preserving storage behavior.

**Architecture:** Keep rusqlite Connection/Transaction ownership and application interfaces unchanged. A private storage/query.rs bridges SeaQuery values to rusqlite and owns typed SQL identifiers; existing responsibility modules compose their own queries.

**Tech Stack:** Rust 1.95.0, sea-query =1.0.2 (only backend-sqlite/derive), rusqlite =0.40.2, bundled libsqlite3-sys =0.38.2 and unchanged static vec1.

**Spec:** [Approved intent and implementation design](../specs/2026-10-22-storage-query-builder-design.md).

## Global Constraints

- Follow AGENTS.md and architecture program-structure/program-structure-review; private builder types remain in storage.
- No async runtime, ORM, pool, public API/schema change, driver downgrade or native source/build change.
- Preserve existing error classification, row decoding, transaction poisoning, affected-row semantics and all TK-9 invariants.
- Use bound values; never use SeaQuery to_string for product query execution.
- Preserve historical evidence. Raw new receipts stay under ignored .artifacts/query-builder/; commit only concise verification.
- Each function aims for 40 lines, maximum 60, without arbitrary wrappers; Arrange/Act/Assert tests.

## Review Focus

1. Quoted/Unicode text must stay parameters; no stored text can change query structure (Task 1).
2. Typed NULL, empty BLOB, bool, signed bounds and overflowing u64 must retain SQLite semantics without panics/wrap (Task 1).
3. Optional filters and attached vec qualification must preserve unfiltered results and snapshot/identity guards (Task 2).
4. Upserts must keep node row_id and attempt budget WHERE guards; a caught SQL error must still poison and roll back the full write unit (Task 2).
5. DDL, FTS scoring/order, vec1 native search/reset and extension registration must survive the refactor unchanged (Task 3).

## Task 1: Introduce a private compatible builder boundary

**Files:** Modify Cargo.toml, Cargo.lock, src/storage/mod.rs; create src/storage/query.rs.

**Interfaces:** Consumes sea_query::QueryStatementBuilder and existing StorageResult/StorageError. Produces private build_query(statement: &impl QueryStatementBuilder) -> StorageResult<(String, Vec<rusqlite::types::Value>)>, plus code-owned identifier enums for tables/columns used by Task 2.

- [ ] Add pinned SeaQuery with only backend-sqlite/derive; resolve/fetch dependencies. Verify Cargo.lock retains exact rusqlite/libsqlite3-sys versions and no new async driver. Do not add sea-query-rusqlite or vendor a patched binder.
- [ ] Add failing private tests query::tests::quoted_unicode_text_remains_bound, query::tests::nullable_and_blob_values_preserve_sqlite_types, query::tests::numeric_values_preserve_bounds_and_reject_overflow. Assert placeholder SQL does not contain payload; round-trip quoted Unicode/NULL/empty and nonempty BLOB/bool/i64 extremes through in-memory rusqlite; reject u64::MAX before execution and preserve typed safe error.
- [ ] Run cargo test --locked --offline storage::query::tests and observe the missing bridge failure before its implementation.
- [ ] Implement build_query plus a focused value conversion helper using build_any(&SqliteQueryBuilder), exhaustive enabled-variant conversion and checked integer conversion. Keep connection execution outside the helper. Add private identifier enums only for actual migrated tables/columns, including attached schema and rowid spelling.
- [ ] Run the binding tests, cargo fmt --all, cargo clippy --locked --offline --all-targets -- -D warnings; verify parameters execute with params_from_iter and no literal interpolation.
- [ ] Review/commit only Task 1 paths with message refactor(storage): add private SeaQuery binding support.

## Task 2: Migrate queries in their existing responsibility files

**Files:** Modify src/storage/{query,graph,files,fts,vectors,read,cache_write}.rs. Extend existing tests/storage_cache.rs and tests/storage_vectors.rs only for uncovered behavior.

**Interfaces:** Consume Task 1 build_query/identifier enums. Preserve all existing function signatures and Storage trait implementations; no builder types escape storage.

- [ ] Add a focused public regression list_edges_keeps_optional_filter_and_order using two distinct sources: None returns all edges in source/target/type order, Some(source) returns only its edges, an absent source returns empty. Add file_text_values_remain_literal covering quote/Unicode in a valid path and safe error string; round-trip/update/delete without affecting a second row. Run these against the original implementation to pin parity, not claim an expected failure for already-correct behavior.
- [ ] Convert graph.rs/files.rs CRUD and subqueries, preserving selected column order, map_mutation, checked integers, ON CONFLICT update lists and RETURNING row_id. Compose optional edge predicates conditionally instead of raw WHERE strings. Run cargo test --locked --offline --test storage_cache.
- [ ] Convert project revision operations in read.rs/cache_write.rs and FTS metadata/replace CRUD in fts.rs. Retain read snapshot establishment before identity capture and the exact revision overflow behavior. Keep native FTS MATCH/bm25 query explicit.
- [ ] Convert ordinary vectors.rs meta/keys/failures/query-error CRUD and joins, including vec schema on attached reads. Preserve atomic reserve_attempt conflict update with attempts<budget, record_failure UPDATE-only, success cleanup and config reset transaction. Keep specialized virtual-table search/reset SQL explicit.
- [ ] Run cargo test --locked --offline --test storage_vectors --test storage_failures --test storage_cache. Rerun storage::sqlite tests for late-reset SQL errors, successful cleanup and poisoned-unit rollback. Fix failures without changing contracts.
- [ ] Review every product SQL occurrence against the migration boundary; no ordinary CRUD should stay raw just to avoid composing a supported statement. Do not hide SQL inside custom expressions except named native-search operations. Keep functions <=60 lines and imports explicit.
- [ ] Commit only migrated code/tests with message refactor(storage): compose ordinary queries with SeaQuery.

## Task 3: Verify behavior, document the decision and summarize evidence

**Files:** Modify AGENTS.md, docs/development.md, docs/zkit/letopis/architecture.md and decisions.md; update this plan. No edits to historical TK-9 receipts or canonical schema SQL.

**Interfaces:** Existing ./scripts/check-rust.sh and tests/storage/run.py; no new runner interface.

- [ ] Run ./scripts/check-rust.sh and RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps; retain full logs outside Git. Run /opt/homebrew/bin/python3 -B -m unittest discover -s tests/storage -p test_runner.py -v and /opt/homebrew/bin/python3 -B tests/storage/check_schema.py.
- [ ] Run /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/query-builder/host and the same command with --platform linux --evidence .artifacts/query-builder/linux. Linux runtime stays network-isolated; verify native/standalone/linkage/FTS/schema checks and actual counts. Do not rerun historical proof harnesses or unrelated negative controls for unchanged native build code.
- [ ] Review module visibility, imports, lib.rs wiring, unchanged driver/native hashes, and parameter binding for every migrated operation. Use existing regressions to confirm stable row IDs, dangling edges, snapshots, attempt budgets, query error persistence and caught-error rollback. Inspect actual queries and results, not only rendered SQL snapshots.
- [ ] Record an accepted follow-up decision: fluent SeaQuery for ordinary storage queries, rusqlite execution, typed identifiers and private checked binding helper; list deliberate SQLite-specific raw SQL exceptions. Update architecture/development/AGENTS consistently without claiming compile-time database schema validation.
- [ ] Check affected Markdown links and git diff --check; mark this plan's steps with actual results/platform limits and retain compact verification here. Record changes and verification in existing Engram store; keep TK-9 historical results pinned to their old source.
- [ ] Commit the intended documentation paths. Report branch/worktree and verification; integration is a separate user decision.

## Self-review and execution handoff

All accepted requirements have an owner above. Tasks share one private helper,
so native execution is recommended: three cohesive tasks, no parallel writers.
No new public API, connection abstraction or generic repository is planned.
Fresh independent builder check: nothing important left. Self-review confirmed
coverage, exact existing storage_failures test target, helper type consistency,
all five review-focus cases and plan/spec proportion; six local links resolve.

Pending: user review of this concrete plan before implementation, as required by
the writing-plans workflow. Product code and dependencies remain unchanged.
