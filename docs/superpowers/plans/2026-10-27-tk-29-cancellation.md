# TK-29 Cancellation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete handled SIGINT, the existing config read command, concurrent-process acceptance and the maintenance skill for TK-29.

**Architecture:** Keep signal delivery and termination in process, response ownership in CLI, and config observation in an application service using existing filesystem/provider interfaces. Wire these in lib.rs; domain and existing publication/drain algorithms remain independent of cancellation.

**Tech Stack:** Rust 1.95.0, synchronous std I/O/threads, signal-hook `=0.3.18` (already used by TK-6), existing rusqlite/SQLite WAL, ureq, serde_json, Unix signals and File::lock.

**Spec:** [Approved TK-29 design](../specs/2026-10-27-tk-29-cancellation-design.md).

**Status:** Executed. Base `34af7d7a7f67cbde7db61622f354bcb6532b8866`; Task 1–4
commits `1584092`, `b68dc1e`, `269e89f`/`59ad8e8`, `e8e296d` and
`bb7847f`; Task 5 functional verification delivered. Final host and Linux runners
on `bb7847f` passed at 844/0/10 per profile. Formal G-2 is **not passed**, so the
TK-29 card is `blocked`. See [report](../../zkit/letopis/checks/TK-29/report.md),
[verification.json](../../zkit/letopis/checks/TK-29/verification.json) and
[G-2 matrix](../../zkit/letopis/checks/TK-29/g2-matrix.md). Task 1–4 step checkboxes
remain as captured in their own task reports; this Task 5 update does not restate
or rerun that evidence.

## Global Constraints

- Follow AGENTS.md and the architecture links in the specification; KISS, DRY, YAGNI and DDD/hexagonal boundaries apply to every task.
- Handled commands: search, traverse, inspect, check, reindex. Cancelled response: one JSON, `ok:false`, `error.code:cancelled`, exit 1. Status/config/drain retain default SIGINT disposition.
- Install before stdin/service I/O; no async runtime, InterruptHandle, execution deadline, PID cancellation API, new schema or persistent cancellation state.
- Commit ordering is against process termination, not signal receipt. Either complete publication may win a free race; never claim cancelled proves rollback.
- A response guard covers write plus flush. Backpressure may delay termination; broken output never causes replacement JSON. Callback/thread teardown must not deadlock.
- Preserve injectable library run_cli; process-global registration belongs only to an executable-oriented entry path.
- Config reads through StatusFiles (the non-creating read_config implementation), never EmbeddingFiles/embedding readiness or storage bootstrap. No setters, credentials output or inference.
- Tests use explicit Given/When/Then, independent expected values, owned child cleanup and bounded watchdogs. No product test flags/environment checkpoints. Keep functions near 40 lines, at most 60; files focused.
- Use disposable repositories/local HTTP only. Preserve historical evidence; new receipts capture exact source inputs/revision and platform. No paid runs.
- macOS/Linux ARM64 cancellation evidence is required; unavailable platforms remain unassessed. x86_64 remains TK-43. TK-17/G-1 and unconditional G-2 remain unapproved.
- Before execution inspect the branch and user changes; run a clean `make check` baseline. Fetch locked dependencies only when missing; routine checks use `--locked --offline`. This plan preparation runs no product checks.

## Review Focus

1. SIGINT while stdin is still open: handler must already exist; no service directory or child appears (Task 3).
2. Cancelled output wins immediately before normal success: caller must not subsequently return exit 0 (Tasks 2/3).
3. Full stdout pipe, broken output and repeated signals: one response owner, no interleaving or teardown deadlock (Tasks 2/3).
4. Config has duplicate keys, unknown allowed parameter values or no service directory: strict parsing, value preservation and no hidden writes (Task 1).
5. Abandoned Git child/foreign drain outlives foreground: no inherited caller response pipe/index lock; foreign work survives and completes (Tasks 3/4).

## File and interface map

