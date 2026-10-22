# TK-13 — Configuration and synchronous embedding executor

Status: conversational design agreed on 2026-10-22; written specification awaiting user review. No implementation plan or product implementation is approved yet.

## Intent and accepted scope

Implement the production library path for reading embedding configuration and draining current knowledge texts through Ollama or an OpenAI-compatible API. Documents and queries use the same texts-to-vectors interface and active configuration. Knowledge, graph metadata and FTS remain usable without a configuration or a successful embedding.

The user selected an independent branch/worktree from local `main`, alongside TK-10, and approved responsibility-separated components and the execution flow below. The user also requested the project-local Rust skill. Keep the design simple, synchronous and hexagonal.

- Branch: `TK-13`.
- Managed worktree: `/Users/zdkadiev/.codex/worktrees/tk-13/letopis`.
- Starting main commit: `bf9b6b9c46ddc9af0864a3a3924e8d0826da5abb`.
- Product baseline: Markdown/domain, Git snapshots, SQLite storage and typed SeaQuery helpers from completed TK-3/TK-6/TK-9 inputs; TK-10 is a separate unmerged development line.
- This specification and its task card are the only intended Git changes at this stage. Existing primary-worktree edits and TK-10 changes are preserved.

## Authority and traceability

Read this specification together with:

