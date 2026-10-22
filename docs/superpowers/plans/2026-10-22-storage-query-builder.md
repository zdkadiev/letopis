# Storage Query Builder Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

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

- [x] Add pinned SeaQuery with only backend-sqlite/derive; resolve/fetch dependencies. Verify Cargo.lock retains exact rusqlite/libsqlite3-sys versions and no new async driver. Do not add sea-query-rusqlite or vendor a patched binder.
- [x] Add failing private tests query::tests::quoted_unicode_text_remains_bound, query::tests::nullable_and_blob_values_preserve_sqlite_types, query::tests::numeric_values_preserve_bounds_and_reject_overflow. Assert placeholder SQL does not contain payload; round-trip quoted Unicode/NULL/empty and nonempty BLOB/bool/i64 extremes through in-memory rusqlite; reject u64::MAX before execution and preserve typed safe error.
- [x] Run cargo test --locked --offline storage::query::tests and observe the missing bridge failure before its implementation.
- [x] Implement build_query plus a focused value conversion helper using build_any(&SqliteQueryBuilder), exhaustive enabled-variant conversion and checked integer conversion. Keep connection execution outside the helper. Add private identifier enums only for actual migrated tables/columns, including attached schema and rowid spelling.
- [x] Run the binding tests, cargo fmt --all, cargo clippy --locked --offline --all-targets -- -D warnings; verify parameters execute with params_from_iter and no literal interpolation.
- [x] Review/commit only Task 1 paths with message refactor(storage): add private SeaQuery binding support.

## Task 2: Migrate queries in their existing responsibility files

**Files:** Modify src/storage/{query,graph,files,fts,vectors,read,cache_write}.rs. Extend existing tests/storage_cache.rs and tests/storage_vectors.rs only for uncovered behavior.

**Interfaces:** Consume Task 1 build_query/identifier enums. Preserve all existing function signatures and Storage trait implementations; no builder types escape storage.

- [x] Add a focused public regression list_edges_keeps_optional_filter_and_order using two distinct sources: None returns all edges in source/target/type order, Some(source) returns only its edges, an absent source returns empty. Add file_text_values_remain_literal covering quote/Unicode in a valid path and safe error string; round-trip/update/delete without affecting a second row. Run these against the original implementation to pin parity, not claim an expected failure for already-correct behavior.
- [x] Convert graph.rs/files.rs CRUD and subqueries, preserving selected column order, map_mutation, checked integers, ON CONFLICT update lists and RETURNING row_id. Compose optional edge predicates conditionally instead of raw WHERE strings. Run cargo test --locked --offline --test storage_cache.
- [x] Convert project revision operations in read.rs/cache_write.rs and FTS metadata/replace CRUD in fts.rs. Retain read snapshot establishment before identity capture and the exact revision overflow behavior. Keep native FTS MATCH/bm25 query explicit.
- [x] Convert ordinary vectors.rs meta/keys/failures/query-error CRUD and joins, including vec schema on attached reads. Preserve atomic reserve_attempt conflict update with attempts<budget, record_failure UPDATE-only, success cleanup and config reset transaction. Keep specialized virtual-table search/reset SQL explicit.
- [x] Run cargo test --locked --offline --test storage_vectors --test storage_failures --test storage_cache. Rerun storage::sqlite tests for late-reset SQL errors, successful cleanup and poisoned-unit rollback. Fix failures without changing contracts.
- [x] Review every product SQL occurrence against the migration boundary; no ordinary CRUD should stay raw just to avoid composing a supported statement. Do not hide SQL inside custom expressions except named native-search operations. Keep functions <=60 lines and imports explicit.
- [x] Commit only migrated code/tests with message refactor(storage): compose ordinary queries with SeaQuery.

## Task 3: Verify behavior, document the decision and summarize evidence

**Files:** Modify AGENTS.md, docs/development.md, docs/zkit/letopis/architecture.md and decisions.md; update this plan. No edits to historical TK-9 receipts or canonical schema SQL.

**Interfaces:** Existing ./scripts/check-rust.sh and tests/storage/run.py; no new runner interface.