| Owner | Files to create or touch | Responsibility |
| --- | --- | --- |
| Configuration service | new `src/service/configuration.rs`, new `src/service/configuration/tests.rs`; `src/service/mod.rs`, `src/service/command_traits.rs` | ConfigReader and pure observation orchestration |
| Config CLI | new `src/cli/config_tests.rs`, new `tests/config_command.rs`; `src/cli/{mod,input,handlers,output}.rs`, `src/lib.rs` | Request/result, interface dispatch, wiring and schema-compatible output |
| Response ownership | new `src/cli/response.rs`; `src/cli/{mod,output}.rs` | Serialize once, arbitrate write/flush and coordinate exit |
| Signal lifecycle | new `src/process/signal.rs`; `src/process/mod.rs`, `src/lib.rs`, `src/main.rs`, `Cargo.toml`, `Cargo.lock`, `docs/development.md` | Scoped listener and executable assembly |
| Cancellation acceptance | new `tests/cli_cancellation.rs`, new `tests/support/cancellation.rs`; `tests/support/cli.rs` only for shared owned-process needs | Shipped binary, real wait observations and isolation |
| Private process proofs | new `src/cli/response/tests.rs`, new `src/storage/cancellation_tests.rs`; `src/storage/sqlite.rs` only to declare its test child module | Deterministic output races, SQL VM and real commit boundaries |
| Drain/publication acceptance | new `tests/cancellation_concurrency.rs`; existing `tests/support/drain.rs` if at least two consumers need a helper | Late results and foreign process lifetime |
| Skill and delivery evidence | `skills/letopis/SKILL.md`, new `tests/maintenance_workflow.rs`, `tests/storage/{Dockerfile,run.py,test_runner.py}` only where required; TK-29 card; new `docs/zkit/letopis/checks/TK-29/{report.md,verification.json,g2-matrix.md}` | Complete workflow, Linux inputs and honest acceptance |

The storage test module is declared inside sqlite.rs with a path attribute so it
can use private connections; production storage behavior does not change solely
for instrumentation. No additional example executable or public test APIs needed.
Split a test/support file by responsibility only if it exceeds the project size
guideline. Do not rearrange unrelated code.

### Task 1: Complete observational config command

> **Completed in the Task 1 report** (`.superpowers/sdd/2026-10-27-tk-29-cancellation/task-1-report.md`, commit `1584092`); the steps below are left as executed there.

**Interfaces:** Add `ConfigReader::read_config(&self) -> CommandResult<Option<EmbeddingConfig>>`
in command_traits.rs and reexport it from service/mod.rs. `ConfigurationService<'a>`
in configuration.rs owns borrowed `&dyn StatusFiles` and `&dyn EmbeddingProvider`
fields and implements ConfigReader. Add `Request::Config`,
`ResponseResult::Config(Option<EmbeddingConfig>)`, and optional
`HandlerServices::config: Option<&dyn ConfigReader>`. Dispatch without background
work. lib.rs constructs LocalFileSystem and HttpProviders and borrows them into
this service; it does not open SqliteStorage. Use existing provider validation
(local validation only), not Embedder::read_configuration, which resets vectors.

- [ ] **1. Write red service and CLI tests.** `reads_missing_config_without_creating_service_state`, `returns_preserved_config_values`, `rejects_invalid_config_without_exposing_bytes`, `rejects_nonempty_config_request_before_io`. Use literal Configuration input below, assert the same JSON value on output, `max_retries:2`, no identity/dimensions-derived fields, warnings absent, exit 0. Test `{}`, malformed/trailing JSON, duplicate keys at root/nested parameters, forbidden credentials and symlink/unreadable paths. A fake provider's embed method must never run. InvalidConfig maps to CommandErrorKind::EmbeddingFailed with safe message `invalid embedding configuration`; filesystem/InternalError maps InternalError with safe message `cannot read embedding configuration`. Preserve causes privately. Expected X-1 codes are `embedding_failed`/`internal_error`, not a new config code.

```json
{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}
```

