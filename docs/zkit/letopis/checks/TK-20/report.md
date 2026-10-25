# TK-20 — source identity and current availability

Date: 2026-10-25. Delivery: implementation and service-level proofs prepared;
the final-review fix is verified; scoped re-review and shared-memory handoff remain pending.
The task stays `in_progress`; G-1/G-2 and v1 readiness are unassessed.

The [approved specification](../../../../superpowers/specs/2026-10-25-tk-20-source-identity-design.md)
and [plan](../../../../superpowers/plans/2026-10-25-tk-20-source-identity.md)
define the accepted scope. Task 1 (`71ff596`) and Task 2 (`838c316`) passed their
independent task reviews. This report describes integrated service behavior,
without claiming future CLI source inspect, historical comparisons or reindex.

## Acceptance map

| Criterion | Evidence and result |
| --- | --- |
| Canonical local whole/range identity is the only source ID; remote spelling is opaque | `tests/source_addresses.rs` and `tests/test_source_contract.py`: local normalization, invalid ranges/paths, remote query/fragment/case preservation, unsafe/malformed rejection and X-1 SourceId parity. Shared pure `LocalSource` selects exact binary/LF/CRLF/empty/unterminated bytes. |
| Minimum effective referencing knowledge timestamp, duplicate links, invalid-file retention and orphan lifecycle | `tests/source_lifecycle.rs::derive_minimum_from_effective_references`: literal `source.txt#L2-L3`, time 3 → 8; invalid referrer retains time 8 and edge/FTS; deletion removes source; time 11 recreates it. Separate earliest-referrer edit test recomputes time 8. |
| Whole-file and range remain distinct; missing source does not delete knowledge | `preserve_whole_and_range_nodes_when_source_file_disappears` retains four nodes/two edges and FTS; refreshed source hashes become None. `tests/source_reading.rs` independently observes disappearance/return without Markdown changes. |
| Cache reconstruction reproduces canonical identity/times and explicitly upgrades old URLs | `rebuild_source_identity_from_canonical_files` checks literal IDs/times for whole/range/remote before and after fixture cache deletion. `repair_legacy_url_only_after_explicit_cache_rebuild` seeds bound fixture SQL `https:/Host/a`, rebuilds from unchanged `https://Host/a`, and proves no legacy source remains. |
| Upgrade preserves canonical data and durable embedding state | Legacy fixture preserves Markdown/Git/config/embed.lock bytes; index.lock inode persists and its bytes survive deletion itself. Normal refresh may rewrite owner PID per MC-5. `vectors.db` bytes and vector `[1.0, 0.0, 0.0]`, attempt count 1 and failure sentinel are independently asserted. |
| Unknown/orphan source gives not-found; missing local bytes have no hash; remote stays unfetched | `service::source::tests` instruments filesystem reads; membership precedes I/O, unknown/knowledge/remote use zero capture calls. SourceReader returns None / SourceNotFound / Unfetched as appropriate. `tests/source_reading.rs` proves pinned membership and current-byte observations without publication. |
| Expected absence differs from unsafe/I/O/storage failure; publication remains atomic | Private/public source tests cover safe typed failures; symlink/nonregular input and deliberate fixture storage corruption remain errors. Lifecycle collision test asserts unchanged complete state/revision, retained FTS and absent rejected text. |
| Architecture remains modular, no source text storage or remote I/O | Reviewed actual domain/service/adapter/composition files listed below. No storage schema, dependency, process or provider change; result contains metadata/state only. |

## Current final-review fix and verification

One confirmed P2 is fixed in `src/domain/source.rs`: a local path whose dot/parent
normalization exposes case-insensitive `http:`/`https:` is rejected as InvalidPath
before publishing SourceUri. It cannot produce an unreadable persisted identity.
Raw absolute remote URLs remain exact and opaque; RepoPath/KnowledgeId are unchanged.
Root-local filenames with these reserved prefixes require a rename or a containing
directory such as `docs/https:notes`, as recorded in the specification and X-4/I-4.

