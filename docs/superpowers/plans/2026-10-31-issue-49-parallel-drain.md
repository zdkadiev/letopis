# Bounded parallel embedding drain Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Execute up to four independent document-embedding requests concurrently,
publish each completion immediately through one coordinator, and measure the effect.

**Architecture:** Keep the synchronous `EmbeddingService`, current provider interface
and blocking `ureq`. A private drain scheduler owns a fixed thread pool and at most
four outstanding hashes; workers call only the provider. The coordinator retains
filesystem/Markdown/storage ownership and joins workers before releasing its lock.

**Tech Stack:** Rust 1.95.0, `threadpool =1.8.1`, unchanged `ureq =3.4.2`,
existing SeaQuery/rusqlite/vec1, Python 3.12 for model-free measurement tooling.

**Spec:** [Parallel-drain design](../specs/2026-10-31-issue-49-parallel-drain-design.md),
including the user-approved per-result transactions and top-of-file four-worker constant.

**Workspace:** `/Users/zdkadiev/.codex/worktrees/issue-69/letopis`, branch
`codex/issue-49`, base `b7f627d1d63d2d7b72989b733838a55925947c17`.
**Execution approval:** 2026-10-31, the user confirmed implementation in voice,
starting with the concurrent local-provider fixture and red concurrency tests.
Execute Native with one independent whole-branch review; no real model/provider
runs are authorized.

## Global Constraints

- One private `const DRAIN_WORKERS: usize = 4;` at the top of
  `src/service/embedding/drain.rs`, after imports and before types/impls. Add a
  short doc comment explaining that it bounds both workers and unconsumed jobs.
  No runtime knob, copied production limit, CPU-count default or whole-corpus queue.
- Keep one drain process/`embed.lock` owner. Four means submitted-but-unconsumed
  jobs, including queued/running/completed-awaiting-publication work together.
- Publish one valid result in one short transaction as soon as it is consumed;
  release its slot only after outcome handling. No all-four or whole-corpus barrier.
- No SQLite/file/Markdown operations in worker jobs; no storage view/transaction
  spans inference or waits. Original hash/config identity guards and retries survive.
- Keep 60-second transport deadline, 10 MiB body cap, redaction, provider payloads,
  one text/request, config/CLI schemas, disabled behavior and foreground SIGINT.
- No async runtime/futures/libcurl, batching, schema migration or Git/provenance work.
- Follow [AGENTS.md](../../../AGENTS.md): DDD/hexagonal boundaries; small verb-named
  methods, meaningful constants, caller-before-callee, explicit Given/When/Then
  comments, independent expected values, typed errors and narrow lint exceptions.
- No paid/model calls, real-cache maintenance, merge/push/issue closure or rewriting
  historical evidence. Scope changes return to the user. Keep failures/censored samples.

## Review Focus

1. Many early IDs share one active hash: selection must reach a later distinct hash
   without redispatching the first or falsely concluding drain is complete — Tasks 1/2.
2. A response completes while another is held: its vector must commit and a fifth
   request begin before the held response is released — Task 2.
3. All four results finish while storage pauses/fails: pending payloads stay bounded
   and normal cleanup cannot deadlock on completion sending or release the lock early — Tasks 2/3.
4. Config and dimension changes occur after dispatch: old results cannot overwrite
   new-identity state, and incompatible dimensions become existing safe failures — Task 3.
5. A worker panics or the owned process dies: no lost-completion hang, no disguised
   provider retry, correct lock/restart behavior and no stray client process — Task 3.

## File map and dependency order

