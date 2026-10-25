# TK-22 — Search filters and embedding fallback

Status: proposed; awaiting user approval. No product implementation or implementation plan is approved by this document yet.

## Intent and scope

Complete the existing CLI search with the X-1 filters `needs_recheck`, `sources`, and `source_revisions`. An agent changing code can find knowledge bound to that code, optionally requiring matching historical source bytes or a reason to recheck it. All three fields and their combinations are in scope; the main example is an editorial choice, not a reduced acceptance scope.

Implement product behavior and its focused acceptance checks. Keep the existing hybrid rank formula, metadata-only output, synchronous I/O and physical hexagonal boundaries. Reuse TK-20 source identity, TK-21 source comparison, and the TK-13 query executor.

Excluded: new ranking algorithms, source text output, remote fetching, new persistent caches/schema, async runtimes, new query syntax, full traverse (TK-23), mixed source inspect (TK-24), and the status command/diagnostics UI (TK-28). Deferred pilot/resource gates are not certified by this development task.

## Sources and verified starting point

- [Task card](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-22-filters.md).
- [Requirements FR-20–FR-24](../../zkit/letopis/requirements.md).
- [Architecture and physical modules](../../zkit/letopis/architecture.md#program-structure), including its review checklist; repository AGENTS.md applies.
- [Decisions D-12, D-14, D-62, D-95, D-102](../../zkit/letopis/decisions.md).
- [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md) and [schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [I-4/I-7 source comparisons](../../zkit/letopis/contracts/I-4-sources.md), [I-8 read view](../../zkit/letopis/contracts/I-8-read-view.md), and [MC-6](../../zkit/letopis/mechanics.md#mc-6--чтение-поиск-и-обход).
- [Accepted TK-21 design](2026-10-25-tk-21-provenance-design.md).

Worktree `/Users/zdkadiev/.codex/worktrees/tk-22/letopis`, branch `tk-22`, starts from `main@cafd60b54032ca878bbca7fdda9a1839c4ffc658`. GitHub main matched that exact SHA during preparation. Existing TK-21 evidence and other worktrees are preserved.

Current code: CLI recognizes filter fields but rejects effective filters as unavailable. Search reads complete FTS/vector candidates, ranks and limits, then observes only returned sources. TK-21 already provides `SourceProvenance::observe_sources` and `compare_revision`; the latter validates an explicitly requested tree before comparing a binding. These capabilities are inputs, not proof of the new filter path. No fresh baseline suite is claimed during documentation preparation; run it before product edits.

## Observable contract

The existing command envelope stays unchanged. Example input:

```json
{"query":"sqlite publication","limit":10,"filters":{"sources":["src/storage/sqlite.rs"],"needs_recheck":true}}
```

A source revision selector, when present, is a complete valid Git tree OID under the existing X-1/X-3 contract; it is not the index publication counter returned in search links. No branch-name, abbreviated-OID or commit-to-tree resolution is introduced.

- Different filter fields combine with AND.
- Values within `sources` or `source_revisions` combine with OR; duplicate values have no effect.
- An absent list and an empty list impose no restriction. An absent `filters` object and `{}` are equivalent.
- Absent `needs_recheck` imposes no restriction. `true` selects knowledge with at least one currently observed X-1 flag (`stale`, `source_changed`, `source_not_found`); `false` selects knowledge with none. Absence of flags is not a claim of truth or remote availability.
- Sources are matched against current source bindings of the candidate knowledge, from the same pinned view as its metadata. A missing source selector is an ordinary nonmatch, not a request-level not_found.
- When both source lists and revision lists are effective, there must be one binding whose source matches the source list AND whose addressed bytes match at least one requested revision. Matching source A and revision bytes only for source B must not qualify the knowledge.
- With only revisions, at least one source binding must match one revision. Knowledge without sources cannot satisfy effective source/revision filters.
- Revision equality means equality of complete addressed bytes in the knowledge's declared basis and the requested tree; unequal tree IDs can match. Line ranges are inclusive and selected identically from both trees. Missing paths/ranges or missing/unusable bases do not match; remote addresses remain unknown and do not establish byte equality.
- Validate every requested revision before query inference and candidate-dependent exits, including empty query, empty corpus or selectors matching no sources. A valid-shaped missing tree rejects the whole request with `not_found`; malformed OID or wrong Git object type is `invalid_request`; Git corruption and operational failures remain safe typed errors. No partial results are returned. For multiple invalid roots, deterministic normalized order determines the reported error; no new error aggregation field.
- CLI keeps strict object/type validation: null filter fields, wrong field shapes, unknown fields and malformed source addresses are invalid requests. Valid empty arrays stay accepted.

## Builder choices proposed for approval

The following details are engineering defaults proposed here, not previous user decisions:

1. Source selectors use the existing `SourceUri` parser and exact canonical identity. A whole-file address and a line-range address are different sources; there is no implicit path prefix, glob or range containment. Local paths normalize as TK-20; HTTP/HTTPS preserves its opaque spelling, query and fragment. A valid remote address can satisfy the source-address filter without fetching it.
2. Reuse the existing SearchService rather than add a general filter engine. Add a typed filter value to SearchRequest and small cohesive private helpers. Deduplicate inputs into ordered collections; the implementation plan chooses concrete type/file details without a new public JSON shape.
3. Add the smallest read-only source-provenance capability needed to validate all requested trees independently of candidate bindings. Search calls that application interface; it does not import a concrete Git implementation or fabricate a dummy source merely to trigger validation. Reuse TK-21's error classification.
4. Establish eligible IDs from the union of complete channels. Apply field predicates before channel rank positions/fusion and before final limit; each channel ranks only eligible candidates, preserving its metric and identity tie-break. Then retain the existing reciprocal-rank sum and final ID tie-break. No-filter rank and output remain unchanged.
5. Observe each candidate's current flags at most once during eligibility; carry the observation into returned links, avoiding a second current-source read that could disagree with the filter decision. Temporary per-request observations are allowed; no persistent source cache. Source/revision predicates may eliminate candidates before unnecessary live reads. Reuse pinned metadata/bindings throughout.

## Search lifecycle and fallback

Preserve refresh -> configuration readiness/reset -> pinned read view -> captured compatible query/vector configuration. Validate revision selectors before calling the model and before any empty-result shortcut. Read graph metadata and source bindings from this one owned StorageRead, including when another process publishes concurrently.

FTS and exact-vector reads supply complete candidates, with no early SQL/vector LIMIT or fixed oversampling. Source checks may perform synchronous Git/filesystem reads but never hold a write unit or mutate basis refs. Normal refresh may still publish and retain bases as already designed.

An unavailable configuration, unavailable query embedding, provider timeout/error or config mismatch uses the existing FTS path with the same effective filters and ordinary success output. Preserve the existing safe singleton query_error lifecycle in vectors.db: last query failure stored, next successful query embedding clears it. No query-embedding warning is added to search JSON. Full status presentation remains TK-28; test the existing storage/executor boundary directly now.

Do not broadly swallow storage, Git, source-read, integrity or diagnostic-persistence errors as embedding fallback. Reuse the query executor's typed available/unavailable outcome; unexpected operational failures retain safe error behavior. On success, empty result or any error, scoped read ownership releases the transaction; no channel returns partial links. Existing background-launch warnings remain unrelated to query embedding.

Live filesystem observations are per-file, not an atomic snapshot across all sources. A file changing after observation can invalidate the answer later; do not promise stronger freshness than TK-21. Historical Git comparisons remain immutable object reads.

## Physical ownership

- `src/domain/`: existing pure source/OID/hash/address values; add domain types only for actual rules, no I/O or transport DTOs.
- `src/service/`: application-owned typed filters and traits declared/reexported in mod.rs; SearchService owns eligibility/ranking orchestration. ProvenanceService owns exact historical byte comparison and revision validation through its Git interface. Each concrete struct/impl remains in its responsibility-named file; private helpers may be split for cohesion.
- `src/git/`: Git object verification and reads through existing interfaces, using argv/stdin. No source-filter or search orchestration here.
- `src/storage/`: complete native FTS/vec1 candidates and pinned graph reads; no builder types escape storage. No new storage API/schema unless implementation identifies a concrete missing capability and documents it.
- `src/embedding/`: existing synchronous client/config/query outcome and safe diagnostics; no provider duplication.
- `src/cli/`: strict JSON-to-typed-request conversion, service call and existing rendering. No Git/storage/provider calls in handlers.
- `src/lib.rs`: retain real wiring; extend constructor calls only where the new read-only interface requires it. `main.rs` remains thin.

Follow AGENTS.md for imports, visibility, verbs, documentation, focused functions, AAA tests and manual mutation controls. Do not create ports/adapters/contracts directory layers or generic registries.

## Acceptance and review evidence

The implementation plan must assign files, focused test targets and meaningful hand mutations for these behaviors:

1. CLI accepts all effective filters and rejects null/wrong-shape/unknown/malformed inputs; empty and absent lists and duplicate selectors are equivalent.
2. AND across fields and OR within each list, including needs_recheck true/false/absent and knowledge without sources. Exact whole-file/range and local normalization plus opaque remote identities are explicit fixtures.
3. Same-binding trap: two sources cannot independently satisfy the source and revision fields. One qualifying binding is sufficient.
4. Different trees with equal selected bytes match; changed selected bytes do not. Outside-range changes do not invalidate a range match; incomplete ranges, missing files/bases and unfetched URLs cannot establish equality.
5. All requested revisions are validated even in an otherwise matching list or with no candidates/empty query. Missing/wrong-type/corrupt roots produce their correct safe codes before any provider call.
6. A high-ranked excluded candidate cannot consume limit; eligible lower-ranked results fill it. Separate FTS-only and vector-only eligible matches and a hybrid fixture establish filtering before channel ranks/fusion, ties and final limit using independent expected IDs/order.
7. Live source changes and stale knowledge drive needs_recheck, retain expected flags and never return raw text. Concurrent index publication cannot mix metadata and source bindings.
8. Controlled query-embedding failure falls back to correctly filtered FTS success without a query warning; safe query_error persists and is cleared after actual success. Missing/bad config and incompatible vectors retain existing safe behavior. No real model/network payment is required.
9. Exercise a failure after the read view is pinned while SearchService remains alive; verify the read is released and later publication remains possible. Existing no-filter regressions remain green.
10. Review module dependencies, visibility, interface implementations and actual lib.rs wiring, not only lint results.

Before product edits run a clean baseline. After implementation run focused tests and hand mutation checks with exact restoration, then make check and make verify under the pinned toolchain. Keep stdout compact and save detailed failure/evidence logs. For documentation-only preparation validate links and consistency without rerunning unchanged product suites.

Record actual changed revision, commands, counts, platform and limits in docs/zkit/letopis/checks/TK-22/, and update the task card. No historical native/Linux suite, large-corpus performance, paid pilot or G-1/G-2 gate is claimed unless actually run. Storage/native build runner is required only if its governed inputs change.

## Approval boundary

This document proposes the complete TK-22 behavior and the five builder choices above. Independent builder check on 2026-10-25 returned “nothing important left”; no open builder questions. Product implementation waits for specification approval and then approval of the separate Superpowers implementation plan.
