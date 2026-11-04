# Issue #90 Node Identifiers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace duplicate node identity columns with an integer `id` and unique textual `uri`, preserving all external identities and reusable vectors.

**Architecture:** Keep the storage interfaces and transaction handles from `main`. SQLite owns numeric keys; services and domain values retain textual element addresses. Update the schema and its inseparable query consumers together, then prove behavior and cache-only upgrade safety.

**Tech Stack:** Pinned synchronous Rust, rusqlite, SeaQuery, bundled SQLite/FTS5 and static vec1; no dependency changes.

**Spec:** [Approved node identifier design](../specs/2026-11-04-issue-90-node-identifiers-design.md).

## Revision status

The original completed plan below is superseded for edge persistence by the
[user-requested endpoint revision](../specs/2026-11-04-issue-90-node-identifiers-design.md#endpoint-revision-requested-after-pr-91):
both endpoint columns must reference integer `nodes.id`. Do not execute its
textual-edge instructions as the new design. The replacement design retains unresolved declarations separately and preserves
the existing diagnostics. The [follow-up plan](2026-11-04-issue-90-integer-edges.md)
contains the separately approved new implementation steps; execution and final
acceptance are recorded in the [integer-edge report](2026-11-04-issue-90-integer-edges-report.md).
Original acceptance results remain evidence for the original tree only.

## Global Constraints

- Work on `issue-90`, based on `origin/main` at `f0d5b8b733e7ab50bb7efb15d0d22daff438874a`; do not import `rafactor` or switch to its transaction interfaces.
- Cache schema becomes **3**; vector schema stays **1**, byte-for-byte unchanged.
- `nodes.id integer primary key autoincrement`; `nodes.uri text not null unique`. Keep all independent metadata constraints.
- `KnowledgeMetadata.id` and `uri` remain Rust fields. Reject unequal inputs with `StorageErrorKind::InvalidRequest`; decode both from the stored URI.
- Keep `StoredNode.row_id`, `TextMatch.row_id`, `GraphNodeKey`, vector key names and public textual identities unchanged.
- Edges remain textual triples; source FK references `nodes(uri)`, target has no FK. Maintain lexical address ordering and bounded traversal.
- Keep SeaQuery/bound values, specialized FTS/vec1 SQL, existing modules and composition. No ORM, async runtime, transaction redesign or speculative wrappers.
- Only explicit quiescent cache rebuilding upgrades schema 2. Incompatible open/status/reindex reject without migration, repair or vector reset.
- Do not delete a live cache, rewrite historical evidence, run real inference, merge, push, or close #90 as part of executing this plan without authorization.
- Tests use Given/When/Then, independent literal/SQLite oracles and restored manual mutations. Keep new/touched functions focused and imports explicit.

## Review Focus

1. Numeric IDs whose insertion order differs from URI order must not change FTS ties, neighbor cursors or embedding selection; Task 2 deliberately reverses insertion order.
2. A caller catches unequal-knowledge input after an earlier valid write: commit must still fail and publish nothing; Task 1 asserts observer counts and revision.
3. An incoming dangling edge must survive target deletion and become traversable after reappearance under a new numeric key; Task 2 tests the complete lifecycle.
4. A schema-2 database with duplicate URIs or mismatched knowledge fields must be rejected intact before vector access; Task 3 tests malformed legacy data and missing-vector no-creation.
5. Rebuilt cache keys deliberately differ from vector keys: exact existing vectors and retry state must survive and remain reusable without inference; Task 3 checks both bytes and provider count.

## File responsibilities

- `src/storage/cache.sql`, `schema.rs`, `query.rs`: executable DDL, compatibility version and private typed column identifiers.
- `src/storage/graph.rs`, `fts.rs`, `neighbors.rs`, `work.rs`: graph persistence/decoding, FTS joins, paged neighbors and embedding work queries.
- `src/storage/cache_write.rs`, `read.rs`, `sqlite.rs`, `status_counts.rs`, `vectors.rs`: audit affected delegated queries and inline fixtures; retain their existing responsibilities.
- `src/service/mod.rs`, `types.rs`: clarify the existing metadata equality contract, without introducing interfaces or changing public signatures.
- `tests/storage_node_identity.rs`: focused schema, persistence and rollback regressions.
- `tests/node_identity_lifecycle.rs`: FTS, address ordering and graph/link lifecycle regressions using existing fixtures.
- `tests/storage_cache_upgrade.rs`, `tests/fixtures/storage/cache-v2.sql`: frozen old-cache upgrade and vector-preservation tests.
- Existing tests, `examples/storage_check.rs`, storage runner/container inputs: migrate current-layout assumptions while preserving historical fixtures.
- Active schema/docs and source-linked knowledge: explain new identities and operator upgrade procedure.

## Task 1: Change the schema and its inseparable query consumers

**Files:** modify `src/storage/{cache.sql,schema.rs,query.rs,graph.rs,fts.rs,neighbors.rs,work.rs}`, `src/service/{mod.rs,types.rs}`, current-schema fixture SQL in `src/storage/` and `tests/`; create `tests/storage_node_identity.rs` and `tests/fixtures/storage/cache-v2.sql`; update active cache DDL in `docs/zkit/letopis/mechanics/storage-schema.sql`.

**Interfaces:** consumes `open_storage(&Path, &Path) -> StorageResult<impl Storage + use<>>`, `Storage::begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>>`, existing `ReadTransaction`/`CacheWrite` contracts. Produces unchanged `CacheWrite::upsert_node(&mut self, &NodeMetadata) -> StorageResult<i64>` and `get_node_by_id`/FTS/neighbor/work interfaces using schema 3. Private `build_node_values(&NodeMetadata) -> StorageResult<[Value; 8]>` persists URI once; `decode_node(&Row<'_>) -> StorageResult<StoredNode>` reconstructs metadata.

- [x] Before schema edits, copy the exact base `src/storage/cache.sql` to `tests/fixtures/storage/cache-v2.sql`. Record its base commit/hash in the execution report. Keep `legacy-cache.sql` and `legacy-vectors.sql` unchanged.
- [x] Add `node_schema_uses_integer_id_and_unique_uri` with raw SQLite PRAGMA assertions for `id` type `INTEGER`, primary key, no `row_id` column, unique URI and `edges.source_id -> nodes.uri`. Test SQL uniqueness by attempting a second row with an explicit different integer ID and the same URI; an ordinary UPSERT is not a uniqueness oracle.
- [x] Add `unequal_knowledge_identity_poisoning_rolls_back_publication`: commit initial revision 1, start another write, insert valid `pending.md`, catch rejection of `id=a.md#part`/`uri=a.md`, then assert `commit()` fails. A new reader sees revision 1, no pending/bad node, and unchanged initial FTS results. Correct the optional-field round-trip fixture to use equal anchored values while keeping its other assertions.
- [x] Run `cargo test --locked --offline --test storage_node_identity`; confirm the new schema and mismatch assertions fail against current code, not due to fixture compilation.
- [x] Implement schema 3 and node-column mapping together. `NODE_COLUMNS` becomes eight metadata columns beginning with URI; UPSERT conflicts on URI, updates metadata only and returns numeric ID. Reject unequal knowledge metadata before SQL mutation, preserving `SqliteCacheWrite::record` poisoning. Decode numeric key plus one textual URI, with existing typed kind/hash/line checks.
- [x] Change textual predicates/order/cursors/endpoint joins from old `Nodes::Id` to `Nodes::Uri`; numeric uses from `Nodes::RowId` to `Nodes::Id`. FTS selects textual URI and numeric ID and joins `nodes.id = text_index.rowid`. Audit `status_counts.rs`, `vectors.rs` and raw SQL without renaming vector keys. Adapt inline current-layout fixtures and `rejects_each_inconsistent_persisted_source_field`: source ID/URI mismatch is no longer representable, but the other invalid-source checks remain.
- [x] Update schema-compatibility tests deliberately: the frozen legacy cache is rejected, not relabeled version 3. Preserve keyword/literal case normalization coverage using the existing independent small-schema cases and unchanged legacy vector fixture. Update active schema-parity DDL at the same time.
- [x] Run `cargo test --locked --offline --test storage_node_identity --test storage_cache --test storage_embedding_work --test storage_vectors` and `cargo test --locked --offline --lib storage::`. Expected: all selected tests pass. Mutation-check removal of the metadata equality check, unique URI constraint and stable-UPSERT behavior individually; restore and rerun their owning tests.
- [x] Commit intended Task 1 paths only: `refactor(storage): use integer node ids and unique addresses`.

## Task 2: Prove external identity, FTS and dangling-link behavior

**Files:** create `tests/node_identity_lifecycle.rs`; extend existing `tests/{graph_read.rs,storage_embedding_work.rs,storage_cache.rs,storage_vectors.rs}` only where their fixtures best own a regression; migrate remaining current-schema raw SQL in tests/examples. No new production service.

**Interfaces:** consumes Task 1 schema 3 and unchanged `StorageRead`, `GraphRead`, `ElementAddress`, `KnowledgeId` and `EdgeTarget` interfaces. Produces tests of unchanged CLI/service results; public numeric IDs are not introduced.

- [x] Add `updates_keep_fts_key_and_address_order`: insert `z.md` before `a.md`, publish identical searchable text, assert lexical result order `a.md,z.md` despite reversed numeric IDs. Update `a.md` text and kind, assert its integer key is unchanged, old text disappears and new text matches; delete it and assert no stale FTS entry. Use literal URI lists and independently query SQLite for the stored key.
- [x] Add `neighbor_cursors_and_work_selection_use_addresses`: reverse numeric insertion order, test outgoing/incoming neighbor pages with cursor and limit 1, and embedding-work selection after a textual cursor. Assert literal addresses without calculating expected order with the implementation. Retain existing index-plan tests and paging quantum.
- [x] Add `dangling_target_reappears_under_new_numeric_id`: publish an edge to absent `b.md`, create `b.md`, remove it without removing the incoming declaration, then recreate it. Assert the edge is stored throughout, diagnostics change as expected, the new integer key differs and traversal resolves the same textual endpoint. Include an `includes` relation and both directions in this or the existing graph fixture.
- [x] Run each new focused test before adding any corrective product changes. These are behavioral guard tests that may already pass after Task 1; do not manufacture a RED claim. If a failure exposes a mapping bug, fix it only in its storage owner and rerun.
- [x] Run `cargo test --locked --offline --test node_identity_lifecycle --test graph_read --test graph_diagnostics --test cli_traversal --test search_consistency --test source_lifecycle --test source_inspection --test knowledge_inspection --test read_verification`. Retain source collision/pruning, opaque HTTP addresses, anchored knowledge and range-source assertions from these suites; inspect their actual coverage before claiming it.
- [x] Mutation-check lexical URI ordering by replacing it with numeric ordering, target joining by breaking the URI join, and deletion by removing FTS cleanup; each corresponding regression must fail. Restore and rerun affected tests.
- [x] Commit only intended Task 2 paths: `test(storage): preserve node addresses across graph and search operations`.

## Task 3: Prove cache-only upgrade and vector isolation

**Files:** create `tests/storage_cache_upgrade.rs`; use frozen `tests/fixtures/storage/cache-v2.sql`; modify `src/storage/schema.rs`/open-order code only if a regression exposes a violation. Update `tests/storage/Dockerfile` and `.dockerignore` so storage fixtures are included in the Linux context.

**Interfaces:** consumes schema 3, frozen schema-2 DDL, unchanged vector schema 1, `Storage::reset_vectors`, `begin_vector_write_tx`, read-by-search-hash and existing reindex/CLI fixtures. Produces a supported temporary-fixture rehearsal of the operator rebuild procedure; no migration API or baseline product build.

- [x] Build a closed, checkpointed temporary schema-2 cache using the frozen DDL and `pragma user_version=2`. Seed cases with a valid old row, unequal knowledge ID/URI, duplicate knowledge URIs and a malformed same-version schema. Use independent SQL literals; do not pass unequal metadata through the new writer or modify a user database.
- [x] Add `old_cache_is_rejected_without_vector_or_file_changes`: capture bytes and inode identities after closing all fixture handles, then try normal open, status and reindex. Assert the existing safe schema diagnostic and unchanged cache/vector/config/lock sentinels. Also test an absent vector file remains absent. Malformed version-3 layouts are rejected separately.
- [x] Add `cache_rebuild_reuses_vectors_independently_of_node_keys`: seed the unchanged vector format through the public vector API, close storage, replace only the temporary cache with frozen schema 2, and establish explicit old node keys distinct from vector keys. Preserve vector `[1.0, 0.0, 0.0]`, identity, attempts/error and query-error sentinel; first prove old-cache rejection, then close all handles, remove only cache-specific files and run supported reindex on canonical Markdown. Choose fixtures so rebuilt cache keys differ and assert that they actually do.
- [x] Assert schema version 3, unchanged vector schema 1, exact vector bytes/identity/failure/query-error records, correct graph/FTS identities, and zero document-provider requests for current stored hashes. Keep read-only comparison distinct from SQLite file byte equality: a supported reopen can change sidecar representation without changing vector records. A no-call provider double fails if inference is attempted.
- [x] Include invalid current Markdown in a rebuild fixture: report its validation error rather than claiming an old last-valid projection was recovered. Preserve canonical files, Git and lock-file identities throughout. Keep old-schema input hashes fixed after Task 1.
- [x] Run `cargo test --locked --offline --test storage_cache_upgrade --test source_lifecycle --test storage_bootstrap --test storage_corruption --test corruption_reindex --test reindex_command --test status_command`. Confirm selected tests cover incompatible status and no-creation behavior, not just normal initialization.
- [x] Mutation-check changing the cache lookup to a numeric vector-key join, resetting vectors during upgrade, and bypassing cache schema rejection. Each owning assertion must fail; restore and rerun.
- [x] Verify Docker context includes `tests/fixtures/storage` explicitly and ignore rules do not exclude it. Commit intended paths: `test(storage): verify cache v3 rebuild preserves vectors`.

## Task 4: Align documentation and run final acceptance

**Files:** modify active `docs/zkit/letopis/{mechanics.md,decisions.md}`, applicable `contracts/I-8-read-view.md` text, `docs/development.md` and affected `letopis/architecture/` knowledge; create `docs/superpowers/plans/2026-11-04-issue-90-node-identifiers-report.md`. Use new ignored evidence directories under `.artifacts/issue-90/`.

**Interfaces:** consumes completed Tasks 1–3. Produces matching active design/DDL, upgrade instructions, final verification receipts and review findings. Historical designs/proof harnesses retain their captured semantics.

- [x] Document the two identity roles, textual edge endpoints, stable integer keys within a row lifetime, schema versions 3/1 and explicit quiescent cache-only upgrade. Explain loss of old last-valid projections for invalid Markdown and normal future vector pruning; do not promise cross-rebuild key/revision stability.
- [x] Follow `skills/letopis/SKILL.md` for affected source-linked knowledge, updating existing concept material rather than creating unrelated fragments. Check/inspect the affected elements against final source and record tooling limits honestly. Keep broad corpus findings outside #90 separate.
- [x] Scan active consumers using `rg -n 'Nodes::RowId|nodes\.row_id|Nodes::Id|nodes\.id|nodes\.uri' src tests examples`. Review each hit by numeric/text meaning; frozen fixtures and historical harnesses are deliberate exceptions. Compare `src/storage/vectors.sql` and `legacy-*.sql` to the base and assert no changes. Run `python3 -B tests/storage/check_schema.py .`, Markdown link checks and `git diff --check`.
- [x] Check disk capacity before building. Run `python3 -B tests/storage/run.py --platform host --evidence .artifacts/issue-90/host-<unique>` and the corresponding `--platform linux --evidence .artifacts/issue-90/linux-<unique>`. Use Python 3.11+ (the host has `/opt/homebrew/bin/python3.12`). The runner includes `make verify`, strict docs, root debug/release tests, schema parity, runner tests, native FTS5/vec1, standalone linkage and the real deadline test; do not duplicate a successful full run. Preserve failed receipts and record unavailable platforms rather than claiming success.
- [x] Verify captured input hashes match the actual final changed tree. Review physical modules, trait signatures, composition and SQL parameter binding. No new service/storage layer or field-removal API change belongs in this task.
- [x] Perform independent final review per the approved execution method. For Native, request one whole-branch reviewer; for Subagent-driven, retain task gates and the final review. Resolve material findings with focused regressions and rerun checks justified by the corrections.
- [x] Record all seven spec acceptance groups, five Review Focus cases, exact commands/results, restored mutations, receipt paths and remaining limits in the report. Update task checkboxes only with evidence. Commit intended docs/report paths: `docs: record node identity refactor acceptance`.

## Self-review and handoff

Spec coverage: identity validation, DDL and SQL mapping are Task 1; FTS/ordering,
graph and source semantics are Task 2; rejected legacy data, operator rebuilding
and vector preservation are Task 3; docs, platform checks and final review are
Task 4. All five Review Focus cases have explicit owning tests. Existing boxed
transaction signatures remain consistent across tasks; no `CacheDatabase` or
`VectorDatabase` from `rafactor` is assumed.

The schema and every live query consumer are one atomic implementation task:
splitting them into separate deliverables would intentionally leave the program
unable to open or query its own cache. Later tasks independently verify behavior
and upgrade isolation. Future executors must recheck the branch and preserve
unrelated work rather than assuming the recorded clean tree is still current.

Status: specification, plan and Subagent-driven execution approved on 2026-11-04.
All four tasks implemented and independently reviewed clean; host/Linux native
acceptance passed. Task 4 review (e73f54b..ff84a37) and whole-branch review
(main f0d5b8b..ff84a37) approved specification/quality with zero findings and no
deferred or excluded judgments. See the [acceptance report](2026-11-04-issue-90-node-identifiers-report.md).
Do not merge, push or update issue closure as an implied final step.