| Task | Files and responsibility |
|---|---|
| 1 | `src/service/storage.rs`, `src/storage/{work,read}.rs`: exclude outstanding hashes in existing work selection. Update direct call sites and forwarding test implementations; test in `tests/storage_embedding_work.rs`. |
| 2 | `Cargo.toml`, `Cargo.lock`, `src/service/embedding.rs`, new `src/service/embedding/drain.rs`, `src/lib.rs`: provider sharing, bounded scheduler and per-result publication. Adapt the existing private provider double in `src/service/embedding/tests.rs`; split its query scenarios into `src/service/embedding/tests/query.rs` to keep files focused; create `tests/support/parallel_provider.rs` and `tests/parallel_drain.rs`; declare that fixture privately in its consuming integration targets. |
| 3 | New `tests/parallel_drain_process.rs` and shared `tests/support/parallel_drain.rs`; focused private service-lifecycle tests in `src/service/embedding/tests/parallel.rs` reuse existing real-storage fixtures. Update drain interface Rustdoc in `src/service/{embedding_traits,command_traits}.rs`. Extend the Task 2 concurrent fixture for real HTTP/process races. Product fixes stay in their Task 2 owners. |
| 4 | New `tests/performance/drain_compare.py`, `tests/performance/test_drain_compare.py`; update performance README. Reuse existing frozen-build/process ownership/measurement helpers without changing historical campaigns. |
| 5 | New `docs/zkit/letopis/checks/issue-49/report.md`; affected mechanics, architecture, resource-math, provider contract and development descriptions. Final checks, semantic mutations and whole-branch review. |

Do not split production helpers into files that add only forwarding. Keep the new
scheduler in one private module; existing unrelated service generics stay untouched.

## Task 1: Select work while excluding outstanding hashes

**Interfaces:** Add one slice to the existing storage operation:

```rust
fn read_next_embedding_work(
    &mut self,
    expected: &Sha256,
    max_attempts: u64,
    after: Option<&KnowledgeId>,
    excluded_hashes: &[Sha256],
) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;
```

`src/storage/work.rs` receives the same exclusion slice after `after`. Empty
exclusions preserve current behavior. Do not impose the drain's four-worker
constant on this general storage interface; only the caller owns that limit.

- [x] **Step 1: Write red exclusion tests in `tests/storage_embedding_work.rs`.**

Add `excludes_every_representative_of_an_outstanding_hash`,
`empty_exclusions_preserve_cursor_and_attempt_policy`, and
`exclusions_preserve_pinned_config_and_read_snapshot`. Seed literal `a.md` and
`b.md` with shared text, `c.md` with distinct text. Assert excluded shared hash
returns `c.md`, excluding both returns `None`, and another pinned revision still
observes its own rows/config. Keep existing budget/source/stored-vector cases.

```rust
assert_eq!(selected.unwrap().id.as_str(), "c.md");
assert_eq!(all_excluded, VectorOutcome::Current(None));
```

- [x] **Step 2: Run `cargo test --locked --offline --test storage_embedding_work`.**

Confirm the red result is the new missing exclusion behavior/API, not unavailable
dependencies. Existing callers compile after adding empty slices in the next step.

- [x] **Step 3: Implement the bounded exclusion query and update consumers.**

Use SeaQuery `NOT IN` with bound hash values only for a nonempty slice. Preserve
identity preflight, eligible attempt/storage predicates, ID cursor and limit one.
Update `has_work`, drain, `src/service/{traversal/tests/traced_storage.rs,
document_refresher/tests/reading.rs}`, and root tests/support consumers found by
`rg -n 'read_next_embedding_work\(' src tests --glob '*.rs'`; pass `&[]` outside
the parallel scheduler. No schema/version changes or extra storage trait.

- [x] **Step 4: Run focused green tests and an exclusion mutation.**

Run the storage target plus `cargo check --locked --offline --all-targets`.
Temporarily omit the new exclusion predicate: the `c.md` test must fail, then
restore exact bytes and rerun green. Confirm cursor and config tests remain green.

- [x] **Step 5: Commit only the storage/API/test changes.**

Commit message: `feat: exclude active hashes from embedding work selection`.

## Task 2: Run four provider jobs with completion-order publication

**Interfaces:** Keep public `Embedder` and `EmbeddingExecutor` operations unchanged.
Remove the provider generic only from the private embedding service:

