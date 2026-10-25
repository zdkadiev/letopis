# TK-23 — Complete graph traversal

Status: approved by the user on 2026-10-25: “Да, спецификацию TK-23 утверждаю. Продолжай к плану реализации по Superpowers.” The user requested a new worktree and exact branch `tk-23` from `main`, and delegated discovery defaults with “По карточке”. The engineering choices below were accepted with this specification; product implementation has not started.

## Intent and boundaries

Give agents the complete existing X-1 `traverse` command: walk from knowledge or source addresses to related knowledge, contained knowledge and referenced sources. Return metadata links and optionally the edges actually traversed, never document text. The main example follows a design decision to related knowledge and source code; all modes, directions and limits remain in acceptance.

Implement product behavior with focused tests and evidence. Preserve synchronous I/O, strict CLI validation, physical hexagonal boundaries and the existing refresh/publication path. Reuse TK-18 graph values, TK-20 source identity/observations, TK-21 provenance and TK-12/TK-22 consistent reads.

Excluded: search ranking/filter changes, mixed batch inspect and agent-skill expansion (TK-24), status UI (TK-28), a new graph database or persisted adjacency/cache, dependency/runtime changes, remote fetching, text output, and fresh paid/model/resource/pilot or gate certification. Earlier G-1/resource/pilot limits remain visible; the user's continuing development instruction does not establish that those gates passed.

## Sources and verified starting point

