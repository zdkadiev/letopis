# Explicit Index Command Implementation Plan

> **Decision 2026-11-05:** Remove the public `reindex` command entirely. Use `index --full` for full rebuilds. Earlier alias references below record the superseded design.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement issue #94: explicit incremental, verified and full indexing, optional owner-file scopes, and a compatible deprecated `reindex` alias.

**Architecture:** Extend the existing `Refresher` operations with an operation-level path scope; existing consumers pass an empty scope. Filter candidates and removal detection in the application layer, retaining the whole effective graph for references and snapshot verification. CLI parsing, output and wiring reuse existing composition, publication, background and cancellation paths.

**Tech Stack:** Rust 1.95.0, synchronous in-process gix, existing SQLite/rusqlite/SeaQuery storage, serde JSON, existing Unix process fixtures. No dependency changes.

**Spec:** [Issue #94 design](../specs/2026-11-04-issue-94-index-command-design.md).

**Status:** implementation is on the `issue-94-index` branch. The user approved execution and selected forced publication for full mode. Verification findings and remaining platform limits are recorded below; the checklist remains the original execution recipe.

## Global Constraints

- `index --full` always reparses selected candidates and publishes a new revision, including unchanged and empty selections.
- Only incremental modes promise an unchanged revision and `published:false` when there is no file delta after initial publication.
- Omitted or empty `paths` selects every candidate.
- Unselected owner file rows, knowledge nodes, outgoing edges, and FTS rows remain untouched.
- Verified and full modes still audit all effective declared source closures before publication, including retained projections outside the selected owners.
- No new lock, schema, runtime, dependency, worker, or publication retry is introduced.
- No foreground provider inference or direct foreground vector writes are added.
- `reindex` retains its existing empty-object request and `{revision}` result; existing implicit refreshes remain unscoped.
- Follow [AGENTS.md](../../../AGENTS.md), including layer ownership, naming, Given/When/Then comments, independent expected values, and manual mutation checks. Preserve unrelated work and historical evidence. Create an isolated task worktree at execution time; do not implement on `main`.

## Review Focus

1. A directory selector must not select a prefix sibling; malformed or unsafe out-of-scope raw candidates must not be probed (Task 1).
2. Deleted, renamed, newly ignored, or symlinked out-of-scope owners must retain their cached rows and projections (Task 1).
3. A scoped full reparse must reconcile shared sources using all retained referrers, without refreshing unrelated sources; invalid selected owners retain their previous references (Task 1).
4. A corrupt snapshot referenced only by an unselected owner must still fail verified/full indexing before cache publication (Task 3).
5. Every valid mode-flag order must retain foreground cancellation, while unsupported argv fails before input reads or service creation (Tasks 2 and 3).

## File ownership

| Responsibility | Files |
| --- | --- |
| Application contracts and scoped publication | `src/service/mod.rs`, `src/service/utils.rs`, `src/service/document_refresher.rs` |
| Existing unscoped consumers and test doubles | `src/service/check.rs`, `src/service/inspection.rs`, `src/service/search.rs`, `src/service/traversal.rs`, `src/commands.rs`; current root-crate call sites found with `rg` |
| Typed CLI, help and JSON | `src/cli/input.rs`, `src/cli/help.rs`, `src/cli/response.rs`, `src/cli/output.rs`, `src/cli/mod.rs`; new `src/cli/index_tests.rs` |
| Command assembly and cancellation selection | `src/commands.rs`, `src/lib.rs` |
| Behavioral evidence | new `tests/document_scope.rs`, new `tests/index_command.rs`; existing `tests/cli_help.rs`, `tests/cli_cancellation.rs`, `tests/cancellation_concurrency.rs` and inline process schedules in `document_refresher.rs` |
| Published contract and usage | X-1 schema/prose/examples, architecture/mechanics, `README.md`, `skills/letopis/SKILL.md`, `skills/letopis/references/operations.md` |

Keep the existing flat service layout and inline tests. Do not split the document refresher into new modules as part of this feature.

---

### Task 1: Scope the existing publication pipeline

**Files:** modify `src/service/mod.rs`, `src/service/utils.rs`, `src/service/document_refresher.rs`; migrate current callers/doubles in the files above and current root tests/examples; create `tests/document_scope.rs`.

**Interfaces:**

- Change the three existing `Refresher` signatures to `fn refresh(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>`, `fn refresh_verified(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>`, and `fn reindex(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>`.
- Change `CandidateSelection::get_candidates(&self, paths: &[RepoPath]) -> DocumentResult<Vec<RepoPath>>`; inspection passes `&[]`.
- Add shared data-only `service::utils::is_selected_path(path: &[u8], paths: &[RepoPath]) -> bool`, visible only inside `service`.
- Keep public composition constructors and `RefreshOutcome` unchanged. Migrate existing implicit refreshes and legacy `reindex` to `&[]`; `check` must never pass its findings selectors to publication.

- [ ] **Step 1: Add failing scope tests.** Use `DocumentRepo` and independent literals in `tests/document_scope.rs`, with small inline tests for raw candidate observations:
  - `scope_matches_complete_path_components`: selectors `notes/a` select `notes/a` and `notes/a/x.md`, not `notes/ab/x.md`; empty selectors match all, duplicates do not duplicate work.
  - `scoped_refresh_preserves_outside_owners`: initially publish `notes/a.md` and `other/b.md`; edit both, delete cached `other/deleted.md`, and select only `notes`. Expect `RefreshOutcome { revision: 2, published: true, files_updated: 1, files_removed: 0, invalid_files_updated: 0 }`; outside file/node/edge/FTS values equal their pre-run snapshots. A following incremental scoped run returns revision 2 and all zero counts.
  - `scoped_detection_ignores_unselected_unsafe_candidates`: use the existing mocked Git/filesystem fixtures; no per-file calls for raw non-UTF-8 or unsafe paths outside `notes`, while selected unsafe paths retain current errors.
  - `scoped_removals_follow_selected_cached_owners`: delete/rename/ignore/symlink selected and unselected owners; only selected removals count, and prefix siblings remain cached.
  - `scoped_initial_publication_does_not_expand_to_all_candidates`: first scoped incremental call publishes revision 1 with only selected files; later `refresh(&[])` adds the remainder. Empty/unmatched first scope initializes once; later unmatched incremental scope is a no-op.
  - `scoped_full_rebuild_repairs_only_selected_fts`: remove FTS entries with an independent fixture SQL action; scoped full mode restores only the selected entry and advances revision on each call, even for an unmatched scope.
  - `scoped_full_preserves_unrelated_and_shared_sources`: include a shared source with selected and unselected referrers, an invalid selected owner, and an unrelated source whose live bytes change; preserve unrelated source metadata, retained invalid edges, and the minimum creation time across all surviving shared referrers.
  - Keep omitted/empty/file/directory/unmatched selectors, selected invalid-file counts and selected removal counts visible in scenario assertions.

  Representative assertions for the changed-owner scenario (all outside values come from the independently captured pre-run state):

  ```rust
  assert_eq!(outcome, RefreshOutcome {
      revision: 2, published: true, files_updated: 1,
      files_removed: 0, invalid_files_updated: 0,
  });
  assert_eq!(after.get_file("other/b.md"), before.get_file("other/b.md"));
  assert_eq!(after.get_node("other/b.md"), before.get_node("other/b.md"));
  assert_eq!(repo.search("OriginalOutsideToken"), ["other/b.md"]);
  ```
- [ ] **Step 2: Run the new tests red.** Run `cargo test --locked --offline --test document_scope` and the added inline candidate test filters. The first run must fail for missing scope support; after introducing the signatures, confirm at least one behavioral failure before implementing filtering. Update only necessary signatures/doubles to reach that failure.
- [ ] **Step 3: Implement selection and publication.** Use exact-byte matching or a `/` boundary after a selector; apply it before candidate leaf probes and to cached owners before calculating removals. Pass selectors through both preliminary and locked re-detection. Preserve the whole effective graph and basis preflight. In full mode, mark all sources only for an unscoped run; for scoped full mode, mark sources referenced by selected owners, including invalid owners' last-valid edges, then let projection application track changed references. Preserve current revision-zero initialization and always-publish full behavior.
- [ ] **Step 4: Migrate unscoped consumers without changing behavior.** Use `rg -n 'Refresher|\.refresh\(|\.refresh_verified\(|\.reindex\(|get_candidates\(' src tests examples`; update all current root-crate implementations, calls and mock closures. Assert empty scope in existing service mock tests, especially both `CheckService` branches. Preserve historical proof harnesses and evidence.
- [ ] **Step 5: Verify and mutation-check.** Run `cargo test --locked --offline --test document_scope --test document_refresh --test document_reindex` and `cargo test --locked --offline --lib`. Temporarily remove the removal-scope guard, weaken component matching, and restore global source marking for scoped full; the corresponding tests must fail. Restore each mutation immediately. Run `make fmt`, then `make check`; expect `ok`.
- [ ] **Step 6: Commit only this task's source/tests.** Stage explicit intended files, inspect the staged diff, and commit as `feat: support scoped document publication`. Do not stage unrelated changes or the user's other work.

### Task 2: Expose index modes through CLI, contracts and help

**Files:** modify `src/cli/input.rs`, `src/cli/help.rs`, `src/cli/response.rs`, `src/cli/output.rs`, `src/cli/mod.rs`, `src/commands.rs`, `src/lib.rs`; create `src/cli/index_tests.rs` and `tests/index_command.rs`; update `tests/cli_help.rs`, existing help tests, `README.md`, `docs/zkit/letopis/contracts/X-1-cli.md`, `docs/zkit/letopis/contracts/schemas/X-1-cli.yaml`, `docs/zkit/letopis/checks/contracts/contracts-examples.json`, `docs/zkit/letopis/architecture.md`, `docs/zkit/letopis/mechanics.md`, `skills/letopis/SKILL.md`, and `skills/letopis/references/operations.md`.

**Interfaces:**

- Add CLI-private `IndexMode { Incremental, Verified, Full }`, typed `IndexRequest { mode: IndexMode, paths: Vec<RepoPath> }`, and `Request::Index(IndexRequest)`; keep `Request::Reindex`. The request represents command data, not constructor arguments.
- Add pure `get_index_mode_by_args(args: &[OsString]) -> Option<IndexMode>` in `cli/input.rs`, reexported only at crate visibility. Parsing and `requires_cancellation_handler` share this exact mode recognition.
- Add `ResponseResult::Index(RefreshOutcome)` and private `ResultBody::Index { revision: u64, published: bool, files_updated: usize, files_removed: usize, invalid_files_updated: usize }`; domain/service types gain no transport serialization dependency.
- Add `commands::run_index(request: &IndexRequest, directory: &Path, executable: &Path, cleanup: Arc<GitWriteCleanup>) -> CommandResult<CommandResponse>`; it calls the existing mode-specific refresher and `finish_foreground`. This is command assembly, not a new application service. Keep `run_reindex` as compatible assembly over the same full-mode operation and shared setup where actually useful.

- [ ] **Step 1: Add failing argv, response and product tests.** In `index_tests.rs`, table-test `index`, `index --verify`, `index --full`, `index --full --verify`, and `index --verify --full`; expect Incremental, Verified, Full, Full, Full. Reject duplicates, unknown flags, extra positionals and non-UTF-8 argv before a panic-on-read input is touched. Accept empty/whitespace stdin and normalized/deduplicated `paths`; reject null, wrong types, duplicate/unknown fields, wrapper objects, malformed JSON and invalid paths. Assert the literal envelope `{"ok":true,"result":{"revision":42,"published":true,"files_updated":3,"files_removed":0,"invalid_files_updated":1}}`, including the existing optional warning behavior with no inline drain.
- [ ] **Step 2: Add binary behavior assertions.** In `tests/index_command.rs`, use `ReadingRepo`: one valid file produces revision 1/count 1; the next incremental call produces revision 1/false/zero counts; editing it produces revision 2; making it invalid produces revision 3 with `invalid_files_updated:1` and preserved FTS. Test JSON scopes against the published counts and outside DB rows, full-mode FTS repair, both combined-flag orders, and exact legacy `{"ok":true,"result":{"revision":N}}` compatibility. Assert invalid input creates no `.letopis` state. Add `index --help` and command-list assertions to the existing help suites.
- [ ] **Step 3: Run red.** Run `cargo test --locked --offline --lib cli::index_tests` and `cargo test --locked --offline --test index_command --test cli_help`. Expect unsupported-command/absent-output failures before implementation.
- [ ] **Step 4: Implement typed transport and assembly.** Recognize valid modes without reading stdin; validate JSON paths using existing `RepoPath` normalization and strict object parsing. Dispatch Incremental to `refresh(paths)`, Verified to `refresh_verified(paths)`, and Full to `reindex(paths)`. Match the existing command setup, redacted errors and background completion. Reuse the pure argv helper for cancellation selection, including all valid orders; retain existing help/version precedence and legacy argv rules. Keep `lib.rs` as the composition root.
- [ ] **Step 5: Publish the contract and usage together.** Add `$defs.IndexInput`, `$defs.IndexOutput`, top-level success alternatives and `x-commands.index`; mark `reindex` deprecated without broadening its request/result. Add valid/invalid examples and full-outcome fixtures. Keep argv flags out of stdin schemas. Help and docs must explain incremental cost, expensive forced full publication, scoped owners versus global closure auditing, unmatched scope behavior, `index.paths` versus findings-only `check.paths`, and the deprecated alias. Preserve the recorded historical TK-25 contract/evidence.
- [ ] **Step 6: Verify and mutation-check.** Run the commands from Step 3 plus `cargo test --locked --offline --test reindex_command`. Temporarily map Full to incremental and omit an output count; the full-repair and exact-envelope tests must fail, then restore. Run `python3 -B docs/zkit/letopis/checks/contracts/check_contract_examples.py docs/zkit/letopis`; require `errors:[]` and exit 0. Keep any pre-existing contract-check failure distinct and fix only regressions in scope. Run `make fmt`, then `make check`.
- [ ] **Step 7: Commit intended files.** Inspect the staged diff and commit as `feat: add index command with mode flags and scoped input`.

### Task 3: Prove mode assurance, concurrent re-detection and cancellation

**Files:** extend `tests/index_command.rs`, `tests/cli_cancellation.rs`, `tests/cancellation_concurrency.rs` and the existing inline fixtures/process tests in `src/service/document_refresher.rs`; reuse `tests/support/cancellation.rs`, `tests/support/provenance.rs` and owned-child watchdogs. Change shared test helpers only when needed by multiple consumers.

**Interfaces:** consume Task 1's scoped `Refresher` and Task 2's argv/response contract. No new production interface or environment-controlled production hook.

- [ ] **Step 1: Add assurance and rollback tests.** Extend existing provenance test fixtures to assert that routine mode retains roots, while Verified and Full pin closures, including an unchanged selected scope. Put a corrupt unaddressed descendant in a snapshot referenced only by an unselected cached owner: routine indexing can succeed under its existing addressed-object rules, but Verified/Full fail with one redacted error and unchanged cache state. Cover verification failure before SQL publication, selected invalid-owner basis retention, and an injected late write failure with exact pre-run files/nodes/edges/FTS/revision preservation.
- [ ] **Step 2: Add a deterministic real-process race.** Publish revision 1, change one selected owner, hold `.letopis/index.lock` in the parent, and start two product `index` processes with the same JSON scope. Use existing native flock observations to prove both are waiting, then release the parent lock. Require exit 0 for both, final revision 2, exactly one response with `published:true/files_updated:1` and one with `published:false/files_updated:0`, with no outside changes. Repeat with verified incremental mode. This catches a stale pre-lock delta without sleeps or product hooks.
- [ ] **Step 3: Extend foreground cancellation coverage.** Exercise each valid index argv shape, including both full/verify orders, while reading partial stdin and waiting on `index.lock`. Send SIGINT to the owned process; expect exactly the existing cancelled envelope and exit 1. Preserve the other holder, lock identity, pre-publication cache and detached drain. Test holder cancellation with the existing Git-read barrier where applicable; late cancellation retains already committed publication.
- [ ] **Step 4: Run and mutation-check the process tests.** Run `cargo test --locked --offline --test index_command --test cli_cancellation --test cancellation_concurrency` and `cargo test --locked --offline --lib service::document_refresher::tests::process`. Temporarily bypass locked re-detection, exclude flagged index argv from cancellation registration, and route Verified to routine refresh; the race, cancellation and closure-specific tests must fail respectively. Restore each mutation and prove the final tree passes.
- [ ] **Step 5: Review the actual changed architecture.** Inspect physical files, imports, visibility, trait implementations, scope propagation and `lib.rs` constructor calls. Confirm no consumer terminology leaked into filesystem/Git, no new service wrapper or dependency appeared, and the known alias and global closure guarantees match the final docs. Independently review the complete feature diff using the execution workflow chosen by the user.
- [ ] **Step 6: Run final checks and record results.** Run `make fmt`, `make check`, `make verify`, the contract-example command from Task 2, `git diff --check`, and a relative-link check for changed Markdown. Check both link targets and any changed anchors. Required success is exit 0 and `ok` from each Make target; record exact results, manual mutation evidence, any pre-existing failures, and any unrun/platform-limited checks here or in a new issue-94 report. Run `python3 -B tests/storage/run.py --platform host --evidence <new-directory>` only if storage/native/build inputs changed; do not imply historical harnesses or Linux ran otherwise.
- [ ] **Step 7: Commit final tests and evidence.** Stage only intended paths and commit as `test: verify index assurance and concurrent publication`. Leave the feature branch ready for review; no merge, push, release, or issue update is included.

## Plan self-review

- Scope/mode/alias/outcome semantics map to Tasks 1 and 2; background completion and cancellation map to Tasks 2 and 3.
- All five Review Focus items have explicit assertions; source reconciliation uses complete retained graph data, and closure verification stays global.
- Existing Refresher implementations, mocks, callers and inspection candidate selection migrate together; their unscoped behavior is preserved through `&[]`.
- Setup, contracts and documentation travel with the feature they describe. Final checks are against the actual changed tree; previous evidence is not reused as new proof.
- Before execution, review this plan and the linked proposed design together and choose Native or subagent-driven execution.

## Execution record (2026-11-04)

Implemented the scoped refresher, all explicit `index` modes and outcome fields, the
deprecated `reindex` compatibility path, cancellation registration, contracts,
help and usage documentation. Existing implicit refresh callers use an empty
scope. Added focused scope, CLI and global snapshot assurance tests.

- `make fmt`: exit 0. `cargo check --locked --offline --all-targets`: exit 0.
  `cargo clippy --locked --offline --all-targets -- -D warnings`: exit 0.
- Focused `document_scope`, `document_refresh`, `document_reindex`,
  `provenance_indexing`, `index_command` and `cli_help` integration suites passed
  before the final race case (46 tests total). Seven of eight `reindex_command` tests passed; the eighth
  could not bind its HTTP fixture in this restricted environment (`EPERM`).
- Three inline CLI parser tests and the raw Git path scope regression passed.
  Manual mutations of the directory component boundary, scoped source marking,
  raw path normalization, Full-to-incremental dispatch and removed-file output
  each caused a relevant behavioral test to fail; production code was restored.
- Independent review found raw candidate selection comparing unnormalized Git
  names to normalized selectors. Selection now resolves byte-level dot components
  before scope matching and still avoids probing structurally unsafe raw paths.
  The reviewer also identified gaps; selected removals, scoped FTS repair, shared
  source retention, index lock-wait cancellation and a two-process publication
  test were added. The process race test compiles and passed when the sandboxed
  process sampler was temporarily bypassed, but its deterministic observation
  fails here with `sample failed for pid`; it needs a normal host run.
- Final `make verify` reached the root lib tests after passing formatting, check and
  Clippy; 556 lib tests passed, 33 process, socket, signal and HTTP fixture tests
  failed with `Operation not permitted` in this sandbox. The strict Rustdoc
  command `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`
  passed separately after the final source change. A full release-profile run is not yet available.
- The contract example runner could not import `yaml` (PyYAML and jsonschema
  are not installed in this environment). Both modified schema and examples
  parsed as JSON; the `IndexInput`/`IndexOutput` and command mappings exist.
  `git diff --check` passed. Storage/native/build inputs were not modified, so
  the historical storage runner was not rerun.

The focused non-process suites, final `cargo check --all-targets`, Clippy,
formatting, strict Rustdoc, JSON parsing, link target checks and diff whitespace
checks passed. The real-process race and cancellation observations failed at
the sandbox process sampler (`sample failed for pid`); these require execution
on a host that allows the existing process and socket fixtures. The prior execution record described a commit; the changes were present as uncommitted work when publication resumed.

## Publication verification (2026-11-05)

The public `reindex` command was removed in favor of `index --full`; its former
CLI tests were migrated to the full mode. The rejection test failed before the
removal and passed afterward. `CARGO_TARGET_DIR=/tmp/letopis-issue94-verify make
verify` passed on macOS, including strict Rustdoc and release-profile tests.
`python3 -B docs/zkit/letopis/checks/contracts/check_contract_examples.py
docs/zkit/letopis` passed with no errors. No Linux or storage platform runner
was executed; storage and native build inputs were not changed.
