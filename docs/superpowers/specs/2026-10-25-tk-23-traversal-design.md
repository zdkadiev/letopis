# TK-23 — Incremental graph traversal

Status: revised specification and implementation plan approved by the user on 2026-10-26; Subagent-driven execution explicitly authorized. The concrete builder choices below, including bounded pages/indexes/cache compatibility, are accepted with this revision. This replaces the earlier full-graph/private-walk and storage-dependent Graph proposals; their committed versions remain in Git history.

## Intent, scope and sources

Implement the complete metadata-only X-1 `traverse` command. An agent follows a decision to related knowledge, included knowledge and referenced source code. All modes/directions/limits remain in acceptance; the example does not narrow the task.

Accepted decisions:

- Graph is a dependency-free domain model. GraphService implements traversal behind an application interface and depends on storage through an interface.
- Use incremental BFS with compact identities and memory proportional to the requested result limit; never load the corpus or a complete adjacency list for traversal. A fixed-RAM disk-backed traversal is not required.
- Apply KISS, DRY and YAGNI throughout Letopis. Avoid unnecessary generics, forwarding services, factories, registries and caches; keep meaningful DDD/hexagonal boundaries.
- Keep synchronous I/O, existing refresh/publication/read-view ownership, metadata-only output, existing source identity/provenance and the existing CLI envelope.