- [ ] **2. Run red tests:** `cargo test --locked --offline --test config_command`; expect config unsupported or missing response until implementation. Unit tests may first fail compilation for the new trait; obtain a behavioral red after the interface exists.
- [ ] **3. Implement observation and transport.** Parse through EmbeddingConfig::parse and validate_config. Render a config object using existing get_provider/get_model/get_parameters/get_max_attempts accessors; max_retries is max_attempts minus 1 (construction guarantees positive budget). Do not serialize private identity or environment. Use a specific configuration result variant, not a raw unvalidated JSON response. Keep this mapping in CLI output and test preservation of allowed unknown values.
- [ ] **4. Run green:** `cargo test --locked --offline configuration`; `cargo test --locked --offline --test config_command`. Include a real CLI fresh directory and an existing cache/vectors fixture: snapshot files before/after, assert absent state stays absent and existing database bytes/metadata remain unchanged. Register zero accepted provider requests; no drain PID. Credentials absent must not block observation of otherwise valid OpenAI config because validate_config does not require inference credentials.
- [ ] **5. Mutation-check then commit.** Separately bypass config validation, remove parameters from output, and invoke readiness/background during config handling; relevant tests must fail. Restore exact bytes, run `make check`, commit intended paths as `feat: expose read-only configuration command`.

### Task 2: Add single-response ownership and scoped SIGINT lifecycle

> **Completed in the Task 2 report** (`.superpowers/sdd/2026-10-27-tk-29-cancellation/task-2-report.md`, commit `b68dc1e`); the steps below are left as executed there.

**Interfaces:** CLI owns `ResponseWriter` in response.rs, default-constructible,
with `write_response(&self, output: &mut dyn Write, response: CommandResult<CommandResponse>) -> u8`.
Split encoding from writing in output.rs using
`encode_response(response: CommandResult<CommandResponse>) -> Result<(Vec<u8>, u8), serde_json::Error>`;
bytes include the final newline. Preserve the existing write_response convenience
function and existing run_cli API. ResponseWriter stores a mutex-protected
`Option<u8>` completion/claim state; once claimed it never tries a second output,
including after write failure. Hold the mutex through write/flush and return the
winning response's exit code to a losing normal caller.

Process owns `SigintListener::install(on_interrupt: Box<dyn FnOnce() + Send>) -> io::Result<Self>`
in signal.rs. It creates Signals for SIGINT, retains its Handle and JoinHandle,
invokes the callback once and then exits 1. Its Drop closes the Handle, wakes and
joins the listener; no blocking forever on normal completion. Register/spawn
failure drops partial resources and returns an error. `run_process_cli() -> u8`
in lib.rs collects argv, constructs Arc<ResponseWriter>, installs for the five
exact command argv shapes, then reads stdin/runs the normal request path and
writes with the shared owner. The callback constructs the existing Cancelled
CommandError with safe message `command cancelled` and uses stdout. main.rs
calls this entry and converts its returned u8 to ExitCode. Never lock stdout
on the main thread while executing services. Status/config/drain skip registration.

- [ ] **1. Write red unit tests** in response/tests.rs: `cancellation_winner_prevents_success_exit`, `normal_winner_prevents_second_json`, `write_failure_never_retries_output`, `guard_covers_flush`. Use a controlled Write implementation that pauses inside write/flush; output expected bytes/exit codes are independent literals. The cancellation-first test must return 1 even when the losing normal response is successful. The success-first test returns 0 to normal path; a signal listener still exits 1 independently.
- [ ] **2. Run red:** `cargo test --locked --offline --lib cli::response`; expect missing module/API initially, then demonstrate failure of a real output/exit assertion before fixing it.
- [ ] **3. Implement writer, listener and composition.** Add exactly `signal-hook = "=0.3.18"`, resolve/fetch locked inputs deliberately and document it in development.md. Its cached iterator source confirms Handle::close wakes blocking iteration; listener owns Signals until join. Keep all serialization on ordinary threads. Handle mutex poisoning and encoding failure safely with exit 1; never unwrap in production. No resettable process-global writer. Use private cfg(test) injection to exercise registration/spawn failures without adding product flags or a runtime framework.
- [ ] **4. Run green unit/process tests.** Add listener tests for ordinary shutdown and registration/spawn failure: no request body executed, one internal_error, exit 1; a spawned listener is reaped. OS-signal tests run in owned child processes so the cargo test host never receives SIGINT. Ensure repeated library run_cli calls do not accumulate handlers/threads and existing injectable outputs still work. Run `cargo test --locked --offline --lib cli::response`, `cargo test --locked --offline --lib process::signal` and `cargo test --locked --offline --test cli_runtime`.
- [ ] **5. Mutation-check and commit.** Remove the writer claim, return the losing success code, release the guard before flush, and remove close/wakeup in normal teardown, one at a time. Assert intended oracle/watchdog failures, restore, `make check`; commit `feat: handle foreground SIGINT with one response`.