```rust
struct EmbeddingService<S, F, M> {
    storage: S,
    files: F,
    markdown: M,
    provider: Arc<dyn EmbeddingProvider + Send + Sync>,
}
```

Its existing constructor accepts that shared provider as its fourth argument.
`build_embedding_provider()` exposes `EmbeddingResult<impl EmbeddingProvider + Send + Sync + use<>>`;
`lib.rs` supplies `Arc::new(build_embedding_provider()?)` at actual assembly sites.
Keep the existing S/F/M bounds for real storage/file capabilities. Do not globally
require every `EmbeddingProvider` implementation to be `Send + Sync`.

Extract the existing computation into one private parent-module function used by
both query inference and worker calls:

```rust
fn compute_vector(
    provider: &dyn EmbeddingProvider,
    config: &EmbeddingConfig,
    text: &str,
    dimensions: Option<NonZeroUsize>,
) -> EmbeddingResult<Vec<f32>>;
```

`drain.rs` supplies `pub(super) fn drain_ready(&mut self, config: &EmbeddingConfig)
-> EmbeddingResult<DrainOutcome>` on the service. A private scheduler state owns
the pool/channel/outstanding hashes and lifecycle behavior; it is actual bounded
state, not a new service/interface layer. Private helper signatures remain local.

- [x] **Step 1: Add a real concurrent provider fixture and red product tests.**

Create `ParallelProvider` in `tests/support/parallel_provider.rs`. It accepts
requests independently, captures synthetic text/request identity, exposes bounded
readiness and individually releasable response barriers, and owns/reaps its server
threads on drop. Reuse shared root/storage fixture helpers. Keep existing serial
`HttpFixture` unchanged so sequence-based old tests retain their meaning.

In `tests/parallel_drain.rs` add
`overlaps_four_requests_without_dispatching_a_fifth`,
`publishes_a_completion_before_other_requests_finish`,
`deduplicates_shared_hashes_while_other_slots_progress`, and
`keeps_completed_unpublished_results_inside_the_four_slot_bound`.

```rust
assert_eq!(observed.max_document_active, 4);
assert_eq!(observed.requested_before_release.len(), 4);
assert_eq!(reserved_unique_hashes_before_release, 4);
assert_eq!(observed.max_active_for_shared_text, 1);
assert_eq!(stored_second, Some(vec![0.0, 1.0, 0.0]));
```

Hold the first/third/fourth response, release the second, observe its literal
stored vector and then fifth request before releasing the first. A write-pause
schedule additionally completes results while the coordinator cannot publish;
read the persisted reservation rows independently and assert exactly four distinct
reserved hashes before any response is released. This catches a fifth job hidden
in the pool queue even though only four HTTP calls can be active. No replacement
reservation may occur while a completed slot still awaits publication. Use barriers/readiness,
owned watchdogs and condition predicates, not sleeps as proof of non-arrival.

- [x] **Step 2: Run the new target against sequential behavior and confirm red.**

Run `cargo test --locked --offline --test parallel_drain`. The bounded readiness
oracle must fail because fewer than four requests arrive before release. Fixture
cleanup must release all barriers even when the test unwinds.

- [x] **Step 3: Implement the pool, owned jobs, sharing and bounded loop.**

Pin `threadpool =1.8.1` and resolve/fetch locked dependencies only now that execution
is authorized; preserve all unrelated versions, especially `ureq =3.4.2`. Inspect
its pinned source: its internal channel is unbounded and `execute` takes owned
`Send + 'static` work, so admission must happen in the coordinator. `join` waits
for jobs, not an application cancellation signal. Its builder has no fallible build
API: OS thread-creation failure panics upstream. Create the pool before reserving
work and document this invocation-fatal startup limit; do not promise a typed
resource-exhaustion recovery the selected crate cannot provide. Such a failure
must not be mistaken for provider failure or silently change to sequential mode.

