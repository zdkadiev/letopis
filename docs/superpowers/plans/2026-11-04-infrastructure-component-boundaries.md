# Infrastructure component boundaries implementation plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development task-by-task. Steps use checkbox syntax for tracking.

**Goal:** Apply the user's component ownership and entity–action–filter conventions across Git, Markdown and the remaining infrastructure, preserving behavior.

**Architecture:** Services/composition select application policy, while each infrastructure component exposes its own concepts and a single main implementation per actual responsibility. Git returns raw worktree paths, Markdown retains its existing X-4 codec, and process launch is generic. Domain and application services retain their established responsibilities.

**Tech Stack:** Existing synchronous Rust toolchain, gix, rusqlite/SeaQuery and ureq; no new runtime or dependencies.

**Spec:** [Infrastructure component boundaries](../specs/2026-11-04-infrastructure-component-boundaries-design.md); [filesystem boundary](../specs/2026-11-04-filesystem-boundary-design.md).

**Workspace and order:** `/Users/zdkadiev/Repos/letopis`, branch `rafactor`. Complete and review filesystem tasks 1–2 before changing their shared service contracts or consumers. Parallel read-only audits and planning are safe; do not run simultaneous source-mutating implementers or mutation tests against the same checkout. User already authorized implementation and parallel work.

## Global Constraints

- `AGENTS.md` is canonical: one main concrete struct per cohesive component, domain-specific interfaces/types/errors, infrastructure entity–action–filter names and service use-case names. Real independent owners, resources, values and guards remain; no speculative wrapper components.
- A trait method may proxy a same-named private function outside `impl`, with the struct and trait implementations together. Application services use receiver methods for helpers accessing injected collaborators.
- Preserve CLI JSON, snapshot/index/ref integrity, paths and exclusions, no-follow/identity/race/alias protection, transaction and lock behavior, synchronous execution, provider security and exact X-4 semantics.
- Git operations remain in-process; provider HTTP stays outside storage writes. Preserve separate cache/vector owners, status snapshot/short-circuit semantics and SQL aggregation. `StatusService` owns status-mode policy and interpretation; storage exposes database observations and retains SQL/integrity validation. Individual SQL action names use exact table and schema-column names and `save/get/delete/upsert/update`.
- New/modified tests use descriptive Given/When/Then phases and independent expectations; record effective manual mutations. No paid provider calls, dependency changes, real user-cache writes, push or main merge.
- Each task ends in focused tests, a source-only local commit and independent task review; fix review findings before the next source-mutating task. Run full verification once after the final integrated source.

## Review Focus

1. An irrelevant non-UTF-8 name or a final symlink with a non-UTF-8 name must not fail document discovery; a relevant ordinary non-UTF-8 name must fail safely (Task 1).
2. A missing tracked path remains visible for deletion; symlinked parents and I/O/replacement failures do not become silent absence (Task 1).
3. Direct snapshot creation of a private root path still fails before publishing any object, while normal paths and historical reads work (Task 1).
4. A failed detached process launch reports the existing safe warning without leaking executable or environment secrets; argv/cwd/session behavior remain identical (Task 3).
5. Storage identity, count and missing-vector queries retain malformed-data and transaction behavior after naming changes (Task 6).

## File responsibilities

- `src/git/`, relevant service interfaces/types/errors and refresh/inspection: Git-native worktree and snapshot operations, with candidate selection in application code.
- `src/filesystem/` and generic service file interfaces: only the generic no-follow symlink observation needed to preserve candidate filtering, if absent.
- `src/markdown/`, `service/mod.rs` and consumers: X-4 parsing and coordinated `file_parse` naming.
- `src/process/`, `src/embedding/`, `src/cli/`, their service interfaces, `lib.rs` and callers: generic launch, single relevant owner and component-first naming.
- `src/storage/`, relevant service traits and callers: existing ownership and SQL behavior with consistent query names.
- `AGENTS.md`, active architecture documentation and final report: canonical rules, actual wiring, verification and limits. Historical documents/evidence retain their captured meaning.

## Task 1: Git-native interface and document selection

