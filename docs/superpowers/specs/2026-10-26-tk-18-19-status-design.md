# TK-18/19 follow-up: current status diagnostics

Date: 2026-10-26. State: proposed design, awaiting specification approval.
Branch: `tk-18-19-status`, based on published main `0f8f92b`.

## Intent and agreed scope

Finish the remaining TK-18 and TK-19 criterion: current knowledge validation
findings are available through the real CLI `status`. The user approved a complete
status response using existing services, while deferring TK-26/27-specific
corruption and embedding lifecycle acceptance. This does not complete TK-16,
TK-17, TK-26, TK-27 or all TK-28 acceptance.

Sources:
- [TK-18](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-18-graph-validation.md)
- [TK-19](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-19-check-renames.md)
- [TK-28](../../zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-28-diagnostics.md)
- [Structure and boundaries](../../zkit/letopis/architecture.md#program-structure)
- [Mechanics MC-7](../../zkit/letopis/mechanics.md#mc-7--текущие-проблемы-и-status)
- [X-1](../../zkit/letopis/contracts/X-1-cli.md), its
  [schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml), FR-32/33 and D-62/95.

## Observable behavior

`letopis status` consumes a strict empty JSON object and returns exactly one X-1
JSON response. Successful observations return exit 0, even when problems exist;
transport/infrastructure failures return safe error JSON and exit 1. Unknown
fields and invalid argv fail before assembling services.

The complete result contains `revision`, `embedding_backlog`, `embedding_failed`,
`locks`, and `problems`. Values are measured/derived, never placeholder zeroes or
empty arrays standing in for unimplemented observations. No document/provider
payload, credentials, or subprocess output enters public diagnostics.

### Published state and freshness — proposed builder decision

Status observes the most recently published cache; it does not refresh the corpus,
wait for index ownership, run inference, reset vector identity, or launch drain.
Otherwise it could block behind the very reindex owner whose PID it reports.
With no published cache, revision is null and published counts/findings are empty;
status does not rebuild it. Existing cache files must be opened without write-side
bootstrap or schema repair. Changes on disk become diagnostic changes after another
operation publishes a refresh. This proposes an explicit exception to X-1's general next-call cache-rebuild and
configuration-reset wording: status observes, while commands that refresh or
synchronize embedding readiness perform those mutations. The implementation must
update X-1 and MC-7 to state this exception before claiming contract compliance.
This distinction must be documented and tested: correcting a file alone
does not silently claim to have changed the published snapshot.

Use one scoped StorageRead for cache revision, graph/file findings, source
bindings/bases, current vector coverage, durable attempts and query_error. Reuse
current graph validation and source-provenance rules rather than calling check
(which refreshes and opens its own snapshot) or maintaining another diagnostic
store. Extract shared behavior only where check and status actually consume it.

Configuration file and lock observations are outside the database snapshot; no
cross-resource atomicity is promised. Scope the read view and release it before
returning or any unrelated operation. Do not hold a database writer while probing
locks or observing source objects.

### Problems

Include current invalid-file findings, dangling knowledge targets, self-loops and
indirect includes cycles, missing/unavailable source_snapshot findings, exhausted
embedding attempts, invalid/unreadable embedding configuration and the existing
singleton query-embedding error. Match existing check diagnostic codes/details
for overlapping findings. Sort deterministically and deduplicate equivalent
findings. Sources/URLs are not downloaded. Successful status never clears errors,
resets attempts, or changes knowledge files.

After a normal refresh publishes an invalid file or a repaired/renamed/restored
target, status reflects the corresponding appearance/disappearance. Restart does
not erase persisted file/query/attempt state. Corrupt-storage failures are errors,
not successful empty reports or claimed persisted diagnostics.

### Embedding counts — proposed builder decision

Count distinct current knowledge search_hash values, not knowledge elements,
orphan vectors or source nodes. Two knowledge elements sharing one missing vector
contribute one backlog item; an exhausted shared hash contributes one failed item.
This proposed clarification must be recorded in X-1 and MC-7. Share the
existing embedding identity and `1 + max_retries` classification rules. A usable
vector must match the current search_hash and active configuration identity;
exhausted texts are excluded from backlog and counted as failed. Aggregate in
storage without loading vector payloads or issuing one query per knowledge item.

When configuration is absent/invalid, or its identity differs from the pinned
vector identity, no vector/attempt is current for an active usable configuration:
all distinct current knowledge search hashes count as backlog and failed is zero. Missing config
is ordinary unconfigured state; invalid/unreadable config adds a safe problem.
This is an explicit unavailable-configuration policy, not a readiness sync or
vector reset. Document it and verify it independently. Query_error remains
observable regardless of configuration availability, following StorageRead's
existing contract. Overflow and malformed stored values fail safely.

### Lock observations — proposed builder decision

Report only observed-busy index/embed kernel locks, with the positive PID recorded
in the lock file. This is a best-effort recorded PID, not a verified current owner: an
owner writes its PID after acquisition, and a takeover can briefly expose the prior
positive PID. Do not claim atomic PID/ownership observation or change the owner
protocol in this task. Probe nonblocking and release any acquired probe immediately; never write
our own PID during observation. An unlocked persistent file with a stale PID is
not a busy lock. Do not delete/recreate an existing lock inode. Retain existing
pinned-directory, regular-file, no-symlink and inode-replacement protections.

A busy lock with unreadable or invalid PID bytes cannot be represented by X-1 Lock
(which requires a positive PID): return a safe observation error rather than
invent a PID or report an unlocked lock. A syntactically valid stale PID during
takeover is indistinguishable and remains within the documented best-effort limit. This includes transient owner-PID
publication races; no unbounded polling or waiting for lock release is added.
PID and lock observations are best effort, not authority to signal processes.

## Program structure

- Pure result values remain dependency-free; no domain infrastructure dependencies.
- A concrete StatusService owns orchestration behind an application-owned trait.
- Shared current check/status diagnostic logic stays in service ownership.
- SqliteStorage implements narrow aggregate observations through scoped storage
  interfaces; use SeaQuery and bound parameters for ordinary queries.
- LocalFileSystem implements safe config and nonblocking lock observations via
  application-owned interfaces. Do not reuse owner acquisition paths that write PID.
- CLI parses, dispatches and serializes; it does not call adapters directly.
- lib.rs performs actual constructor wiring. Keep existing scoped-storage generics
  where required; add no generic framework, factory or pass-through services.
- Synchronous I/O, existing schema/dependencies, no new runtime/background worker,
  diagnostic tables, counters, migrations, broad refactor or cancellation scheme.

Exact helper/module splitting and object-safe versus scoped dependency choices
are left to the implementation plan within AGENTS.md boundaries.

## Acceptance and validation

1. Strict real CLI status with all required fields and correct exit/one-JSON rules.
2. Check/status parity for invalid markup, dangling file/anchor targets, self-loop,
   indirect includes cycles and missing source bases; repair/rename/restore after
   refresh updates both without rewriting source documents.
3. Current vector coverage, deduplicated text hashes, retry threshold boundaries,
   stale hashes/config identity, zero/missing config and malformed config covered
   with independent expected values; orphan/source rows do not inflate counts.
4. Query error survives status/restart, disappears only through the existing
   successful query path, and status never calls the provider or launches drain.
5. Real held index/embed locks report recorded PID promptly; stale unlocked PID does
   not. Missing locks, invalid PID, symlink/inode replacement and I/O errors covered.
6. Concurrent publication/drain cannot mix the pinned database observations;
   status can report a busy index lock without waiting for its release. No partial
   successful report after a late storage/provenance failure.
7. Architectural review names actual files and interface/composition boundaries.
   New behavioral tests use Arrange/Act/Assert and focused manual mutation checks.
8. Run make check and make verify on final changed tree. Storage changes require
   fresh host/Linux native/schema/copied-executable evidence according to AGENTS.md;
   record platform limits honestly rather than silently omitting acceptance.

Keep test output small. No historical whole-source mutation campaign or paid
model/pilot run is needed for this scope.

## Closure and remaining work

Record commands, exact revision, results and evidence in a dedicated follow-up
report. Update TK-18/19 criteria only after real status integration passes. Keep
TK-28 open with delivered versus pending criteria explicitly identified, including
its TK-26/27 dependency-specific acceptance. Do not claim G-1 or TK-16/17 closure.
New GitHub publication/merge/push remains subject to the user's later instruction.

## Design review

Independent builder review raised three questions: absent-cache/config-sync
exceptions, shared-hash counting, and PID publication races. The proposed policies
above resolve each explicitly without a new schema or coordination protocol.

## Approval

Scope approved in chat. Proposed freshness, unavailable-config counting and busy
lock/PID error policies above require specification approval before product code.
