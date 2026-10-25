# TK-23 Graph Traversal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the complete metadata-only X-1 traverse CLI with deterministic bounded BFS, source links and actually traversed edges.

**Architecture:** Refresh and synchronize existing configuration before opening one caller-owned StorageRead. Read the typed graph once, perform service-owned in-memory BFS, then observe returned knowledge/source links using existing application interfaces. CLI handlers call Traverser; concrete dependencies are constructed in lib.rs.

**Tech Stack:** Pinned Rust 1.95.0, synchronous standard I/O/collections, existing serde/serde_json, Git CLI, SQLite/rusqlite and existing embedding readiness; no new dependency, runtime, schema or native component.

**Spec:** [Approved TK-23 design](../specs/2026-10-25-tk-23-traversal-design.md). Read it and repository AGENTS.md before each task. Status: plan prepared for user review; no implementation execution authorized yet.

## Global Constraints

- Start from clean `tk-23` at `/Users/zdkadiev/.codex/worktrees/tk-23/letopis`, originally `main@912c15fe0faef7dc53c88d47f4ae6c246d7f71ab`; preserve all other worktrees, ignored evidence and user edits.
- “Preserve synchronous I/O, strict CLI validation, physical hexagonal boundaries and the existing refresh/publication path.” No concrete storage/Git/HTTP imports in services or CLI handlers; actual constructor wiring stays in lib.rs.
- “No new SQL/storage interface or persisted graph representation.” Reuse StorageRead::list_nodes/list_edges and the existing SourceReader/SourceProvenance capabilities.
- “Every link carries the same pinned cache publication revision.” Live knowledge/source flags retain the existing separate read-only observation limits.
- “Traverse performs no query embedding, vector ranking or source HTTP fetch.” Reuse Embedder::read_configuration before begin_read to preserve MC-6 readiness/reset ordering; missing/invalid configuration does not prevent graph traversal.
- “Upon appending the node that reaches the limit, stop considering further transitions altogether.” The independent edge cap must not truncate element discovery.
- Open EdgeType grammar, no relation-name allowlist; only includes has containment semantics. Output has no text, scores, edge IDs, kind or new flag fields.
- AAA tests with independent literal expected outputs; meaningful manual production-path mutations must fail at runtime and restore exact bytes. Do not count compile failures or fixture-only sensitivity as successful production controls.
- Methods/tests aim for 40 lines and never exceed 60 without refactoring; explicit imports, verb names, public Debug/docs, typed results, caller-before-helper order and narrow visibility apply to touched code. Split files by real responsibility, not scaffolding.
- Retain the baseline receipt: make check V=1 passed 620/0/5 across 45 root debug executables. Documentation-only preparation does not rerun unchanged suites. Execution runs focused checks, make check and final make verify on actual changed inputs.
- TK-24 mixed inspect/skill, TK-28 status, native/Linux/historical proofs unless affected, paid models/pilots/resources and G-1/G-2 verdicts remain outside this work. No merge/push/issue closure without a later integration request.

## Review Focus

1. A request has one known start plus an unknown one with depth zero: reject the whole request before observations, with NotFound (Task 1 tests and Task 2 real CLI).
2. Incoming/both traversal encounters the same edge twice and reaches another start: retain original orientation, deduplicate triples and exclude all starts without losing their expansion (Task 1 pure BFS fixtures).
3. A large positive depth or cap, fractional/exponent/null numeric spelling, duplicate object fields, or a malformed reserved HTTP address must not overflow, change URL identity or initialize storage on invalid transport (Tasks 1/2).
4. A referenced local source becomes unsafe/unreadable, or provenance fails after earlier links were built: return a safe whole-call error and release the read while the service remains alive (Task 3).
5. Another writer changes a source binding or deletes a node after pin: membership/edges/basis/coordinates/revision remain from the old view while current flags may change; configuration readiness must happen before that pin (Task 3).

## File map and interfaces

Modify `src/service/command.rs` only for shared ElementAddress remote-source normalization and its unit tests. Add `src/service/traversal_types.rs` for request/result values, `src/service/traversal.rs` for the concrete service, and `src/service/traversal/walk.rs` for its private pure graph walk. Add `src/service/knowledge_link.rs` for the existing current-knowledge comparison shared by search and traversal; it is a helper, not a forwarding service. Declare/reexport interfaces through `src/service/mod.rs` and `command_traits.rs`. Connect constructors in `src/lib.rs`. Tests live beside the walk/service and in `tests/traversal.rs` and `tests/cli_traversal.rs`; new CLI unit tests go in `src/cli/traversal_tests.rs`, not the already 509-line tests.rs. Do not precreate unused modules.