**Files:** `src/git/{repository,corpus,objects,refs,reader,tree,durability,cleanup,validation}.rs`, relevant `src/filesystem/` generic helpers, `src/service/{mod,types,error,document_refresher,inspection,provenance,utils}.rs`, `src/lib.rs`, Git/document tests and cancellation observations.

**Interfaces:** Consume the reviewed filesystem contracts. Produce `GitWorktreePaths::worktree_paths_get(&self) -> GitResult<Vec<u8>>`, where each returned path is nonempty and NUL-terminated; no UTF-8 or document-policy requirement. If necessary expose generic `FileReader::symlink_is_exist(&self, path: &Path) -> FileResult<bool>` with confined no-follow behavior. Choose the smallest Git-specific error that preserves safe diagnostics and real causes. Snapshot operations use entity-first names; implementations, consumers and test doubles agree. Composition supplies generic excluded tree-root names to `GitRepository` and direct `open_git_snapshots` continues rejecting `.letopis` tree creation.

- [x] Add and run regressions for a non-UTF-8 irrelevant filename, a non-UTF-8 Markdown symlink, a non-UTF-8 ordinary Markdown file, deleted tracked Markdown, unsafe parents and ignored/untracked/sparse-index entries. Assert concrete candidate results/error classifications; capture a failing boundary test before changing code.
- [x] Add and run direct-public-snapshot regressions asserting `.letopis` requests fail before any Git object is written for both tree-creation forms; capture a meaningful RED when the policy is removed. Keep normal snapshot and historical reads intact.
- [x] Return raw paths from Git and move document selection to application code. Share raw filtering and parsing as data-only helpers; filesystem observations remain service receiver methods or a meaningful private operation-scoped receiver, never a free helper forwarding injected collaborators. Do not introduce a pass-through service. Preserve byte-filter/symlink/UTF-8 ordering and filesystem confinement. Skip raw filesystem probes for structurally unsafe components and retain existing `RepoPath` normalization/root-escape validation; do not add blanket rejection that changes current behavior. Add the generic filesystem observation only if needed, including a no-follow regression for raw path bytes.
- [x] Replace borrow-only Git helper wrappers with private functions, apply entity-first operation names and preserve existing verification/retention semantics. Update app-owned traits, all consumers/test doubles and active cancellation symbol observations together.
- [x] Run focused Git, snapshot corruption/retention, refresh, inspection and filesystem tests; mutation-check filtering, path rejection and symlink handling. Record exact output, intended-path commit and independent task review.

## Task 2: Markdown component naming

**Files:** `src/markdown/*.rs`, `src/service/{mod,document_refresher,inspection,search,traversal,embedding}.rs`, Markdown integration tests and any test doubles/public callers.

**Interfaces:** Consume the existing X-4 types and return values. Produce `MarkdownCodec::file_parse` and `markdown::file_parse` with unchanged arguments/results; retain `MarkdownParser` as the only main concrete implementation.

- [x] Confirm independent existing tests assert exact parsed elements, link errors, spans, search text/hashes and persisted lifecycle behavior; no new behavioral test is needed solely for renaming.
- [x] Rename the trait, concrete implementation, free operation, related private implementation functions and Serde callback references consistently; update active callers and test doubles. Keep grammar and projections unchanged.
- [x] Run Markdown elements, links, navigation, search and lifecycle tests plus compile checks; record results, intended-path commit and independent task review.

## Task 3: Generic detached process

**Files:** `src/process/detached.rs`, `src/service/{mod,error,background_runner}.rs`, `src/lib.rs`, process/CLI/background tests and doubles.

**Interfaces:** Produce an application-owned `ProcessLauncher::process_spawn(&self) -> ProcessResult<()>` and process-specific failure. The existing `DetachedProcess` directly owns its executable, generic argument vector and working directory; composition chooses `embed --drain`. `BackgroundRunnerService` retains the existing use-case names and exact public warning.

- [x] Write generic launch regressions for literal executable, arbitrary arguments, working directory and missing-executable private cause/redaction. Add an independent assertion for the existing warning `cannot start background embedding drain`; capture meaningful RED or mutation evidence.
- [x] Replace role-specific launch with the configured generic process owner and typed failure; composition supplies the real arguments. Preserve null stdio, inherited environment, detached session, exact command/cwd and no wait. Construct the fixed warning at its existing call sites rather than introducing a forwarding component or fixed-response helper.
- [x] Run CLI process, session-failure, background and cancellation tests, and mutation-check launch parameters/warning. Run required `make check`; record results, intended-path commit and independent task review.

