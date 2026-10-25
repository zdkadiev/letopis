# TK-21 provenance — independently accepted implementation

Date: 2026-10-25. Product revision: `cd1d94ab7709901d34bdb29dbf7d226c51d982eb`. Task3 base: `16f837aa119dbb7c68c1af43093130bf4834b614`.
Approved scope: [spec](../../../../superpowers/specs/2026-10-25-tk-21-provenance-design.md).
Three independent task reviews and the whole-branch final review approved the scoped implementation; exact review receipts and evidence limits are recorded below.
Machine-readable full hashes, criteria, commands and executable manifests: [verification.json](verification.json).

## Result and accepted boundaries

Tasks 1–2 supply shared SourceProvenance/BasisRetainer and the pre-publication retention gate.
Task3 wires live source observations into check/search through their existing single pinned readers.
Check replaces the root-only private algorithm, groups Source targets by knowledge, preserves full graph validation and owner selection, emits exact safe reasons, sorts/deduplicates.
Search observes only ranked bindings, merges source flags with stale, and preserves ranking, limit, query-provider semantics and the exact X-1 Link keys `element_id/path/lines/revision/flags`.
Missing source/basis retains knowledge, FTS and links; no source text/excerpts/new fields are returned.
Canonical edit preserves the prior explicitly retained field; removing the field removes canonical basis, including rebuild. Letopis never writes Markdown.
HTTP/HTTPS remain Unfetched without fetch or false 404. C1/C2 comparison is a service primitive; JSON filters remain TK-22 and source CLI inspect remains TK-24.

## Verification

- One final `make verify V=1`: exit 0, fmt/check/Clippy, strict Rustdoc and root debug/release suites passed on frozen product inputs.
- Debug: **594 passed / 0 failed / 5 ignored**, 43 executable Cargo targets.
- Release: **594 passed / 0 failed / 5 ignored**, 43 executable Cargo targets.
- Totals take the LAST outer summary per Cargo Running target. Nested subprocess summaries and the 0-test Rustdoc target are excluded.
- Focused command `cargo test --locked --offline --test provenance_observation --test provenance_revisions --test provenance_indexing --test provenance_commands --test check_command --test check_lifecycle --test knowledge_search --test search_consistency --test search_contention --test source_projection`: **64 passed / 0 failed**, 10 targets. Private check7/search13 passed; final successful-query scoped regressions2 passed, then both rerun in final debug/release.
- Full logs: `.artifacts/tk21/task3/make-verify.log`, `green-focused-final.log`, `controls-restored-cli.log`, `controls-restored-check.log`, `controls-restored-search.log`, `scoped-available-query.log`.
- `inputs-pre-verify.json` equals `inputs-post-verify.json`; all 298 inputs match committed product blobs. Supplemental six schema/build fixtures equal original branch base. Receipt contains before/after hashes including new tests, executable snapshots bracketing verification and log digests.
- Native runner not required/run: storage/native/schema/build/vendor/dependencies are unchanged from branch base. No historical receipts were rewritten.

## Acceptance mapping

Normal regression tests for every row ran under final `make verify V=1`; the two manual controls ran separately with exact restoration. Expected behavior, commands and evidence are in verification.json.

| Requirement | Representative test | Actual |
| --- | --- | --- |
| Spec §2/§8 and card architecture | `service::check::tests::pinned::check_observes_metadata_and_bindings_from_original_pinned_view` (+1 tests) | pass; physical audit below |
| Spec §3 and card edit basis | `preserve_declared_basis_on_edit_and_reproduce_it_after_cache_rebuild` (+2 tests) | pass |
| Spec §4 and card indexing/pin/history | `retain_basis_before_publishing_and_keep_history_after_gc` (+3 tests) | pass |
| Spec §5 and card changed/not_found | `observe_exact_binary_range_and_ignore_unaddressed_edits` (+3 tests) | pass |
| Spec §5 HTTP/HTTPS | `leave_remote_unknown_without_file_reads_or_retention` (+1 tests) | pass |
| Spec §6 and card freshness flags | `search_observes_source_only_changes_without_new_cache_revision` (+4 tests) | pass |
| Spec §6 graph, sorting and owner selection | `check_paths_filters_owners_after_full_graph_validation` (+2 tests) | pass |
| Spec §7 and card C1/C2 primitive | `compare_different_trees_by_one_addressed_pair` (+3 tests) | pass; JSON filters integration deferred to TK-22 |
| Spec §9 safe failures and mandatory Task2 handoff | `service::search::tests::scoped::metadata_failure_after_query_releases_pinned_view` (+2 tests) | pass in debug and release |
| Spec §9 negative controls and evidence | `search_observes_source_only_changes_without_new_cache_revision` (+1 tests) | pass; each control exit101, 0 passed / 1 failed |

