# TK-18 Graph Validation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development,
> explicitly selected by the user on 2026-10-25.
> User approved plan and selected Subagent-driven on 2026-10-25; execute task-by-task.

**Goal:** Implement deterministic graph validation and reusable current diagnostics
over the caller's existing read view.
**Architecture:** Pure domain rules, one stateless concrete service behind its
application trait, real constructor wiring in lib.rs. No additional read connection.
**Tech Stack:** Rust 1.95.0, stdlib collections, existing StorageRead/rusqlite/SeaQuery,
serde_json. No new dependencies.
**Spec:** [Approved design](../specs/2026-10-25-tk-18-graph-validation-design.md).

## Global Constraints

- User approved specification on 2026-10-25. Worktree/branch tk-18 from
  main@75be0deead9d1317b2fd0f833e2bbc83a526c9b5; baseline make check PASS.
- [D-103](../../zkit/letopis/decisions.md#D-103): develop before deferred G-1 measurements;
  no paid pilot, RAM/load/platform campaign or claim that G-1/TK-17 passed.
- Synchronous I/O; no new runtime, schema, problem table, registry or dependency.
- [Hexagonal structure](../../zkit/letopis/architecture.md#program-structure):
  pure domain; service-owned trait; concrete implementation in its own file; actual lib ctor.
- Caller refreshes/pins; diagnostic service reads the supplied StorageRead and never
  refreshes, writes, opens another transaction or calls a concrete adapter.
- Custom names are verbs; explicit imports, Debug, safe typed errors; about40/max60
  lines per function, focused files, minimal visibility, caller-before-helper order.
- Ordinary changed-code tests/fmt/Clippy/verification remain required. Manual chosen
  mutation controls only, no cargo-mutants sweep.
- Real CLI check/status/traverse remain TK-19/TK-28/TK-23; their acceptance stays pending.
- Commit only authorized task paths when requested; no automatic merge/push/issue closure.

## Review Focus

1. Filtering nodes before diagnostics hides a cross-file cycle or missing-target cause — Task 2.
2. Recursive traversal or repeated SCC member arrays grows stack/space unexpectedly — Tasks 1/2.
3. Arbitrary persisted error text or SQL cause leaks through public findings — Task 2.
4. A writer changes the graph while one caller-owned read must stay pinned — Task 3.
5. Bad markup suppresses other files, or fixing it leaves stale diagnostics — Task 3.

## Physical Files

- Create src/domain/validation.rs: GraphProblem, pure algorithm and inline unit tests.
- Modify src/domain/mod.rs: declarations/explicit public reexports.
- Modify src/domain/graph.rs narrowly: shared domain-owned includes constant for
  derive_element_kind and validation, minimum sibling visibility.
- Create src/service/graph_diagnostics.rs: CurrentGraphDiagnostics struct/impl and private tests.
- Modify src/service/mod.rs: GraphDiagnostics trait and explicit implementation declaration.
- Modify src/service/model.rs: shared DiagnosticKind message/code helpers.
- Modify src/markdown/diagnostic.rs: use the shared message helper, no grammar change.
- Modify src/lib.rs: wire_graph_diagnostics actual constructor.
- Create tests/graph_diagnostics.rs: real public API + DocumentRefresher/Storage integration;
  reuse tests/support/documents.rs without exporting new production test hooks.
- Update task/spec/plan and docs/zkit/letopis/checks/TK-18/report.md with exact results/limits.

### Task 1: Pure graph findings

**Consumes:** validated KnowledgeId/Edge/EdgeTarget/EdgeType.
**Produces:** public Debug/Clone/PartialEq/Eq GraphProblem with DanglingTarget
{source: KnowledgeId,target: KnowledgeId} and IncludesCycle {members: Vec<KnowledgeId>};
public find_graph_problems(&BTreeSet<KnowledgeId>, &BTreeSet<Edge>) -> Vec<GraphProblem>.

- [x] Add inline tests and module/reexports; run
      cargo test --locked --offline --lib domain::validation::tests -- --quiet.
      Expected RED: new function/types unavailable, then failing assertions for missing behavior.
- [x] Pin independent scenarios:
      a->missing as includes+supports yields ONE DanglingTarget(a,missing);
      adding/removing missing node clears/restores it without editing edges.
      Absent source edges and external Source targets yield no knowledge dangling finding.
- [x] Pin SCC scenarios: a<->b, d<->e plus b->c and one-way b->d produce exactly [a,b] and [d,e],
      c is not a cycle member. Ordinary depends_on cycles and shared-child includes DAG are clean.
      Mixed dangling+cycle output lists sorted dangling first, then lexicographic components;
      input nodes/edges remain equal to saved originals.
- [x] Implement iterative two-pass SCC with a private adjacency representation and
      explicit work stacks; use the shared includes constant, no recursion or graph crate.
      Only existing knowledge endpoints enter includes adjacency; no new self-loop variant.
- [x] Add a fixed10_000-node chain: no findings. Add last->first:
      exactly one component whose members equal the independently created ordered node IDs.
      This is a bounded stack-safety regression, not a RAM/load benchmark.
- [x] Run focused tests GREEN, make check. Manually remove includes filtering and
      disable missing-target handling one at a time; covering tests must fail; restore.

### Task 2: Safe caller-view diagnostic service

**Consumes:** Task 1 API plus StorageRead::list_nodes/list_edges/list_files.
**Produces:** GraphDiagnostics trait:

    fn find_problems<R: StorageRead>(
        &self, view: &mut R, paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>>;

CurrentGraphDiagnostics is crate-visible, stateless, Debug/Default, with new().
Task2 also owns src/lib.rs wire_graph_diagnostics() -> impl GraphDiagnostics,
constructed with CurrentGraphDiagnostics::new(), so strict non-test builds stay clean.
DiagnosticKind adds get_message(&self)->&'static str and get_code(&self)->&'static str.
Exact cached-message classification is private to service; unknown/None never echoed.

- [x] Write private/helper tests for safe formatting and classifying all9 existing categories,
      plus cause privacy. Run cargo test --locked --offline --lib graph_diagnostics -- --quiet.
      Expected RED for missing service/helpers; do not add forwarding mocks for every StorageRead method.
- [x] Move existing fixed message strings to DiagnosticKind::get_message; get_code maps
      InvalidUtf8/InvalidMetadata/InvalidAnchor/DuplicateAnchor/OrphanMetadata/NestedElement/
      InvalidLinkTitle/InvalidTarget/SelfLoop to invalid_utf8/invalid_metadata/invalid_anchor/
      duplicate_anchor/orphan_metadata/nested_element/invalid_link_title/invalid_target/self_loop.
      Diagnostic::new_at calls the same helper, preserving every old message byte-for-byte.
- [x] Implement the concrete service from full nodes/edges/files of the passed view.
      Read failure returns InternalError with message cannot read graph diagnostics
      and retained private StorageError cause; no partial success.
- [x] Format code=ValidationFailed for every finding:
      invalid file uses details.path/reason, recognized static parser message/code;
      unknown/None reason -> invalid_markup, message invalid knowledge file.
      Dangling uses path, reason=dangling_target, target AND element_id=source.
      This element_id clarifies the approved owner rule for two anchors in one file.
      Message: knowledge target is missing.
      Cycle uses path, reason=includes_cycle, minimum local member element_id for each
      (SCC,file); message knowledge participates in an includes cycle. Never repeat full members.
- [x] Add real-storage fixtures in the private graph_diagnostics.rs tests module:
      selected a.md must still detect SCC with b.md; duplicate selectors do not duplicate;
      two SCCs within a.md remain separate with representatives a.md#a and a.md#c.
      Different anchored sources to one missing target retain distinct element_id.
      Final order is path/reason/element_id/target; absent optional sort fields use empty string.
- [x] Unknown cached reason SECRET_RAW_BODY must yield only the static fallback.
      A wrapped StorageError cause SECRET_SQL_CAUSE stays reachable through source(),
      but absent from public Display/Debug. Use private error construction test, no public test hook.
- [x] Wire the actual lib.rs constructor/import and public trait reexports now,
      without I/O, extra transactions, CLI wiring or new handlers.
- [x] Run focused tests + existing Markdown tests GREEN, make check.
      Mutation-check filtering nodes before SCC and emitting raw cached reason; restore.

### Task 3: Real composition and publication/read lifecycle

**Consumes:** Task 2 trait + already wired public factory,
existing open_storage/wire_document_refresher and DocumentRepo.
**Produces:** component integration proof and TK-18 report.

- [x] Add public API integration tests in tests/graph_diagnostics.rs; run
      cargo test --locked --offline --test graph_diagnostics -- --quiet.
      Test-only cases may initially pass against Task2; prove strength through chosen
      mutations. If a missing behavior is found, preserve RED before fixing production.
- [x] Verify the actual lib constructor/trait documentation/reexports from Task2.
      Cross-file problems remain read-time; keep CLI Check unavailable, no status/traverse stub.
- [x] Real lifecycle fixture: a.md points at missing.md; bad.md has direct self-loop.
      Refresh then diagnose gives a dangling finding and bad.md self_loop.
      Adding missing.md removes only dangling; correcting bad.md removes file finding.
      Original Markdown bytes stay exactly the agent-written bytes; no file rewriting.
- [x] With anchored knowledge, insert three ordinary prefix lines before its heading.
      After refresh, exact ID a.md#one and author created_at remain unchanged; coordinates
      shift. Duplicate/nested anchors remain parser errors and do not suppress another file.
- [x] Pin read revision, publish a target addition through a SECOND existing storage
      connection, diagnose old view (still dangling), drop old view, open new (clean).
      Assert revision stays pinned. Do not open a second read inside the service.
- [x] In a private temp cache with a real invalid-file row, drop edges using a deliberate
      corruption probe BEFORE opening read; find_problems must return InternalError,
      not a clean/partial report. Raw SQL only in this test-native corruption probe.
- [x] Run focused tests GREEN and selected manual controls for error swallowing/pinned view;
      restore. Apply make fmt, run make check and make verify, retain concise logs.
      No native platform runner unless implementation actually changes storage/native/schema.
- [x] Review physical boundaries/visibility, generic trait use, no raw SQL in service,
      lib actual ctor and nonrecursive SCC; one final independent review in chosen workflow.
- [x] Update report/task with commands, actual findings and pending TK19/23/28 integration.
      Mark component implementation separately from full CLI/G-1/G-2 acceptance; record Engram.
      Preserve unrelated primary checkout/refactor and TK-17 private evidence.

## Handoff

Specification and plan approved; user selected Subagent-driven execution.
User explicitly selected Subagent-driven after reviewing the plan. All3tasks are implemented and independently Approved; whole-branch review Approved,
make verify PASS. Product code/docs remain uncommitted pending user integration choice.


## Execution ruling (2026-10-25)

Strict warnings reject an unused crate-private service before its lib constructor
exists. Task2 therefore owns src/lib.rs constructor wiring as well; Task3 consumes
that existing API for real storage/publication lifecycle tests. Its first tests may
legitimately pass when no production behavior changes; prove them with chosen manual
mutations, and use RED->GREEN for any actual missing behavior. No artificial stub.
No automatic commits: review packages capture exact working-tree file snapshots.