Public service values and contracts (introduced in Task 1):

```rust
pub enum TraversalMode { Relations, Containment, Both }
pub enum TraversalDirection { Outgoing, Incoming, Both }
pub struct TraversalRequest {
    pub element_ids: BTreeSet<ElementAddress>,
    pub mode: TraversalMode,
    pub direction: TraversalDirection,
    pub depth: u64,
    pub limit: NonZeroUsize,
    pub edge_types: BTreeSet<EdgeType>,
    pub edge_limit: Option<NonZeroUsize>,
}
pub enum ElementLocation { Local(RepoPath), Remote(SourceUri) }
// ElementLocation::as_str(&self) -> &str; Remote must contain an opaque URL.
pub struct TraversalLink {
    pub element_id: ElementAddress,
    pub path: ElementLocation,
    pub lines: Option<LineRange>,
    pub revision: u64,
    pub flags: BTreeSet<SearchFlag>,
}
pub struct Traversal {
    pub elements: Vec<TraversalLink>,
    pub edges: Option<Vec<Edge>>,
}
pub trait Traverser {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal>;
}
```

Use existing domain Edge/EdgeType/SourceUri rather than stringly typed copies. Give public values appropriate Debug/comparison traits and API documentation. A direct typed call with empty element_ids is InvalidRequest; CLI additionally validates all numeric/list/object spellings before service construction. Existing ElementAddress::parse(&str) -> CommandResult<Self> remains the selector constructor; add remote support without adding mixed inspect behavior.

### Task 1: Working traversal service and library API

**Files:** Create `src/service/traversal_types.rs`, `src/service/traversal.rs`, `src/service/traversal/walk.rs`, `src/service/traversal/walk/tests.rs`, `src/service/knowledge_link.rs`, and `tests/traversal.rs`. Modify `src/service/{mod.rs,command_traits.rs,command.rs,search.rs}` and `src/lib.rs`. Update an existing search unit-test import only if moving its actual shared helper requires it.

**Interfaces:** Consume Storage::begin_read, StorageRead::{list_nodes,list_edges,get_revision}, Refresher::refresh, Embedder::read_configuration, FileSystem, MarkdownCodec, SourceReader::read_source and SourceProvenance::observe_sources. Produce the public values/Traverser above and the concrete composition factory `pub fn wire_traverser(start: &Path) -> CommandResult<impl Traverser + use<>>` in lib.rs, following existing opaque service factories.

The concrete `TraversalService<D,S,E,F,M,P,R>` and `TraversalDependencies<D,S,E,F,M,P,R>` live together in traversal.rs. The dependency bundle owns `refresher:D`, `storage:S`, `embeddings:E`, `filesystem:F`, `markdown:M`, `provenance:P`, `sources:R`; `new(dependencies: TraversalDependencies<...>) -> Self` only stores them. Bounds are Refresher/Storage/Embedder/FileSystem/MarkdownCodec/SourceProvenance/SourceReader respectively. Private helper contexts can borrow disjoint collaborators while the GAT reader borrows storage; no extra shared ownership or public collaborator lists.

`walk.rs` produces private `GraphWalk { element_ids: Vec<ElementAddress>, edges: Option<Vec<Edge>> }` via `pub(super) fn walk_graph(nodes: &[StoredNode], edges: &[Edge], request: &TraversalRequest) -> CommandResult<GraphWalk>`. It returns only graph choices, not live links, and is used by the working service in this task. The shared helper is `pub(super) fn is_current_knowledge<F: FileSystem, M: MarkdownCodec>(node: &KnowledgeMetadata, path: &RepoPath, filesystem: &F, markdown: &M) -> DocumentResult<bool>` in knowledge_link.rs; SearchService maps that result through its existing diagnostic path, preserving search behavior and messages.