## Required post-provider cleanup boundary

`service::search::tests::scoped::metadata_failure_after_query_releases_pinned_view` exercises actual SearchService with real SqliteStorage, deliberately malformed `nodes.created_at`, a controlled no-op refresher, and a successful controlled query vector (no HTTP/model charge).
The query collaborator is called once before the real candidate metadata read fails. The service returns safe InternalError with private cause and no fabricated successful result.
With the service still alive, real WAL checkpoints succeed for cache/vectors; metadata repair and subsequent publication advance revision and checkpoint again. This passes in debug and release.
The separate Task2 early-refresh corruption test still asserts zero provider calls; it does not stand in for this later boundary.

## RED → GREEN and controls

`red-commands-runtime.log`: 1 pass / 5 fail on real absent source flags (initial missing-limit input failure is preserved separately in `red-commands.log`, not claimed as behavioral RED).
`red-private.log`: 0 pass / 1 fail because old check ignored a missing source address.
Initial focused failures in check snapshot fixture and knowledge search exact expectation were real contract changes: absent code.rs now adds SourceNotFound; repairing root metadata alone with an empty tree cannot repair addressed bytes. Fixture now repairs both actual file/tree and keeps exact complete JSON assertions. First knowledge-link test now expects precisely SourceChanged + SourceNotFound for its missing basis/missing source.
Private check verifier moved to SourceProvenance and observes per knowledge; the old per-OID root-only cache was removed with its algorithm and obsolete comment. Pinned metadata and bindings are asserted separately.
Initial Clippy type-complexity finding in private recording structs was fixed with explicit private type aliases, no allowance.

Two selected manual controls compiled and failed at runtime (each exit101, 0 pass / 1 fail):

1. Remove search source flag merge (consume observed flags and remove unused mut to preserve compilation): `search_observes_source_only_changes_without_new_cache_revision` fails.
2. Suppress only check SourceNotFound issues: `missing_source_remains_searchable_and_check_reports_owner` fails.

Both product files restored byte for byte; SHA256 values recorded in controls.json and receipt match final files. Restored CLI6/private check7/private search13 all pass, followed by final make verify. Earlier Task1/Task2 controls remain their own historical evidence, not newly executed controls here.

## Physical architecture audit

src/service/check.rs owns CheckService and its trait impl; SourceProvenance replaces SnapshotChecks entirely. All nodes and external Source targets come from the same view; selection occurs after graph diagnostics. Issues become existing safe CommandProblem fields and stable sort/dedup.

src/service/search.rs owns SearchDependencies and SearchService; generic SourceProvenance observes only ranked knowledge bindings read from the existing reader. No second read/write, hidden pin, provider/ranking/limit changes or filters implementation.

src/lib.rs performs actual constructors with independent GitCli/LocalFileSystem instances for check/search provenance. Service product imports contain application/domain types, no concrete Git/filesystem/storage adapters. Domain/adapters/main unchanged.

Existing service/mod.rs declarations/reexports and typed requests remain as agreed in Task1; no new public module or adapter interface. No new dependencies/runtime/process/cache/schema/history.

Public Checker/Searcher/SearchFlag and X-1 Link comments finalized before make verify. Debug derived on concrete generic search assembly types; private causes remain safe. Explicit imports, verb names, caller/helper order and <=60-line methods reviewed; Clippy confirms selected policy.

New private src/service/check/tests/pinned.rs and src/service/search/tests/scoped.rs isolate concurrent read and cleanup fixtures to keep parent test files focused (494 and 389 lines). These are test submodules, not product boundaries or future scaffolding.

## Scope and limits

Task3 product paths: src/lib.rs; src/service/check.rs and tests.rs; new check/tests/pinned.rs; src/service/search.rs and tests.rs; new search/tests/scoped.rs; src/service/command.rs and command_traits.rs; src/cli/output.rs comments; new tests/provenance_commands.rs; narrow tests/check_command.rs and tests/knowledge_search.rs expectation updates.
I-4/I-7, MC-4 and the TK-21 card record approved signatures and boundaries. Test submodules are cohesive fixtures, not new product scaffolding.

