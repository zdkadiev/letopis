# Letopis — full design audit

Date: 2026-10-20. Audited commit: `bf2a7abffb686a379f648006058759644692bade`.

## Verdict

**The project is structurally consistent, but not yet fully semantically consistent.** Four actionable findings remain: one P1 publication-protocol defect and three P2 contract/acceptance gaps. Passing existing checkers does not close them. No canonical design, accepted decision, task estimate, task status, or historical evidence was changed by this audit.

Scope: all nine stage documents, decisions, current external/internal contracts and schemas, domain/storage supplements, diagrams, executable design models, all 45 task cards and eight epics, schedule, and archived research provenance. The initial inventory contains 247 files, including 113 Markdown documents. Archived probes were inspected as historical evidence, not executed or represented as product implementation. Three independent scoped reviews were reconciled with primary project files; only supported findings are retained.

## Findings

### A-1 — P1: repairing an invalid external edit can leave the published revision ahead of its index

**Locations:** [mechanics.md:75](../../mechanics.md#mc-2--commit-rollback-и-reindex), particularly the final write unit; detection/repair at lines 80–84; [projection port](../../contracts/I-2-projections.md). The current model initializes files and the old projection together in [model_commit.py](../../mechanics/model_commit.py), lines 17–24; its reindex interleavings also treat apply as moving both states together, line 124.

**Failure scenario:**

1. The index contains A0 and B0.
2. External Git changes A to invalid Markdown and changes B to valid B1.
3. Automatic reindex fails validation, preserving the previous index as required.
4. An apply repairs only A, producing a valid tree containing A2 and B1. This repair is explicitly permitted at mechanics line 80.
5. The final unit publishes the apply's own affected projection, A2, and records the whole after-tree as `project.revision`. B can remain indexed as B0.
6. The next detection compares live bytes with that after-tree: B1 already matches, so the missed B update is no longer detectable from that comparison.

The index can therefore claim the revision A2/B1 while containing A2/B0, violating MC-2's publication invariant and FR-13/FR-28. The rule to include all outstanding external changes is explicit for selective reindex (line 84), but absent from the apply protocol.

**Minimal correction:** compute publication scope from the difference between the captured *published* revision and the final after-tree, not just the apply's requested writes. Publish all outstanding graph/FTS/jobs changes together. Keep canonical writes and recovery paths limited to the requested file changes; this does not require a full reindex on every apply. Add the above repair scenario to the commit model and later integration cases.

**Verification:** the attached evidence includes a finite counterexample to the written delta rule. This is specification analysis, not an observed runtime failure.

### A-2 — P2: status cannot reliably distinguish currently failed embeddings from recovered historical errors

**Locations:** [FR-32](../../requirements.md), line 102; [MC-7](../../mechanics.md#mc-7--диагностика-и-состояние), lines 174–178; [diagnostic lifecycle](../../contracts/I-11-diagnostics.md), lines 15–21; [CLI schema](../../contracts/schemas/X-1-cli.yaml), Diagnostic lines 135–160 and StatusOutput lines 771–833.

FR-32 requires status to distinguish active work, failed work, and vector lag. Internally, a failed job has an active error; a later successful replacement completes that error but retains its message until clear. `embedding_jobs` excludes failed jobs. Public diagnostics contain only code/message/details, with no required lifecycle information; status is not defined as selecting only active errors.

Thus a currently failed job and a recovered job can both yield `embedding_jobs: 0` plus the same `embedding_failed` diagnostic. An agent cannot determine whether intervention is still required. The JSON Schema accepts this indistinguishable reply; a top-level diagnostic `active` field is currently rejected.

**Minimal correction:** specify the lifecycle distinction using existing `details`, or explicitly define status.errors as active-only and keep historical diagnostics in the existing errors command. Settle one rule consistently across the card, schema, examples, internal projection and TK-28 acceptance. No new command or persistent table is necessary.

### A-3 — P2: the snapshot/inspection port only represents nodes, leaving edge inspection without a complete internal contract

**Locations:** [I-12 snapshot](../../contracts/I-12-snapshot.md), lines 8–17; [I-2 metadata](../../contracts/I-2-projections.md), line 21; [InspectInput/Output](../../contracts/schemas/X-1-cli.yaml), lines 364–443; [domain model](../../appendix/domain-model.md), line 105. The retained detailed contract explicitly demonstrates edge inspection at [legacy contracts](../../contracts/legacy/contracts.md), lines 456–483; no later decision withdraws that capability.

Public inspect accepts element UUIDs and returns their independent hashes. Edges have independent IDs and hashes, and may be optimistic context preconditions. However, `SnapshotRequest` has only knowledge/source ID sets and `CanonicalSnapshot.elements` is `Vec<NodeMetadata>`. NodeMetadata excludes edge fields; there is no specified edge-location/hash projection for inspect.

**Impact:** an implementer following the current internal shape can implement node inspection while omitting the existing edge use case. Requesting an edge's current hash for `base_hashes` lacks a fully specified path through the ports.

**Minimal correction:** define how inspect resolves edge IDs and computes their canonical location/hash from the same snapshot. This may use shared element inspection metadata or an explicit edge-resolution step; retain the current public command and output. Add a mixed node/edge batch example and an acceptance case to TK-24.

### A-4 — P2: resource checks do not require a populated, current vector index

**Locations:** [TK-15](../../roadmap/epics/EP-7-early-validation/tasks/TK-15-target-corpus.md), lines 28–39; [TK-16](../../roadmap/epics/EP-7-early-validation/tasks/TK-16-early-memory.md), lines 28–40; [TK-34](../../roadmap/epics/EP-4-system-validation/tasks/TK-34-mixed-memory.md), lines 28–40; [TK-35](../../roadmap/epics/EP-4-system-validation/tasks/TK-35-search-latency.md), lines 28–39.

The target fixture fixes 100k knowledge nodes, assumed 100k sources, 500k edges and file packing. It does not fix the vector-coverage state before RAM/latency acceptance. Since vectors are asynchronous and FTS fallback is intentional, the literal criteria can be met while most vectors remain queued/failed or vec1 is nearly empty. Even zero queued jobs does not establish vector coverage, as resource-math already warns.

**Impact:** the resource gates can accept measurements that do not exercise semantic search at the stated scale. This is a gap in planned acceptance, not evidence that the eventual implementation exceeds memory or latency targets.

**Minimal correction:** record knowledge count, current vector count, active configuration/search-hash compatibility, queued/failed counts and the actual A-2 byte profile. Require a populated semantic-index phase as well as a backlog-processing phase for the relevant RAM/search checks. Do not introduce a queue-drain deadline or a new product status field.

## Lower-priority completeness and reproducibility notes

- **URI escaping remains explicitly open.** [storage-model.md](../../mechanics/storage-model.md), lines 467–487, defines dot components, case and line numbers but defers encoding details. TK-7 should specify handling of encoded paths, including `src/a.rs` versus `src/%61.rs`, and distinguish `a%23b.rs` from `a%2523b.rs`. This is recorded as codec work, not a new architecture or a claim that aliases are currently resolved incorrectly.
- **Validation environment is not self-contained.** [checks README](../README.md), line 12, uses `python3` but provides no dependency setup. The default interpreter lacked PyYAML/jsonschema: the standard contracts checker exited 0 while explicitly skipping schema checks, and the two dedicated checkers failed imports. All checks were rerun successfully with the existing `/Users/zdkadiev/bin/python3`, PyYAML 6.0.2, jsonschema 4.23.0 and referencing 0.36.2. Document a small reproducible validation environment and treat skipped schema validation as incomplete evidence. This is tooling reproducibility, not a production-library compatibility failure.
- **TK-1 execution details remain.** Its objective and inputs suffice to start. An OS/architecture test matrix, Linux execution environment and concrete unsupported-build cases must be recorded during execution. The recent conversation about retaining integration tests instead of a disposable harness was a recommendation, not an approved scope change; this audit does not rewrite it into a decision.

## Checks and positive findings

[Raw evidence and commands](full-audit-checks-2026-10-20.json) include initial environment failures as well as complete successful reruns. Historical results were not overwritten.

| Check | Fresh result | Limit of conclusion |
|---|---|---|
| Eight native zkit stage checkers | All exit 0 | Structure/traceability checks; not a semantic proof |
| Layout | Exit 0 | zkit 0.2.6 layout matches |
| Weekly schedule and cards | Exit 0: 45 tasks, 8 epics, 372 h, 38 weeks; dependency/gate errors 0 | Estimates remain estimates; no execution marked complete |
| External contracts/examples | Exit 0: 67 CLI cases, 6 provider examples, 8 metadata cases, 97 assertions | Shapes and specified example checks only |
| Internal contracts/models | Exit 0: 69 cases, 3 normal models with no violations, all 14 isolated guard removals detected | Does not cover A-1's missing external-repair scenario or physical durability |
| Resilience arithmetic | Exit 0: 17 checks | Illustrative arithmetic, not measured availability/RPO/RTO |
| Linker | Exit 0; 9 stage docs, 89 decisions, 9 research reports; no changes on a disposable copy | Original files untouched |
| Local Markdown links | 2,620 references inspected; no broken navigational links found | Three nonexistent targets are fictional inline examples in archived documents, not navigational defects; external local skill links are not portable artifacts |
| Data syntax | 69 JSON files and 4 YAML schemas parsed; all local schema references resolve; 1,039 JSONL records and 2 TOML files parsed | No archived probe executed |
| Python syntax | 18 files parsed without syntax errors | No research program execution or product certification |
| Mermaid syntax | 9 blocks/files parsed, including all four standalone diagrams | Syntax only; no visual-rendering certification |
| Research hashes | 58 of 60 entries match current bytes; remaining two are README link migrations | Both exceptions are expressly documented in the migration report; no raw-result mismatch found |
| Git diff whitespace | Exit 0 before report creation | Source tree unchanged by checks |

The high Must ratio warning (42/44, 95%) is informational and already explained by imported accepted scope. It is not a reason to weaken requirements. `LIMITS=[]` in resource/resilience models is not interpreted as proof that resource or recovery goals have been met. RAM, complete search quality/latency, platform builds, vec1 reset with live readers, filesystem power loss and external backup restore remain explicitly assigned to implementation tasks/drills.

## Recommended correction order

1. Resolve A-1 in the save/publication protocol and extend its finite model.
2. Align A-2 and A-3 across public/internal contracts, schemas, examples and task acceptance.
3. Strengthen A-4 before running the resource gates.
4. Close the URI-escaping detail within the existing codec task and document validation prerequisites.

These are corrections inside the accepted scope. They do not require extra services, caches, registries, commands, a new design framework, or new benchmarks during this audit. No task is marked done, and no claim of runtime readiness is made.
