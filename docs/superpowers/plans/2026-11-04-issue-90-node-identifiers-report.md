# Issue 90: node identity acceptance

Status: implementation verified; all four tasks and independent review gates
complete. Task 4 and whole-branch reviews approved with zero findings.
Specification, plan and Subagent-driven execution were approved on 2026-11-04.
No merge, push, issue update or live cache operation is part of this acceptance.

## Result and operator action

Cache schema **3** stores one internal `nodes.id integer primary key autoincrement`
and one unique textual `nodes.uri`. Public IDs, edge endpoints and address order
remain textual. URI UPSERT retains a numeric key for the lifetime of its row;
knowledge metadata still exposes `id`/`uri`, rejects unequal inputs with typed
`InvalidRequest` and poisons the transaction. FTS uses the cache integer key.
Vector schema **1** and its bytes are unchanged; cache/vector membership uses
`search_hash`, independently of numeric allocations.

Existing schema-2 caches are rejected intact by open/status/reindex before vector
access. Operators must stop commands/drain and close handles, optionally back up
the closed cache, remove only cache-specific files, preserve vectors/config/lock
identities/Markdown/Git, then run reindex/check. See the
[current upgrade procedure](../../development.md#issue-90-версия-кэша-3).
A rebuilt cache cannot recover old last-valid projections for currently invalid
or missing Markdown. Old keys/revisions are not stable across rebuild; subsequent
ordinary vector pruning/config resets retain their rules.

## Accepted scope and commits

- Base: `f0d5b8b733e7ab50bb7efb15d0d22daff438874a` from main.
- `d7011c9`: approved specification; `c764634`: approved plan.
- `d4afb64`: schema and every inseparable production query consumer (Task 1).
- `c7c0c05`: textual identity/search/graph lifecycle regressions (Task 2).
- `e73f54b`: exactly three Task 3 test/container paths, saved externally with the
  unrelated subject `edit main letopis skill`; retained without rewriting history.
- `d0fd345`: two test-only corrections exposed by full acceptance (described below).
- `ff84a37`: Task 4 documentation and native acceptance report. This closing
  documentation commit records the final independent review verdicts.

The primary checkout was externally switched to `rafactor` after the Task 3 save.
Work resumed in `/Users/zdkadiev/.codex/worktrees/issue-90/letopis`; primary edits,
branch and build cache were preserved. Main's boxed transactions remain in place.
There are no new service/storage layers, runtime, dependency, field-removal API
or transaction redesign. Ordinary SQL uses SeaQuery/bound values; specialized
FTS5/vec1 SQL binds values. Existing modules own persistence, parsing, orchestration
and composition. Only inline tests in `search.rs` changed during Task 4.

## Seven specification acceptance groups

| Group | Evidence and result |
| --- | --- |
| 1. Writer/decode identity and rollback | One production node writer/decoder in `storage/graph.rs`; document publisher derives equal knowledge IDs/URIs and single source URI. `storage_node_identity` rejects unequal anchored metadata after valid staged work and observes unchanged revision/FTS. `storage_cache`, source lifecycle and inspection suites retain whole-file/anchored/metavertex/local/ranged/opaque source cases. Task 1 report and restored focused tests pass. |
| 2. Actual SQLite schema and stable UPSERT | `node_schema_uses_integer_id_and_unique_uri` independently checks PRAGMA types/PK/no row_id/FK and duplicate insert rejection; `cache_commit_preserves_node_row_id` checks row lifetime. Independent SQLite/literal oracles, not generated SQL alone. Cache3/vector1 exact schema parity passes. |
| 3. FTS/graph/external identity | Three `node_identity_lifecycle` tests reverse insertion order, retain stable FTS key through kind/text update, remove stale FTS rows, page both directions and work by address, and resolve a dangling includes declaration after target deletion/recreation under a new integer key. Existing source/check/status/inspect suites retained and exercised. Task 2 focused coverage passes. |
| 4. Old/malformed cache rejection | `storage_cache_upgrade` checks schema2 equal/mismatched/duplicate URI fixtures with present/absent vectors, direct open and real CLI status/reindex; exact closed bytes/inodes/config/lock sentinels survive. Three malformed-current-layout cases reject without vector creation. Legacy schema1 coverage is retained. |
| 5. Cache-only rebuild/vector isolation | Rebuild test uses old node keys 100/200/300 and deliberately reversed new cache/vector allocation; independent KNN identities and literal float bytes, full vector/config/failure/query-error records and inode identities survive. Synchronous drain makes zero fake-provider requests. Invalid current Markdown reports its error and loses its discarded old projection. |
| 6. Active design/docs/knowledge parity | Mechanics, D-108, I-8, development guide and existing projection/search knowledge updated. Executable/design DDL parity, scoped knowledge check/search/inspect, local links and frozen-schema hash audit pass. Captured source basis `2952c3bd52d92e83ed99a730329d25e3350beefd` matches every addressed source byte. |
| 7. Root/native checks, mutations and final review | Full host/Linux acceptance results below. Twelve effective manual mutations were restored; two exploratory survivors are explicitly not claimed as kills. All task reviews and whole-branch independent review approved with zero findings; no deferred or excluded judgments. |

## Five Review Focus cases

| Case | Owning regression |
| --- | --- |
| 1. Numeric insertion order differs from URI order | `updates_keep_fts_key_and_address_order`, `neighbor_cursors_and_work_selection_use_addresses`; literal ordered URI results and bounded page/work cursors. |
| 2. Caught unequal knowledge write poisons earlier valid work | `unequal_knowledge_identity_poisoning_rolls_back_publication`; rejected commit, revision 1, no pending/bad nodes, unchanged FTS. |
| 3. Incoming declaration survives replacement numeric identity | `dangling_target_reappears_under_new_numeric_id`; stored triple, diagnostics and both containment directions. |
| 4. Unequal/duplicate old URI rejected before vector access | `old_cache_is_rejected_without_vector_or_file_changes`; present/absent vectors, open/status/reindex and preserved sentinels. |
| 5. Cache keys differ from vector keys | `cache_rebuild_reuses_vectors_independently_of_node_keys`; independent vector/hash/KNN oracles, preserved state and zero requests. |

## Commands and receipts

All commands run in the isolated worktree. Python is
`/opt/homebrew/bin/python3.12` (3.12.14). Host target:
`CARGO_TARGET_DIR=/tmp/issue90-task3-fresh-clippy`, with `CARGO_INCREMENTAL=0`,
`CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`. Linux launcher:
`BUILDX_CONFIG=/tmp/issue90-task4-buildx DOCKER_CONTEXT=desktop-linux`.
Available disk before building: 155 GiB; all caches are isolated from the primary
checkout. The earlier APFS-cloned target had inconsistent third-party metadata;
it was not used for this final acceptance.

```sh
/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/issue-90/host-20261104-final3
/opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform linux --evidence .artifacts/issue-90/linux-20261104-final3
/opt/homebrew/bin/python3.12 -B tests/storage/check_schema.py .
cargo test --locked --offline --lib
cargo clippy --locked --offline --all-targets -- -D warnings
cargo test --locked --offline --test storage_node_identity
cargo fmt --all -- --check
git diff --check
rg -n 'Nodes::RowId|nodes\.row_id|Nodes::Id|nodes\.id|nodes\.uri' src tests examples
```

Both final3 manifests are **PASS**; every recorded command exited 0.

| Platform | Debug and release (each) | Other native checks |
| --- | --- | --- |
| macOS 26.6.2 arm64, Rust 1.95.0, Git 2.56.0, Python 3.12.14 | 1,030 passed, 0 failed, 11 intentional ignored; 76 harness/doc-test segments | Runner 10/10, native capability 1/1, real deadline 1/1 (60.00s), strict docs, standalone/linkage PASS. |
| Linux 7.0.14 aarch64/glibc2.36, Rust 1.95.0, Git 2.50.1, Python 3.11.2 | 1,024 passed, 0 failed, 11 intentional ignored; 76 harness/doc-test segments | Runner 10/10, native capability 1/1, real deadline 1/1 (63.04s), strict docs, standalone/linkage PASS. |

Totals include two compile-fail doctests per profile. The six extra macOS tests
are two cfg(macos) Git-read observer tests compiled into each of three integration
binaries; Linux does not compile those tests. Eleven ignores comprise nine private
subprocess entry points (covered through parent tests), one separately executed
real deadline, and the existing deferred real-corpus entry point. Both standalone
checks report copied executable, FTS, native vector, exact bytes, reopen and revision 1;
otool/ldd show no dynamic SQLite/vec1 dependency. Host and Linux images are arm64.

Each full runner includes make verify (format/check/all-target Clippy/debug tests/
strict docs/release tests), runner tests, native bundled FTS5/vec1, real fake-provider
deadline, copied standalone and dynamic-link inspection. No successful full run was
duplicated. Manifests/build/link/process logs preserve exact inputs, commands and
tool/platform versions. Linux runtime uses `--network none`.

`final-reconciliation.json` verifies all 272 host input hashes and all 229 inner
Linux hashes against actual final files; the 43 unchanged host-only inputs are
listed explicitly. Every changed product/build/test/schema input is present in
both receipts, with zero hash mismatches or missing changed inputs. The outer
Linux launch manifest also captures all 272 host inputs. Product receipts record
`d0fd345` plus the dirty documentation paths; final prose/report SHA-256 values
are recorded separately. Vector DDL and both historical fixtures remain byte-equal
to main; frozen cache-v2 SHA-256 remains
`57d25817073a6522326e4370bdbf328f8a1efd76c6edb274818ce0f4a79b1ac6`.

Supplementary scoped receipts in `.artifacts/issue-90/task-4/`:

- `restored-full-lib.log`:459 passed, 0 failed, 9 private ignored;
  `restored-all-target-clippy.log`:clean; `node-identity-lint-regression.log`:2/2.
- `node-consumers.log`, `residual-raw-sql-audit.log`: numeric node handles/FTS joins,
  textual URI lookups/order/edges, separate vector keys and frozen schema2
  exceptions reviewed. No remaining active textual node-id predicates found.
- `frozen-inputs.json`, `schema-parity.log`: vector DDL and both legacy fixtures
  byte-equal to main; frozen cache-v2 equals main's old executable cache DDL.
- `knowledge-{check,search,inspect}.json`, `knowledge-source-hashes.json`,
  `knowledge-verification.json`: two concepts found without flags, empty scoped
  validation errors, inspect snapshot bytes/hash match all 15 knowledge/source
  files. Checks run in an isolated temporary Git corpus; no root cache was written.
- `markdown-links.json`: changed active documentation local file/anchor checks;
  source code spans/fenced examples excluded; 874 local links/anchors checked, 0 findings.
- `changed-inputs.json` and `final-reconciliation.json` retain actual SHA-256
  values. Prose/report hashes are separate because the standard runner captures
  executable/build/test/schema inputs, not arbitrary documentation.

Task1/2 scratch outputs are preserved under `.artifacts/issue-90/task-{1,2}/`;
all Task3 `/tmp/issue90-task3*.log` outputs copied to `.artifacts/issue-90/task-3/`.
The detailed task reports and reviewed execution ledger remain under ignored
`.superpowers/sdd/2026-11-04-issue-90-node-identifiers/`.

## Full-suite findings and narrow corrections

1. Initial host `host-20261104-final` failed all-target Clippy: the unequal-input
   rollback test had 61 lines (limit 60). Extracted its unequal-metadata fixture into
   a focused helper; assertions and Given/When/Then remain visible. Focused 2/2,
   formatting and all-target Clippy then passed. Corresponding initial Linux run
   was interrupted and its one owned container removed by the runner. Its FAIL
   manifest preserves `KeyboardInterrupt` and successful cleanup.
2. Host `host-20261104-final2` reached the full lib target:458 passed, 1 failed,
   9 ignored. Inline `metadata_failure_after_query_releases_pinned_view` still
   corrupted/repaired metadata with `where id=?1` bound to textual a.md; SQLite
   matched zero numeric IDs. Focused reproduction failed 0/1. Both predicates now
   use URI and require exactly one affected row, retaining query/error/reader
   release/checkpoint assertions. Entire lib target then passed 459/459 and
   all-target Clippy passed. Linux final2 independently hit this same fixture
   failure and was already stopped/removed by its runner when cancellation was
   attempted. Its copied inner FAIL receipt and cleanup are retained.

The two fixes are test-only in `d0fd345`; no product rule was weakened or changed.
The expanded raw-SQL audit includes unqualified/aliased predicates throughout
src/service, tests and examples. Schema2 fixture textual IDs and observer numeric
ordering are deliberate exceptions. Each failed receipt remains distinct from
final3; no successful complete run was repeated.

## Restored mutations

| Owner | Mutation | Observed failure |
| --- | --- | --- |
| Task1 | Disable input ID/URI equality | Invalid write returned Ok instead of typed rejection. |
| Task1 | Remove unique URI | Explicit different numeric key duplicated the URI. |
| Task1 | Change numeric key during URI UPSERT | Stable key 1001 instead of 1. |
| Task2 | Order FTS ties by numeric key | z.md,a.md instead of a.md,z.md. |
| Task2 | Order neighbor page by numeric key | z.md instead of a.md. |
| Task2 | Order embedding work by numeric key | z.md instead of root.md. |
| Task2 | Join target on numeric key | Missing b.md traversal result. |
| Task2 | Omit FTS deletion | Native stale-row count 1 instead of 0. |
| Task2 | Truncate FTS before metadata validation | Corrupt beyond-limit candidate escaped validation. |
| Task3 | Join vectors by numeric keys | Nearest public identities reversed. |
| Task3 | Reset vectors in rebuilt-cache open | Preserved vector missing. |
| Task3 | Accept schema2 as schema3 | Rejection guard accepted old cache. |

Every effective mutation compiled, failed its owning behavioral assertion, was
restored byte-for-byte and passed the owning rerun. Task2 also recorded two
exploratory parser relaxations that survived because other validation layers
still rejected malformed paths; these are not mutation kills. Task3 restored an
earlier broad reset experiment that caused a lock race before rerunning its
precise reset mutation. Full outputs/scripts and exact commands live in the
respective task evidence; no mutation remains in the final source.

## Limits and reviews

Native acceptance establishes the recorded macOS/Linux arm64 environments,
SQLite/FTS5/vec1 linkage and the tested concurrency/compatibility behavior. It does
not establish x86_64, power-loss durability, arbitrary filesystem fault coverage,
production corpus migration, real-provider quality, throughput or paid inference.
Only local fake-provider traffic is used; no live cache was removed or migrated.
Scoped knowledge validation makes no claim about unrelated corpus findings.
D-108 is used because D-107 is reserved by separate pending issue49 work; no
pending issue49 code/docs are imported into this main-based branch.

Task1, Task2, Task3 reviews: compliant/approved, no findings (controller ledger).
Task4 reviewer `/root/issue90_review4` reviewed `e73f54b..ff84a37`: specification
compliant, quality approved, zero findings; independently verified all three PASS
receipts and their current input hashes (host 272/16 commands, Linux outer 272/8,
Linux container 229/14).

Whole-branch reviewer `/root/issue90_final_review` (`gpt-6-astra`, high) reviewed
main `f0d5b8b733e7ab50bb7efb15d0d22daff438874a..ff84a37`: **ready for integration**,
specification compliant and quality approved. Critical 0, Important 0, Minor 0;
no deferred or excluded judgments. A focused external check additionally confirmed
both storage open paths validate cache schema before opening vectors
(`src/storage/sqlite.rs:44` and `:64`). The controller supplied both final verdicts;
no code correction or suite rerun was requested. This closing commit updates only
documentation/review status; captured product hashes are rechecked unchanged. Final documentation validation
checks 874 local file/anchor links across nine affected documents with zero findings,
and `git diff --check` passes. Independent SHA-256 recomputation confirms unchanged
272/272/229 product inputs in all three receipts; the explicit closeout result is
`.artifacts/issue-90/task-4/review-closeout-product-hashes.json`.

## Execution rulings and costs

The controller's execution ledger recorded these four rulings; their costs are
preserved here alongside the acceptance evidence.


- Ruling: Reuse the clean user-selected issue-90 checkout instead of creating another worktree — the user required a branch from main, and current environment instructions prefer suitable existing checkouts — cost if wrong: less physical isolation from concurrent local edits; inspect dirty state before each gate.

- Ruling: Replace the old persisted knowledge ID/URI mismatch corruption fixture with an invalid ../wrong.md URI while retaining validation-before-limit assertions — schema 3 stores one URI, so a different valid address is no longer a two-field mismatch — cost if wrong: corruption coverage could change; writer-level unequal-input rejection remains independently tested, and reviewer must check before-limit validation is retained.

- Ruling: Continue in native isolated worktree /Users/zdkadiev/.codex/worktrees/issue-90/letopis after external save/checkout — e73f54b preserved exactly Task3 files and the primary checkout now belongs to rafactor — cost if wrong: build caches and absolute evidence references require care; preserve original evidence and verify transferred bytes. External commit retained without rewriting its message.

- Ruling: Use free decision identifier D-108 for issue90 instead of D-107 — pending issue49 work already reserved D-107 — cost if wrong: later documentation references may need renumbering; no runtime effect.
