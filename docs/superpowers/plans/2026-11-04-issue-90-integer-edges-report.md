# Issue 90: integer edge endpoint acceptance

Status: Tasks 1–4 passed independent specification and quality gates. Whole-branch review of
`5872c9e` found I1 (persistent cache mutation on refusal), M1 (SHM guarantee) and
M2 (stale MC-1 lifecycle wording). The final correction is implemented; fresh
host/Linux acceptance and exact input reconciliation passed. The scoped rereview
of `5872c9e..c1f2810` closed all three findings, found no new breakage or out-of-scope
observations and approved final code/evidence readiness.
Earlier native PASS receipts below cover the pre-review-fix tree only.

This report covers the endpoint revision after PR #91, based on the original
verified tree `166aaa8c059aabe38814f689d739dcd0833d7a7a`. The
[original schema-3 report](2026-11-04-issue-90-node-identifiers-report.md), its
source snapshot `2952c3bd52d92e83ed99a730329d25e3350beefd` and all original
receipts remain historical evidence for textual edges. They are not acceptance
of schema 4. The approved [specification](../specs/2026-11-04-issue-90-node-identifiers-design.md)
and separate [follow-up plan](2026-11-04-issue-90-integer-edges.md) govern this revision.

## Result and operator action

Cache schema **4** stores both resolved endpoints as integer foreign keys to
`nodes(id)`, with restrictive deletion. Public element IDs, edge endpoints,
ordering and cursors remain textual URI values. `unresolved_edges` retains a
missing target's URI and an integer source FK without creating a placeholder
node. Exactly one representation exists per logical declaration in a committed
state. Declaration reads expose the ordered union; traversal uses resolved edges.

Storage demotes incoming declarations before deleting a target, promotes every
currently resolvable declaration at commit, and removes both representations on
owner cleanup. Missing sources/duplicate declarations poison the write unit.
Document publication writes all changed nodes/FTS/files and derived sources
before changed owners' edges, using the existing effective metadata graph and
one-file captured/search text lifetime. Unchanged owners are not reparsed or
reinserted. Nodes, declarations, FTS, files and one revision increment commit or
roll back together. Invalid Markdown retains its last-valid projection; missing
source bytes retain the existing source node/null-hash semantics.

URI paging uses the pinned SQLite VM's bounded top-K path with LIMIT 1..32 and
no OFFSET. Retained sorter records are limited to K plus constant working-record
overhead; any filter lookup retains only supplied filter constants. A page may
scan adjacency rows, and repeated-page latency can grow with degree. No equivalent
index-only performance, speedup or production latency is claimed.