Place the single documented four-worker constant visibly at the top of `drain.rs`.
Move drain scheduling there while reusing parent text/reserve/publish/failure/prune
operations. Count admitted hash/config work until completion handling finishes;
select using Task 1 exclusions, reserve durably, submit HTTP, receive whichever
result finishes, publish in one short transaction and refill. Never scan past an
empty selection to exit while jobs remain. Recheck committed config/dimensions
before publication. Keep original stale-ID skip semantics and one-second failure
wait. Empty/stored/exhausted/disabled cases produce no worker HTTP.

The result channel may use `std::sync::mpsc::channel` because the admission limit
bounds messages to four; never add an unbounded producer. Cleanup drops/keeps
receivers deliberately so sending never blocks, stops dispatch, waits all submitted
jobs and preserves the initial failure. Catch worker unwind into a private completion
outcome, join remaining work and resume it on the coordinator, without retrying it
as a provider error. No silent task loss or waiting forever for a vanished result.

Adapt the existing test `Provider`: use atomics/mutexes for shared observations
and retain a typed `Arc<Provider>` handle for assertions/configuration. Keep
coordinator-only `Files` state single-threaded; do not send its `Cell`/`RefCell`
or SQLite into workers. A shared provider lock must not span HTTP.

- [x] **Step 4: Run green concurrency, old behavior and cap/dedup mutations.**

Run `cargo test --locked --offline --test parallel_drain`,
`cargo test --locked --offline --lib service::embedding::tests`, and root
`embedding_service`, `embedding_provider`, `embedding_boundaries`, `embedding_lifecycle`
targets. Temporarily admit a fifth job or remove active-hash exclusion: the relevant
new assertions fail. Restore exact bytes and rerun focused green. Ensure query
validation still passes through the same computation rule.

- [x] **Step 5: Commit the scheduler, wiring, dependency and focused proof.**

Commit message: `feat: drain embeddings with four bounded synchronous workers`.

## Task 3: Prove race, failure and process lifecycle behavior

**Interfaces:** Consume Task 2's unchanged public `wire_embedder` /
`wire_embedding_executor` and its shared concurrent HTTP fixture. Tests may access
private scheduler state only inside its own unit module. No production test knobs,
new public drain commands or generic mock layers.

- [x] **Step 1: Add red regression scenarios with independent expected state.**

In `tests/parallel_drain_process.rs` add
`keeps_one_drain_owner_with_four_active_requests`,
`preserves_reserved_attempts_after_kill_and_restart`,
`discards_old_identity_completions_after_reset`,
`never_assigns_old_results_to_edited_or_deleted_text`, and
`rejects_out_of_order_dimension_mismatch`.

In focused private scheduler tests add
`joins_submitted_jobs_before_returning_storage_failure`,
`propagates_worker_panic_after_joining_other_jobs`,
`propagates_worker_panic_during_error_cleanup`, and
`waits_for_active_jobs_before_treating_empty_selection_as_complete`.
Check shared-hash retry non-overlap, malformed/wrong-count/nonfinite responses,
preparation failures and one-second retry waits using existing literal/error cases.

```rust
assert_eq!(reserved.attempts, 1);
assert_eq!(new_identity_vector, Some(vec![1.0, 0.0, 0.0]));
assert_eq!(old_result_in_new_identity, None);
assert_eq!(active_jobs_at_normal_return, 0);
assert!(observed_worker_panic);
```

Use owned children/actual lock observers. Stop only the test-owned drain, not a
PID read from arbitrary state; release the server to prevent orphaned handlers.
A detached drain retains default SIGINT behavior, not foreground cancelled JSON.

- [x] **Step 2: Run targeted scenarios and establish semantic sensitivity.**

Run `cargo test --locked --offline --test parallel_drain_process` and the private
drain test filter. If complete behavior already passes from Task 2, demonstrate
red by compiled, isolated mutations of the exact guarded path; do not weaken an
assertion or manufacture unrelated syntax failures.