The current executable tree is covered by the separate
[final-fix verification receipt](final-fix-verification.json): base
`aee592970efd5fa4a3482f7d3d38a1bf577228de` plus its exact dirty input map.
All **291** source/test/build/schema input hashes match before/after verification.
One new `make verify V=1` passed: debug and release each **551 passed, 0 failed,
5 ignored across 39 root binaries**, with the counting method below. Restored
focused checks passed **26 Rust and 2 Python tests**. New logs live only under
`.artifacts/tk20/final-fix/`; the historical Task 3 receipt and its 20 evidence
hashes remain unchanged. The historical phase below records its own tested tree.

Runtime RED proved all three failures before the fix: ambiguous source acceptance,
fresh-cache actual CLI `internal_error` / `cannot read graph diagnostics`, and
invalid edits bypassing last-valid retention. New regressions cover literal
case/range variants, accepted local/remote identity/classification round trips,
repeated safe CLI checks over fresh SQLite, empty node/edge rows for invalid files,
repair without rebuilding, preserved prior graph/FTS, and removal of the referrer.
An initial GREEN attempt found a fixture assumption: ReadingRepo also contains
unmarked `live.md`; the test now expects two files and locates `bad.md` explicitly.
That failed attempt is retained as `green.log`; no additional product fix was needed.

The single control removed only the new guard and reproduced the CLI failure
(exit 101). Before/restored SHA-256 of `src/domain/source.rs` is
`6081ea9a355783d4cbffd0878d9801fc22798d7ce682ece48b257ad402b0852e`;
restored focused tests then passed before final verification. No other controls
or historical suites were rerun. Physical review confirms the only product change
is pure domain validation, reusing the existing scheme predicate; imports,
visibility, service-owned traits, storage decoding and constructor wiring are
unchanged. Native/storage/build/vendor/dependencies/schema are unchanged, so the
conditional native runner is not required. Ignored/platform/consumer limits below
still apply. Scoped re-review and administrative acceptance belong to the controller.

## Historical Task 3 verification and input identity

The machine-readable [verification receipt](verification.json) holds the exact tested
implementation revision, dirty path list, SHA-256 input map, platform/tool versions,
command exits, selected control results and log hashes. Logs live in the local
`.artifacts/tk20/task3/` directory and are not historical receipts rewritten in place.

Tested revision: `838c31611d71eb1591b0ca2248a1e218b16c8799` plus new
`tests/source_lifecycle.rs` and the exact dirty documentation paths in the receipt.
Before/after SHA-256 maps contain **291 identical inputs**, including X-1 CLI schema.
Later report/card/plan/spec completion edits are documentation-only and excluded from
the executable input map. Host: macOS 26.6.2 arm64; rustc/cargo 1.95.0,
Git 2.50.1; prepared Python 3.12.0 with jsonschema 4.26.0.
All Rust commands use `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target`
and `RUST_TEST_THREADS=2`.

| Command | Result | Local log |
| --- | --- | --- |
| `cargo test --locked --offline --test source_addresses --quiet` | 4 passed, exit 0 | `.artifacts/tk20/task3/source-addresses.log` |
| `cargo test --locked --offline --lib service::source::tests --quiet` | 5 passed, exit 0 | `.artifacts/tk20/task3/source-unit.log` |
| `cargo test --locked --offline --test source_projection --test source_reading --test source_lifecycle --quiet` | 2 + 6 + 6 passed, exit 0 | `.artifacts/tk20/task3/source-integration.log` |
| `/Users/zdkadiev/.codex/worktrees/tk-11/letopis/.artifacts/tk11/python-env/bin/python -B tests/test_source_contract.py` | 2 passed, exit 0 | `.artifacts/tk20/task3/source-contract.log` |
| `make verify V=1` | exit 0; fmt/all-targets check/Clippy -D warnings, debug tests, strict Rustdoc and release tests | `.artifacts/tk20/task3/make-verify.log` |
| `cargo clippy --locked --offline --test source_lifecycle -- -D warnings` | exit 0 (focused preflight) | `.artifacts/tk20/task3/clippy-lifecycle.log` |

Debug: **548 passed, 0 failed, 5 ignored across 39 root binaries**.
Release: **548 passed, 0 failed, 5 ignored across 39 root binaries**.
Counting splits the raw log at each Cargo `Running <root target> (<binary path>)`
marker and takes the **last** test-result summary before the next marker or
`Doc-tests` heading. Binary paths select debug/release. Earlier child-result
summaries in each segment are nested helper invocations and are excluded.
Doc-tests ran zero tests separately in each profile. These totals were computed
from this raw log, without extrapolating any historical TK totals.

