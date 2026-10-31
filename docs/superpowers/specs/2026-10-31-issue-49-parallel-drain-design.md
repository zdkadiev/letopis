# Bounded parallel embedding drain — issue #49

Date: 2026-10-31. Status: accepted; separate implementation plan and Native execution
were subsequently approved. Implementation and model-free results are tracked in
the [report](../../zkit/letopis/checks/issue-49/report.md); real-provider acceptance
remains open. The thread-pool approach, per-result publication and four-worker bound
are user decisions.
Task: [issue #49](https://github.com/zaurkadiev/letopis/issues/49).
Base: `main@b7f627d1d63d2d7b72989b733838a55925947c17`.
Branch: `codex/issue-49`, reusing the clean managed worktree after issue #69 merged.

## Intent and accepted decisions

Improve document-embedding throughput by overlapping independent blocking HTTP
requests inside the existing detached drain process. Preserve the synchronous
CLI, one drain owner, hash-addressed vectors and durable retry accounting.

The user selected a ready-made thread pool with the current HTTP client, after
considering futures and nonblocking HTTP. Keep `ureq`; do not introduce Tokio,
other async runtimes, futures orchestration, libcurl, Isahc or a replacement HTTP
transport. The worker count is a named constant, not a config/environment/CLI knob.
Use Superpowers: approve this specification, then prepare and approve a separate
implementation plan before changing product code or dependencies.

The user also explicitly approved completion-by-completion publication: the
coordinator saves each valid returned vector in its own short transaction without
waiting for the other requests or the full corpus. Only after handling that result
does its slot become available for another job. HTTP requests overlap; SQLite
publication stays sequential. A result committed before process death remains
saved; outstanding/uncommitted work follows the existing restart/attempt policy.

The user approved four workers, held in one clearly visible named constant at the
top of the scheduling implementation file, after imports and before types/impls:
`const DRAIN_WORKERS: usize = 4;`. The same constant bounds all dispatched work
whose result has not yet been consumed; do not duplicate it across production
modules. Four is an agreed initial bound, not a measured optimal or a passed
minimum-hardware resource verdict. Use the `threadpool` crate, exactly pinned to
`=1.8.1`, as the author-selected implementation of the agreed pool approach.
The builder may settle private helper names, channel representation and focused
file splits, but may not change these public behavior and resource boundaries.

## Baseline behavior and design sources

This section describes the pinned main revision before issue #49, not the final
implementation. Current mechanics are updated by D-107.

- [AGENTS.md](../../../AGENTS.md) and
  [program structure](../../zkit/letopis/architecture.md#program-structure):
  application services own coordination; domain data has no infrastructure.
- [Mechanics MC-3/MC-5](../../zkit/letopis/mechanics.md): work is selected from
  knowledge hashes missing vectors and with attempts remaining; there is no
  persistent job queue. One process holds `embed.lock`; HTTP is outside SQLite
  transactions. [Decisions](../../zkit/letopis/decisions.md) D-92/D-94/D-95/D-96
  remain authoritative except the explicit sequential-drain refinement here.
- [EmbeddingService](../../../src/service/embedding.rs) currently reads one item,
  reconstructs its search text, durably reserves an attempt, calls the provider,
  records its outcome and repeats. Retry delay is one second when budget remains.
- [Provider interface](../../../src/service/embedding_traits.rs),
  [HTTP providers](../../../src/embedding/provider.rs), and
  [transport](../../../src/embedding/http.rs): blocking `ureq =3.4.2`, existing
  request policy, 60-second HTTP attempt deadline, 10 MiB response-body cap,
  redacted errors, no redirects or proxy delegation. Preserve those policies.
- [Work selection](../../../src/storage/work.rs) currently returns one eligible
  ID/hash; [vector storage](../../../src/storage/vectors.rs) guards configuration,
  dimensions and durable attempts. Do not replace these with a second queue.

## Scope and non-goals

Only document inference becomes concurrent. Keep one detached drain process and
one lock owner. Query embeddings, foreground command behavior, vector identity,
configuration format, schemas, public JSON responses and provider payload shape
remain unchanged. Each document attempt still sends exactly one text.

No provider batching, multi-owner drain, automatic concurrency tuning, progress
API, new service/process, permanent daemon, provider startup, storage redesign or
snapshot/provenance-verification change. Batch source verification is unrelated
and must not be copied into this work. No new query priority or rate-limit policy.
Existing whole-stack hardware and gate requirements are not waived.

## Ownership and physical boundaries

`EmbeddingService` owns the scheduling state, work selection, attempt reservation,
retry policy and result publication through existing application interfaces.
SQLite connections, transactions, file reads and Markdown reconstruction remain
on the coordinator thread. No connection/read view/write transaction is sent to
workers or kept open across provider calls or waiting for completions.

Workers own captured text, immutable configuration, requested dimensions and the
provider access needed for one call. They return a result associated with the
original configuration identity and search hash. Workers never discover work,
reserve attempts, read project files or write storage. Technology types stay
inside their existing modules; `threadpool` must not appear in domain data or
public CLI types. Keep component wiring in `lib.rs`.

The actual cross-thread boundary needs `Send` and safe provider sharing. Use
owned/shared provider access only where the pool's `Send + 'static` job lifetime
requires it; do not propagate new generic lists, factories or executor interfaces
through unrelated services. Check transport/provider thread safety from their
actual fields. Do not hide blocking provider calls behind async functions or
hold a mutex around the entire HTTP call, which would serialize the pool.

## Scheduling and bounded memory

1. Validate/capture configuration and obtain the existing exclusive drain lock.
   Do not create workers for missing/disabled/invalid configuration or a busy lock.
2. While fewer than four jobs are outstanding, select an eligible hash not already
   outstanding. A bounded exclusion of at most four hashes must allow selection
   to progress past shared-hash nodes. Extend the existing storage read capability
   narrowly if needed, using SeaQuery/bound values; no whole-corpus materialization.
3. Reconstruct text using the existing Markdown codec and confirm its search hash.
   Preserve existing skip behavior for stale/deleted live documents. A skipped
   candidate does not occupy a worker or consume a provider attempt. Preserve
   the existing attempt/error behavior for actual preparation failures.
4. Durably reserve one attempt under the captured configuration before submitting
   HTTP. If reservation is refused, do not send. Count the reserved/submitted job
   until its result is applied/discarded; never dispatch the same hash twice.
5. Submit one blocking provider call. Once capacity is full, receive the next
   available completion and apply it on the coordinator. Refill the free slot;
   do not wait for an entire batch or for a slow earlier item to finish first.
6. If selection returns empty while jobs remain, wait for completions and repeat
   selection. Empty selection is final only with no outstanding work. Then prune
   unreferenced vectors/failures as today and join the pool before releasing lock.

An outstanding slot includes a running/queued task or a completed result waiting
for the coordinator. Receiving does not free it until publication/discard finishes.
Do not enqueue the corpus into `threadpool`'s internal queue. At most four jobs
can be submitted and unconsumed across that queue, active workers and result
buffers together. Prepare the next text only when a slot is available. The
bounded hash set and completion channel cannot become unbounded work backlogs.

For example, three jobs may be waiting for provider responses while a fourth has
returned a vector that the coordinator is writing. All four slots remain occupied.
Only after that write or failure/discard handling finishes can the coordinator
admit another job. Eligible work beyond these four stays represented by the
existing database query; it is not loaded into a separate in-memory job queue.

This is a bound on scheduling payload count, not a claim of constant total process
bytes: per-response JSON decoding, text/file buffers, thread stacks and existing
stale-ID tracking retain their own costs. Measure actual RSS and state its scope.

## Completion, retries and races

For example, if the second of four submitted requests finishes first, commit its
validated vector, release its slot and dispatch the fifth job while the first,
third and fourth requests continue. There is no all-four barrier or whole-drain
transaction. Failed or obsolete results follow the guarded handling below.

- Successful vectors use the existing validation rules: exactly one nonempty,
  finite vector; supported/requested/current dimensions; original hash key;
  identity-guarded short publication and failure removal. Share the relevant
  validation rule with query inference rather than duplicating it.
- If dimensions were initially unknown, the first successfully committed vector
  establishes them. Later results are checked against current dimensions at
  publication; incompatible completions become existing invalid-response failures.
  Network completion order is not a promised ordering of graph results.
- A hash remains outstanding through its completion handling. Provider failure
  records only a safe error under the original identity. Retry consumes a fresh
  durable attempt and cannot overlap an earlier attempt for that hash.
- Preserve `max_retries` and the one-second retry delay. The simplest initial
  implementation may reuse the coordinator's existing failure wait; this can
  temporarily delay processing/refilling other slots. Do not add a timer scheduler
  or an unbounded deferred-retry table. Such a pause is not provider concurrency
  failure and its throughput cost must remain visible.
- A changed committed configuration stops new dispatch. Discard old-identity
  completions without changing the new cache, attempts or errors. Already running
  requests may finish; they cannot be synchronously interrupted by dropping a
  pool handle. Existing external config-file/committed-identity observation
  semantics remain: this does not make arbitrary file edits atomic with dispatch.
- A file edit/delete/rename while HTTP is running must never bind the old vector
  to a new hash. A valid original-hash result may be reused by another current
  element with that hash; unreferenced data is pruned by the existing policy.
  This does not add a fresh live-file existence check before each vector write.
- Storage/environment failure stops dispatch and preserves the existing error.
  Let already submitted tasks finish, discard their unapplied results and join
  before releasing ownership/returning. Reservations already committed remain
  spent, even if shutdown prevents publication. No compensating attempt rollback.

## Termination and unexpected failures

Keep the lock until all normal-path worker jobs have finished and the pool has
been joined. All early returns need the same lifecycle guarantee; dropping the
pool handle alone is insufficient. During shutdown, workers must be able to send
or discard completions without deadlocking on a receiver no longer being drained.
Preserve the original coordinator error when cleanup also observes an error.

Worker panic is a programming failure, not a provider failure or retry. It must
not strand the coordinator waiting forever for an unreported completion, nor be
silently hidden by the crate's worker replacement. Under unwinding builds,
forward panic information through a private completion outcome, stop dispatch,
finish/join remaining jobs, and resume the panic on the coordinator. Under the
existing distribution `panic=abort`, process death keeps its normal semantics.

Do not add a graceful drain-cancel command or a signal handler for drain. Existing
detached drain signals/process death terminate the process and all its threads;
the kernel releases the lock, committed attempts remain and a later drain resumes.
Remote model computation may continue after the client exits. Foreground SIGINT,
single JSON output and foreign-process isolation remain as implemented in TK-29.
A normal early return may wait for submitted HTTP attempts; the existing per-call
deadline is not a new end-to-end drain/shutdown deadline.

## Measurements and report

Use immutable new evidence under `.artifacts/issue-49/` and a committed summary in
`docs/zkit/letopis/checks/issue-49/report.md`. Record source/input/executable hashes,
compiler/build profile, host, corpus shape, exact config, repetitions, provider
behavior, failures, attempts and cache/vector state. Reuse existing measurement
helpers where they fit; do not turn this into a second benchmarking platform.

Required model-free comparison: (A) fresh-base sequential product, (B) the new
scheduler compiled with its constant set to one in an isolated evaluation copy,
(C) the final four-worker product. No runtime tuning knob is introduced for (B),
and no one-worker evaluation mutation is retained in the product tree. Keep the
controlled provider concurrently responsive with fixed delay and distinguishable
literal vectors. At least 64 distinct texts plus shared-hash cases; independent
fresh projections for each sample; one conditioning sample and five measured
samples per variant, interleaved where practical. Setup/build/model loading are
outside drain timing. Preserve all failures and censored samples.

The report's before/after table includes completed unique vectors, total time,
vectors/sec, request/attempt counts, observed maximum active requests, failures,
latency median/range, drain-process peak RSS and foreground query latency under
concurrent drain. Do not report tail percentiles from an inadequate sample or
sum separate process peaks as simultaneous whole-stack memory. If separate
provider/whole-stack memory cannot be observed, mark it not assessed and give the
actual scope, rather than calling CLI RSS whole-stack RSS.

Use an additional correctness/contention schedule with real query requests and
identify query/document requests separately: the four-slot bound applies to
document requests inside one drain, not aggregate provider requests from other
CLI processes. Check query result correctness and existing error persistence.
An optional samply capture can explain a regression; it is separate from ordinary
timing and cannot substitute for measured concurrency or correctness.

Issue #49 also requires a same-corpus, same-model/config/host sequential-versus-
parallel real-provider assessment, including retries and whole-stack memory.
This remains a separate explicit authorization/checkpoint before any real model
or paid-provider calls. Without it, label measured speedup **controlled-provider
client throughput only**, leave that acceptance item open, and do not call the
issue fully complete. A local model may serialize inference or regress under
concurrency; no fourfold speedup, model-capacity or minimum-hardware verdict is
promised. Earlier TK-15 timings are historical context, not this change's baseline.

## Acceptance and verification

- A held controlled provider receives more than one document request before any
  is released, and never more than four, across a corpus much larger than four.
- Literal vectors and independent HTTP request logs prove that each result stays
  bound to its submitted hash/config; same-hash nodes cause one active attempt.
- Reservation is visible before the provider responds; crash/restart, retry
  exhaustion and out-of-order mixed success/failure preserve durable budgets.
- Editing/deleting a file, switching committed config, racing dimensions and
  malformed/nonfinite/wrong-cardinality responses cannot publish wrong vectors.
- A second drain cannot own the lock. Concurrent search/query still works with
  correct results and bounded storage transactions; no HTTP spans a DB lock.
- Four total outstanding slots bound tasks plus queued completions; a slow storage
  consumer cannot grow the pending-text/vector queue with corpus size.
- Empty selection with active jobs, normal completion, configuration change,
  storage failure, worker panic and process termination have explicit lifecycle
  tests; no normal return with owned worker requests still active and lock freed.
- Existing transport policy, disabled/offline no-inference and foreground signal
  behavior remain covered. Use readiness/barriers instead of timing-only races.
- Mutation-check focused new assertions: remove the cap, permit duplicate hash
  dispatch, skip durable reservation, bypass identity/dimension checks, detach
  cleanup or misassociate completions; each affected test must fail.
- Run `make check` and `make verify` on actual final product changes. Run affected
  root embedding/provider/storage/process suites and host/Linux focused integration
  coverage; if storage/native/build dependencies are affected, run the applicable
  storage runner into a fresh evidence directory. Document unrun platforms and
  historical standalone manifests instead of implying they were covered.
- Review physical modules/import direction/visibility/trait implementations and
  `lib.rs` construction, plus the mandatory Rust/test/readability rules in AGENTS.

During implementation update mechanics MC-3/MC-5, dependency/architecture text,
resource assumptions and relevant config/provider contract descriptions only where
behavior changes. Keep config/CLI schemas unchanged. Update the issue's outdated
sequential description/acceptance with actual findings; preserve historical
measurements, gates and deferred acceptance. Do not renumber TK tasks or declare
G-2/G-3/G-4 passed.

## Reference libraries and current stage

The library is a fixed worker pool, not a bounded queue and not an async runtime.
See [ThreadPool execute/join](https://docs.rs/threadpool/1.8.1/threadpool/struct.ThreadPool.html)
and the [threadpool source](https://docs.rs/threadpool/1.8.1/src/threadpool/lib.rs.html).
Check the exact pinned source and transitive dependencies in the implementation
plan, particularly worker creation/panic/join behavior. The existing transport
remains pinned; do not silently update `ureq` while adding this dependency.

This specification records the approved design. No product code,
manifest/lockfile, benchmark campaign or GitHub write has been performed for #49.
The independent builder check below preceded presentation; the user subsequently
approved the discussed decisions. A separate implementation plan is next, and its
approval must precede implementation.

## Specification review

2026-10-31: a fresh independent builder reviewer read this draft and returned
“nothing important left”; no decision-changing questions remained before planning.
All ten local document/source links resolve. This is document/source consistency
verification only: no product suites, benchmarks or provider calls were run.
Subsequent user approval confirmed per-result transactions, four workers and one
top-of-file constant. These refinements preserve the reviewed ownership, lifecycle
and measurement rules; the implementation plan still requires separate approval.