Sources: [task card](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-23-traversal.md), [requirements FR-5/FR-21/FR-26/NFR-7](../../zkit/letopis/requirements.md), [architecture and review](../../zkit/letopis/architecture.md#program-structure), [decisions](../../zkit/letopis/decisions.md), [MC-6](../../zkit/letopis/mechanics.md#mc-6--чтение-поиск-и-обход), [X-1](../../zkit/letopis/contracts/X-1-cli.md)/[schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml), [I-8](../../zkit/letopis/contracts/I-8-read-view.md), [I-4](../../zkit/letopis/contracts/I-4-sources.md) and [AGENTS.md](../../../AGENTS.md).

Worktree `/Users/zdkadiev/.codex/worktrees/tk-23/letopis`, branch `tk-23`, began at `main@912c15fe0faef7dc53c88d47f4ae6c246d7f71ab`. The retained unchanged-product baseline `make check V=1` passed 620 tests, 0 failed, 5 ignored across 45 root debug executables, with fmt/check/Clippy. Receipts: `.artifacts/tk23/baseline.{log,json}`. No fresh release/native/Linux/pilot result is implied by documentation work.

Excluded: search ranking/filter changes, mixed inspect and agent-skill expansion (TK-24), status UI (TK-28), remote fetching, source/document text output, a new runtime/dependency/database/graph cache, whole-project generic refactoring and paid/model/resource/pilot/gate certification. Earlier G-1 limits remain; continuing development does not certify those gates.

## Approaches and choice

1. **Chosen: incremental storage-backed BFS with compact bookkeeping.** Preserve BFS ordering, deduplication and snapshot semantics while reading bounded neighbor pages. The user accepted result-limit-proportional memory and entity/service separation.
2. **Rejected: complete nodes/edges plus in-memory adjacency.** Even a tiny request would allocate with total corpus size. The user explicitly rejected that cost.
3. **Deferred: disk-backed frontier/visited/output.** Can support a separate fixed-RAM policy at extra I/O/lifecycle cost. It is not needed for the accepted bound.

## Input and identity contract

`letopis traverse` reads one strict JSON object from stdin:

```json
{"element_ids":["decisions/storage.md"],"mode":"both","direction":"outgoing","depth":2,"limit":20,"edge_limit":30}
```

Required: nonempty `element_ids`; mode `relations|containment|both`; direction `outgoing|incoming|both`; integer depth >= 0; integer limit >= 1. Optional: `edge_types`, integer edge_limit >= 1. Reject missing/duplicate/unknown fields, null optionals, wrong shapes, fractional/exponent numeric spellings, negative values, zero caps and unrepresentable integers before service construction or background launch. Support representable u64 depth, including u64::MAX, without overflow; no new public cap.

Normalize and deduplicate starts and types. Local paths/knowledge anchors/ranges follow existing domain parsers. Recognize actual raw absolute HTTP(S) scheme:// addresses first, validate them through SourceUri and never fall back to local knowledge if they are malformed; preserve their opaque spelling/query/fragment without fetching. Otherwise try a valid KnowledgeId before SourceUri. This preserves canonical local knowledge addresses such as http:notes.md and ./http:notes.md#A with stable normalize/reparse behavior; source-shaped malformed http:notes or https:/Host/a remains invalid. A Markdown-shaped spelling such as https:/Host/a.md can be valid local knowledge, with unknown membership yielding not_found. This controller clarification resolves the existing knowledge/source union without changing any domain grammar. Knowledge and source membership is resolved by the pinned node, not guessed from file extension. Every start is validated before BFS/depth-zero exits; any well-formed unknown start rejects the whole call with not_found. A referenced missing local file remains a source node; removing its last reference removes that membership. Empty typed starts are invalid_request.

`relations` excludes includes, `containment` selects includes, and `both` accepts both. A nonempty edge_types set intersects that selection; absent/empty imposes no additional restriction. EdgeType remains an open validated grammar, not a fixed relation-name allowlist.

## Deterministic incremental BFS

1. Resolve unique starts in canonical identity order to compact snapshot-local node handles. Validate all before expanding; mark all visited and put them at depth zero in the FIFO. Handles are internal and never become canonical IDs or JSON fields.
2. Reuse an append-only discovery-handle sequence as FIFO with a head index. Track depth by layer-end positions, not full node objects. Keep a compact visited-handle set; starts are expanded but never returned as elements.
3. Expand one node at a time through bounded ordered neighbor pages. Each per-direction page has at most 32 lightweight edge/neighbor records. Both-direction traversal incrementally merges two ordered streams, retaining at most two pages (64 records), not the complete frontier or degree-sized adjacency list. The page quantum is a builder choice/internal constant, not a CLI quota; smaller page requests are allowed.
4. A page uses a strict keyset cursor `(type, reached-neighbor identity)` within its fixed node/direction/filter request, never OFFSET. Skip missing endpoints before applying the page limit. Do not load complete node metadata for neighbor enumeration. Per-direction neighbors are ordered by type then reached ID; merge ties by original source/target/type. Returned edges retain original orientation.
5. Every relation/includes consumes one depth step. Nodes at the requested depth are returned but not expanded; visited IDs are never expanded again. Ordinary/includes cycles therefore terminate. Depth zero returns empty elements and only a requested empty edges list, after complete start validation.
6. A newly discovered non-start is appended once in BFS discovery order. Stop processing transitions immediately when the returned-node limit is reached; discard unused page records and issue no further page/metadata reads for additional nodes. Prefetching a bounded page does not make every buffered edge traversed.
7. Only considered eligible transitions may supply returned edges, including alternative paths/back edges to an already visited node or start within depth and before the node cap. Deduplicate original triples. Omit edges when edge_limit is absent; otherwise retain at most edge_limit edges in first-encounter order. Once that cap is full, stop edge bookkeeping but continue node discovery. Do not build an induced subgraph.

With S starts, K returned nodes, P buffered neighbor records and R returned edges, traversal bookkeeping is O(S + K + P + R); K <= limit, P <= 64 and R <= edge_limit when requested. Do not preallocate from an untrusted enormous requested limit. Data sizes still include variable-length IDs/types; this is not a fixed-byte RSS guarantee. Final links/JSON, SQLite caches, live file parsing and refresh/publication buffers are additional.

## Entity and service boundaries

- `domain::Graph`: pure partial traversal graph data, ordered selected knowledge/source identities and optional traversed edges; no storage/service/HTTP/callback dependencies, SQL handles, cache, or invented Graph ID. Starts may be absent from selected elements while edges mention them; there is no requirement that every returned edge endpoint be in the selected-node list. Pure access/local invariants are allowed. Graph does not perform traversal or I/O.
- `service::GraphService`: concrete non-generic traversal algorithm implementing GraphTraversal. It accepts the caller's scoped GraphRead capability as a method dependency, owns all BFS bookkeeping and returns bounded Graph data. GraphRead exposes minimal node-handle resolution and per-direction neighbor pages, no SQL/SeaQuery/rusqlite types. No global/full graph lookup is available through that narrow capability.
- `storage::SqliteRead`: implements GraphRead inside its existing pinned transaction. Storage::Read supplies StorageRead plus GraphRead. StorageRead itself keeps its existing responsibilities; no second graph connection/view is opened.
- `service::TraversalService`: real orchestration responsibilities: refresh, readiness-before-pin, ownership of one read view, GraphTraversal invocation, and selected metadata/freshness observations. It never repeats BFS or knows concrete technology types. CLI calls Traverser; lib.rs performs actual constructor wiring including GraphService injection.

Keep GraphService and its interfaces object-safe/concrete. For orchestration, retain only generics required by existing scoped Storage GATs and FileSystem's associated lock type (two, S/F). Own the other substitutable dependencies through small object-safe trait objects where this simplifies the factory's lifetime. Make SourceReader::read_source accept `&mut dyn StorageRead` instead of a needless method type parameter; preserve its behavior and all existing tests. Do not add type-erasure factories/adapters merely to remove necessary generics. Existing unrelated services stay unchanged except actual shared helpers and the warm-refresh fix below.

## Storage access, indexes and compatibility

Builder choices for the revised plan:

- Use existing integer row identities as opaque view-local handles, never public/cross-view identity. Add minimal handle lookup and bounded neighbor-page reads. SQL is composed with existing SeaQuery/bound parameters; source/target membership and filters execute before page limits.
- Add indexes `(source_id,type,target_id)` and `(target_id,type,source_id)` for ordered outgoing/incoming reads. Existing primary-key and target-only order do not provide the required type-first order. Merge directions in the algorithm rather than rely on a large UNION sort. Verify first/cursor pages and filter variants with the actual native SQLite query plans; no degree-sized temporary sorting or full Rust adjacency collection.
- Warm refresh currently calls list_nodes just to obtain declared source_snapshot roots. Replace that with a narrow distinct-root query and an index on non-null source_snapshot, preserving existing retention/locking behavior. This avoids loading all node metadata before an otherwise tiny unchanged traversal. The root set and file-state list remain existing refresh state, not BFS node objects.
- A changed/first-index publication still uses the existing publication/validation machinery, which can load graph projections. This task does not redesign publication or promise a fixed memory ceiling for the whole command. Evidence must distinguish unchanged traversal state from publication work; no full-call RSS claim is inferred from the BFS bound.
- The required indexes change the exact recognized cache DDL. Use cache schema version 2 and retain vectors schema version 1. Older/incompatible cache schemas fail safely without automatic mutation; document an explicit quiescent cache.db rebuild following the existing TK-20 pattern. Preserve vectors.db, config, lock-file identities, Markdown and Git. No automatic migration or silently dropped user data. Schema parity and host/Linux native runners are now in the verification scope.

These index/cache/query choices were accepted with the revised specification on 2026-10-26. They are implementation requirements, not claims that the corresponding product behavior already exists.

## Metadata, provenance and lifecycle

After refresh and existing embedding configuration readiness/reset, pin exactly one traversal read view. Missing/invalid config does not prevent traversal; no query embedding/vector ranking or HTTP source fetch occurs. GraphService uses only that view. Hydrate complete metadata only for selected identities, retaining pinned lines/revision and safe existing errors. Release the reader by RAII on success/empty/error before background handling; no partial result on failure.

Knowledge flags reuse current-file comparison and source provenance; missing/invalid current knowledge is stale. Read source bindings incrementally from outgoing external pages in the same pinned view, irrespective of traversal filters. Observe each binding once for that knowledge, union the three possible flags and discard per-page diagnostic collections after use. Reuse SourceProvenance::observe_sources on one bounded source page at a time; call it with an empty set only when that knowledge has no bindings, preserving basis validation. Repeated immutable-basis checks are a known simple tradeoff, not a new cache/API. Build each final link once, never re-observe it after selection. Different live-file observations have no cross-file atomicity guarantee.

Local source links use canonical ID, local file path, optional addressed range and SourceReader availability. Missing file/incomplete range adds source_not_found; unsafe/I/O failures remain errors. Standalone sources have no knowledge basis, so do not invent source_changed. Remote sources use their verbatim URL as ID/path, omit lines and remain unfetched without false missing/changed flags. Link keys remain element_id/path/optional lines/revision/flags; no text/hash/kind/score fields are added. All links share the pinned publication revision.

Background launch and safe warnings follow the existing successful-foreground path; failures launch nothing. Normal JSON/exit behavior is preserved. The baseline has ordinary OS signal termination; handled SIGINT, cancelled JSON and a stdout once-guard remain assigned to TK-29 and are not implemented or certified by TK-23. Exhaustive interruption/platform drills are not certified merely by these tests. Search JSON/ranking and existing check/inspect behavior remain unchanged; shared knowledge-comparison code is extracted only for its actual two consumers.

## Acceptance and evidence

- Independent literal fixtures for all modes/directions/types; multiple/duplicate/unknown starts; depth 0/1/u64 boundary; one-step includes; ordinary/includes cycles; dangling targets and stable ordering.
- Node versus independent edge caps; omitted/empty edges; original incoming orientation; actually traversed alternatives/back edges; no induced/post-node-limit transitions.
- Domain Graph has no infrastructure dependency; GraphService contains BFS; SQL and indexes stay in storage; lib.rs wiring and minimum necessary generic/object-safe interfaces reviewed physically.
- Bounded pages and first/cursor query ordering, high-degree/unrelated-corpus fixtures, strict keyset progress, both-direction merge ties, capped bookkeeping, metadata reads only for selected nodes and source bindings observed pagewise. Unchanged refresh uses narrow basis reads, not list_nodes. Report limits for changed publication/serialization rather than a false whole-call memory verdict.
- Strict input/output schemas, opaque URL/source starts/targets, live flags, missing/unsafe sources, no text/remote fetch/query inference, and existing background behavior.
- Controlled concurrent publication preserves old membership/edges/coordinates/revision/basis; late page/source/provenance errors fail the whole call and release the reader while the service stays alive; a later checkpoint/publication succeeds.
- Cache version 2 versus unchanged vectors version 1, old-cache safe rejection, documented rebuild preserving vectors/config/locks/corpus/Git, canonical DDL parity and native platform checks.

Use AAA and independent expected data. Hand-mutate meaningful production conditions/calls, require intended runtime failures, restore exact bytes and rerun covering tests. Keep full logs privately and report compact root executable counts separately for debug/release/ignored. Required make check/make verify and affected native runners must pass before completion; do not duplicate full runs when a captured runner already executes the exact required command on unchanged inputs. Historical evidence stays pinned; ignored/corpus/paid/pilot/gate limits stay explicit.

## Review and next step

The accepted user decisions are consolidated here. Fresh independent builder check `/root/tk23_revised_builder_check` (gpt-6.1-sol, high, isolated context) returned “nothing important left”. It covered this incremental/entity-service revision; the original builder check is historical and covered the superseded design only. The [revised implementation plan](../plans/2026-10-25-tk-23-traversal.md) contains four independently reviewable tasks and 20 tracked steps, with controller self-review complete. The user approved the revised documents and their page/index/cache/protocol choices on 2026-10-26 and selected Subagent-driven execution. Implementation proceeds through the approved plan and its task/final review gates. Merge/push/GitHub closure remain separately unauthorized.

## Implementation evidence — 2026-10-26

Code/test checkpoint `e9a9cd4a4c9d6807b0a75d79022dc8f838796e78` passed the host native runner, including exact `make verify V=1`: debug/release separately 684 passed, 0 failed, 5 ignored across 48 roots. Ten actual-production controls failed unchanged fixtures at runtime and were restored byte-exactly. The Linux runner was attempted at the same frozen inputs; captured debug is 684/0/5, but read-only Docker prevented final native/standalone receipts and cleanup. That initial Linux gap is closed by the fresh native/standalone PASS at741930e; the failed receipt remains historical. Input maps, partial Linux counts, failure diagnostics and memory limits are in the [TK-23 report](../../zkit/letopis/checks/TK-23/report.md). **Task4 review approved after I1 re-review; final whole-branch review completed with no Critical/Important product findings and code quality approved with M1/M2 documentation corrections; M1/M2 documentation corrections approved by scoped re-review; no open code or documentation finding**; controller owns final task-card/plan checkboxes and integration remains a later user decision.

### I1 test-only review fix — 2026-10-26

Checkpoint `5859e3ea15929b57abc6aafdb3dc477241c87f2d` strengthens the source-binding oracle with a literal 70-URI sequence and exact observed identity/multiplicity comparison while preserving page/observation order. The separate production substitution passes the old fixture and fails the strengthened unchanged fixture at the identity assertion; bytes are restored and all four binding cases pass. Supplemental `make verify V=1` on the revised 319-input map passes debug/release 684/0/5 across 48 roots each. Only bindings.rs differs; all other 318 production/native/build/DDL/test inputs match the original native map. Host native PASS and failed Linux receipts remain at e9a9cd4, not relabeled to this checkpoint. Fresh Linux native/standalone passed at741930e; Task4 independent rereview approved I1 with no new breakage; final whole-branch review completed with no Critical/Important product findings and code quality approved with M1/M2 documentation corrections; M1/M2 documentation corrections approved by scoped re-review; no open code or documentation finding. Full maps/control receipts are in the linked TK-23 report.

## Linux acceptance closure — 2026-10-26

Fresh Linux arm64 verification at `741930ed871e60a0987d655cf79defdff67defd3` passed: root debug/release684/0/5 across48executables each; schema/native/deadline/standalone/linkage complete. The outer319 and copied inner202 input hashes match current code/build/tests/DDL. The [report](../../zkit/letopis/checks/TK-23/report.md) retains both the failed original and new complete receipts. Implementation, code/document reviews and required native acceptance are complete; later docs commits do not relabel either code capture.