- [ ] **Step 1: Add named failing walk/address/service tests.** In the private walk tests, use literal graph: a includes b, a z_custom_link c, a external code.rs, b includes c, b z_custom_link d, c z_custom_link d, d z_custom_link a; one dangling a z_custom_link gone. All letters denote `.md` knowledge IDs. Handwrite expected IDs for all nine mode/direction combinations, not a second traversal oracle. With both/outgoing/depth 2, expect `[code.rs,b.md,c.md,d.md]`; limit 2 gives `[code.rs,b.md]` and exactly the first external/includes edges; edge_limit 1 still returns all four elements. Check depth 0/1, empty/duplicate types, all-start exclusion/expansion, incoming original triples, alternative/return paths, includes cycles, omitted edges versus Some(empty), no induced edges, and no transitions after the node cap. Use `z_custom_link` to prove no hardcoded type allowlist. Address tests preserve URL spelling/query/fragment and reject malformed/reserved-scheme fallbacks. In tests/traversal.rs exercise real wire_traverser: zero-config graph, all-start NotFound before depth-zero exit, local whole-file/range and HTTP sources as starts/targets, disappearance after final reference removal, and invalid-current knowledge/source flags. Assert exact path/optional lines/revision/flags and no raw content. Name the key tests `keeps_edge_budget_independent_from_node_budget`, `validates_all_starts_before_zero_depth`, and `returns_source_links_without_fetching_remote_bytes`.
  The core fixture uses depth 2, limit 20 and no type restriction. Its independent expected ID table is:

  | mode | outgoing | incoming | both |
  | --- | --- | --- | --- |
  | relations | code.rs,c.md,d.md | d.md,b.md,c.md | code.rs,c.md,d.md,b.md |
  | containment | b.md,c.md | empty | b.md,c.md |
  | both | code.rs,b.md,c.md,d.md | d.md,b.md,c.md | code.rs,b.md,c.md,d.md |

  Add a separate c includes a edge only in the includes-cycle case; do not change the table fixture. Representative assertions inside `keeps_edge_budget_independent_from_node_budget` (with the fixture request selecting both/outgoing/depth 2/edge_limit 1) are:

  ```rust
  assert_eq!(result.element_ids.iter().map(ElementAddress::as_str).collect::<Vec<_>>(),
             ["code.rs", "b.md", "c.md", "d.md"]);
  assert_eq!(result.edges.as_ref().unwrap().len(), 1);
  assert_eq!(result.edges.as_ref().unwrap()[0].get_edge_type().as_str(), "external");
  ```

- [ ] **Step 2: Record RED results.** Run `cargo test --locked --offline --lib service::traversal` and `cargo test --locked --offline --test traversal`; initially missing new API may be a compile RED, distinct from subsequent behavioral REDs. Existing address tests must also demonstrate the old URL-normalization gap before its fix. Save full failed diagnostics privately; do not manufacture a behavioral failure for already-working reused behavior.
- [ ] **Step 3: Implement the service, walk and real factory.** Refresh -> read_configuration -> exactly one begin_read -> typed nodes/edges -> all-start validation -> deterministic BFS -> links. Use ordered adjacency by type/reached neighbor/original triple, unique starts previsited, bounded queue and checked depth handling (including u64::MAX). Ignore absent endpoints, not cyclic includes edges; expand each visited ID once. Build all bindings from the same original edges, regardless of selected mode/type, before knowledge provenance observations. Knowledge links reuse current-byte comparison and existing provenance; source links use SourceReader under the same reader, `source_not_found` only for absent local bytes, and opaque URL locations without RepoPath conversion. Missing known source metadata inside that same view is an integrity error, not a partial success. Observe each returned element once, preserve original revision/coordinates, and fail the whole call on unexpected errors. Move only the shared comparison out of search.rs; leave ranking, filters and JSON unchanged. Wire all concrete dependencies in lib.rs with existing constructors, no fake service or dead-code lint exemption.
- [ ] **Step 4: Verify service behavior and production controls.** Run both new targets, `cargo test --locked --offline --lib service::command`, `cargo test --locked --offline --test knowledge_search`, `cargo test --locked --offline --test provenance_commands`, then `make check`. Hand-mutate one condition at a time: let includes consume zero depth, make edge cap stop BFS, omit unknown-start validation at depth zero, allow induced/post-cap edges, or let dangling targets through. Each named covering runtime test must fail for its intended assertion; restore exact bytes and rerun it. Preserve existing search regression output. Review actual import directions, public docs/visibility, helper ownership and constructor wiring.
- [ ] **Step 5: Commit the independently usable library service.** Commit only intended Task 1 paths as `feat: add consistent bounded graph traversal service`. Record actual checks/mutation restoration and module review in the execution ledger/Engram. The real library API works here; CLI traverse remains unrecognized until Task 2, without temporary unavailable stubs.