### Task 3: Prove cancellation at actual wait and commit boundaries

> **Completed in the Task 3 report** (`.superpowers/sdd/2026-10-27-tk-29-cancellation/task-3-report.md`, commits `269e89f`/`59ad8e8`); the steps below are left as executed there.

**Interfaces:** Consume Task 2's crate-private ResponseWriter and SigintListener
inside unit child processes. Shipped-binary tests use `env!("CARGO_BIN_EXE_letopis")`.
Reuse OwnedChild's send_sigint/collect_output; child cleanup must kill/reap only
owned children, even after failed readiness/assertions. New support helpers stay
private to tests or shared only between real consumers. Signal test input readiness
requires a positive platform observation, not an elapsed sleep.

- [ ] **1. Write red acceptance tests** in cli_cancellation.rs: `cancels_each_foreground_command_while_reading_stdin`, `cancels_waiting_index_lock_without_killing_holder`, `cancels_while_git_child_is_running`, `cancels_while_provider_request_is_pending`. Loop the five exact argv commands only for the stdin case; HTTP is reachable via search, not all commands. Parse all stdout as exactly one value plus newline; assert literal `ok:false`, code `cancelled`, exit 1 and no request canaries. Before SIGINT prove the product process is waiting for stdin/inside blocking flock using native observer (macOS sample, Linux /proc syscall+fd identity, following TK-4). Git wrapper uses argv exec and a Unix socket acknowledgement from the live child; local HTTP must fully consume and acknowledge the request. These are shipped-binary tests. Preserve paths/PIDs and ensure the Git child cannot keep response pipes or the index lock open after parent exit.
- [ ] **2. Add deterministic private proofs** in response/tests.rs and storage/cancellation_tests.rs: `cancel_output_wins_before_main_output`, `success_output_survives_late_sigint`, `repeated_sigint_keeps_one_response`, `sigint_during_backpressure_finishes_one_response`, `cancels_inside_sqlite_step`, `termination_before_commit_keeps_old_publication`, `termination_after_commit_keeps_new_publication`. Re-exec the unit test binary with an exact private child test and socket barriers. Instrument a disposable private SQLite connection with a test-only sqlite3_progress_handler callback via existing rusqlite::ffi: acknowledge inside the SQL VM and pause there, then signal the child running the real storage operation. Keep context live and document unsafe lifetime invariants. This proves SQL-step cancellation without InterruptHandle or new rusqlite features. For commit tests stage actual CacheWrite node/text/file mutations, hold before CacheWrite::commit until death, or acknowledge only after its successful return. A separate process reads independently expected old/new node/FTS/revision, verifies integrity and completes another publication. Label these storage/unit-process proofs separately from full CLI tests.
- [ ] **3. Run tests:** `cargo test --locked --offline --test cli_cancellation`; `cargo test --locked --offline --lib cli::response`; `cargo test --locked --offline --lib cancellation_tests`. Baseline after Task 2 may already pass correct cancellation; do not claim a fabricated red. Establish each oracle with the mutation in Step 5.
- [ ] **4. Complete boundary fixtures.** Full-pipe test waits for verified pipe pressure/write checkpoint, delivers SIGINT, then resumes draining and asserts one complete original envelope; closed output exits 1 without fallback JSON. A cancellation-winning fixture holds main before output, lets signal output flush, then releases main and checks exit 1. An after-output fixture keeps the process alive until signal delivery to establish the late-signal case. Do not alter production paths solely to expose these controls. If a current Git child inherits a lock FD, fix close-on-exec only at its actual owning adapter and rerun the failing case.
- [ ] **5. Mutation-check and commit.** Disable registration for one command (then all named commands as individual cases), bypass claim/exit coordination, replace the flock observer with a pre-lock acknowledgement, commit before the held pre-commit barrier, and bypass SQL-step barrier. Each must fail the intended test, not merely time out for unrelated setup. Restore exact bytes; `make check`; commit `test: verify cancellation wait and commit boundaries`.

