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
- Git operations remain in-process; provider HTTP stays outside storage writes. Preserve separate cache/vector owners and approved status-query ownership; individual SQL action names use exact table and schema-column names and `save/get/delete/upsert/update`.
- New/modified tests use descriptive Given/When/Then phases and independent expectations; record effective manual mutations. No paid provider calls, dependency changes, real user-cache writes, push or main merge.
- Each task ends in focused tests, a source-only local commit and independent task review; fix review findings before the next source-mutating task. Run full verification once after the final integrated source.

## Review Focus

1. An irrelevant non-UTF-8 name or a final symlink with a non-UTF-8 name must not fail document discovery; a relevant ordinary non-UTF-8 name must fail safely (Task 1).
2. A missing tracked path remains visible for deletion; symlinked parents and I/O/replacement failures do not become silent absence (Task 1).
3. Direct snapshot creation of a private root path still fails before publishing any object, while normal paths and historical reads work (Task 1).
4. A failed detached process launch reports the existing safe warning without leaking executable or environment secrets; argv/cwd/session behavior remain identical (Task 3).
5. Storage identity, count and missing-vector queries retain malformed-data and transaction behavior after naming changes (Task 4).

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

- [ ] Add and run regressions for a non-UTF-8 irrelevant filename, a non-UTF-8 Markdown symlink, a non-UTF-8 ordinary Markdown file, deleted tracked Markdown, unsafe parents and ignored/untracked/sparse-index entries. Assert concrete candidate results/error classifications; capture a failing boundary test before changing code.
- [ ] Add and run direct-public-snapshot regressions asserting `.letopis` requests fail before any Git object is written for both tree-creation forms; capture a meaningful RED when the policy is removed. Keep normal snapshot and historical reads intact.
- [ ] Return raw paths from Git and move document selection to application code. Share raw filtering and parsing as data-only helpers; filesystem observations remain service receiver methods or a meaningful private operation-scoped receiver, never a free helper forwarding injected collaborators. Do not introduce a pass-through service. Preserve byte-filter/symlink/UTF-8 ordering and filesystem confinement. Skip raw filesystem probes for structurally unsafe components and retain existing `RepoPath` normalization/root-escape validation; do not add blanket rejection that changes current behavior. Add the generic filesystem observation only if needed, including a no-follow regression for raw path bytes.
- [ ] Replace borrow-only Git helper wrappers with private functions, apply entity-first operation names and preserve existing verification/retention semantics. Update app-owned traits, all consumers/test doubles and active cancellation symbol observations together.
- [ ] Run focused Git, snapshot corruption/retention, refresh, inspection and filesystem tests; mutation-check filtering, path rejection and symlink handling. Record exact output, intended-path commit and independent task review.

## Task 2: Markdown component naming

**Files:** `src/markdown/*.rs`, `src/service/{mod,document_refresher,inspection,search,traversal,embedding}.rs`, Markdown integration tests and any test doubles/public callers.

**Interfaces:** Consume the existing X-4 types and return values. Produce `MarkdownCodec::file_parse` and `markdown::file_parse` with unchanged arguments/results; retain `MarkdownParser` as the only main concrete implementation.

- [ ] Confirm independent existing tests assert exact parsed elements, link errors, spans, search text/hashes and persisted lifecycle behavior; no new behavioral test is needed solely for renaming.
- [ ] Rename the trait, concrete implementation, free operation, related private implementation functions and Serde callback references consistently; update active callers and test doubles. Keep grammar and projections unchanged.
- [ ] Run Markdown elements, links, navigation, search and lifecycle tests plus compile checks; record results, intended-path commit and independent task review.

## Task 3: Generic processes and component naming

**Files:** `src/process/*.rs`, `src/embedding/*.rs`, `src/cli/*.rs`, relevant `src/service/{mod,error,background_runner,embedding,configuration,init,status}.rs`, `src/lib.rs`, provider/process/CLI tests and doubles.

**Interfaces:** Produce a process-specific `ProcessLauncher::process_spawn` with generic executable/arguments/cwd and typed process errors. Composition/application chooses `embed --drain`; background service owns the existing warning. Produce entity-first `EmbeddingProvider::config_validate` and `vectors_compute_by_texts` with existing types/behavior; service `Embedder` use cases keep their names.

- [ ] Write generic launch regressions with literal executable/arguments/cwd, failed launch and private-cause/no-secret assertions. Verify RED against the old role-specific interface, then implement and update wiring while retaining null stdio, detached session and no wait.
- [ ] Consolidate protocol dispatcher and protocol helper structs into one provider owner only where the resulting code retains both existing HTTP pools and credential state. Move protocol-specific logic into private functions and preserve exact auth, endpoint, request, timeout, response and diagnostic behavior.
- [ ] Rename component operations and corresponding traits/callers across embedding, process and CLI, retaining standard trait names and the allowed foreground orchestration name. Keep service use-case names intact.
- [ ] Run focused CLI process/signal/cancellation and provider protocol/boundary tests. Mutation-check launch argv/warning and provider security behavior covered by new tests; record output, intended-path commit and independent task review.

## Task 4: Storage naming and integrated acceptance

**Files:** `src/storage/{cache,vector}.rs`, `src/service/{mod,graph,traversal,embedding,utils}.rs`, storage/graph tests, active architecture docs and a new final report.

**Interfaces:** Preserve `CacheStorage`/`VectorStorage`, closure scopes, schemas, query results and all approved multi-statement use-case method names. Individual SQL names use the exact table/action/filter grammar in `AGENTS.md`; where two traits naturally have the same query name, resolve calls through trait qualification rather than adding a wrapper component. Non-executing SQL-expression builders are not individual executing SQL actions.

- [ ] Inventory actual query naming gaps and existing test expectations. Add independent regression tests before touching queries where a rename/refactor could alter identity classification, counts, missing vectors or scope behavior; observe RED for a semantic mutant rather than counting a missing symbol as behavior.
- [ ] Correct the identified single-query names and callers while preserving SQL semantics. Rename expression builders distinctly where needed; preserve malformed-identity classification and role/status aggregation. Resolve trait-name collisions explicitly without relaxing `AGENTS.md` or merging separate transaction owners.
- [ ] Review every component's files, visibility, types, imports, trait implementations, boundary vocabulary and `lib.rs` wiring against the user's rules. Update active architecture docs and record findings/remaining exceptions with justification. Reconcile active `mechanics.md` and the older D-101 wording with the accepted issue #84/D-97/X-4 rule: final symlink candidates are skipped, unsafe parent components and observed replacement remain errors. Preserve historical decision text by adding an explicit successor clarification. Update `letopis/architecture/markdown-projection.md` after the Git ownership move and verify its source basis against the exact captured bytes.
- [ ] Run the host and Linux storage acceptance runners in fresh evidence directories on the final combined source; each runner invokes `make verify`, so do not separately duplicate a successful full run. Record exact command/results, hashes and platform limitations. Independently review the whole branch and fix important findings before claiming completion. Commit intended changes and final report; leave publication for a separate user request.