### Task 2: Strict traverse CLI, links and background integration

**Files:** Modify `src/cli/{input.rs,handlers.rs,output.rs,mod.rs}`, `src/lib.rs`, and existing handler-construction sites in `src/cli/{tests.rs,search_tests.rs}`. Create `src/cli/traversal_tests.rs` and `tests/cli_traversal.rs`; add malformed traverse rows to `tests/cli_runtime.rs`. Reuse existing `tests/support/{reading.rs,documents.rs,embedding.rs}`; change a helper only for a demonstrated fixture need, preserving existing callers.

**Interfaces:** Consume Task 1 TraversalRequest/Traversal/Traverser and wire_traverser. Add Request::Traverse(TraversalRequest), ResponseResult::Traverse(Traversal), and private `read_traversal(input: impl Read) -> CommandResult<TraversalRequest>`. Parse required u64 depth and checked positive NonZeroUsize caps; use existing read_non_null_option for edge_limit and strict Vec<String> for edge_types.

Replace the now-five-argument handler constructor with `Handlers::new(services: HandlerServices<'a>) -> Self`, where the pub(crate) bundle has optional borrowed checks/inspections/searches/traversals/background trait objects. Optional collaborators justify existing dyn use; retain no-I/O construction and Debug for new public types without adding Debug bounds to collaborator traits. Update every existing construction site; no unrelated handler/CLI layout refactor. lib.rs adds `run_traverse(request: Request, directory: &Path, executable: &Path) -> CommandResult<CommandResponse>` with the same root discovery/background composition as other commands and actual wire_traverser constructor call.

Render Traverse as `{elements, optional edges}`. Make private output Link.lines optional with skip-if-none; existing SearchLink always maps to Some(lines), preserving search JSON exactly. A private edge DTO renders stored source/target/type without reversal or extra fields. Some(empty edges) remains `[]`; None omits the field.

- [ ] **Step 1: Add/run transport and handler RED tests.** In traversal_tests.rs assert strict input acceptance, all modes/directions, deduplicated normalized starts/types, empty edge_types equivalence and distinction between absent and positive edge_limit. Reject null/list/scalar top level, duplicate/unknown fields, missing required fields, empty starts, null optional fields, non-string types, uppercase/invalid edge type names, negative/zero caps, fractional/exponent/overflow numeric spellings, u64 depth overflow and malformed remote addresses. Accept u64::MAX depth without arithmetic overflow; no artificial cap. Stub Traverser records exactly one typed call; failure returns its safe error and never invokes BackgroundRunner, success invokes background only afterward, including existing launch warnings. Assert complete literal serialized JSON for source links with omitted lines, knowledge links with lines, omitted/empty/limited edges, original incoming orientation and unchanged search JSON. Run `cargo test --locked --offline --lib cli::traversal_tests`; expected current RED is the missing traverse transport/dispatch.
  In `serializes_remote_source_links_without_lines`, with a fixed service response at revision 7 and a URL source, compare the entire transport value to an independent literal:

  ```rust
  assert_eq!(response_json, serde_json::json!({"ok":true,"result":{"elements":[{
      "element_id":"https://Host/a?q=1#part", "path":"https://Host/a?q=1#part",
      "revision":7, "flags":[]
  }]}}));
  ```