Three ignored entries are private filesystem/document/embedding subprocess entry
points driven by parent tests. Two acceptance tests remain intentionally unrun:
`attempt_obeys_real_sixty_second_deadline` (separate real provider deadline) and
`observe_existing_corpus` (external corpus/pilot request required). There was one
final make verify run, which includes the required check steps; no redundant
full make check run was added. Storage/native/build/vendor/manifests are unchanged
from branch base, so the conditional native storage runner was not required.
No Linux, standalone linkage/native-runner or historical separate-manifest harness
execution is implied by these root suites. Documentation links and `git diff --check`
were checked separately; no executable inputs changed during verification.

New lifecycle tests initially produced four passes and two fixture failures:
`created_at=0` was valid rather than malformed metadata, and index.lock is normally
rewritten with the refresh owner's PID. Those assumptions were corrected without
product changes (retained `lifecycle-initial.log`). The corrected baseline and
restored lifecycle suites both passed six tests. These are already-green regression
proofs over Tasks 1/2, not a fabricated product RED phase.

Selected controls (each runtime exit 101): replace minimum with maximum → time 8
instead of literal 3; suppress orphan deletion → a source remains after its last
referrer disappears. The focused lifecycle test detects each. Exact restoration
SHA-256 of `src/service/projection.rs` before/after:
`b2edb92289ce385889946c749636017b118ba3b1e35b6047db9f49e5c13145f3`.
`controls.json` and `control-*.log` retain failures and restoration; the restored
suite passes. Task 1's two and Task 2's three controls remain historical local
receipts in their own directories, with no claim that those controls were rerun here.

## Architecture review

- `src/domain/source.rs`: pure validated source classification/normalization and exact byte selection, reused by projection and observation; no SQL/filesystem/network imports. `domain/mod.rs` explicitly reexports public domain types.
- `src/service/mod.rs`: application-owned SourceReader and documented Debug result/availability types. `src/service/source.rs`: crate-private concrete SourceService with generic FileSystem, membership before capture, caller-owned pinned StorageRead and safe typed errors.
- `src/service/projection.rs`: reconciles effective references/minimum/orphans in the existing atomic write path. Remote metadata precedes local path grouping; one local capture serves affected ranges using shared domain selection.
- `src/lib.rs`: actual `LocalFileSystem::open(root)` → `SourceService::new` construction behind SourceReader; no discovery/refresh/storage reopening or side effects added to reader setup. Technology implementations stay in their owning modules.
- Storage persists metadata/edges only; source observations expose no bytes. Domain/service contain no HTTP client or concrete storage imports. CLI handlers remain unchanged and do not call adapters directly.
- Task 3 adds only `tests/source_lifecycle.rs` and documentation/evidence. Public product APIs are unchanged from reviewed Tasks 1/2. New helper/test bodies remain focused; final fmt/Clippy/Rustdoc verify the actual input tree in addition to this structural review.

## Upgrade and limits

See the [explicit upgrade procedure](../../../../development.md#tk-20-явная-пересборка-кэша-при-обновлении).
Stop all repository Letopis/drain processes, close SQLite handles, remove only
quiescent cache.db and present cache sidecars, then use existing check/refresh.
Preserve vectors/config/locks/Markdown/Git. No live or repository-owned cache was
deleted during this task: tests remove disposable fixture caches only, after
storage/read/refresher handles leave scope. No automatic URL migration is claimed.
The root README assumed by the plan did not exist; the controller approved using
`tests/cli/README.md` and `docs/development.md` rather than adding an entry point.
The controller also reconciled literal lock-content wording with MC-5 PID updates.

CLI inspect source consumers remain TK-24; historical source_snapshot comparison
and source_changed remain TK-21; filters/traversal/status/SIGINT remain their later
tasks. The common typed service is the acceptance boundary, with no new public CLI
fields or flags. Full FR-20/FR-25 coverage, pilot/resource measurements and G-1/G-2
are not established by this work. Independent final branch review, shared Engram
recording and any authorized GitHub/merge/push/closure actions belong to the controller.