- [Task card](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-23-traversal.md).
- [Requirements FR-5, FR-21, FR-26 and NFR-7](../../zkit/letopis/requirements.md).
- [Architecture and physical modules](../../zkit/letopis/architecture.md#program-structure) and [architecture review](../../zkit/letopis/architecture.md#program-structure-review); repository AGENTS.md applies.
- [MC-6](../../zkit/letopis/mechanics.md#mc-6--чтение-поиск-и-обход), [decisions](../../zkit/letopis/decisions.md), [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md) and its [schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [I-8 consistent reads](../../zkit/letopis/contracts/I-8-read-view.md) and [I-4 source observations/provenance](../../zkit/letopis/contracts/I-4-sources.md).

The native managed worktree is `/Users/zdkadiev/.codex/worktrees/tk-23/letopis`, branch `tk-23`, based on freshly fetched `main@912c15fe0faef7dc53c88d47f4ae6c246d7f71ab`. Other worktrees and their evidence are preserved.

At this revision CLI input, handlers and composition do not implement traverse. `StorageRead` already provides typed `list_nodes` and `list_edges` within one caller-owned read view. `SourceReader` observes local availability without opening another view or fetching URLs; `SourceProvenance` observes a knowledge element's source bindings and declared basis. `SearchLink` currently represents knowledge only; CLI link rendering requires lines, whereas the X-1 Link schema makes lines optional. `ElementAddress` currently normalizes local selectors; remote source starts need opaque HTTP/HTTPS handling.

Fresh baseline `make check V=1` passed on the unchanged product tree: 620 root debug tests passed, 0 failed, 5 ignored, across 45 root executables; fmt, all-target check and Clippy passed. Full log and its SHA-256 are retained under `.artifacts/tk23/baseline.log` and `baseline.json`. Nested child/doc-test summaries are excluded. No fresh release, native/Linux, historical proof, paid-model, pilot or gate run is claimed.

## Approaches and accepted choice

1. **One pinned graph, in-memory adjacency and service-owned BFS — recommended choice.** Use existing complete typed graph reads once, build request-local ordered neighbors, and walk them. No new SQL/storage interface or persisted graph representation. The cost is O(V + E) temporary graph memory, plus deterministic sorting and requested live observations; this is a simple baseline, not a performance acceptance claim.
2. **Incremental neighbor queries.** Add incoming/outgoing filtered neighbor operations to the storage interface and SQL, querying each BFS frontier under the same view. This could lower memory for small traversals on large graphs, but adds interfaces/query round trips and implementation work without a measured requirement. Defer unless measurement justifies it.

The user delegated remaining discovery choices with “По карточке” and subsequently approved this specification, including approach 1. Incremental SQL traversal remains deferred pending measurement.

## Input and address contract

Example JSON on stdin to `letopis traverse`:

```json
{"element_ids":["decisions/storage.md"],"mode":"both","direction":"outgoing","depth":2,"limit":20,"edge_limit":30}
```

Required: nonempty `element_ids`, `mode`, `direction`, nonnegative integer `depth`, positive integer `limit`. Optional: `edge_types` and positive integer `edge_limit`. Preserve strict object parsing: reject unknown fields, null optional values, invalid enum values, malformed addresses/types, negative/fractional/overflowing numbers, and non-object input before constructing services or starting background work. Do not introduce a product cap; represent supported numeric values with checked conversions and safe depth handling.

Modes are `relations` (all types except `includes`), `containment` (only `includes`), and `both`. Directions are `outgoing`, `incoming`, and `both`. A nonempty `edge_types` list intersects the selected mode; absent or empty means unrestricted within that mode. Types use the existing open `EdgeType` grammar; there is no allowlist of agent-defined relation names.

Deduplicate normalized starts and edge types. Resolve starts by actual node membership in the pinned graph, including sources. Whole-file sources and line ranges are distinct identities; knowledge anchors remain case-sensitive. Local normalization and opaque HTTP/HTTPS spelling, query and fragment use the TK-20 rules. Malformed reserved HTTP schemes must not fall back to local paths. Extend the shared address abstraction only as necessary for this existing ElementId contract; do not implement TK-24 source inspection along the way.

All starts must exist before BFS or a depth-zero shortcut. A well-formed unknown start rejects the entire call with `not_found`, even if another start exists or no transition would occur. A referenced missing local file is still an existing source node; removing its final reference removes that source from graph membership. No partial response is emitted on failure.

## Deterministic traversal

Read nodes and declared edges from one pinned view. Only transitions whose endpoints belong to that graph are usable; dangling knowledge targets remain stored and diagnosed elsewhere, but cannot be crossed. Do not persist activity flags or recompute/publish the graph while walking it.

- Initialize the BFS queue with unique starts in normalized identity order. Mark every start visited at depth zero before expansion; exclude every start from returned elements even when reached again.
- For each expanded node, enumerate matching transitions in `(edge type, reached-neighbor ID)` order. For equal keys use the stored `(source, target, type)` identity as a deterministic tie-break. For incoming traversal the reached neighbor is the stored source. Keep returned edge orientation as originally stored.
- Each relation and each `includes` transition consumes exactly one depth step. Do not recursively flatten containment, invent transitive edges or exclude a node because its includes component has a cycle. A visited node is never expanded twice, so ordinary and includes cycles terminate.
- Return each newly discovered non-start node once, in BFS discovery order. A node first reached at the boundary depth is returned but not expanded.
- `depth=0` returns empty elements and, only when requested, empty edges after validating all starts.
- `limit` caps returned knowledge/metaknowledge/source nodes together. Upon appending the node that reaches the limit, stop considering further transitions altogether. Do not scan remaining neighbors or add edges after that point.
- When `edge_limit` is absent, omit `edges`. When present, record unique actually considered eligible transitions, in first-encounter order, up to that independent cap. A transition to an already visited node or start counts as traversed if considered within depth and before the node cap. Such a transition does not add another element.
- Reaching the edge cap stops adding returned edges but continues BFS/node discovery until depth/node bounds. It does not change which elements are returned.
- Never manufacture an induced subgraph among returned nodes. A stored edge between two returned nodes is absent unless the BFS actually considered its transition.

These ordering/boundary choices make the MC-6 contract reproducible; they add no new public fields.

## Output and freshness

Return the existing X-1 envelope `{"ok":true,"result":{"elements":[...],"edges":[...]}}`, with `edges` omitted unless requested. Edges are original `source`/`target`/`type` triples, no IDs, payloads or timestamps. Links retain exactly `element_id`, `path`, optional `lines`, `revision`, and `flags`.

Every link carries the same pinned cache publication revision. Knowledge links use the pinned knowledge ID, containing path and lines. Reuse existing current-knowledge comparison and source-provenance semantics for `stale`, `source_changed` and `source_not_found`; bindings and basis must come from this same graph view, including source edges that were not selected by traversal filters.

Local source links use the canonical source ID, its containing local path and its addressed line range when present. Use `SourceReader` under the caller's pinned view to observe availability: absent files/incomplete ranges yield `source_not_found` without removing the graph node. A standalone source has no own declared knowledge basis, so do not invent a `source_changed` comparison for it. A whole-file source need not invent lines. An opaque HTTP/HTTPS source uses its verbatim URL for both ID and path, omits lines, remains unfetched, and receives no fabricated missing/changed flag. The existing JSON Path lexical schema permits that address; the typed Rust output must not force it through `RepoPath`.

Observe metadata only for returned links as needed, at most once per returned element. No persistent observation cache. Missing knowledge bytes produce `stale` as in search; unsafe or operational filesystem/Git/storage errors remain errors, not empty results or missing flags. Flags concern current observed bytes, not knowledge truth. File observations have the same existing live-read limits: no cross-file atomic snapshot of the working tree is promised.

Preserve existing search JSON and ranking behavior if private knowledge-link logic is shared with traversal. Shared helpers belong by their actual consumers; do not introduce a forwarding service or global utilities bag.

## Lifecycle, interfaces and files

The CLI validates first, then the traversal service refreshes the corpus. Preserve the current MC-6 readiness/reset-before-pin order using the existing embedding configuration capability; graph membership/output does not depend on having a valid configuration or completed embeddings. Traverse performs no query embedding, vector ranking or source HTTP fetch. Readiness may synchronize vector configuration as already designed; it must not happen through or reopen the pinned reader.

After readiness, open one owned `StorageRead`, read graph metadata/bindings, validate starts, perform BFS and construct links under that view. Release it through RAII on success, empty output, failure or cancellation before returning to handlers/background launch. Concurrent publication must not change nodes, edges, bindings, lines or revision halfway through this response. Source/knowledge live observations are separate read-only observations, as in search.

Introduce the application-owned `Traverser` trait with typed request/result and a concrete `TraversalService` in a responsibility-named service file. Keep graph-walking decisions in the service or its private pure helper. Storage/Git/filesystem/Markdown/embedding implementations stay in their existing modules and never orchestrate one another. CLI parsing/dispatch/output calls the service interface only; `lib.rs` makes the actual constructors and connections, and `main.rs` remains thin. Concrete output enums/newtypes and small helper file layout are left to the implementation planner.

After successful foreground work, reuse the existing background runner/launch warning behavior without waiting for drain. Errors do not launch background work and do not leak document, provider or subprocess payloads. The ordinary CLI signal handler remains in effect; exhaustive interruption/platform drills stay with their owning task and are not certified merely by traverse tests.

## Acceptance and evidence

Cover deterministic service and real CLI behavior with independent expected literals:

- All three modes and directions, arbitrary valid edge types, empty/duplicate selectors, and intersecting type filters.
- Multiple/duplicate starts, start exclusion, validation of every start including depth zero, unknown starts, and source membership after the final reference is removed.
- Depth zero/one/boundaries, one-step includes, diamond/alternative paths, ordinary/includes cycles, missing targets, stable ordering and finite termination.
- Combined knowledge/source node cap; edge cap that does not truncate elements; omitted versus empty edges; original incoming orientation; traversed back/alternative edges; no induced edges or post-node-limit transitions.
- Local whole-file/range sources and opaque URLs as targets and starts, normalized identities, missing/incomplete local sources, current knowledge/source flags, no text leakage or HTTP fetch.
- Strict X-1 input/output shapes and existing search/inspect/check behavior, including no service initialization or child launch for malformed input.
- Concurrent publication while the original view remains pinned; late storage/source/provenance errors fail the whole call and release the reader while the service is still alive. Verify subsequent checkpoint/publication without dropping the whole service.
- Foreground success plus existing drain triggering/warnings; no query embedding for traversal, and graph traversal works before model configuration.

Mutation-check new meaningful behavior by temporarily breaking the production condition/call, observe the intended assertion fail, restore exact bytes and rerun the covering test. Distinguish fixture sensitivity from production-path mutation evidence. Run `make check` for Rust changes and full `make verify` before claiming task completion; summarize root executable counts without nested child summaries. Native/historical/Linux harnesses only if their governed inputs change. Preserve logs and receipts in a new ignored evidence directory; do not rewrite historical evidence.

Track final results/limits in the task card, a TK-23 report/receipt and the agreed spec/plan. Record useful decisions and evidence in the existing shared Engram store with optimistic tokens, preserving other writers and historical provenance. Merge, push and issue closure require subsequent authorization; starting this task alone does not authorize integration.

## Approval and next step

The full specification and its engineering choices are approved; preparation of the implementation plan is authorized. Fresh independent builder check `/root/tk23_builder_check` returned “nothing important left”; no planning-blocking questions were identified. The review covered the proposed design and relevant current contracts/code, not implemented product behavior. The implementation plan is [TK-23 traversal](../plans/2026-10-25-tk-23-traversal.md), prepared with Superpowers writing-plans. Product implementation awaits that plan's approval and execution choice.