- [ ] **Step 2: Implement strict parsing and interface-only dispatch/rendering.** Add the DTOs and enum variants, checked conversions, HandlerServices bundle and handler match arm. Use the shared normalization/type values rather than new parsing rules. Wire run_request -> run_traverse -> Traverser in lib.rs; handlers do not import Git/SQLite/filesystem implementations. Preserve one JSON/exit code/once-guard behavior, existing no-op drain entrypoint, all non-traverse responses and background warning semantics.
- [ ] **Step 3: Add/run real CLI cases and implement only proven fixes.** In cli_traversal.rs use isolated ReadingRepo fixtures to assert IDs/triples/revision/flags for all modes/directions, mixed knowledge/source cap, source starts, multiple/unknown starts at depth zero, missing sources, opaque URL identity, no text/HTTP fetch and strict output key sets. An HTTP fixture plus fully seeded backlog must receive zero inference requests during a real traverse; this isolates foreground readiness from detached drain. Test malformed requests from root and nested cwd before `.letopis` or child creation, and a valid nested command using the repository root. Test actual background integration with the existing process fixture, proving launch warning rendering separately via the handler double if triggering real spawn failure is impractical. Run `cargo test --locked --offline --test cli_traversal` and `cargo test --locked --offline --test cli_runtime`; record actual RED/GREEN evidence, with already-passing reuse labeled regression coverage.
- [ ] **Step 4: Verify compatibility and controls.** Run new CLI tests, existing `--test cli_reading`, `--lib cli::tests`, `--lib cli::search_tests` and `make check`. Mutate optional edges to always render, reverse a returned incoming edge, and allow dispatch on malformed input; each covering assertion must fail at runtime (the no-directory/no-child test covers the last). Restore exactly and rerun each target. Review optional lines do not remove knowledge lines or add nulls; metadata output contains no hashes/text/provider fields beyond X-1.
- [ ] **Step 5: Commit working traverse CLI.** Commit intended Task 2 paths as `feat: expose graph traversal through the CLI`. Record constructor-bundle change and concrete ownership/check outcomes. The product command is now usable; Task 3 establishes remaining scoped failure/concurrency guarantees and final acceptance.

### Task 3: Pinned-view races, late errors and final acceptance

**Files:** Create `src/service/traversal/tests.rs` for private scoped fixtures and failure assertions; modify the parent traversal.rs only to declare the test module or fix a demonstrated bug. Add `docs/zkit/letopis/checks/TK-23/report.md` and `verification.json`; update the TK-23 task card, approved spec and this plan. Update only affected X-1/I-8/MC-6 prose to describe actual implementation boundaries; preserve historical evidence and deferred tasks.

**Interfaces:** Consume the completed real TraversalService and all public Task 1 values. Follow existing search/tests/scoped.rs fixture patterns without depending on private search internals: a test-only Storage wrapper delegates real SQLite read/write capabilities and invokes an owned hook immediately after begin_read pins revision; read wrappers can inject typed operational errors. Source/provenance/readiness doubles delegate or fail deterministically and record events/inputs. Hooks belong to test doubles, not new product APIs. Fresh readers/writers are independent real connections; do not use sleeps to arrange publication.

- [ ] **Step 1: Add/run deterministic concurrent-publication tests.** Name `retains_original_graph_bindings_and_revision_after_concurrent_publication`: original revision 1 has a -> b and b -> old.rs with basis S1; after pin, a separate writer publishes a -> c and b -> new.rs/basis S2 at revision 2. With a depth-2 both/outgoing traversal, assert literal original `[b.md,old.rs]`, original edge triples/coordinates/revision 1 and observed b provenance inputs S1+old.rs, never c/new.rs/S2; live stale flags are allowed and asserted separately. Include removal of the original source's final reference in the new graph: SourceReader must still find old membership under the original view. Name `synchronizes_configuration_before_pinning_and_never_embeds_query`: record exact event order refresh, readiness, begin_read; incompatible old/new configuration can reset before pin, while missing/invalid config still traverses. Both query embedding methods panic if invoked; storage begin_read count must be one. Run `cargo test --locked --offline --lib service::traversal::tests`; report real outcomes, not an invented failure if the code is already correct.
  The publication test's result assertions use the original graph, not a newly computed walk or newly loaded metadata:

  ```rust
  assert_eq!(result.elements.iter().map(|link| link.element_id.as_str()).collect::<Vec<_>>(),
             ["b.md", "old.rs"]);
  assert!(result.elements.iter().all(|link| link.revision == 1));
  assert_eq!(*recorded_sources.borrow(), vec![(Some(s1), old_sources)]);
  ```

  Here `s1` and `old_sources` are fixed test inputs captured before publication, never read back from the implementation under test. Also compare exact old edge triples and pinned coordinates.