## Task 4: Embedding provider ownership and naming

**Files:** `src/embedding/*.rs`, `src/service/{mod,embedding,configuration,init,status}.rs`, `src/lib.rs`, provider and embedding tests/doubles.

**Interfaces:** Retain a single `HttpProviders` implementation with separate native and compatible HTTP pools and private credential state. Produce `EmbeddingProvider::config_validate` and `vectors_compute_by_texts` with unchanged data/behavior; application `Embedder` use cases retain their names.

- [x] Confirm independent existing tests exercise both wire protocols, credential validation/redaction, proxy/redirect policy, deadline, response-size limits and malformed vectors. Add an independent owner-Debug redaction assertion where absent; capture relevant mutation RED.
- [x] Consolidate dispatcher/protocol helper structs into the existing provider owner and private protocol functions, retaining two independently constructed HTTP agents and exact protocol/credential semantics. Rename component operations and corresponding traits/callers together; do not introduce a runtime, dependency or wrapper.
- [x] Run provider and embedding behavior/boundary tests and the required `make check`. Record results, effective mutations, intended-path commit and independent task review.

## Task 5: CLI and signal naming

**Files:** `src/cli/*.rs`, `src/process/{signal,input,mod}.rs`, `src/lib.rs`, `src/commands.rs` if needed, CLI/process/cancellation tests and test doubles.

**Interfaces:** Preserve command/JSON contracts, response serialization, cancellation/signal lifecycle and the existing `finish_foreground` use case. Rename CLI/signal infrastructure operations with entity-first naming; application-service methods and external trait methods remain unchanged.

- [x] Inventory CLI and signal operations/callers after Tasks 3–4; retain external API names and existing Serde semantics. Existing independent CLI/signal tests are the behavior baseline for naming-only changes.
- [x] Rename component operations, their imports/reexports, callers and Serde callback strings together; update literal cancellation stack observers to the actual symbol while retaining thread/native-frame checks. Keep construction/environment access in composition.
- [x] Run focused CLI process, parsing/output, signal and cancellation tests plus required `make check`; record results, intended-path commit and independent task review.

## Task 6: Storage naming and status-policy ownership

**Files:** `src/storage/{cache,vector}.rs`, `src/service/{mod,types,status,graph,traversal,embedding,utils}.rs`, service-status/storage/graph tests, `tests/status_command.rs` and test doubles.

**Interfaces:** Preserve `CacheStorage`/`VectorStorage`, closure scopes, schemas, observable status results and legitimate multi-statement storage operation names. `StatusService` selects Disabled/Unavailable/Ready behavior and interprets database observations; storage retains SQL aggregation, attempt validation/filtering order, decoding and integrity checks, without accepting `EmbeddingStatusPolicy`. Individual SQL names use the exact table/action/filter grammar in `AGENTS.md`; where two traits naturally have the same query name, resolve calls through trait qualification rather than adding a wrapper component. Non-executing SQL-expression builders are not individual executing SQL actions.

- [x] Inventory actual query naming gaps and existing test expectations. Add independent regression tests before touching queries where a rename/refactor could alter identity classification, counts, missing vectors or scope behavior; observe RED for a semantic mutant rather than counting a missing symbol as behavior.
- [x] Move status-mode selection and backlog/failed interpretation to `StatusService`; expose the minimum database observations from the existing storage owner. Retain config capture timing, the same status read transaction, mode-specific short circuits, SQL-side aggregation, checked arithmetic, integrity checks and exact errors. Test that Disabled and Unavailable/mismatch do not request coverage or exhausted-failure observations; Disabled suppresses query-error diagnostics, but Unavailable/mismatch must still expose them. Disabled must not bypass the existing status-scope metadata/revision validation.
- [x] Correct the identified single-query names and callers while preserving SQL semantics. Rename expression builders distinctly where needed; preserve malformed-identity classification. Resolve trait-name collisions explicitly without relaxing `AGENTS.md` or merging separate transaction owners.
- [x] Run focused storage/graph tests and required `make check`; record exact results and restored mutations. Commit intended paths and request independent task review.