Root skipped entries remain five: three private child entrypoints invoked by parent process tests, the real 60-second deadline entry, and existing-corpus observer entry. The latter two were not explicitly selected in this run.
Linux and historical standalone manifests were not rerun; no paid/real-model pilot, G-1/G-2/resource gates or v1 readiness is claimed. No merge, push, GitHub or Engram writes. Live bytes remain per-file observations without multi-file atomicity.

## Independent acceptance and execution closure — 2026-10-25

All three task gates and the independent whole-branch review are approved, with no open Critical/Important/Minor findings. The final review accepts all five Review Focus cases and reports **Ready to merge: Yes** for the approved TK-21 scope. This is acceptance evidence, not a merge/push action or a claim about broader gates.
Execution was approved Subagent-driven after the user superseded the initial Native choice; all 16 planned steps are complete. The verified product revision remains `cd1d94ab7709901d34bdb29dbf7d226c51d982eb`; reviewed head `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` adds acceptance docs only. This final reconciliation also changes documentation only.
Local ignored review/evidence receipts are retained against their captured revisions; SHA256 values below identify their exact contents. Historical input, binary and log hash tables remain intact.

| Review | Scope and verdict | Model / effort | Reviewed base → head | SHA256 |
| --- | --- | --- | --- | --- |
| [task-1](../../../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-1-review.md) | Task1 shared provenance API, exact byte observation, revision comparison, retention and read-only wiring. Spec compliant; task quality Approved; no open findings. | `gpt-6.1-sol` / `high` | `f3167b91e24efec51287b0f85d9b590033425c34` → `1d8b2e057401b3456ccc0e7185a04f75e28f1a91` | `c1f0f2480ca2ab3b4e56ebf13ff5102540f85bdf8a69f8966125ae624f89ae95` |
| [task-2](../../../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-2-review.md) | Task2 pre-publication retention gate, locking, unchanged refresh and last-valid basis; mandatory late cleanup carried to Task3. Spec compliant; task quality Approved; no open findings. | `gpt-6-astra` / `high` | `1d8b2e057401b3456ccc0e7185a04f75e28f1a91` → `16f837aa119dbb7c68c1af43093130bf4834b614` | `46ba3a163ae949ad17228c84992b90bacb55d59587f26dd9e93995a35cd0d300` |
| [task-3](../../../../../.superpowers/sdd/2026-10-25-tk-21-provenance/task-3-review.md) | Task3 check/search, X-1, same-view bindings, mandatory real-storage post-query metadata-error cleanup and acceptance evidence. Spec compliant; task quality Approved; no Critical/Important/Minor. | `gpt-6.1-sol` / `high` | `16f837aa119dbb7c68c1af43093130bf4834b614` → `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` | `5a38ac039b4c69f90c384baa6ea1af137270175b3b6bb9e5c429e9a95b97d5d7` |
| [final](../../../../../.superpowers/sdd/2026-10-25-tk-21-provenance/final-review.md) | Whole branch: all37 changed files including implementation, tests, spec/plan/contracts/card/report/receipt; all5 Review Focus cases satisfied. Ready to merge: Yes for approved TK21 scope; no Critical/Important/Minor. | `gpt-6-astra` / `high` | `9b798be522782e64375f4cbd293b76778e517117` → `a61507d8827fc5fd75c0b4d47698f9a259fbd73b` | `05bd79b116991c1142e86e0aed3419aaed0026d83372e596d0f102d6a0db555a` |

Provider-failure evidence limit: the existing barrier test drops its temporary SearchService before checkpointing and alone does not establish cleanup while that service remains alive. The reviews inspected local reader ownership and RAII; the mandatory post-query metadata-error test separately proves live-service checkpoint/publication after a successful controlled query and real metadata failure. No stronger provider-live runtime claim is made.

JSON search filters remain TK-22; source CLI inspect remains TK-24. Linux/native-runner reruns, historical standalone harnesses, real 60-second deadline, corpus/paid/real-model pilot, resource gates, G-1/G-2 and v1 readiness remain unclaimed. No unchanged product suites were rerun for this documentation-only closure. No Engram/GitHub/merge/push action is performed here.
