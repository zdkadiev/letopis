# Task 3 report

Status: implemented and verified on TK-9, Darwin arm64, Rust 1.95.0. Source parent: `422b34becf23a5597a7b0ff92618901141afbae1`. Commit is appended below after creation.

## Implemented

- Plain typed metadata/file/match models and service-owned `CacheWrite`/read operations.
- Dedicated unattached cache connection with Immediate caller-owned transaction; graph, files, contentless FTS and revision commit together. No vector methods or future service stubs.
- Stable UPSERT row IDs, all declared edges including dangling targets, incoming-edge retention, caller-driven source reconciliation, checked unsigned integer conversion, typed persisted-value decoding.
- Every mutation result flows through the unit's failure flag. Caught input or SQL errors prevent commit and RAII rolls back. Revision checks i64 overflow and increments once even for an empty unit.
- Parameters bind SQL values; fixed public messages retain native internal causes. FTS accepts prepared expressions and returns native BM25 then ID without an SQL candidate limit.

## RED and GREEN evidence

Before implementation, added the initial 12 projection/atomicity tests plus fixture constructors, then ran `cargo test --locked --offline --test storage_cache`.
Actual RED diagnostics in [task-3-red.log](task-3-red.log): unresolved imports `KnowledgeMetadata`, `NodeMetadata`, `SourceMetadata`, `FileRow`, `CacheWrite`; missing `begin_cache_write`, `read_node`, `list_edges`, `read_file`, `match_text`. After implementation those 12 scenarios passed. Initial exploratory RED also exposed a test-module import mistake; corrected it before the captured missing-API RED. Additional acceptance/characterization tests were then added for already implemented primitives; they passed immediately, and are not claimed as separate test-first implementation cycles.

Final focused command: `cargo test --locked --offline --test storage_cache` — 18 passed, 0 failed; [output](task-3-focused.log).
Final full command: `./scripts/check-rust.sh` — formatting, compiler, Clippy and all 89 root scenarios passed, plus 0 doctests; [output](task-3-check.log).
Final documentation command: `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` — passed; [output](task-3-rustdoc.log).
`git diff --check` — passed.

Intermediate full checks found redundant field naming, then a partial helper rename caused unresolved identifiers/unused imports. Both were fixed before the final checks; no unresolved failing tests remain.

## Scenario coverage

18 integration tests in `tests/storage_cache.rs` cover stable row IDs and FTS replacement; missing source bytes/dangling targets; incoming edges and target deletion/reappearance; source creation time 3 -> 7 and removal with caller-owned referring-set changes; valid/invalid/unmarked files; caught FK mutation error poisoning; dropped units; late missing-knowledge FTS failure; u64 size/inode/creation/line overflow; i64 revision overflow; duplicate edges/schema CHECK failures; contentless FTS and public error privacy; empty cache publications/vector metadata separation; native BM25/ID ordering; malformed persisted graph/files; optional fields after reopen; and outgoing-edge deletion enforcement/FTS cleanup.

One private `cache_write.rs` test writes metadata, drops text_index through the private transaction, catches a real SQL failure from replace_text, attempts commit, then proves rollback restores the index and removes uncommitted metadata with revision 0. It asserts internal cause retention and no unique payload/Authorization/index name leakage from public diagnostics. No public fault or SQL API was introduced.

All original 53 scenarios remain, and native/bootstrap suites are retained. Full count is 89 (10 unit + 7 domain + 11 retention + 5 snapshots + 10 elements + 6 links + 8 search + 14 bootstrap + 18 cache).

## Self-review

Reviewed physical files, visibility and imports: domain remains pure; service exposes only domain/application values and interfaces; private storage modules own SQL; SqliteStorage and its implementations stay together; existing lib.rs continues actual constructor wiring. Only actual cache capabilities were added. All custom helpers use verbs; function-size audit found no new functions over 60 physical lines. The storage schema/native/bootstrap files and historical evidence were not changed.

Mental mutation checks: replacing UPSERT with REPLACE breaks row-ID preservation; omitting poison breaks caught-error tests; skipping FTS replacement/deletion breaks match assertions; unchecked integer casts break overflow cases; dropping incoming edges breaks declaration retention; skipping revision overflow checks breaks atomic rollback; leaking native error messages breaks privacy assertions.

## Limits / concerns

No known unresolved Task 3 functional issue. Only macOS arm64 root checks were executed here. Linux/platform/static-executable harness and physical power-loss drills were not run by this task. Vector state checks preserve existing metadata/failure state; Task 4 owns vector operations. Source minimum times, projection validity and safe file-error content remain caller responsibilities, as agreed. FTS content is NULL on SELECT; token/posting data is expected on disk, with no claim that all text-derived bytes are absent. The parent-owned Task 2 concurrency discrimination issue was not changed. Parent spec/plan/task-card dirty files remain untouched and uncommitted by this task.

## Source fingerprints

- cache_write.rs: `29a4b37d52833784ae05e4c000e70e9f806d1585e80889478ab9880ee5d4e960`
- graph.rs: `96a38979ca2f3a6e460a5b71766767fdea4cd4702f979a2f06cbeb04f84ee304`
- files.rs: `e4d500a6f91009c17a259b13fe2945817f3690387b075c7878ee8f06890a1c15`
- fts.rs: `6cdc34fc3b17d7f20364bdc06f0650bba42c86c7b403cde599b60537bdb117ce`
- storage_cache.rs: `606c0cbe89b543084d483530604a5431a8206d7a23ac6981ee9b930b44142094`

Commit: `9f20142` — `feat: persist graph file state and FTS atomically`.
