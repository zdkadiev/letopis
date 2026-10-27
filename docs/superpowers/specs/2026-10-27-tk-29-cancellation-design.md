# TK-29 — Handled cancellation and concurrent processes

Status: approved by the user on 2026-10-27. Branch: `tk-29`. Base: `origin/main`
`34af7d7a7f67cbde7db61622f354bcb6532b8866`. Product implementation has not started.

## Intent and authoritative scope

Complete [TK-29](../../zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-29-cancellation.md):
handled SIGINT for the caller's search, traverse, inspect, check and reindex;
publication and drain concurrency acceptance; and the existing agent skill's
complete maintenance workflow. Protect FR-31 and preserve FR-32/FR-33 diagnostics.

Sources: [D-95/D-96 and D-103](../../zkit/letopis/decisions.md),
[MC-2/MC-3/MC-5/MC-6](../../zkit/letopis/mechanics.md),
[X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md),
[X-1 schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml),
[program structure](../../zkit/letopis/architecture.md#program-structure) and its
[review rules](../../zkit/letopis/architecture.md#program-structure-review).

The user selected the task-card approach and approved this written specification
on 2026-10-27, including the config completion and commit-order clarification.
Only preparation of the implementation plan is currently authorized; execution
must wait for a separate instruction.

## Approaches and decisions

Use the already accepted D-96 design: a synchronous command and a dedicated
`signal-hook` iterator thread, sharing a single response writer. The signal
thread emits the existing cancellation envelope and terminates this process
with exit 1. Reuse the TK-6 transport proof's output/exit coordination rule,
not its fixture-only CLI. This is the one approach compatible with the card.

Cooperative polling or SQLite InterruptHandle would require separate cancellation
paths for SQL, flock, Git and HTTP and would not satisfy the agreed design.
Default signal termination alone cannot return the X-1 cancellation JSON.
No async runtime, cancellation token propagation, execution deadline, PID-based
cancel API, process registry or new persistent cancellation state is introduced.

## Current implementation and identified gap

`src/main.rs` passes locked stdin/stdout to `run_cli`. `src/lib.rs` parses input,
wires services and calls the one-shot formatter in `src/cli/output.rs`.
There is no product signal handler. Keeping a stdout lock on the command thread
throughout service execution would prevent a signal thread from responding;
the executable must acquire it only while actually writing a response.

`src/process/detached.rs` already launches `embed --drain` in a new session with
null standard streams. Storage publishes atomically; vector writes check current
identity and hash. Extend these owners only for a demonstrated TK-29 defect.

**Approved completion of an existing contract:** X-1 specifies `config`, but
the current request enum/parser/dispatch has no config command. Complete that
existing read-only command here because the card requires the full CLI/skill
workflow. Input is `{}`; absent config returns `ok:true,result:null`; a valid
configuration returns its Configuration JSON value, including allowed provider
parameters. Malformed configuration returns the existing safe configuration
error, and unreadable configuration returns a safe typed error. No raw invalid
bytes, credentials from environment, or internal causes appear in diagnostics.
Read through the existing confined filesystem and config parser interfaces;
do not call embedding readiness merely to read, reset vectors, open/bootstrap
storage, refresh, launch drain or call a provider. Do not add setters or schema
fields. The implementation plan must identify the existing parser's exact error
mapping and the smallest service-owned interface for this observation.

## Observable cancellation protocol

1. Install the handler for the five named commands before stdin consumption,
   repository discovery, adapter construction or service execution. Select this
   from argv without waiting for JSON. A signal before registration has the OS
   disposition; no stronger process-start guarantee is claimed. Status and config
   retain their current/default SIGINT behavior; the card does not require handled
   cancellation for them. `embed --drain` never installs this handler.
2. Normal responses and cancellation use one response owner. Serialize a complete
   envelope before taking the output guard. Hold the guard through writing and
   flushing; never release it between JSON fragments. Once output starts, no
   other response may append bytes. Use the existing safe cancelled error code,
   message and details shape; never expose request, provider or subprocess text.
3. If SIGINT wins output ownership, write one failure envelope and exit 1. A
   racing command completion must not return exit 0 after cancellation won.
   If normal output wins, finish that single envelope; a subsequently handled
   SIGINT exits 1 without adding a cancellation envelope. Normal completion before
   signal handling retains its normal exit code. Repeated signals add no response.
4. Cancellation must terminate the caller while it is in a SQL operation, waiting
   on the real blocking index lock, waiting on Git, or waiting on HTTP. No stack
   unwinding or Rust destructor execution is promised on process exit. SQLite
   rollback/recovery and kernel descriptor closure release this process's state.
   Do not signal another publisher, an already detached drain or its process group.
   A running Git child is not promised remote/subprocess cancellation: show that
   it cannot keep the caller's lock or response pipe alive after caller exit.
5. Stdout backpressure does not permit interleaved or truncated replacement JSON:
   once writing starts, finish that response. Test a filled pipe that the reader
   subsequently drains, with SIGINT during output. No bounded exit latency or
   guaranteed delivery is possible when the consumer never reads, closes the
   pipe, or output fails; report write failure with exit 1 and no second envelope.
6. If signal registration or listener-thread creation fails, return one safe
   infrastructure error with exit 1 before executing the request. Do not continue
   as though handled cancellation were installed.

## Publication and process invariants

- Process termination before the atomic publication commit preserves the old
  complete graph, FTS data and revision. The next process can read and publish
  successfully. After commit, cancellation never rolls back that publication,
  even if the caller did not receive a success response. Corpus Markdown is
  untouched. Signal receipt, cancellation output and process termination are
  distinct events: a commit may win between them. A `cancelled` response promises
  caller termination, not proof that no publication committed. This is the
  approved precise interpretation of the card's before/after-commit wording under
  D-96. No commit-vs-signal coordination
  protocol is added. The pre-commit fixture must keep commit blocked until actual
  process death; receiving SIGINT alone is not its rollback oracle.
- Concurrent publishers serialize through the existing persistent index lock.
  Cancelling a waiter does not kill its holder or remove/replace the lock file.
  Cancelling a holder releases its kernel lock; another publisher can finish.
- An already running drain survives cancellation of the foreground caller.
  SIGINT sent directly to drain has the default disposition and no JSON response;
  its previously committed attempt reservation remains durable.
- Late embedding results cannot stand for newer text or a changed configuration.
  Assert actual query-visible/vector-key state for both races and reuse TK-27
  evidence at its captured revision where it already proves the case. No new
  queue, reset protocol, inference parallelism or storage schema is needed.

## Ownership and implementation discretion

Signals and process termination belong in a responsibility-named process file;
JSON encoding and single-response ownership belong in CLI transport. `lib.rs`
wires the executable path and service dependencies; `main.rs` stays thin. Domain
entities and application algorithms gain no signal, stdout, SQL or process
dependencies. Existing `run_cli` callers/tests must retain explicit output
injection; a new executable entry function is acceptable if required to keep
process-global signal registration out of library/unit-test calls. Do not add a
general runtime framework or propagate generics merely for injection.

Exact private types, helper names and test fixture placement are implementer
choices, subject to AGENTS.md. Keep narrow interfaces, functions near 40 lines
(60 maximum), and implementation files cohesive. Pin the selected signal-hook
version when adding it, update Cargo.lock, and document its purpose. The plan
must inspect signal-listener lifecycle and races before choosing its concrete
writer/registration API; an atomic flag alone does not protect write/flush.

## Acceptance and evidence

Use local, deterministic fixtures and explicit readiness barriers rather than
sleep-based guesses. Do not add product command flags or environment gates for
testing. A private fixture executable may coordinate the same product code, but
label that coverage separately from executing the shipped binary.

| Criterion | Required observation |
| --- | --- |
| Five commands | For each named command, real-process SIGINT gives one schema-valid cancelled envelope and exit 1 before normal output. Include waiting for stdin. |
| Four wait boundaries | Observe actual SQL execution/wait, kernel blocking flock, live Git child and accepted local provider request before signalling. Cover reachable combinations, not an artificial five-by-four cross-product. Product-binary coverage must be identified. |
| Output/exit races | Deterministically exercise cancellation winning, normal response winning, repeated SIGINT and output backpressure. Parse all stdout and assert exact exit code; no trailing JSON. |
| Commit boundary | Hold before a real publication commit until process death, then test termination after an observed commit; compare independently expected graph/FTS/revision and fresh-process readability. A freely racing signal/commit may leave either complete publication, never partial state. |
| Process isolation | Foreign holder/drain remains alive and can finish; cancelled holder releases its lock; lock inode remains stable; abandoned Git child cannot retain locks/stdout. |
| Late drain result | Delay provider response across changed text and separately changed config; old vector cannot represent current text/identity. Cite or rerun existing relevant evidence honestly. |
| Maintenance skill | Extend `skills/letopis/SKILL.md`; run save/check/search/traverse/mixed-inspect, config absent/valid/invalid, status, reindex, cancellation and retry against disposable real CLI repositories. Reuse source-basis semantics and validate X-1 envelopes. |
| Architecture | Review changed files, visibility/import directions, trait ownership, process lifecycle and lib.rs construction. |

The full skill example edits `.letopis/config.json` as a file, reads it with
`config`, observes diagnostics with `status`, refreshes through normal commands,
and shows reindex as explicit maintenance. It explains cancelled exit 1, uncertain
commit outcome after cancellation, safe retry and independent drain lifetime.
It must not invent a cancel subcommand or claim SIGINT changes authored knowledge.

Every new behavioral test needs a meaningful temporary mutation that fails at
the intended assertion; restore exact source afterward. Unit/integration tests
have explicit Given/When/Then phases. Keep success logs concise. Run `make check`
and `make verify` for the actual final changed tree. Run the storage/native runner
if storage/native/build inputs change; otherwise explain its omission. Verify
cancellation on macOS and Linux ARM64; retain unavailable platform limits rather
than extrapolating results. Preserve historical receipts; place new evidence in
a new directory and record source revision, input hashes, commands and outcomes.

## Gate accounting and exclusions

Produce a G-2 matrix covering the roadmap's full functional scope (FR-1…FR-6,
FR-11…FR-13, FR-17…FR-36 and C-1…C-10), with each item linked to current checks or
historical evidence at its captured revision. Mark unverified behavior and stale
evidence explicitly; no blanket acceptance from root test counts. C-10 x86_64 is
the roadmap's stated TK-43 exception.

The user previously deferred TK-17/minimum-hardware G-1 measurements (D-103).
TK-29 development is authorized, but this does not approve G-1 or waive its
requirements. Report functional completion separately from formal gate status.
G-2 cannot be declared an unconditional continue while a required prerequisite
or functional criterion lacks evidence; retain pending checkboxes and reasons.

No paid pilot, real model quality assessment, system load campaign, physical
power-loss proof, packaging or installation work is included. Broader drills,
release platforms and delivery remain TK-30+ and TK-43/TK-44 work. Record results
and outstanding criteria in the task card, report and existing Engram store.

## Specification review

Independent Superpowers builder check identified one ambiguity: signal delivery
does not atomically prevent the main thread from committing before process exit.
The publication section now states the termination/commit ordering and the
corresponding test oracle explicitly. The user subsequently approved this
interpretation and config completion with the specification. All other reviewed
implementation choices were sufficiently bounded.
