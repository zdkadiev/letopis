# TK-12 — Search and consistent reads

Status: approved by the user on 2026-10-23: «Да, утверждаю спецификацию. Продолжай к плану реализации TK-12 по Superpowers». The independent builder check and its resolved configuration-readiness question are complete. Preparation of the implementation plan is authorized; product implementation awaits plan review and execution-method selection.

## Intent and verified starting point

Make the real CLI search indexed knowledge and inspect current knowledge metadata. Reuse the completed synchronous document, storage, Git and embedding implementations, retaining the hexagonal boundaries and metadata-only X-1 responses.

Work happens on branch `TK-12` in `/Users/zdkadiev/.codex/worktrees/tk-12/letopis`, created from fetched, matching main/origin main `02b009059ea7a6490f97c2d9480b3db501e7f437`. Before changes, `make check V=1` passed on macOS ARM64: 390 root tests passed, 0 failed, 4 ignored; 8 filtered nested summaries are separate. Evidence: `.artifacts/tk12/start-2026-10-23/`. This is a baseline, not TK-12 acceptance or fresh Linux evidence.

Storage already provides a scoped read transaction pinning cache revision and vector identity, complete BM25 matches, native exact cosine candidates joined by current `search_hash`, and guarded vector writes. TK-11 supplies strict inspect parsing, response rendering, background orchestration and actual drain composition; its inspect service is intentionally unavailable. TK-13 supplies validated configuration, providers, query embeddings and safe query-error persistence. Neither existing acceptance nor interface doubles prove this new CLI path.

## Accepted scope decision

On 2026-10-23 the user selected: “Подключить готовую TK-13 сейчас (рекомендую)”. TK-12 therefore depends on completed TK-13 and owns real query-embedding integration into CLI search now. Controlled HTTP fixtures exercise the actual existing provider implementations. Do not reimplement providers or claim actual model quality from fixtures. TK-14 still owns the new-agent end-to-end workflow, initial agent skill and target-corpus preparation.

The user approved this complete description, including the choices labelled builder calls. Those labels preserve their origin as engineering recommendations, now accepted through full-specification approval.

## Design sources and precedence