- [TK-13 task](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-13-embeddings.md), covering FR-22, FR-29, FR-30 and NFR-6.
- [Program structure](../../zkit/letopis/architecture.md#program-structure) and [structural review](../../zkit/letopis/architecture.md#program-structure-review).
- [Mechanics](../../zkit/letopis/mechanics.md), especially MC-1 search text, MC-3 work/attempts and MC-5 drain exclusion.
- [X-2 embedding boundary](../../zkit/letopis/contracts/X-2-embeddings.md), [HTTP schema](../../zkit/letopis/contracts/schemas/X-2-embeddings.yaml), and the Configuration definition in [X-1](../../zkit/letopis/contracts/schemas/X-1-cli.yaml).
- [Decisions](../../zkit/letopis/decisions.md): D-62 query failure, D-78 additional retries, D-83 deadline, D-92 text-addressed vectors, D-95 derived work, D-96 synchronous dependencies, D-98/D-99 component boundaries and D-100 query construction.
- [Storage specification](2026-10-22-tk-9-storage-design.md), including vector transaction/identity guards and the absence of a default model or dimension.
- [TK-3 evidence](../../zkit/letopis/checks/TK-3/report.md) and [TK-6 evidence](../../zkit/letopis/checks/TK-6/report.md). Historical harnesses are design inputs, not production integration evidence.
- [Agent rules](../../../AGENTS.md) and [development checks](../../development.md).

Existing canonical decisions remain authoritative. This spec resolves the production dimension bootstrap omitted from fixture-supplied dimensions; it adds no provider, configuration field, persistent business table, queue, daemon, async runtime or model registry.

## Components and dependency boundaries

`service/mod.rs` owns small application interfaces and explicitly reexports necessary values. An `Embeddings` service interface exposes configuration readiness, one synchronous drain invocation and a query-embedding operation suitable for the future search consumer. Typed outcomes distinguish unavailable configuration, an existing drain, changed configuration and ordinary provider failures from storage/I/O failures. Names and exact signatures are fixed in the implementation plan before coding.

`service/embedding.rs` contains the concrete `EmbeddingService` and its implementations. It coordinates config readiness, work selection, Markdown preparation, durable attempt reservation, retry policy and publication. It depends on domain values and application-owned storage/filesystem/Markdown/provider interfaces. It contains no HTTP requests, SQL execution, environment reads or direct filesystem calls.

`embedding/ollama.rs` and `embedding/openai.rs` each contain their concrete provider struct, constructors and `EmbeddingProvider` implementation. A private shared HTTP helper may contain the genuinely shared ureq policy and response validation. HTTP client types and provider payloads remain private to embedding. Provider selection is a finite choice at composition time; it does not require a plugin registry.

`filesystem/` owns reading configuration and current regular Markdown files and obtaining the persistent `embed.lock` kernel lock. Its concrete implementation checks unsafe/symlink/nonregular paths and observed file replacements. Missing config is an expected observation, distinct from unreadable or malformed config. Config JSON is decoded at the application boundary into a validated, application-owned value, without leaking filesystem or ureq types into service contracts.

`storage/` owns queries for eligible texts, expected index dimension and orphan cleanup, using the existing Storage capabilities. Its extensions return typed metadata and accept configuration identity, hashes and vectors. Ordinary queries use SeaQuery and bound values; specialized vec1 introspection remains private storage implementation detail. No SQL builder or native shadow-table identifier appears in domain/service.

`markdown/` owns the existing X-4 parse and exact shared search-text codec. Embedding uses that codec to select the same knowledge ID and search hash as indexing, including whole-file and anchored elements. It does not invent another Markdown renderer or read raw lines as the embedding text.

`domain/` remains pure. It gains a value only if that value enforces an actual reusable invariant. Validated configuration and embedding request/result types belong to the consuming application boundary; raw JSON and HTTP structures are not domain entities.

`lib.rs` makes actual constructor calls and wires filesystem, Markdown, storage and the selected provider into the service. Public consumers use the service interface. No product CLI, empty future module, forwarding-only service, HTTP server or standalone worker is introduced here.

### Parallel integration agreement

TK-10 owns document discovery/publication; TK-13 owns embedding configuration/providers/execution. Both may change `service/mod.rs`, `lib.rs` and filesystem/storage interfaces in their own worktrees. These are explicit integration points, not independently owned replacement modules.

Keep TK-13 based on its own main line and do not modify, merge, rebase or cherry-pick TK-10's worktree implicitly. Reuse the existing Markdown codec and storage implementation. Match shared file-reading and lock semantics, keep implementation details in their owning module, and preserve each line's service contracts during the eventual integration review. The final combined tree must have one implementation per concrete responsibility, rather than competing copies of the same filesystem or parser. Neither branch's passing tests establish that the combined tree passes; integration requires its own check.

## Configuration and credentials

Read `.letopis/config.json` as the whole object containing required `provider`, `model`, `parameters` and `max_retries`. Reject unknown top-level fields. Provider is the finite `ollama`/`openai` choice, model is nonempty, and parameters must contain an explicit valid HTTP(S) `base_url` without userinfo. There are no hidden hostname/model/endpoint defaults.

Preserve additional parameter keys as permitted by X-1 for configuration identity, but forward only parameters supported by the selected X-2 protocol. Client-owned `base_url`, `max_retries`, credentials, input and mandatory protocol fields cannot be smuggled into or overridden in the model body. Reject forbidden credential keys and invalid values for recognized parameters locally.

Build identity from the complete validated JSON object using RFC 8785 JCS and SHA-256. Whitespace and object-key order do not change identity; changing any actual value, including only `max_retries`, does. Do not write an ad hoc canonicalizer or reuse the TK-3 fixture's ordinary JSON serialization as JCS. Use the already-selected `serde_json_canonicalizer` dependency, pinned exactly when implementation first adds it.

Represent absent, invalid/unreadable and ready configuration explicitly. Absent/invalid configuration performs no inference and exposes a safe current diagnostic/readiness outcome so the later caller can use FTS. It must not silently use an earlier provider or an old incompatible vector configuration. Do not clear an existing vector database as a side effect of failing to parse a config file.

For ready configuration, use the existing atomic `Storage::reset_vectors` identity recheck at the application entry point. Equal identity is a no-op. Changed identity resets the index, keys and attempts in one vectors.db write transaction and leaves cache revision/graph/FTS unchanged. Reset takes neither index.lock nor embed.lock. Readiness synchronization and a drain keep their captured identity, and every subsequent vector write independently checks it.

`OPENAI_API_KEY` is read from the invoking process environment at the concrete construction boundary; the future drain process inherits its parent's environment. It is used only for the compatible provider's Authorization header and is not automatically sent to Ollama. No `.env`, credential file, hot-rotation service or secret field is added. Missing/invalid credentials yield a closed safe embedding failure under the same retry policy, not a new early-permanent-failure state.

All counters and `1 + max_retries` use checked arithmetic and the storage-supported integer range. Values that cannot be represented produce an explicit safe configuration error; they never wrap, silently clamp or become an unlimited retry budget.

## Dimension bootstrap and validation

`parameters.dimensions` remains optional. If supplied it must be a positive supported integer and every returned vector must match it from the first response.

Without an explicit dimension, validate the first response as nonempty, finite float32 vectors with one common length. The first successful document-vector insertion establishes the durable dimension in the existing native vec1 index for this config identity. Subsequent document and query vectors must match that persisted dimension, including after restart and after removal of the last obsolete vector. No arbitrary dimension or model-name lookup table is used.

This persistence uses the pinned vec1 implementation already vendored in this repository: `vec1FixVectorSize` writes `VEC1_CONFIG_NELEM` on first insertion. Native metadata interpretation is confined to storage, validated against the pinned version and covered by real native tests. Application code receives an optional typed dimension; it never reads or writes vec1 shadow tables itself. Do not add an application dimension column or modify native C code.

A query embedding does not insert a document vector or cache a query vector. With no established index dimension, validate its own response shape and any explicit dimension; an empty vector index contributes no vector matches. Durable size is established by the first stored document vector. Two first writers must be resolved by the native vector write transaction: an incompatible second response must not leave a partial key, vector or dimension change.

A failed/rolled-back first insertion does not establish a dimension. A reset clears the previous native dimension transactionally. A provider that changes the response size under unchanged config produces a safe embedding failure; the service must not reset the index automatically to accept that response.

## Drain flow and consistency

1. Observe ready configuration and capture its identity. No config means no inference. Attempt the persistent exclusive `embed.lock`; if held, return the expected already-running outcome without waiting or touching another owner's lock. The guard lives for this drain invocation; the lock file is never removed or treated as a PID ownership oracle.
2. Query current knowledge for a search hash with no vector and fewer than `1 + max_retries` reserved attempts under the captured identity. Sources are not embedding work. Choose work in a deterministic order and requery after each completed step so new published texts can become visible.
3. Read a current regular file and run the same Markdown codec as indexing. Find the selected knowledge element and reconstruct exactly its search text. Process at most one file's text at a time; no corpus text cache or in-memory job list is created.
4. If the current element/hash differs, skip that stale work for this invocation. Do not send current text under an old hash or spin forever selecting the same stale item. Equivalent text in another current element must remain eligible: one stale representative cannot starve a valid representative of that hash.
5. Reserve one attempt and commit it before provider computation. Losing a process after reservation may consume an unsent attempt; this is intentional. Restart does not replenish the budget. A bounded input-preparation failure uses the same durable failure budget; simple stale-hash skipping performs no inference and does not consume an HTTP attempt.
6. Release the selection/read and reservation transactions before parsing/computing/waiting as applicable; no write transaction spans file I/O, HTTP, or retry delay. Invoke the provider synchronously for one text. Validate the entire response before it can reach a successful vector write.
7. Begin a vector write with the captured config identity. If identity changed, discard the result and end the old drain. Otherwise publish the original search hash and vector atomically; repeated storage for an existing hash preserves its key/vector. Successful storage clears that hash's failure row. Graph, files, FTS and cache revision do not change.
8. On computation/preparation failure, record a safe error under the same guarded identity. If budget remains, retry after the existing assumed one-second delay; otherwise the hash remains exhausted until the existing reset policy applies. Never sleep with an open write transaction. No hidden HTTP retries or error-class-specific unlimited attempts are added.
9. When eligible work is exhausted, remove vectors/failure rows not referenced by current knowledge using a guarded vector transaction, then release the lock. Keep entries still referenced by exhausted or skipped knowledge. Cleanup never deletes knowledge or FTS and does not need index.lock.

A file may change after its text was validated and while the model computes. The late vector remains addressed by the original hash and cannot represent the edited text. After the new file is published, selection/joins use the new hash. This is the D-92/D-95 rule; an extra post-HTTP full-file rehash is not required to establish that safety.

Configuration-file replacement is observed by the next readiness synchronization. A running drain protects publication against a changed committed `meta.config_identity`; it does not claim filesystem/database atomicity or hot-reload an environment key. Detached process creation and the CLI trigger belong to TK-11/TK-14.

## Provider and query boundary

Each provider implements the same texts-to-vectors interface. Requests have no document/query purpose flag or provider registry lookup. Native Ollama uses `/embed` relative to its explicit API base and mandatory `truncate:false`; the compatible provider uses `/embeddings` and `encoding_format:float`. Only documented supported parameters are forwarded.

Use the TK-6-proven blocking ureq 3 policy: a 60-second attempt deadline, no implicit environment proxy, zero redirects, system-certificate verification through rustls/platform verifier and no background HTTP runtime. Pin the concrete dependencies when added. Product code may adapt the proven transport logic but must not depend on a historical test harness crate or rewrite its captured evidence.

Require a successful protocol status and complete valid response: correct count, nonempty vectors, finite representable f32 values, consistent/expected dimension, and for compatible responses unique indices covering the inputs regardless of output order. Malformed JSON, provider errors and incomplete/mismatched vectors are failures. Do not silently truncate, chunk or repair vectors/text.

The query service operation uses the same config/provider and at most one provider attempt for a query. It does not reserve or consume document retry attempts and does not cache a query vector. Provider failure returns an expected unavailable-vector outcome so the future search caller can continue FTS; it writes the safe singleton query error under the captured identity. Success clears that error under the same guard. A changed identity makes the vector unusable. Keep actual search ranking/read-view integration with TK-12/TK-14.

Preserve useful internal error causes, but public error formatting, Debug output and persisted errors never expose credentials, authorization headers, knowledge text or raw provider response bodies. Storage corruption/unsupported schema and I/O failures remain typed failures; do not label every unexpected database error as an ordinary provider failure or claim FTS succeeded when no search ran.

## Rust implementation rules

Apply the user-selected project-local `rust-skills` rules for ownership, validated boundary types, error handling, numerical checks, API design, test fixtures and documentation. The pinned Rust 1.95.0 toolchain, project lint configuration and explicit agent/user rules take precedence over generic guidance in the skill.

In particular: keep synchronous I/O; small service-owned traits and concrete responsibility-named structs; minimal visibility; explicit imports; verb-form custom method names and project `get_` accessors; meaningful owner-local constants; focused functions around 40 lines and at most 60; deliberate ownership with no speculative cloning/Arc/Mutex/dynamic dispatch. Use typed Result/enum outcomes, checked conversions and arithmetic, RAII for file/transaction/lock lifetime, and redacted secret-bearing values. Do not add thiserror/anyhow, tracing, a mock library, feature scaffolding, blanket lint groups or optimized profiles merely because a generic rule mentions them.

Tests use Arrange, Act, Assert and private helpers. Public fallible APIs document Errors; genuine panic contracts document Panics. Concrete providers implement the application interface in their own files. `mod.rs` defines interfaces/children/reexports, and `lib.rs` performs actual assembly.

## Acceptance and evidence

| Area | Required observable behavior |
|---|---|
| Configuration | Absent/invalid/unknown root fields, explicit endpoint validation, forbidden credentials, supported parameter types, safe diagnostics, no inference before ready; JCS whitespace/key-order equality and changed-value identity |
| Credentials | Key read only from environment; compatible Authorization works; Ollama does not inherit it; no secret in errors/Debug/persisted diagnostics |
| Providers | Real controlled HTTP fixtures for both protocols; request construction, output reordering, count/index/f32/shape failures, non-200/provider errors, timeout, no proxy or redirect |
| Dimensions | Explicit size, first stored vector bootstrap, mismatch rejection, reopen/restart, all-vector deletion retaining size, reset/rollback and incompatible competing first writes using real native vec1 |
| Work selection | Only knowledge, shared text deduplication, already-stored/exhausted exclusion, anchored/whole-file codec parity, changed/missing elements skipped without spinning, valid same-hash representative not starved |
| Attempt lifecycle | Reservation committed before HTTP; crash after reservation consumes budget; restart preserves count; additional-retry semantics and exhaustion; safe preparation/provider failures; no reset on process restart |
| Transactions | Another connection can write while HTTP is paused; no SQL write transaction spans compute/delay; identity rechecked at write; old-config late success/failure cannot pollute new config; idempotent vector/key writes |
| File edits and FTS | Edit before compute skips; edit during compute cannot attach old result to new hash; provider/parse failures preserve corpus, published graph, FTS and cache revision |
| Drain exclusion | Two real processes, one embed.lock owner; loser returns; process death releases ownership without removing/replacing the persistent lock file; isolated temporary databases |
| Query boundary | Same provider contract, one query attempt, no document budget/query-vector cache, safe last-error/set/clear behavior, identity change returns unavailable vector; future FTS caller contract tested without claiming a product search command exists |
| Cleanup | Only unreferenced vector/failure entries removed, live references retained, config guard respected and graph/FTS unchanged |
| Architecture | Review physical files, import direction, visibility, concrete trait implementations and actual lib.rs constructor wiring |

Run `./scripts/check-rust.sh` on the implemented TK-13 tree, plus focused provider/storage/process checks and strict Rustdoc as applicable. Capture current source/lockfile hashes, commands, actual outcomes, platforms and limitations in a new TK-13 report and evidence directory. Run the changed product path on macOS and Linux glibc ARM64 with isolated fixtures; use an offline Linux runtime after dependencies are prepared. Run relevant historical proof suites only if their source/contracts are affected, and report those runs separately.

Use deterministic fixture barriers rather than scheduling sleeps for hash/config/lock races. A negative control is evidence only if the deliberately weakened code compiles and the behavioral check detects the intended violation, followed by byte-identical restoration and a passing check. Useful controls remove the input-hash guard and publication identity guard. Preserve failed/development receipts separately from accepted evidence.

Controlled provider fixtures are the reproducible acceptance baseline. Real inference, model quality, minimum Ollama version, arbitrary compatible-server guarantees, target RAM/performance, x86_64 and physical power loss are not established by this task. Such limitations remain explicit in the report.

## Scope boundary and next handoff

TK-13 delivers the library service, provider implementations, configuration readiness, basic safe drain/query execution and their own tests. It uses existing storage reset/attempt primitives sufficiently to avoid shipping an unsafe executor; it does not claim the complete TK-27 lifecycle/system-drill acceptance.

CLI argv/JSON formatting, detached spawning and foreground cancellation ownership stay with TK-11/TK-29; search/ranking/read-view integration with TK-12; the full agent path with TK-14; broad config/rebuild/corruption lifecycle acceptance with TK-25/TK-26/TK-27; current status presentation with TK-28; model/resource/security drills with TK-16/TK-41 and later tasks. No existing task is silently marked done by these library interfaces.

After user review of this written specification, use Superpowers writing-plans to define exact interfaces, files, ordered tasks, focused RED/GREEN checks, evidence ownership and the parallel integration check. The user then reviews that written plan and selects the execution method before product implementation. Merge, push and GitHub issue publication require the corresponding user authorization.