## Task 7: Cross-component guidance and active proof controls

**Files:** touched Git/refresh tests and helper ordering, `tests/cli/{negative_controls.py,run.py,test_runner.py}` as necessary, `AGENTS.md` only if rules need reconciliation, active `docs/zkit/letopis/` and `docs/subsystem-map.md`, source-linked `letopis/` knowledge.

**Interfaces:** Preserve current application behavior, current Markdown/X-4 semantics and exact verified source bindings. Historical receipts and captured compatibility fixtures remain unchanged; active proof scripts must actually compile and detect behavioral mutants.

- [x] Resolve the Task 1 review's deferred style findings on the final source: caller-before-callee order for touched selection helpers and embedding HTTP operations, the touched CLI test helper before its consumers, long crate-qualified paths in the new status test fake, and fully descriptive Given/When/Then comments plus current ownership language in touched Git/refresh tests. Do not retrofit unrelated untouched tests.
- [x] Review every component's files, visibility, types, imports, trait implementations, boundary vocabulary and `lib.rs` wiring against the user's rules. Update active architecture docs and record findings/remaining exceptions with justification; explicitly update the prior status-aggregation placement to distinguish service-owned policy from storage-owned SQL observations, and reconcile `docs/subsystem-map.md` Markdown interface naming to `MarkdownCodec` throughout. Reconcile active `mechanics.md` and the older D-101 wording with the accepted issue #84/D-97/X-4 rule: final symlink candidates are skipped, unsafe parent components and observed replacement remain errors. Preserve historical decision text by adding an explicit successor clarification. Reconcile all affected `letopis/` knowledge claims and local source bindings against final source, including the removed Git corpus/filesystem database paths, process and provider ownership, Git operation names and repository conventions. Inspect every binding in an element before updating its `source_snapshot`; do not infer that a changed source automatically makes prose false or blindly retag it. Preserve `created_at` and historical evidence. Recheck the full subsystem map against the actual component layout.
- [x] Audit the active `tests/cli/negative_controls.py` and runner after the final component names settle. At baseline `66eb8d5`, `kernel-exclusion`, `candidate-hash` and `reopen-cache` anchors no longer match source, and the provider rename also changes the `captured-config` anchor; the `live-inspect-reread` injected snippet still uses the removed `get_file` API. Restore genuinely compiling controls and their independent behavioral oracles without rewriting historical receipts; run their relevant Python runner tests and record any unavailable dependency/platform limit. Also verify the CLI runner still executes its named lock cases rather than relying on a zero-test success.
- [x] Recheck the CLI stdin observer on final source with retained logs: restoring the old parser symbol must fail the new-symbol test, and separately weakening the main-thread, native-frame and parser-owner checks must fail the negative test. Restore every mutation byte-for-byte. The original Task5 RED/mutation outputs were transient; do not present them as archived evidence.
- [x] Run affected proof-script and documentation/link/source-basis checks, plus focused Rust tests if touched; record exact results and independent task review. The full Rust API/source `make verify` remains the immediate combined Task 8 gate.

## Task 8: Integrated macOS/Linux acceptance and final review

**Files:** final combined source and test tree, fresh ignored evidence directories, a new report under `docs/superpowers/plans/`; no product changes unless a failure is diagnosed and fixed.

**Interfaces:** Consume the completed, independently reviewed component work. Report actual results separately for macOS and Linux and preserve all earlier receipts.

- [ ] On Linux, explicitly run and mutation-check both platform-only invalid-byte regressions: `database_path_tests::rejects_non_utf8_targets_resolved_from_utf8_symlinks` and `service::document_refresher::corpus::non_utf8_symlink_is_skipped_before_decoding_but_regular_markdown_fails` (or their renamed successors). Restore source byte-for-byte before collecting final native input-hash receipts.
- [ ] Run the host and Linux storage acceptance runners in fresh evidence directories on the final combined source; each runner invokes `make verify`, so do not separately duplicate a successful full run. Record exact commands/results, hashes and platform limitations in both the promised filesystem-boundary report and a combined infrastructure-boundary report, with links between them. Independently review the whole branch and fix important findings before claiming completion. Commit intended changes and reports; leave publication for a separate user request.
