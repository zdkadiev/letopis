# TK-27 — Embedding configuration and retry lifecycle

Status: revised proposal for user review; implementation not approved. Base: local main `6f59bbb` (includes TK-25 and TK-26). Task: [TK-27](../../zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-27-embedding-lifecycle.md). The early pilot TK-17 and G-1 are not established as complete by this task.

## Goal and chosen approach

Complete and demonstrate the existing configuration, vector reuse and bounded retry contracts. Assess the TK-13 implementation against the task and fix only demonstrated gaps, preserving the current service/storage boundaries. The user selected this focused approach over rewriting the lifecycle; rewriting adds risk without an identified need.

Authoritative behavior: [MC-3 and MC-5](../../zkit/letopis/mechanics.md), [embedding contract](../../zkit/letopis/contracts/X-2-embeddings.md), [read view contract](../../zkit/letopis/contracts/I-8-read-view.md), and TK-27. Where existing behavior already meets a requirement, retain it and cite or add meaningful evidence; do not infer completion from code inspection alone.

## Behavior

- Configuration identity is SHA-256 of canonical JSON. Reordering keys or changing whitespace preserves identity, vectors and failures; changing any field, including `max_retries`, changes identity. Missing or invalid config cannot wipe existing vector state.
- A valid changed identity triggers one guarded `BEGIN IMMEDIATE` vector-store transaction. It preserves an already matching identity, rejects an obsolete observation as specified below, and otherwise rebuilds vec1, clears vector keys and embedding failures, and updates identity atomically. A concurrent observer of the same identity must not erase subsequently stored work. A failure during reset rolls back the entire change; a previously pinned read view must remain consistent across reset and rollback.
- Work is derived from current knowledge search hashes without usable vectors and with remaining attempts. Sources are excluded; equal search hashes share a vector. Rebuilding the disposable cache leaves `vectors.db` intact and reuses matching vectors without provider calls, while newly introduced hashes are computed.
- Before each provider call, commit an attempt reservation in `embed_failures`; computing happens outside the write transaction. `max_retries` means additional attempts: zero gives one total and three gives four. Reservation survives process termination and restart, including termination before a response; an exhausted hash does not restart automatically while the same vector-store identity and failure state are retained. Changing identity or deleting vectors.db resets that state. Errors retain safe diagnostics and obey the same finite budget.
- Drain captures config identity. Publication or failure recording checks it within the write transaction. When the stored identity differs from the captured identity, a late result cannot modify vector/failure state and the old drain stops. An obsolete configuration observation cannot itself reset a different identity back to its captured value. A changed document is guarded by `search_hash`; its old vector must not represent the new text.
- For over-context input, preserve the whole searchable text in FTS and record a safe embedding failure using the ordinary retry budget; no splitting or silent truncation. Ollama native requests send `truncate: false`; compatible providers must meet the existing non-truncation contract. Do not add a universal guessed size limit or claim model-specific guarantees without evidence.
- Verify SIGINT against an actual drain process after an attempt is committed: process termination releases the lock, the attempt remains durable, and subsequent runs respect the remaining budget. Process-level handled cancellation and a `cancelled` JSON response for CLI commands remain TK-29, not this task.

## Boundaries

Use existing synchronous I/O, provider interfaces, application service and SQLite/vector storage; no async runtime, background daemon, job table, model registry or speculative abstraction. Keep `domain` independent of infrastructure and maintain composition through existing application-owned interfaces. Change provider, storage, CLI or service code only for a demonstrated defect within TK-27. Current diagnostics in `status` are TK-28; broad cancellation is TK-29; actual model quality, hardware capacity, physical power loss and system-wide drills remain separate tasks.

## Verification and deliverables

Map every TK-27 acceptance criterion to named, reproducible checks and record commands, platform, revision, observed results and limitations in the task report. Reuse prior evidence only when its exact source and scope still apply; run focused new checks for missing coverage. Include concurrent reset, rollback and old read-view compatibility; formatting-only config versus real change; delayed old-identity result; cache deletion/reindex with provider call counts; durable retries with restart and SIGINT; and provider context rejection with retrieval of a marker at the end of the full text and a safe error. Use a real SIGINT, not SIGKILL, for the SIGINT scenario. Exercise failures as well as successful paths. Tests should check independently observable behavior and avoid unnecessary long-running model calls.

For changed Rust code, run repository-required checks including `make verify`; run the storage/native platform runner when changes warrant it. If only documentation or tests change, report the checks actually performed. Review physical module boundaries and wiring; record any untested platform or gate rather than extrapolating from host results. Save the eventual implementation plan under `docs/superpowers/plans/`, and update the TK-27 card and evidence report after execution.

## Proposed correction: guard reset against an obsolete observation

Code inspection identifies this schedule: P observes A and pauses; the file changes to B; Q installs B and records a vector or attempts; P resumes and unconditionally resets storage to A. This remains a risk derived from code until a deterministic regression reproduces it. The old MC-3 model only covers duplicate observations of B. A stale reset can delete both vectors and attempts, so accepting it merely as recomputation is insufficient.

Use an observed identity as the precondition of reset, through the existing application-owned storage boundary:

1. Observe stored identity (including the initial absent state) before reading the configuration file. Release that read view before further work.
2. Read, parse and validate the file, obtaining the desired identity. Missing or invalid config never resets state.
3. In the existing short vector write transaction, read current identity again. If it already equals the desired identity, keep all vectors and attempts. Otherwise, if it differs from the observed identity, report a configuration conflict without changing anything. Only if it still equals the observation may reset install the desired identity.
4. On conflict, use existing mismatch handling to skip embeddings for this command/drain and allow the next invocation to observe current state. No unbounded reread/reset loop and no provider request using a mismatched identity. The exact typed internal result and how readiness is represented are implementation-plan decisions; they must preserve successful CLI/FTS behavior and the pinned search-view contract.

This conditional update preserves B in the schedule above. It adds no persisted version, generation, job state, configuration lock, editing command, or requirement for external editors to cooperate. SQLite owns the conditional check and write; the application service owns observation and file parsing, so storage never reads the config file.

The guarantee is conditional on the database observation, not immediate agreement with every arbitrary external file write. A file edit immediately after reading cannot be made atomic with SQLite under the existing file-edit contract. A subsequent invocation reconciles that change. Intentional later configuration changes can reset vectors and attempts as already specified; retry bounds apply between resets, not to the lifetime of a text across configuration changes or database deletion.

## Acceptance for the proposed correction

First add a deterministic regression for P(A)/Q(B)/late-P. After Q has stored both a B vector and a B failure attempt, resuming P must preserve B identity, the exact vector and attempt count, and must not call provider A. Also check two observations targeting B: the later observer must leave newly stored B data intact. Cover absent initial identity, successful A-to-B reset, and transactional rollback. Existing delayed-result tests continue to prove that old vector and error responses cannot overwrite the active identity.

If the schedule is not reproducible, or the conditional update requires a materially different contract, report it before changing the accepted design. No broader claim of atomicity with filesystem edits, lifetime retry budget, or completed project gates is made.