Vector schema **1** and its DDL remain byte-exact. Cache/vector membership uses
`search_hash`, never numeric cache keys. The operator must stop calls/drain, close
all handles and follow the [quiescent cache-only upgrade](../../development.md#issue-90-версия-кэша-4).
Keep vectors, sidecars, configuration, locks, canonical Markdown and Git; remove
only closed cache artifacts, then use supported reindex. Old cache keys/revision
and discarded last-valid invalid/missing projections are not recovered.

## Explicit acceptance matrix

| Invariant / required behavior | Current evidence |
| --- | --- |
| Both resolved endpoints have integer affinity and FK to nodes.id; unresolved source also has that FK; no placeholder | Independent PRAGMA/raw-SQL tests in `storage_node_identity`, missing-numeric-endpoint rejection and lifecycle `foreign_key_check`; not generated-query assertions alone. |
| Unique URI/metadata equality, stable row-lifetime key, FTS join and no stale FTS | Existing `storage_node_identity`, `storage_cache`, `node_identity_lifecycle` and FTS/search regressions retained and executed. |
| Every declaration survives missing/appearing/deleted/recreated targets with one representation | Raw resolved/unresolved rows and literal diagnostic/traversal assertions in `storage_node_identity` and `integer_edge_publication`. Recreated targets have distinct numeric keys. |
| Ordered logical union and source filter include both tables | `unchanged_owner_resolves_new_target_and_preserves_logical_union_order` asserts literal multi-owner union plus filtered owner rows. This resolves Task 1's review minor. |
| Nodes/sources precede edges for full and incremental publication | Task 2 forwarding observer records node/source UPSERTs before edges. `integer_edge_publication` covers reverse file order, anchored targets and local/opaque remote sources. |
| Unchanged owners avoid reparse/reinsert; owner edits/deletion clear both tables | Real SQLite observer plus lifecycle/cleanup regression; target-only refresh parses only the target and writes no unchanged-owner edges. |
| Invalid Markdown retains last-valid edges/FTS and existing diagnostics | `invalid_owner_keeps_last_valid_declarations_as_targets_change`; source-snapshot warning and exact invalid-metadata messages remain visible, rather than falsely reporting a clean check. |
| Any mutation failure preserves the complete old publication | Faults after nodes, demotion and promotion compare independent raw revision/nodes/files/both edge tables/AUTOINCREMENT/FTS backing tables; retry publishes exactly one revision. Unit test checks outgoing preflight before demotion/FTS mutation. |
| Lexical cursors and page-bounded native sorter retention | `pages_retain_limit_bounded_top_k_for_every_query_variant`: 72 actual production query variants, 4096 neighbors per type/direction, reverse key/URI allocation, limits 1 and 32. Both platforms pass in final debug/release runners. |
| Incompatible 1/2/3 and malformed 4 reject before persistent vector access | `storage_cache_upgrade`: 4 tests/31 data scenarios, including 10 malformed layouts × vectors present/absent and frozen schema-3 rejection. Independent per-entry preservation checks cover direct open, status and reindex. |
| Cache-only rebuild preserves vector identity, bytes, retries/config/query error and makes no inference calls | Reversed cache/vector numeric allocation, literal float blobs/hash/KNN oracles, exact vector/config/failure/query-error records/inodes and zero local fake-provider requests. |
| Active schema/docs/knowledge match current code; historical bytes remain | Executable/design DDL parity; exact source tree comparisons, scoped knowledge check/search/inspect, local links and frozen byte audit, receipts below. |
| Architecture and public contracts stay focused | Private storage `edges.rs` owns persistence; existing refresher owns orchestration; traits/boxed transaction signatures, domain purity and `lib.rs` wiring unchanged. No new runtime, dependency, shared ownership, forwarding service or full-corpus text buffer. |
| Test helper handles early argv rejection without hiding response failures | Task 4 deterministic `early_argv_rejection_preserves_response_after_closed_stdin` waits for actual child rejection with stdin still open, then sends input. Only BrokenPipe is tolerated; existing exit/status/JSON/no-service assertions remain. |
| Complete root/native acceptance and final input reconciliation | Corrected host and Linux runners PASS; every recorded command exits 0 and every final input hash matches. Exact per-profile totals below. |
| Independent final reviews | Tasks 1–4 specification/quality approved. Whole-branch With fixes (I1/M1/M2), one correction wave `c1f2810`, then clean scoped rereview; final code/evidence readiness approved. |

## Pre-review-fix commands and native receipts

Python launcher: `/opt/homebrew/bin/python3.12` 3.12.14. Native execution uses
escalation because sandboxed macOS process inspection causes unrelated permission
failures. Host build uses the prepared isolated target and no incremental/debug
symbols; Linux uses Docker desktop-linux, arm64 and the pinned Dockerfile, with
runtime network disabled. No real/paid provider traffic is sent.

```sh
CARGO_TARGET_DIR=/tmp/issue90-task3-fresh-clippy CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform host --evidence .artifacts/issue-90/integer-edges-host-20261104T105947Z
BUILDX_CONFIG=/tmp/issue90-task4-buildx DOCKER_CONTEXT=desktop-linux /opt/homebrew/bin/python3.12 -B tests/storage/run.py --platform linux --evidence .artifacts/issue-90/integer-edges-linux-20261104T105947Z
/opt/homebrew/bin/python3.12 -B tests/storage/check_schema.py .
/opt/homebrew/bin/python3.12 -B .artifacts/issue-90/integer-edges-task-evidence/verify-knowledge.py
/opt/homebrew/bin/python3.12 -B .artifacts/issue-90/integer-edges-task-evidence/verify-links.py
git diff --check
```

| Platform | Debug and release, each | Other native acceptance |
| --- | --- | --- |
| macOS 26.6.2 arm64; Rust 1.95.0; Git 2.56.0; Python 3.12.14 | 1,048 passed, 0 failed, 11 ignored; 77 harness/doc-test segments | Runner 10/10, native capability 1/1, explicit deadline 1/1 (60.01s), strict docs, copied standalone/linkage PASS. |
| Linux 7.0.14 aarch64/glibc 2.36; Rust 1.95.0; Git 2.50.1; Python 3.11.2 | 1,042 passed, 0 failed, 11 ignored; 77 harness/doc-test segments | Runner 10/10, native capability 1/1, explicit deadline 1/1 (60.97s), strict docs, copied standalone/linkage PASS. |

Totals include two compile-fail doctests per profile. The six extra macOS tests
are two cfg(macos) Git-read observer tests compiled into each of three integration
binaries; Linux does not compile them. Eleven ignores are nine private subprocess
entry points exercised by their parent tests, the separately executed deadline,
and the existing deferred `target_corpus::observe_existing_corpus` entry point.
The exact target/ignore inventory is in `final-reconciliation.json`.

Both native stacks report SQLite 3.53.2, vec1 0.8 NEON, WAL, synchronous=2 and
foreign_keys=1. Both copied standalone executables pass FTS, native-vector exact
bytes, reopen and revision 1; otool/ldd report no dynamic SQLite or vec1 dependency.
The 72-variant bounded top-K regression passes in debug/release on both platforms;
Task 1's detailed host VM programs and pinned source excerpt are retained.

Each successful runner includes `make verify V=1`: formatting, all-target cargo
check/Clippy, full debug tests, strict Rustdoc and release tests. It also runs
runner unit tests, the real 60-second local fake-provider deadline, native
capability test, copied standalone executable, exact vector bytes/reopen and
`otool -L`/`ldd` linkage inspection. Successful complete runs are not duplicated;
new runs below are required by a failed run and changed test input hashes.

`final-reconciliation.json` recomputes all **276 host** and **233 inner Linux**
input hashes against the actual final tree. Linux outer captures the same 276
host inputs. All command exits are zero: host 16, outer Linux 8, inner Linux 14.
There are zero hash mismatches or missing changed product/build/test/schema
inputs. Receipts record source commit `246f8c17697bacb192e48c376cd66d695a3273e2`
plus dirty paths; hashes, rather than that pre-documentation commit alone, identify
the tested tree. Final prose/knowledge/report hashes are recorded separately
because the standard runner hashes executable/build/test/schema inputs.

The 43 host-only inputs are listed individually in the reconciliation and verified
byte-equal to revision `166aaa8`. Docker intentionally copies root Rust tests,
needed fixture/support trees and storage runner inputs, not every historical
CLI/performance/release/target-corpus/source-contract harness file. The Git oracle
installer is a special case: Docker consumes it in the build stage via `/tmp`,
then removes it before the inner `/repo` inventory; the outer hash still covers
it. No changed input is omitted. The controller independently confirmed the same
hashes/exits/counts and frozen bytes in
`.artifacts/issue-90/integer-edges-controller-reconciliation.json`; this receipt
audit preceded the independent code reviews and does not replace them. No unexecuted historical harness is implied to
have passed merely because its bytes were inventoried.

## First-run failure and narrow fixture correction

The first Linux receipt `.artifacts/issue-90/integer-edges-linux-20261104T105328Z`
is **FAIL**, not hidden or overwritten. Debug
`cli_runtime::malformed_requests_fail_before_service_directory_creation` failed
at `tests/support/cli.rs:92`: the fixture unconditionally unwrapped `write_all`
after an invalid-argv child could exit before reading stdin. The product had
correctly returned invalid_request. `early-argv-stdin-probe.json` deterministically
reproduces four invalid argv cases: safe JSON, exit 1, empty stderr, then EPIPE 32.

The controller explicitly extended Task 4 ownership to a narrow test-only fix.
`OwnedChild::send_input` tolerates only `ErrorKind::BrokenPipe`; every other write
error still fails. Callers retain all response, exit and side-effect assertions.
One new `cli_runtime` regression waits for the child with stdin still open before
writing, without sleeps or changes to production parsing. RED: exit 101 at the
old unwrap. GREEN: all 5 cli_runtime tests pass. A compiled mutation replacing
BrokenPipe tolerance with PermissionDenied fails that regression; byte-restored
helper then passes all 5 again. These receipts are in the new task evidence.

The concurrent first host run was deliberately interrupted during release
compilation because its recorded test inputs were superseded. Its distinct
`.artifacts/issue-90/integer-edges-host-20261104T105328Z` manifest records
`KeyboardInterrupt`, interrupted make exit -9; no successful host acceptance is
claimed for it. Both initial receipts remain intact. Corrected native runs use
new evidence directories and hashes for the actual final test tree.

## WAL-reader preservation limit

The final review disproved the earlier whole-file preservation claim for
writable direct/reindex opens: DELETE journal initialization changed main bytes,
and writable close could checkpoint committed WAL and remove sidecars. I1 fixes
that order by recognizing the **exact** schema read-only before writable open.
The existing bootstrap still revalidates after concurrent initialization.

Rejected cache main/WAL bytes and identities, committed logical data, vectors,
configuration and locks are preserved. Direct open, status and reindex can each
create an empty reader WAL and SHM index (32768 bytes on the tested stack).
An existing SHM can update reader marks/index bookkeeping while retaining its
identity and safe lifetime; SHM bytes are not guaranteed immutable. Status stays
read-only and all readers observe committed WAL. No product immutable bypass,
writable status or sidecar unlink is used. Vector storage is not opened on refusal.

The original Task 3 raw-reader probe and strict failure remain historical in
`integer-edges-task-evidence/task-3-wal-probe/`. Four final-review probe directories
are copied byte-for-byte to `integer-edges-final-review-fix/`, with source paths
and SHA-256s in `preserved-review-probes.json`. No live user cache was touched.

## Restored behavioral mutations

These are **16 pre-review-fix effective mutants for this revision**: Task 1 seven, Task 2 five,
Task 3 three, plus one Task 4 fixture mutant. They are separate from the original
schema-3 report's historical campaign. Every listed mutant compiled, failed its
owning behavioral assertion and was restored before focused/full final checks.

| Task | Mutation | Behavioral detector |
| --- | --- | --- |
| 1 | Resolved target affinity integer → text | Independent raw schema/endpoint type assertion. |
| 1 | Target FK → nodes.uri | Independent FK target assertion. |
| 1 | Bypass cross-table duplicate rejection | Duplicate after target UPSERT no longer poisons pending declaration unit. |
| 1 | Demote with wrong target URI | Literal deletion/reappearance declaration assertion. |
| 1 | Skip commit promotion | Edge-before-target is not resolved at commit. |
| 1 | Remove SQL LIMIT | Native VM detects unbounded SorterOpen. |
| 1 | Move outgoing preflight after demotion/FTS | Transaction-local old projection changes before rejection. |
| 2 | Insert edges before source reconciliation | Exact operation sequence has edges before source node UPSERTs. |
| 2 | Skip unchanged-owner promotion | Traversal omits the newly available anchored target. |
| 2 | Omit per-owner delete_outgoing | Valid owner edit fails from retained declarations. |
| 2 | Mark every owner changed | Target-only refresh reinserts unchanged declarations. |
| 2 | Commit after promotion before revision | Raw old-publication snapshot exposes leaked nodes/FTS/files/declarations after failure. |
| 3 | Accept frozen schema 3 as current | Direct open unexpectedly accepts old cache. |
| 3 | Join KNN membership by cache ID | Literal public KNN identities reverse. |
| 3 | Clear query_error on reopen | Expected retained query-error record becomes absent. |
| 4 | Replace BrokenPipe tolerance with PermissionDenied | Deterministic early-argv helper regression fails at the error-kind assertion. |

One Task 2 exploratory ordering attempt removed the only production call to a
method and was rejected by the compiler before tests ran. It was restored and
replaced by the effective reordering mutant above; it is **not** a behavioral
kill. Task 3's obsolete inherited rebuild oracle first failed at schema version
3 versus 4; added recognition tests already passed existing Task 1 behavior, so
no artificial pre-implementation RED is claimed for them.

## Pre-review-fix preserved evidence and exact knowledge basis

All new supplementary receipts are in
`.artifacts/issue-90/integer-edges-task-evidence/`; original task-4 receipts and
source snapshots are not overwritten. `preserved-task-evidence.json` records
source paths and SHA-256 comparisons for 28 copied files: 16 Task 1 receipts,
5 Task 2 mutant logs, 3 Task 3 mutant logs and 4 WAL probe/strict-failure files.
Only named evidence directories were copied, without targets or broad temp trees.
Controller archives SDD scratch after final review; this task does not delete it.

The new complete source tree is `637231a8227b17b60de15af5f6a3e7622e054bd4`.
Every addressed whole-file source byte is compared with `git show` from that tree;
14 sources cover the two existing knowledge concepts, including private edges.rs.
`knowledge-source-hashes.json` retains exact SHA-256s and the separate historical
basis. Scoped check has no findings, search finds both concepts without flags,
and inspect's one reading snapshot matches all 16 knowledge/source files and
reported hashes. The reading tree and author source basis have distinct roles.
All commands run in an isolated temporary Git corpus; no repository cache is
created for this documentation check. Source basis verification confirms bytes
and provenance; it is not a production corpus or arbitrary full-closure audit.

Executable/design cache and vector DDL parity passes; the final local-link
check covers **887 links/anchors with zero findings** across 11 affected/historical
navigation documents, excluding fenced/inline code examples. `git diff --check`
passes. Frozen byte audit (`frozen-inputs.json`):

| Preserved input | SHA-256 |
| --- | --- |
| `src/storage/vectors.sql` | `8887d0947c9e9fbf15bb4b27eb10d1b2fbfede34abd3d2a504e47b3483973f50` |
| `tests/fixtures/storage/cache-v3.sql` | `ddabeba5678cd62cbb68367f595a71a3864ebf0a55c255a5630563d1c49f400a` |
| `tests/fixtures/storage/cache-v2.sql` | `57d25817073a6522326e4370bdbf328f8a1efd76c6edb274818ce0f4a79b1ac6` |
| `tests/fixtures/storage/legacy-cache.sql` | `8ecc01afe902405194fa6dbc6cc84680d56916a8ab11d3aecf3d3836dd6bb9d5` |
| `tests/fixtures/storage/legacy-vectors.sql` | `2730146067837208abf0698b08d6632c1c3efcbf766b1f16c1717b3147a0e6ca` |

Schema 3 is byte-equal to the original verified product DDL at `166aaa8`; schema
2 equals the pre-issue product DDL at `f0d5b8b`. Legacy fixtures and product vector
DDL equal their preserved revisions. The original report is also byte-equal
(SHA-256 `4536208ff43e8bf2cf09e876de5edc52a519441c3ab40d44d325085d5a44ff24`).

## Self-review and limits

Actual physical files, visibility, imports, trait implementation direction and
composition were inspected. Only the private storage implementation from Task 1
and the existing document service from Task 2 own new product behavior. The Task 4
fixture correction is test-only; Given/When/Then phases have described setup,
action and assertions. No SQL/domain/runtime/interface change is hidden in docs.
The original report remains byte-equal to its captured revision. Historical
read-workflow invariants, schemas and proof harnesses retain their original scope.

Host/Linux evidence covers the recorded arm64 environments, native linkage,
compatibility, concurrency and tested rollback. It does not establish x86_64,
physical power-loss durability, all filesystem fault schedules, production
migration, real-provider quality or latency/throughput. Historical standalone
harnesses with separate manifests are not implied to have been rerun.

The whole-branch review requested I1/M1/M2 corrections. One consolidated fix wave
at `c1f2810` addressed them; the scoped independent rereview approved their closure
and final code/evidence readiness, with no new or out-of-scope finding.
No merge, push, PR update, publication or issue closure was performed.

## Final-review correction (I1, M1, M2)

I1's root cause is permitting a writable cache handle before schema recognition.
Moving only `journal_mode=wal` would still allow checkpoint/removal on close.
`SqliteStorage` now observes existing cache files read-only, in a pinned read
transaction with bounded initialization contention handling, before opening its
writer. Normal paths use the existing native opener; confined paths reuse
`ConfinedDirectory::open_observed_database` and its pinned directory/leaf checks.
Missing/empty files still initialize; writable bootstrap repeats exact recognition
and rechecks under initialization ownership. No new dependency, interface, runtime,
lock policy, service or vector schema was introduced. One extra read-only open
and schema comparison is an unmeasured startup cost; no speed claim is made.

M1 updates all active guarantees and the projection knowledge: existing SHM
reader bookkeeping may change, while main/WAL bytes and identities stay strict.
M2 replaces MC-1's obsolete join-only/no-recalculation text with MC-2 promotion/
demotion; unchanged owners still need no reparsing or declaration reinsertion.

Four new native integration tests exercise 24 independent fixtures: direct open,
status and reindex, each with vectors absent/present, across closed schema-3 DELETE,
retained committed WAL, live WAL with existing SHM, and malformed schema-4 DELETE.
The retained WAL fixture uses native `SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE`, not a
helper process or unsafe code. A separately copied main file is supported schema 4
with no nodes; WAL contains exact frozen schema 3, source `(71, wal-only.rs)` and
revision 7. Thus the tests prove committed-WAL visibility as well as byte/inode
preservation. The copied review crash probe remains evidence of the original
abrupt-exit failure; the regression fixture reproduces its retained-WAL state,
not physical power loss.

RED against `5872c9e`: all four new tests failed persistent `cache.db` preservation.
Focused GREEN: 40 integration tests (15 bootstrap, 4 journals, 4 upgrade, 5
corruption, 12 status) and 46 storage unit tests passed, 3 unit tests ignored.
Strict Clippy passed after correcting one `doc_markdown` backtick finding.

Three new distinct semantic mutants compiled and failed behaviorally: bypass
direct preflight, bypass confined preflight, and ignore committed WAL during
read-only opening. The WAL-blind mutant was run twice: first caught by persistent
bytes, then with status first, where exit 0/`ok:true` was caught against expected
refusal. Both observations are retained. All mutations were byte-restored and
focused journal/upgrade checks passed. The total revision campaign now has **19
distinct effective mutants**, with the earlier 16 retaining their historical scope.

Final native acceptance and exact input reconciliation **PASS**. Both use the
unchanged runner and new directories `.artifacts/issue-90/integer-edges-host-review-fix-20261104T113831Z`
and `.artifacts/issue-90/integer-edges-linux-review-fix-20261104T113831Z`.
Host uses Python 3.12.14 and the isolated target; Linux orchestration uses the same
Python and Docker desktop-linux, with Python 3.11.2 inside the pinned container.
Prior successful native receipts remain pre-fix evidence.

| Final platform | Each debug/release profile | Additional acceptance |
| --- | --- | --- |
| macOS 26.6.2 arm64 | 1,052 passed, 0 failed, 11 ignored; 78 segments | `make verify V=1`, runner 10/10, native 1/1, explicit deadline 1/1, strict docs, standalone/linkage PASS. |
| Linux 7.0.14 aarch64/glibc 2.36 | 1,046 passed, 0 failed, 11 ignored; 78 segments | Same acceptance PASS. |

SQLite 3.53.2 / vec1 0.8, WAL/FULL/foreign keys and host fullfsync confirmed.
The 11 ignored entries are unchanged: nine subprocess-only helpers, the deadline
test (explicitly executed separately and passed), and `observe_existing_corpus`
(requires an external corpus and remains unrun). Detailed per-target counts and
ignored names are in `integer-edges-final-review-fix/final-reconciliation.json`.
No ignored/unrun test is relabeled as acceptance.

All 277 host inputs, 277 Linux outer inputs and 234 Linux inner inputs match the
actual tree; no changed input is omitted. The 43 host-only files absent inside
Linux are byte-equal to `5872c9e`. All 16 host, 14 inner Linux and 8 outer Linux
commands exit zero. Logs prove the four new journal regressions ran in debug and
release on both platforms. The separate final docs/link audit checks 889 local
links/anchors with no findings; frozen fixtures/vector DDL/original report remain
byte-equal to the reviewed base.

| Final receipt | SHA-256 |
| --- | --- |
| Host manifest | `a6d138090d9083594c025b2c76cc5ab0b9428a14f989e39ed22a3ebfac95d530` |
| Linux outer manifest | `0a37351fcfe836acd8aa15ddf51d718811da331c7f5e099900e5539cc4bfbfa6` |
| Linux inner manifest | `1f04f04f43833424c91f27bdf06bfa9430272b21eaea474573ab781da4691848` |
| Host standalone executable | `bae1d28276478c160f5ecd52906ef666ce23a1beb4bb9e5904d5c203445ea11a` |
| Linux standalone executable | `af41ea40902ac0f38b06ce3b55fe1cee2fe774e85a4b1b4f2c5870410e3cd41c` |

The new projection source basis is `c10d57c3676b3c54105d3c896e9f7f19e7cfe3d7`.
Exact bytes of all 16 addressed source files match its complete temporary-index
Git tree; real index and HEAD were untouched. Scoped check reports no findings,
needs-recheck search returns no entries, and inspect matches all 18 source/knowledge
files and hashes in one reading snapshot. These checks use an isolated Git corpus.
The prior `637231a...`/`2952c3b...` bases and receipts remain historical.

### Execution rulings and costs

1. Scoped local implementation commits were allowed for SDD review packages.
   Reason: review needs concrete reversible revisions. Cost: branch history gains
   task commits; independent approval still gates completion, and this grants no
   merge/push/PR/issue authorization.
2. New empty reader WAL/SHM artifacts were allowed after native READ_ONLY evidence.
   Reason: preserve live-WAL visibility without unsafe cleanup. Cost: callers may
   observe extra reader files; zero-filesystem-effects is not a promise.
3. Task 4 narrowly tolerated stdin BrokenPipe after an early argv-validation exit.
   Reason: the existing test helper raced legitimate rejection. Cost: an early
   child exit could be hidden at the write boundary, so response/status assertions
   and its semantic mutant remain required. The correction is test-only; its
   deterministic RED/GREEN evidence and first interrupted receipts are retained.
4. Existing SHM reader-index updates refine ruling 2. Reason: the final live-WAL
   probe demonstrated ordinary READ_ONLY read-mark changes. Cost: SHM bytes can
   change while identity/lifetime are preserved. Read-only preflight means the
   reader-artifact exception now applies to direct open and reindex as well as
   status. It never permits main/WAL persistent changes, vector opening, immutable
   reads or sidecar removal; I1 remained a required product fix.

Controller independently reconciled the final receipts against the actual tree:
`integer-edges-controller-review-fix-reconciliation.json` confirms 277/277/234
matching inputs, 16/8/14 zero-exit commands, all six changed Rust/test inputs
covered, unchanged frozen/vector bytes and the same 1052/1046 profile totals.
This is an evidence audit; the separate scoped independent code rereview also
approved the correction and final readiness.

## Independent review closeout

| Gate | Recorded outcome |
| --- | --- |
| Task 1: persistence and lifecycle | Specification compliant; quality approved. The minor mixed-union ordering coverage observation was addressed by the later literal-order regression recorded in the acceptance matrix. |
| Task 2: nodes-first publication | Specification compliant; quality approved; no findings. |
| Task 3: compatibility/vector safety | Specification compliant; quality approved. The reader-artifact phase-comment observation was corrected in the subsequent documentation/acceptance work. |
| Task 4: contracts and native acceptance | Specification compliant; quality approved; no findings. |
| Original whole-branch review, `f0d5b8b..5872c9e` | **With fixes**: I1 persistent rejection mutation, M1 SHM guarantee, M2 MC-1 wording. The original findings and probes remain preserved. |
| Single correction wave, `5872c9e..c1f2810` | I1/M1/M2 corrected with fresh host/Linux acceptance and exact receipt reconciliation. |
| Scoped final rereview of that correction | All three findings addressed; no new Critical, Important or Minor issue; no out-of-scope observation. Final code/evidence readiness **approved**. |

The task verdicts are recorded in `task-1-review.md` through `task-4-review.md`;
the original final verdict in `final-review.md`, and the clean correction verdict
in `scoped-final-rereview.md`, retained with the controller's SDD review artifacts.
The scoped reviewer inspected the diff, sources and recorded evidence without
rerunning suites. This administrative closeout changes only specification/plan/
report status and checkboxes; all 277/277/234 native input hashes remain unchanged.
Historical receipts, source snapshots and all four execution rulings/costs above
retain their scope. No merge, push, PR update, issue update or publication occurred.