### Task 4: Verify publisher/drain isolation and late embedding results

> **Completed in the Task 4 report** (`.superpowers/sdd/2026-10-27-tk-29-cancellation/task-4-report.md`, commit `e8e296d`); the steps below are left as executed there.

**Interfaces:** Existing Refresher, EmbedLocker, EmbeddingExecutor, real CLI
and local HTTP; no new production abstraction. Reuse existing drain support and
TK-27 fixture expectations. Storage writes may be fixed only for a demonstrated
failure and within the accepted identity/hash protocol.

- [ ] **1. Add concurrent-process scenarios** in cancellation_concurrency.rs: `cancelling_holder_releases_index_lock`, `cancelling_waiter_preserves_holder_and_lock_inode`, `foreground_cancel_preserves_detached_drain`, `direct_drain_sigint_keeps_durable_attempt`, `late_drain_vector_does_not_represent_changed_text`. Hold/release with sockets or consumed HTTP requests; verify foreign process survival by subsequent completion, not kill(pid,0) alone. Check persistent lock inode before/after, successful successor publication and actual complete search results. Start drain with a durable reservation before signalling it directly; assert no stdout JSON and a fresh process sees attempts=1. Text race: delay response for old text, publish a new search_hash, release old result; current query/view must not associate the old vector with the new hash. New text may remain backlog or get its own later correct vector.
- [ ] **2. Run focused tests:** `cargo test --locked --offline --test cancellation_concurrency`. For identity race rerun existing `cargo test --locked --offline --lib stale_configuration_observation_preserves_current_vectors_and_attempts` and `cargo test --locked --offline --test search_consistency`. Cite TK-27's historical reset/reservation evidence at `14d1e47`; do not relabel as a fresh run. If new tests pass immediately, prove their assertions with deliberate mutations rather than claiming red tests happened.
- [ ] **3. Fix only reproduced product defects.** Preserve one drain lock owner, durable reservations before HTTP, transaction release during inference and identity checks before save. Never introduce queue tables, generic process registries, shared inference locks or retries outside the existing budget.
- [ ] **4. Mutation-check acceptance.** Remove a stale identity/hash check individually; temporarily signal the foreign drain instead of only foreground; remove durable reservation; replace/delete the persistent lock file. Expected: current vector/attempt/foreign-completion/inode assertions fail. Restore each mutation and capture which guards were independently exercised; report redundant guards honestly.
- [ ] **5. Run green and commit:** `make check`; commit intended changes as `test: verify cancellation and drain isolation`.

### Task 5: Complete the skill workflow and report functional acceptance

**Interfaces:** Real product CLI including Task 1 config; schema X-1 and the
existing agent skill. This task consumes all previous test names and evidence.
It produces report.md, verification.json and g2-matrix.md, not a new runtime API.