- [x] **Step 3: Fix only evidenced gaps in existing owners.**

Preserve old-identity attempts/errors after reset by rejecting stale completions;
concurrent first vectors must use storage's committed dimension check. Ensure
all normal exits join before lock drop, including errors and old config. A panic
remains a panic after cleanup; a disconnected completion without a panic is an
internal error, not successful completion. No global locks around provider work.

- [x] **Step 4: Run focused process/storage/foreground regressions and mutations.**

Run new targets plus `cli_drain`, `search_contention`, `storage_embedding_work`
and existing `service::embedding::tests::process` tests. Run existing root
cancellation tests found by `rg -n 'fn .*sigint|fn .*cancel' tests src/service/embedding`.
Mutate reservation, identity guard, dimension guard and early cleanup separately;
confirm intended assertion failures, restore hashes, then rerun green.

- [x] **Step 5: Commit the lifecycle proof and any narrowly justified fixes.**

Commit message: `test: verify parallel drain races and worker lifecycle`.

## Task 4: Measure sequential, one-worker and four-worker drain

**Interfaces:** Create `tests/performance/drain_compare.py` with CLI
`--before <build.json> --one-worker <build.json> --after <build.json> --evidence <new-dir>`.
Reuse frozen manifests from `compare.freeze_build`, verification from
`compare.verify_frozen_build`, `measure.measure_process` and exact owned-process
cleanup from `ownership.FixtureProcesses`. Do not claim existing `FakeProvider`'s
single-request barrier proves four active jobs; use a concurrent synthetic fixture.

- [ ] **Step 1: Write red measurement/oracle tests.**

In `test_drain_compare.py` add `test_refuses_existing_evidence`,
`test_rejects_wrong_literal_vector_or_request_count`,
`test_keeps_failed_samples_in_denominators`,
`test_distinguishes_document_and_query_concurrency`, and
`test_cleans_owned_drain_after_interruption`.
Assertions compare captured literal expected vectors/counts and explicit incomplete
outcomes, never derive expected output through product embedding/storage code.
A fixture with five active document requests must fail the four-worker oracle.

- [ ] **Step 2: Run the test file and confirm missing behavior is red.**

Run `python3 -B -m unittest discover -s tests/performance -p test_drain_compare.py`.
Use the prepared Python 3.12 executable if system Python is older. Keep output
compact, expose full logs on failure.

- [ ] **Step 3: Implement the narrow model-free runner and freeze variants.**

Use 64 distinct synthetic texts plus shared-hash nodes, a concurrent provider with
25 ms fixed response delay and independent literal vector mapping. Create new
standalone repositories; suppress automatic drain during initialization with an
owned embed lock; then measure explicit `embed --drain`. Freeze (A) base sequential,
(B) Task 3 with only the constant changed to one in a disposable evaluation copy,
(C) unchanged Task 3 four-worker product. Preserve the evaluation patch and hashes;
never leave the one-worker edit in the active checkout.

Give each variant one conditioning plus five measured fresh-projection samples.
Run in alternating A/B/C order, 120 seconds per drain and a 20-minute stage watchdog.
Record raw monotonic durations, unique vectors, request/attempt counts, observed
concurrency, failures and drain-process RSS/CPU with exact accounting scope.
Hash the fixture/input/config/tooling/binaries. Setup/build is outside drain timing.
Add a separate bounded query-overlap schedule using actual CLI query calls;
identify query/document HTTP separately and check retrieval correctness. Do not
sum per-process RSS peaks as a whole-stack peak or call fixture vectors model quality.

- [ ] **Step 4: Run green oracle tests, negative controls and the bounded campaign.**

Run the new test file, then the runner on the three frozen manifests. Show
complete/incomplete receipts, not timing alone. Mutate expected mapping, cap and
failure accounting independently; each new oracle rejects its bad input. Write
a table for A/B/C: vectors, time, throughput, actual concurrency, attempts/errors,
latency median/range, RSS scope and overlapping query latency. A real-provider
comparison is explicitly not assessed pending separate authorization.