- [TK-12 task](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-12-search.md), [TK-14 handoff](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-14-thin-slice.md).
- [Architecture and physical boundaries](../../zkit/letopis/architecture.md#program-structure), [architecture review](../../zkit/letopis/architecture.md#program-structure-review), [decisions](../../zkit/letopis/decisions.md), especially D-62, D-77, D-91 through D-101.
- [Mechanics MC-4 and MC-6](../../zkit/letopis/mechanics.md), [X-1](../../zkit/letopis/contracts/X-1-cli.md), [X-1 schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml), [I-8](../../zkit/letopis/contracts/I-8-read-view.md), [I-12](../../zkit/letopis/contracts/I-12-snapshot.md), [source observations](../../zkit/letopis/contracts/I-4-sources.md).
- Existing [TK-13 design](2026-10-22-tk-13-embeddings-design.md) and [TK-11 design](2026-10-22-tk-11-cli-runtime-design.md) establish reusable behavior and historical acceptance boundaries.

Final v1 schemas remain unchanged. Incremental-build limits below are explicit limitations, not acceptance of unfinished v1 behavior. Do not add runtime, dependency, storage schema, daemon, queue, public score, extra response fields or configuration keys.

## Search request and response

Implement `letopis search` with a strict JSON object on stdin: required string `query`, positive integer `limit`, optional schema-shaped `filters`. Validate argv, object shape, scalar types, numeric conversions, unknown fields and trailing JSON before constructing services or touching a repository. Keep existing one-JSON/exit 0 or 1 conventions and safe diagnostics.

Builder call: this baseline supports absent filters, `{}`, and empty `sources`/`source_revisions` lists as equivalent. Valid nonempty source filters or either explicit `needs_recheck` boolean remain unavailable until TK-22; reject them before refresh with the existing `internal_error` and a fixed safe unsupported-operation message. Malformed filter shapes, element types and OIDs remain `invalid_request`. Never silently ignore a valid restriction and return a broader successful result. This follows the incremental unavailable-operation approach accepted in TK-11; it does not introduce an error code or redefine the final schema.

Results are X-1 Link objects containing `element_id`, `path`, inclusive `lines`, pinned cache `revision`, and unique `flags`. A knowledge ID already supplies the path/anchor address; do not invent a separate URI field. Return no content, excerpts, titles, embeddings, native distances, ranks or combined scores. Sources do not enter the search-text candidate set.

## One read view and pinned query configuration

1. Refresh document projections through `Documents`, using its existing lock/redetection/publication. Do not wrap it in another index lock.
2. Reuse TK-13 readiness to read, validate and synchronize configuration before opening the search read view. Missing configuration permits FTS. Preserve the accepted TK-13 readiness contract: unreadable, unsafe or invalid configuration is represented by `ConfigState::Invalid` and permits FTS, including the filesystem failures currently mapped to that state. Do not change shared readiness error classification for drain/background as part of this task. Failures of subsequent storage synchronization remain typed errors; ordinary knowledge-file/confinement failures outside config readiness also remain errors.
3. Begin one scoped storage read view, pinning cache revision and vector identity; read native dimensions through that view. Only a captured ready configuration with matching identity can supply a vector channel.
4. With compatible available vector storage, compute at most one transient query vector using that captured configuration and the dimensions observed by the search view. Compatibility here means matched configuration identity and known compatible native dimensions. Stored vectors whose hashes no longer join current pinned nodes may produce an empty channel after this one attempt; do not add a pre-query corpus/vector scan solely to predict that empty result. No write transaction, index lock or embed lock spans inference. Query inference does not reserve document attempts, retry a query, launch another runtime or cache query vectors.
5. Reuse the TK-13 validation and guarded singleton query-error update: provider failure gives ordinary successful FTS without a query warning; a successful query clears the error. Infrastructure/diagnostic-write failures remain real errors. An identity change discards the vector and cannot overwrite the new model's diagnostic.
6. Use FTS and vectors only through the original view. Verify vector identity and dimension against it; no reopening midway, global config reread, newest-node lookup, or mixed cache revisions. If vector computation becomes unavailable, continue this same view with FTS. RAII releases the view on all returns; release it before background readiness/launch.

Builder call: extend the existing embedding service boundary only enough to execute a query against a caller-captured immutable configuration/dimension. The existing standalone `embed_query` delegates to the same implementation after its own readiness capture. Do not blindly call the current method while search holds its view: it rereads and may synchronize a different model. Keep HTTP parsing, deadline, validation, safe diagnostics and identity-guarded publication with their existing owners. Use independently owned connections for the search read and the embedding service's short diagnostic/config transactions; never share a live connection or upgrade a read view into a write unit.

The two database files need not share one cross-file instant. Cache metadata/FTS must share one cache snapshot; vectors join only the pinned node search hashes and configuration. Concurrent vector publication/reset must produce a valid compatible channel or explicit FTS fallback, never another model's result. Native reset/schema-change behavior with an older reader is a required executable acceptance case, not assumed from WAL.

I-8 currently says a mismatch “otherwise ... resets MC-3”, while MC-6 says mismatch gives FTS-only. Approved clarification: normal synchronization occurs before the view; a mismatch discovered after pinning gives FTS-only for that request, with no reset or repin from the read view. A later call can synchronize again. Align I-8 with that sequencing as a clarification in the approved design, not a write-on-read capability.

## Literal FTS and ranking

Build FTS expressions from literal tokens, quote each token and bind the completed expression as a SQL value. User input is never an SQL fragment or an FTS language expression: operators, quotes, wildcards, column syntax and punctuation cannot change the query grammar. Empty or untokenizable text returns `[]` successfully without calling FTS MATCH or HTTP.

Builder call: use a documented deterministic Unicode alphanumeric token split, retaining non-Latin letters and digits, then literal-quote tokens and join them with FTS AND. Verify Cyrillic, Latin, digits, accents, punctuation and operator-like strings against the real pinned FTS5 tokenizer. This is a baseline policy, not a claim of stemming, semantic quality or complete language coverage. Add no new tokenizer dependency or hand-written FTS parser.

Read all eligible FTS and vector candidates without an early limit. FTS order is ascending BM25 then normalized ID; vector order is ascending cosine distance then ID. Each channel has one-based ranks. Union by knowledge ID and compute internal score `sum(1/rank)` for the ranks present; order by score descending, then ID ascending. Apply the requested limit last. Sources are excluded, ties deterministic, and no undocumented RRF offset or weighting is added. Check native/derived numeric validity before ordering, retaining typed failures instead of NaN-sensitive sort panics. Normalized duplicate IDs or reused text hashes must not duplicate an element in results.

## Metadata and baseline freshness

Use metadata and revision from the pinned cache view. Compare each returned knowledge element with a confined current Markdown observation through the existing codec: missing/changed bytes, hash or coordinates produce `stale`; do not substitute current coordinates into an older index result. Invalid current markup retaining an old valid projection must also be observable as stale. Files may change after observation; this is not an atomic live-corpus guarantee.

Builder call: TK-12 implements knowledge `stale` and the existing X-1 flag container. Full source lifecycle, historical-basis validation/pinning and `source_changed`/`source_not_found` integration remain TK-20/TK-21/TK-22. Empty flags mean no cause detected by the currently implemented checks, not verified truth or completed provenance validation. Do not claim all FR-20/v1 freshness semantics or enable `needs_recheck` before those checks exist. Task/spec/report must identify this baseline explicitly; TK-22 must consume it rather than assume all flags were implemented here.

Real filesystem/confinement/permission/corruption failures are errors, not stale-only success. Stable disappearance or a valid but changed element is an expected stale observation. Do not log or serialize the reread text. Keep repeated observations temporary within one request; no persistent text/source cache.

## Basic inspect from one retained Git tree

After document refresh, perform canonical capture through an application service using Git candidate enumeration, confined current-file reads and the Markdown codec. SQL supplies no authoritative current hash, coordinates or element membership. Capture current marked Markdown files, create one immutable tree, and pin the returned tree before reporting success. Only the knowledge IDs located in that captured tree supply response metadata. Validate membership from captured content, never an old index row or a later live reread.

Builder call: implement basic knowledge-only inspect, including knowledge IDs spanning multiple files and duplicate normalization, with one common tree and all-or-nothing errors. This avoids an artificial single-ID limit. Full source/mixed-batch behavior, complete v1 lifecycle coverage and agent examples remain TK-24. Snapshot capture may omit external source files at this baseline; it must not claim complete I-12 capture until TK-24 includes every addressed source. A valid source-only selector outside this knowledge baseline fails closed as unavailable; an absent knowledge ID is `not_found`. A source absent from all current references is `not_found`, not a fabricated source response. Resolve path-shaped knowledge/source membership from canonical captured declarations; syntax alone is not sufficient to classify it.

For a knowledge element, `path`, inclusive `lines` and SHA-256 cover exactly its bytes in that tree. Return existing X-1 `snapshot` and `elements`, without flags or content. Missing or removed requested knowledge rejects the entire request, including when old SQL metadata remains after an invalid file. Invalid marked input preventing reliable canonical membership fails as `validation_failed`; Git/filesystem failures remain safe typed errors. No partial response is published. Earlier committed refresh and harmless unreferenced Git objects are not rolled back by later capture failure.

Capture current candidates once and use those captured bytes consistently. Per-file safe reads can detect concurrent replacement and fail; they cannot guarantee one atomic instant across live files. A later edit never changes the returned retained tree, and subsequent Git reads of it reproduce the reported metadata. User refs, HEAD, Git index and corpus bytes are unchanged; only the existing service snapshot refs may be created.

Do not accumulate full corpus text merely to satisfy the current `create_tree(&[SnapshotFile])` interface. The implementation plan should introduce the smallest application-owned incremental Git object/tree capability needed to write each captured file before releasing its bytes, retaining only path/object identities and metadata until tree assembly. Git handles objects/refs; the service handles filesystem/Markdown orchestration. No Git adapter calls a filesystem or Markdown adapter. This is an actual inspect need, not speculative general infrastructure.

## Physical ownership and composition

Follow the [program structure](../../zkit/letopis/architecture.md#program-structure) and AGENTS.md in every task and review.

- Domain contains pure values/ranking rules where useful; no I/O, serde envelopes or orchestration.
- Application-owned search/inspection interfaces and typed requests/results are declared or reexported by `service/mod.rs`. Keep the existing `Inspections` interface. Each concrete search or inspection service lives with its struct/impl in its own responsibility-named file. Services depend on interfaces, not technology implementations.
- Storage retains rusqlite, SeaQuery, FTS5/vec1 details and scoped read lifetime. Reuse the existing complete-candidate methods; alter storage only for demonstrated missing behavior.
- Git owns Git argv/stdin/object/ref operations. Filesystem owns confined reads. Markdown owns parsing. Embedding/provider transport remains existing synchronous ureq code.
- CLI parses/renders and calls service interfaces only. It adds real search and replaces the basic inspect unavailable route; full check remains owned by TK-19.
- `lib.rs` performs real constructor calls and wires independent storage connections, document refresh, search, inspection, existing embeddings and background triggering. `main.rs` remains thin. No locks, trait-object registries or shared mutable state just to simplify wiring.

This design-stage document left exact files/signatures to the separately reviewed implementation plan, which now records the committed file map. Prefer a few cohesive files to one file per helper/type. Respect caller-before-callee order in new/touched code, minimal visibility, explicit imports, verb names, meaningful constants, approximately 40/max60-line functions, 300–500-line responsibility files, documentation contracts and Arrange/Act/Assert tests. No generic runtime/framework or new dependency is needed by this design.

## Verification and deliverables

| Area | Required observable evidence |
|---|---|
| CLI contract | Real binary with strict X-1 search/inspect inputs and schema-validated metadata-only output; one JSON and correct exit; malformed/unknown/trailing input rejected before repository mutation |
| FTS | Real contentless FTS matches, Cyrillic/Latin/quoted/operator input, empty untokenizable input, literal binding and deterministic ties |
| Hybrid | Actual existing Ollama/OpenAI-compatible clients against controlled fixtures; both channels, disjoint/overlap/ties, duplicate text hashes, final-only limit, vector-only results and comparison with expected reciprocal ranks |
| Fallback | Missing/invalid config, no vectors, provider failure/bad vectors and model switch; FTS success without query warning; singleton diagnostic written/cleared safely, no document attempts consumed |
| Consistency | Deterministic concurrent cache publication after pinning, vector insertion and native reset during a read/query, config change before/after pinning; one cache revision and no mixed-model results |
| Freshness | Live edit/delete/invalid markup after pinning yields stale with pinned metadata; confined I/O failures remain errors; full source flags explicitly deferred |
| Inspect | Real refresh + immutable retained tree, exact knowledge hash/lines, cross-file IDs, duplicates, missing/deleted ID all-or-nothing, edits after capture, actual Git reads/GC retention and unchanged user refs/index/corpus |
| Ownership | Physical file/trait/visibility/import inspection and actual lib.rs assembly; handlers do not invoke technology implementations |
| Regression | Fresh `make check`, then `make verify`; focused binary/process suites, relevant native storage runners on host/Linux when storage/native integration is affected; record actual scope/platforms |
| Mutation | Changed-code mutations during implementation and full crate per AGENTS.md before merge; report caught/missed/timeout/unviable with narrowly justified equivalents, plus compiling negative controls for read-view/config/hash-join invariants |

Store the approved spec in this directory and the subsequent implementation plan in `docs/superpowers/plans/`. Record final results in the TK-12 task card and `docs/zkit/letopis/checks/TK-12/report.md`, with new ignored receipts under `.artifacts/tk12/`. Do not overwrite prior evidence or mark pending platform cases done. Preserve test-helper nested counts separately from root test counts.

No claims of live model usefulness, minimal-hardware RAM/latency, x86/other platforms, physical power loss, full source provenance/filters/traverse, full check or handled SIGINT are made here. Those retain their existing roadmap owners. This design-stage document originally gated implementation on separate plan review and execution-method selection; those preparation gates have since passed and implementation is committed as `facad76bddf3f02b7c206cd152070e8a447929b7`. Whole-branch review and the final scoped re-review are approved; completed acceptance is recorded in the TK-12 report and acceptance-document commit. This scope does not merge, push or close issues automatically.

## Open status

The user-approved scope question is resolved: reuse TK-13 now. The independent builder check raised one question about configuration-readiness errors. The existing TK-13 interface and implementation answer it: preserve their Invalid/FTS outcome, without widening this task into a shared error-policy change. The text above is corrected accordingly; no user-only question remains from that check. The user approved the full description, including the labelled builder calls, on 2026-10-23. The implementation is committed as `facad76bddf3f02b7c206cd152070e8a447929b7`; no dependency change was introduced. Platform, changed-scope and completed whole-scope mutation receipts, together with completed independent reviews, are tracked in [the TK-12 acceptance report](../../zkit/letopis/checks/TK-12/report.md).

Independent check and controller resolution: [design review](../../zkit/letopis/checks/TK-12/design-review-2026-10-23.md).

## Implementation acceptance

Accepted on branch `TK-12` after Task 5, whole-branch and scoped final re-review. Product source remains `facad76bddf3f02b7c206cd152070e8a447929b7`; final test-only ordering commit is `a38f3f764d9fe5a61622faa9c496489ce256fafd`. Whole mutations account for all 2208 configured candidates: 662 caught / 0 missed / 0 timeout / 1546 unviable. Fresh host verification after the test relocation passed 454/0/4 in each root profile; earlier platform/native/mutation receipts retain their exact frozen input chronology. The [acceptance report](../../zkit/letopis/checks/TK-12/report.md) retains the deferred embedding-test maintenance, all review findings, rulings and limits.
