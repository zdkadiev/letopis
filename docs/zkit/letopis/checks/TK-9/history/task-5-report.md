# Task 5 implementation report

Status: implemented and verified on Darwin arm64 with rustc 1.95.0 (59807616e 2026-04-14). Base: bbf0ef43cab3550f77e8869200e047c40986b2f, branch TK-9. Parent-owned dirty plan/spec/task documents were preserved.

## Changes

Added application-owned EmbeddingFailure and QueryError values, guarded failure reads, singleton query diagnostic reads, and four vector-unit mutation operations. Attempt reservation uses a single bounded UPSERT (`attempts < max_attempts`), rejects zero and integers above i64::MAX, and never increments at the integer limit. Failure recording is UPDATE-only; absent reservations return false. All new failing mutation operations poison the existing atomic unit. Existing success behavior deletes only the matching hash's failure. Identity is verified under the existing BEGIN IMMEDIATE writer lock before any vector capability is exposed. No table, scheduler, provider call, retry loop or history was added.

## RED before product changes

`cargo test --locked --offline --test storage_failures` exited 101. All seven requested behavior scenarios were present before implementation. Output: `task-5-red.log`.

The expected missing-API output included E0432 unresolved imports EmbeddingFailure/QueryError, and E0599 missing methods read_failure, reserve_attempt, record_failure, set_query_error, clear_query_error and read_query_error. This compile RED is explicitly required by the Task5 brief. It was not a behavioral assertion RED.

## Exact persistence and stale-state assertions

- Total budgets 1 and 3: dropping an uncommitted reservation leaves no row; each successful committed reservation is followed by close/reopen and exact attempts 1 (or 1,2,3), error None; next reservation is false with count unchanged. No provider call occurs: reserved-but-unsent attempts remain durable.
- Absent record_failure returns false and leaves no row. Two committed failures have attempts=1 and safe message; dropping vector success preserves its failure; committed success removes only its own failure and preserves the other's exact hash/count/message.
- Query failures at 10 and 20 replace the same singleton; close/reopen plus M2 reset retains exactly at=20 and latest safe message. SQL confirms exactly one query_error row and the exact non-vec1 tables embed_failures/meta/query_error/vector_keys (no jobs/history). Explicit successful-query clear leaves None.
- M2 is populated before every late M1 action: failure M2/attempts=1/error="M2 failure", query at=42/error="M2 query", vector under M1's text hash=[1,0]. Five M1 begin attempts, covering reserve/failure/store/query-set/query-clear, all return ConfigChanged. Exact M2 identity, failure, query, and vector state remain unchanged; M2's own hash has no vector. Stale failure reads return ConfigChanged.
- Zero/u64::MAX budgets reject and poison a unit containing a prior reservation, leaving no partial row. u64::MAX timestamp rejects and poisons a prior query write, leaving None. A SQL-seeded i64::MAX attempts row cannot increment, retains its safe error and exact count; i64::MAX timestamp round-trips.
- Cache revision is asserted unchanged at zero through reservation/failure/success/reset and diagnostic scenarios.

## GREEN and final verification

- `cargo test --locked --offline --test storage_failures`: exit 0, 7 passed, 0 failed. Output: task-5-green.log.
- `./scripts/check-rust.sh`: exit 0, formatting/compiler/Clippy successful; 106 root tests passed, 0 failed, 0 doc tests. Includes all earlier storage/native/vector scenarios. Output: task-5-check.log.
- `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`: exit 0. Output: task-5-rustdoc.log.
- `git diff --check`: exit 0.

An initial full check caught two missing documentation backticks around i64::MAX; fixed before the successful complete run. Initial shell result reporting attempted zsh's read-only `status` variable after the commands had completed; reran with task-specific exit variables and confirmed both exits 0.

## Self-review and limits

Reviewed physical files, private SQL helpers, service-owned interfaces, explicit imports, poisoned write handling, checked integer conversions and unchanged composition wiring. New functions/test functions are within the 60-line cap; stale-action execution is a named private test helper and exact outcome assertions remain visible. No concrete SQLite types cross the application interface. Existing reset preserves query_error; existing success deletes only its hash's failure.

Mental mutations covered: remove budget predicate, reset counter on UPSERT, auto-create failure on error, clear all failures on success, clear query error on reset, omit identity gate, allow invalid integer conversion, or forget unit poisoning would fail the new scenarios. No Linux/x86_64, physical power-loss, real provider, or separate historical proof harness was run for this task. Caller supplies already-safe messages; storage does not inspect them for secrets. Parent owns documents/checklists/Engram. No implementation concerns found.