- [ ] **Step 5: Commit reusable runner/tests/instructions, not ignored raw artifacts.**

Commit message: `perf: compare sequential and bounded parallel embedding drain`.

## Task 5: Document results and verify the final tree

**Interfaces:** Produce `docs/zkit/letopis/checks/issue-49/report.md` with exact
source/build/harness provenance, invariant matrix, before/after table, limitations
and remaining real-provider acceptance. Update this plan's checkboxes only from
evidence; do not claim full issue completion while required real runs are deferred.

- [ ] **Step 1: Reconcile design descriptions with the implemented behavior.**

Update MC-3/MC-5 and its memory statement, architecture/dependency list,
resource-math concurrency assumptions and affected provider-interface wording.
Keep the fixed limit in the owning Rust constant; docs may state its agreed value.
No config/CLI schema changes, hardware/gate claims or historical evidence rewrites.
Record package/source review, joins, imports, trait boundaries and actual lib wiring.

- [ ] **Step 2: Run the required final host checks once on the actual changed tree.**

Run `make verify` (which includes required `make check`, root debug/release tests
and strict Rustdoc); capture compact logs. Repeating checks is justified only by
new changes/failures. Run the relevant storage runner because selection changed:
`python3 -B tests/storage/run.py --platform host --evidence <new-dir>`.
Rerun new measurement-tool tests if its inputs changed since Task 4; historical
standalone manifests require separate relevant commands, not an implied root pass.

- [ ] **Step 3: Verify Linux with matching input hashes.**

Run new `parallel_drain`, `parallel_drain_process`, `storage_embedding_work`,
`embedding_provider`, `cli_drain` and `search_contention` targets and the storage
runner `--platform linux --evidence <new-dir>` in the available isolated environment.
Compare source/lock/build/vendor/toolchain input hashes to the host tree. Record
architecture/libc/toolchain and unrun targets; no x86_64 or power-loss claims from
an ARM64 host/container. If environment is unavailable, state the actual limit.

- [ ] **Step 4: Complete architectural/semantic review and resolve findings.**

Review physical files, visibility, bounds and `lib.rs` constructor calls against
AGENTS; check the constant is visible at the top of its single owning file. Audit
all test expectations, readiness cleanup and compiled-mutation receipts. After
implementation request one independent whole-branch review (plus per-task gates if
Subagent-driven was selected); fix actionable findings and rerun affected checks.
Record reviewer scope separately from actual executable verification.

- [ ] **Step 5: Finalize the report and save the complete branch.**

Write exact commands/results/input revisions and the controlled before/after table.
State unrun real model/provider/whole-stack/minimum-hardware acceptance. Preserve
prior baselines, private artifacts and unrelated edits. Commit intended paths only:
`docs: report parallel drain measurements and verification`. Report branch status
and next integration choice; do not push/merge/close the issue automatically.

## Plan self-review

2026-10-31: specification mapped to Tasks 1–5: selection/dedup (1), bounded jobs,
per-result publication and shared computation (2), retries/races/shutdown/panic (3),
three-way causal measurements and query contention (4), actual-tree/platform/
architecture review and deferred acceptance (5). All five Review Focus conditions
have named tests. Interface names and paths were checked against current sources.
The old HTTP fixture and provider double cannot prove concurrency as written;
the plan names their separate replacement/adaptation without sending storage or
coordinator-only RefCell state across threads. Self-review also strengthened the cap oracle with persisted reservation counts,
so an overfull internal queue cannot hide behind four active threads, and removed
redundant whole-suite reruns. The pinned crate's fatal thread-creation startup
behavior is explicit; no typed recovery is fabricated. No independent plan review
or implementation has been claimed. Native execution was subsequently authorized in voice; implementation starts after
this plan and the final accepted specification are saved.
