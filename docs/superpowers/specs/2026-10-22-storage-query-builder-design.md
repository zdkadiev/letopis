# Storage query builder

Date: 2026-10-22. Scope: a readability refactor following completed TK-9; not TK-10.

## Accepted intent

The user finds SQL strings embedded in Rust unreadable and approved a fluent SQL
builder with named table/column identifiers for ordinary storage queries. Keep
SQLite-specific schema/native/search operations as SQL where a builder obscures
the operation. Preserve synchronous rusqlite execution and hexagonal boundaries.

## Technical choice

Use `sea-query = "=1.0.2"`, with `default-features = false` and only
`backend-sqlite` and `derive`. No SeaORM, async runtime, connection pool, new
storage interface, migration framework, or database schema change.

Keep `rusqlite = "=0.40.2"` and `libsqlite3-sys = "=0.38.2"` with bundled SQLite,
and the pinned vec1 C source/build/registration unchanged. The published
sea-query-rusqlite 0.8.0 requires rusqlite ^0.38, so it cannot be used directly.
Builder choice was approved by the user; the small private binding conversion
below is the implementer's compatibility choice, not a previously agreed fact.

## Physical structure and execution

Keep existing responsibility files. Add only private `src/storage/query.rs` for
named SQL identifiers and building SQL plus rusqlite-owned bound values. Declare
it privately from storage/mod.rs; do not reexport a builder API. Query composition
stays visible in graph.rs, files.rs, fts.rs, vectors.rs, read.rs and cache_write.rs.
Do not create a generic repository layer or one file for each query.

`build_query(statement: &impl QueryStatementBuilder)` returns
`StorageResult<(String, Vec<rusqlite::types::Value>)>`. Use SeaQuery's SQLite build
path, never SQL rendering with interpolated values. Convert enabled primitive
Value variants, including typed NULLs, text and BLOBs, without panics or lossy
integer casts. Preserve enum values as their text and enum NULL as SQL NULL; never stringify
arbitrary parameters. Overflowing u64 must fail before execution.
Use rusqlite::params_from_iter for execution on the existing Connection or
Transaction; retain current error mapping and row decoding.

## Migration boundary

Convert ordinary SELECT/INSERT/UPDATE/DELETE operations, including joins,
subqueries, upserts, ON CONFLICT guards and RETURNING, across graph/files,
project revision, vector keys, vector metadata, failure counters and query errors.
Use identifier enums rather than scattered table/column strings. Keep predicates,
selected column order, conflict targets, affected-row semantics, ordering and
connection/schema qualification unchanged. Attached vectors.db reads must
continue to address vec tables; writer queries address their own main schema.

Retain cache.sql and vectors.sql, schema validation/introspection SQL, connection
PRAGMAs/ATTACH/native probes, FTS MATCH/bm25 search and vec1 virtual-table search
and reset commands. Format retained substantial SQL as readable multiline text.
Routine metadata lookup and CRUD around FTS/vec1 still use the builder. Existing
test-fixture SQL may remain explicit, preserving an independent test oracle.

## Invariants and acceptance

- No changes to public Storage traits, lib.rs composition or pure domain code.
- Preserve stable row_id, contentless FTS, declared dangling edges, integer
  bounds, poisoned write-unit rollback, cache/vector writer independence, read
  snapshots, identity/search_hash guards, atomic attempt budgets and diagnostics.
- All user-supplied values remain parameters, including quotes, Unicode, NULLs
  and BLOBs. Identifier enums are code-owned; no user input becomes an identifier.
- Query functions remain readable and meet project verb naming and size rules;
  tests use Arrange/Act/Assert. No blanket lint allowances or new framework.
- Run root strict checks, targeted binding regressions, schema parity, strict
  Rustdoc and the existing host/Linux storage acceptance runners. Preserve
  historical TK-9 evidence; keep new raw receipts ignored under .artifacts/.
- Record concise verification, source revision, changed boundaries and actual
  platform limits in this plan and Engram. Do not claim unrun suites.

## Baseline and sources

Isolated branch `storage-query-builder`, based on main 360bbf74f1d583acda212bafaf4847d9cc2e6dd9.
Before dependency/product changes, ./scripts/check-rust.sh passed formatting,
compiler, Clippy and all 106 Rust tests on macOS ARM64.

- [Architecture](../../zkit/letopis/architecture.md#program-structure)
- [Decisions](../../zkit/letopis/decisions.md)
- [TK-9](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-9-storage.md)
- [SeaQuery documentation](https://docs.rs/sea-query/1.0.2/sea_query/)
- [Published binder dependencies](https://docs.rs/crate/sea-query-rusqlite/0.8.0)

At planning time, compatibility was established from published manifest/source
inspection. The helper is now implemented and verified on source8f19ba9; the
[implementation plan](../plans/2026-10-22-storage-query-builder.md#verification-and-implementation-record) records commands, results and limits.