- [x] **1. Add `tests/maintenance_workflow.rs`.** `maintenance_roundtrip_preserves_knowledge_and_source_basis` writes a marked knowledge file referencing a real source and verified existing Git tree, then runs check → search → traverse → mixed inspect and reads both returned tree paths. Compare addressed source bytes/hash independently. Read absent config (null), write valid local config, read it, observe status, reindex, replace with invalid config and observe safe config failure plus status problem/FTS availability; fix config and continue. Cancel one refreshing invocation with Task 3's verified barrier, retry successfully, and assert authored Markdown/Git index/HEAD untouched by CLI. Config editing belongs to the test agent, not Letopis.
- [x] **2. Update `skills/letopis/SKILL.md`** to demonstrate config/status/reindex and SIGINT of the caller, exit 1, uncertain committed outcome, safe retry and independent drain. Remove obsolete TK-26–29 cancellation disclaimers only when the new checks substantiate the claims. No cancel subcommand or unknown config setter.
- [x] **3. Run focused workflow and mutation.** `cargo test --locked --offline --test maintenance_workflow`. Bypass one maintenance operation/CLI dispatch used by the scenario and confirm the expected cross-command assertion fails; restore. Validate actual captured CLI envelopes against X-1 using the project's existing schema-validation tooling, not only a handwritten success-field check. Preserve captured raw responses separately from concise logs.
- [x] **4. Prepare Linux inputs and run final verification.** Ensure tests/storage/Dockerfile includes any new nested support inputs and `skills/letopis` if a compiled test reads that file; run.py hashes those same skill inputs. Any runner input change gets a focused regression in test_runner.py proving omission is detected. Run `python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk29/<fresh-host-run>` and `python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk29/<fresh-linux-run>` because private storage test modules/build dependencies change. Substitute unique directory names for the angle-bracket placeholders; never reuse an existing evidence path. Each runner executes `make verify` on the final tree: use those host/Linux results to satisfy the final verification requirement without another identical run. Record actual architecture; unavailable Docker/native observers keep their acceptance pending. No unrun-platform pass claims.
- [x] **5. Review actual final changes and write evidence.** Check caller/signal exit races, once-only guard lifetime, normal listener shutdown, library repeatability, private causes, CLI→service direction, lib.rs constructors and absence of new domain dependencies. Mutation source must be fully restored. Record exact commands, passed/failed/ignored counts, source/input hashes, platform, shipped-binary versus private fixture coverage and historical receipt revisions. docs-only final edits require link/diff consistency checks, not rerunning unchanged Rust suites.
- [x] **6. Fill `g2-matrix.md`** with individual FR-1…FR-6, FR-11…FR-13, FR-17…FR-36 and C-1…C-10 rows. Each has evidence path/revision, coverage classification (current/historical/unverified), result and open limitation. Link prior task reports instead of broad reruns; changed paths make old evidence explicitly historical. Keep TK-17/G-1 and any missing functional/platform criterion visible. C-10 x86_64 remains the roadmap's TK-43 exception. Do not declare unconditional G-2 continue.
- [x] **7. Update task/spec/plan and Engram.** Task/spec/plan updated; the controller recorded the final review, evidence and blocked G-2 state in the existing Engram task `entity_85dbf2db30be411884170e6ac9b48c36` while preserving its prior attributes. Only demonstrated steps/criteria are checked; the signal-receipt/termination clarification and the delivered-versus-gate distinction remain in the card. Commit `docs: report TK-29 cancellation and maintenance acceptance`. Merge/push/GitHub updates require their own user instruction.

## Self-review and execution handoff

Spec coverage: config gap → Task 1; single response and listener setup/lifecycle →
Task 2; five commands/four waits/output/commit races → Task 3; late drain and
foreign-process safety → Task 4; skill, platforms, architectural review and G-2
matrix → Task 5. Each Review Focus entry has a named test above. Interfaces use
existing EmbeddingConfig/CommandResult and explicit service-owned ConfigReader;
process consumes only a callback and knows no CLI/error envelope. No product
implementation, dependency installation, baseline or behavioral test was run
while preparing this plan.

Recommend Native for execution: Tasks 2–4 share delicate output/process fixtures,
so one implementer keeps interfaces coherent and avoids repeated context. If the
user selects Subagent-driven instead, preserve these task interfaces and apply
fresh implementation/review gates. This is a recommendation, not a selected method.
Wait for plan approval and execution authorization; the user explicitly requested
plan preparation only.


## Independent implementation review (2026-10-28)

The subsequent review strengthened actual repeated-SIGINT coverage, replaced
the pressure timing heuristic with a WouldBlock checkpoint and corrected scoped
AGENTS.md violations. The accepted behavior and task boundaries are unchanged.
See [review findings and verification](../../zkit/letopis/checks/TK-29/review-2026-10-28.md).