- [ ] **Step 2: Add/run late error and release tests.** Name `releases_pinned_reader_after_late_failure_while_service_is_alive`; independently inject storage list_edges failure after nodes, SourceReader operational failure after an earlier link, and provenance observation failure after an earlier link. Assert InternalError with a retained cause, no partial Traversal or private payload in Display/Debug, and no successful-empty fallback. While keeping TraversalService alive, use another connection for `PRAGMA wal_checkpoint(TRUNCATE)` and an actual subsequent cache publication; assert checkpoint is not busy and new revision is visible. Include an unsafe symlink/nonregular local-source path case: operational failure must not become SourceNotFound. Also verify reader cleanup after success and depth-zero empty output. Record no background launch for an errored service via Task 2 handler assertions. Run the scoped target plus `--test traversal` and `--test cli_traversal`.
- [ ] **Step 3: Mutation-check actual scoped guarantees.** Mutate production traversal to reopen/read graph metadata after the publication hook, skip readiness-before-pin, swallow a late source error, and swallow a late provenance error. Each unchanged controlled fixture must fail at the intended result/event/error assertion; do not change its injected failure into success and count that as production coverage. Restore bytes exactly and rerun covering tests. Log the product path/condition, command, runtime assertion and restored GREEN for every counted control.
- [ ] **Step 4: Verify and review the final product tree.** Capture the actual code revision and source/build/test input hashes including Cargo files, build.rs, native inputs if referenced, rust-toolchain.toml and clippy.toml; preserve baseline and earlier failed logs separately. Run `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target RUST_TEST_THREADS=2 make verify V=1` into a fresh evidence directory. Expect exit 0, fmt/check/Clippy/strict Rustdoc success and zero failed debug/release tests; count only each root executable's final summary and report ignored separately. Review actual files, visibility, trait implementations, interfaces, lib.rs constructors, source address handling and all spec/Review Focus cases. When using the selected execution skill, complete its task review gates and final whole-branch review; fix findings and rerun affected checks, with final verification against the final product inputs. Storage/native/build/vendor changes are not planned; if such changes become necessary, run the mandated native/platform harness for that affected scope and record any unrun platform limit.
- [ ] **Step 5: Complete reports and commit.** Fill the task criteria only with actual evidence, exact commands/counts/hashes, production versus fixture controls, review outcomes, restore state and remaining TK-24/TK-28/platform/pilot/gate limits. Keep the observed baseline and captures historically accurate; do not rewrite receipts to a later docs commit. Validate affected Markdown links/JSON, mark executed plan steps, and record decisions/results in existing Engram using clean-tree/token guards. Commit intended tests/fixes/docs as `test: verify traversal snapshot and failure guarantees` (a later documentation-only receipt commit is allowed). Deliver the completed task for the user's later integration decision; this plan does not authorize merge/push/issue closure.

## Plan self-review

- Coverage: Task 1 owns all graph/address/link/flag behavior and the usable library API; Task 2 owns strict transport, rendering, handlers/background and real CLI; Task 3 owns concurrent publication, readiness order, late errors/RAII and acceptance evidence. Every spec section maps to those tasks; no uncovered requirement identified.
- Type consistency: TraversalRequest/Traversal/TraversalLink/ElementLocation and enum spellings are the same across factory/service/CLI/tests. Element IDs remain ElementAddress, locations distinguish local paths and opaque URLs, edges remain domain Edge triples, and flags remain SearchFlag. SourceReader always receives the caller's same StorageRead.
- Step scan: every task has named files, exact interfaces, observable literal expectations, commands, controls and a working commit boundary. Private helper bodies and fixture scaffolding remain implementation decisions; no unused product scaffold, dead-code exemption or extra backend is prescribed.
- Review Focus: case 1 maps to Task 1 Step 1 and Task 2 Step 3; case 2 to Task 1 Step 1; case 3 to Task 1 Step 3/Task 2 Step 1; cases 4/5 to Task 3 Steps 1–3. Self-review only; no separate plan-review agent dispatched.
- Proportion: three sequential deliverables, each independently testable and reviewable, with signatures/expectations rather than implementation bodies. The readiness dependency follows the approved spec rather than introducing a new configuration service.

## Execution handoff

Plan prepared and self-reviewed; all boxes are intentionally unchecked. Ask the user to review it and choose Native or Subagent-driven. Task approval from older TK work does not select the execution method for this new plan. Recommended: Subagent-driven, because independent service/CLI gates and a final branch review help catch subtle limit, source-identity and snapshot errors before integration; Native is the lower-context-cost alternative. No product work starts until the plan review and execution choice are received.