- [x] Run ./scripts/check-rust.sh and RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps; retain full logs outside Git. Run /opt/homebrew/bin/python3 -B -m unittest discover -s tests/storage -p test_runner.py -v and /opt/homebrew/bin/python3 -B tests/storage/check_schema.py.
- [x] Run /opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/query-builder/host and the same command with --platform linux --evidence .artifacts/query-builder/linux. Linux runtime stays network-isolated; verify native/standalone/linkage/FTS/schema checks and actual counts. Do not rerun historical proof harnesses or unrelated negative controls for unchanged native build code.
- [x] Review module visibility, imports, lib.rs wiring, unchanged driver/native hashes, and parameter binding for every migrated operation. Use existing regressions to confirm stable row IDs, dangling edges, snapshots, attempt budgets, query error persistence and caught-error rollback. Inspect actual queries and results, not only rendered SQL snapshots.
- [x] Record an accepted follow-up decision: fluent SeaQuery for ordinary storage queries, rusqlite execution, typed identifiers and private checked binding helper; list deliberate SQLite-specific raw SQL exceptions. Update architecture/development/AGENTS consistently without claiming compile-time database schema validation.
- [x] Check affected Markdown links and git diff --check; mark this plan's steps with actual results/platform limits and retain compact verification here. Record changes and verification in existing Engram store; keep TK-9 historical results pinned to their old source.
- [x] Commit the intended documentation paths. Report branch/worktree and verification; integration is a separate user decision.

## Self-review and execution handoff

All accepted requirements have an owner above. Tasks share one private helper,
so native execution is recommended: three cohesive tasks, no parallel writers.
No new public API, connection abstraction or generic repository is planned.
Fresh independent builder check: nothing important left. Self-review confirmed
coverage, exact existing storage_failures test target, helper type consistency,
all five review-focus cases and plan/spec proportion; six local links resolve.

Execution authorized by the user's go on after this handoff; native execution.
Tasks 1–3 implemented. Final independent branch review completed; no product correctness or architecture findings.

## Verification and implementation record

- Baseline: main360bbf7, 106 Rust tests. Product source after refactor: 8f19ba9529802c84182fc9b96b5d52c17b132985.
- Task 1: tests added before build_query; missing helper rejected (E0432). All four binding tests then passed, covering quoted Unicode, NULLs, BLOBs, bool/enum text and numeric bounds/overflow. Clippy passed.
- Task 2: two public parity regressions passed against pre-migration queries. After migration:20 cache+7 failure+8 vector tests passed; full root suite112 passed. Existing SQL reset-failure and poisoned-unit tests ran as part of the root suite.
- Host: `/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/query-builder/host` PASS, macOS ARM64. Includes `./scripts/check-rust.sh` (112 Rust tests), strict Rustdoc,8 Python runner tests, schema parity, native capabilities, copied executable and linkage.
- Linux: same runner with `--platform linux --evidence .artifacts/query-builder/linux` PASS, source-built Linux glibc ARM64. Runtime uses `--network none`;112 Rust+8 Python tests and all corresponding native/strict/standalone checks passed.
- All15 host and13 Linux inner command exits are0; the Linux outer manifest separately records8 successful orchestration commands. All78 shared source-input SHA-256 values match. Raw manifests/logs remain ignored locally; no new generated evidence tree is committed.
- Existing locked package/version pairs retained, including rusqlite0.40.2 and libsqlite3-sys0.38.2. No ORM/async runtime added. Native C, build.rs, connection/bootstrap SQL, public domain/service API and lib.rs are unchanged.
- Raw SQL exceptions audited: schemas/introspection/connection/native probes and formatted FTS/vec1 native queries/commands. Ordinary CRUD uses the builder. Query construction and its identifier/binding helper are private to storage.
- Historical TK-9 evidence unchanged. Not claimed: physical power loss, x86_64, resource/model-quality gates, full CLI or rerun of unrelated historical proof harnesses.

### Rulings

Task 1 includes the project revision SELECT as the bridge's first production
consumer, rather than deferring it to Task 2: deny(warnings) rejects an unused
private bridge in an otherwise complete intermediate commit. This changes only
commit sequencing; revision/read-snapshot regressions remain the check against
an unintended behavioral change.

## Final independent review

Reviewed range: 360bbf7..7b9c587, fresh independent reviewer. Critical/Important
findings: none. Minor: the Linux inner command count above incorrectly said15;
it is13 (host15, Linux outer8), verified against all three manifests and corrected.
No product code changed after dual-platform verification; all78 shared build-input
hashes still match the checkout. Physical power loss/x86_64, resource/model-quality,
full CLI and unrelated historical proof harnesses were outside this refactor's
review; these untested boundaries remain explicit rather than implied proven.

Host manifest SHA-256: ac6ad8191830f05ffd0e4779569399f43c4fa71c7cac06ba390cc356e4523071.
Linux inner manifest SHA-256: f374794cbe694f549bdfeddc5c21d8c9ebb3e90e3c08993ad8e544e468a69ef8.
Branch remains separate from main; no merge/push is part of this implementation.
