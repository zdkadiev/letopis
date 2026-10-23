# TK-11 CLI Runtime Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the synchronous CLI transport, persistent kernel locks and detached drain launcher, then connect the completed TK-13 executor for real drain acceptance.

**Architecture:** CLI handlers convert strict JSON into application-owned values and call service interfaces. A concrete background service coordinates work readiness, a released embed-lock probe and a concrete process launcher. The composition root constructs these components; business check/inspect services remain TK-12/TK-19.

**Tech Stack:** Pinned Rust 1.95.0/edition 2024, std argv and synchronous I/O, existing serde/serde_json/rustix, std File locks, direct libc pinned to the already locked `=0.2.191` for setsid. Config/JCS/HTTP dependencies are reused from TK-13 rather than introduced independently.

**Spec:** [Approved TK-11 design](../specs/2026-10-22-tk-11-cli-runtime-design.md), approved 2026-10-22. Branch `TK-11`, worktree `/Users/zdkadiev/.codex/worktrees/tk-11/letopis`; initial main `12c5ad0`, specification/card commit `1bac8cb`. The user approved this plan and selected Subagent-driven execution on 2026-10-22: «План утверждаю. Давай Subagent-driven».

## Global Constraints

- There is no server, persistent worker, process registry or async runtime.
- Parse argv with `std`, not clap: command names and their documented flags are argv, the command-specific object is JSON on stdin.
- Strict DTOs use serde `deny_unknown_fields`; do not deserialize a general command wrapper absent from X-1.
- For public commands, require the documented JSON object; `{}` is the empty request.
- No task is marked done with fake or pending acceptance.
- Do not duplicate its configuration validation/JCS implementation, HTTP clients, work selection, attempts, vector publication or provider logic in TK-11.
- TK-13 owns the drain's lock for its entire execution.
- CLI handlers do not acquire index.lock around a service that already owns it; double locking is not extra safety.
- File existence or PID text is not ownership.
- Release through RAII and never unlink the lock file.
- Use the pinned Rust toolchain and project Rust conventions: explicit imports, verb-form methods, meaningful local constants, focused functions aiming at 40 lines and no more than 60, typed failures, minimal visibility and Arrange/Act/Assert tests.
- Preserve unrelated primary `.gitignore` edits and the active TK-13 worktree. No implicit merge/rebase/cherry-pick, push or GitHub publication.

## Review Focus

1. A malformed or duplicate-field JSON object, a second JSON value, non-UTF-8 argv or unknown flag must fail before creating cache files or invoking any service (Task 1/4).
2. The same path/ID written in equivalent lexical forms must be normalized and deduplicated; a whole-file Markdown address must not be prematurely classified as knowledge rather than a source (Task 1).
3. PID writing must never truncate another owner's file, and replacing a lock name during contention must fail closed without unlinking or bypassing the original lock (Task 2).
4. A detached child must not keep a foreground output pipe or SQLite/lock descriptor alive, and spawn/setup failure must leave one usable foreground response with a safe warning (Task 3/4).
5. Invalid/missing config, exhausted work and a model/config change must not launch useless repeated drains or permit an old executor to publish into the new identity; process fixtures alone cannot prove this (Task 5).

## File structure and ownership

| Files | Responsibility |
|---|---|
| `src/service/mod.rs`, new `src/service/command.rs` | Application-owned command traits, typed addresses/results and safe command errors; mod.rs declares/reexports only |
| New `src/cli/mod.rs`, `input.rs`, `output.rs`, `handlers.rs` | Small transport module: strict decoding, envelope rendering and concrete handlers; implementation bodies remain in responsibility files |
| `src/filesystem/local.rs`, `mod.rs` | Existing LocalFileSystem, one shared persistent File-lock guard, index/embed capabilities and focused private tests |
| New `src/service/background.rs` | Concrete BackgroundService: eligible-work decision, released probe, launch warning and drain delegation |
| New `src/process/mod.rs`, `detached.rs` | Concrete DetachedProcess implementation with exact argv, cwd, setsid and null standard descriptors |
| `src/lib.rs`, new `src/main.rs` | Actual constructor wiring and thin process entry point |
| New `tests/cli_runtime.rs`, `tests/cli_process.rs`, `tests/support/cli.rs` | Product executable checks and isolated process scheduling/support; helper module is not a separate test target |
| New `examples/cli_runtime_fixture.rs` | Explicit test-only consumer of production background/launcher APIs; never linked as a product backend |
| New `tests/cli/run.py`, `test_runner.py`, `Dockerfile`, `negative_controls.py`, `README.md` | Fresh TK-11 receipts, controlled process ownership, platform runs and compiling negative controls |
| Task 5 only: `src/service/embedding.rs`, `lib.rs`, filesystem implementation after TK-13 integration | Reuse its real executor and connect existing capabilities, with no duplicate config/provider logic |

This is five reviewable tasks across four concerns: transport, locks, launch, and composition/integration. Shared module/Cargo/lib edits are sequential. Do not split DTOs/constants into a file per trivial value. Existing `domain/`, Git, SQL and parser algorithms remain unchanged except for a demonstrated integration need reviewed against this spec.

## Frozen interfaces

All fallible public APIs document `# Errors`; errors implement Error with redacted Debug/Display and an optional retained internal cause. `CommandResult<T> = Result<T, CommandError>`. In `service/command.rs`, define `CommandErrorKind::{InvalidRequest, NotFound, ValidationFailed, EmbeddingFailed, InternalError, Cancelled}`, matching X-1 wire names. `CommandError::new(kind, message: &'static str)` and `new_with_cause(kind, message, cause)` keep public messages fixed; access through `get_kind()`/`get_message()`. `CommandProblem { code: CommandErrorKind, message: &'static str, details: BTreeMap<String, serde_json::Value> }` carries explicitly safe structured details, never raw error strings. No serialization of Error::source or Debug.

Application values: `ElementAddress::parse(&str) -> CommandResult<Self>`/`as_str()` stores a normalized string; plain paths use RepoPath without deciding knowledge/source membership, an anchored knowledge uses KnowledgeId, a line range uses SourceUri. Do not add a new domain hierarchy. `CheckReport { errors: Vec<CommandProblem> }`; `Inspection { snapshot: TreeOid, elements: BTreeMap<String, InspectedElement> }`; `InspectedElement { path: RepoPath, lines: Option<LineRange>, hash: Option<Sha256> }`. These are typed metadata, not a free-form success Value. Keys supplied by the real service must be normalized addresses; handler tests check rendering without implementing lookup or snapshot algorithms.

Declare consumer-driven traits in `service/mod.rs`:

```rust
pub trait Checks {
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
pub trait Inspections {
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>;
}
pub trait EmbeddingTasks {
    fn has_work(&mut self) -> CommandResult<bool>;
    fn drain(&mut self) -> CommandResult<()>;
}
pub trait EmbedLocks {
    type EmbedLock;
    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>>;
}
pub trait DrainProcess {
    fn spawn_drain(&self) -> CommandResult<()>;
}
pub trait Background {
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>>;
    fn drain(&mut self) -> CommandResult<()>;
}
```

`EmbeddingTasks` is a narrow consumer capability of the existing executor, not another provider/config/executor. Task 5 implements it on TK-13's concrete EmbeddingService in its owning file. `has_work` uses its existing readiness synchronization and one eligible StorageRead row, with no HTTP, corpus-sized list or open transaction returned.

`cli::input` privately defines `Request::{Check(Vec<RepoPath>), Inspect(Vec<ElementAddress>), Drain}` and `parse_request(args: &[OsString], input: &mut impl Read) -> CommandResult<Request>`. JSON DTOs are private and separate from service values. Missing/empty check.paths means whole corpus; null is invalid. Inspect IDs must be nonempty. Normalize/deduplicate deterministically without changing case/Unicode. Only exact `embed --drain` permits whitespace/EOF as `{}`; explicit nonempty input must be one strict empty object. No JSON-number/boolean coercion.

`cli::handlers::Handlers<'a>` borrows `Option<&'a mut dyn Checks>`, `Option<&'a mut dyn Inspections>`, `Option<&'a mut dyn Background>` and exposes private `execute(Request) -> CommandResult<CommandResponse>`. Optional borrowed trait objects represent genuinely absent/current independently wired services without heap allocation, Arc or fake implementations. `CommandResponse` is private finite `Check(CheckReport)`, `Inspect(Inspection)`, `Drained`, plus per-call warnings. Check/inspect absence and missing drain backend return `internal_error` with fixed `operation unavailable in this build`; do not invoke refresh or a test fixture for an unavailable route. Present services own refresh/read semantics. After a successful check/inspect, call Background::start_if_needed once if configured; never recursively trigger a drain after the drain command.

`cli::output::write_response(output: &mut impl Write, response: Result<CommandResponse, CommandError>) -> u8` serializes the complete envelope before its first write, emits it plus one newline and flushes once. Exit 0 only on successful `ok:true` output; error envelopes and output failures return 1. An output failure does not attempt a second/fallback JSON. No streaming progress or unconditional stderr dump. Deliberate cancellation/once-guard implementation is TK-29.

`BackgroundService<'a,F,P>::new(tasks: Option<&'a mut dyn EmbeddingTasks>, files: F, process: P) -> Self`, where F: EmbedLocks and P: DrainProcess, lives in service/background.rs and implements Background. A missing tasks capability means no work for triggering and unavailable for direct drain. A ready work result probes/releases the lock before spawning. Busy means no launch. Lock-probe/spawn setup failure becomes one fixed safe per-call warning; failure to inspect durable work remains a typed error, not fabricated no-work. Direct drain delegates once; it does not acquire the executor's lock itself.

`DetachedProcess::new(executable: PathBuf, root: PathBuf) -> Self` in process/detached.rs implements DrainProcess with a synchronous Command::spawn; it never waits for model work. Add exact direct dependency `libc = "=0.2.191"`, already present transitively in this branch's lockfile. Use Stdio::null for all three standard fds; the only post-fork pre_exec work is checked libc::setsid and io::Error::last_os_error. No formatting, allocation, logging, locks, SQL, environment modification or other callbacks in that closure. Store/drop Child without kill/wait; owned test fixtures explicitly reap their peers.

Composition in `lib.rs`: public `open_background<'a>(root: &Path, executable: &Path, tasks: Option<&'a mut dyn EmbeddingTasks>) -> CommandResult<impl Background + use<'a>>` constructs LocalFileSystem and DetachedProcess from the supplied owned/validated root and executable paths; the product composition obtains its path exclusively with current_exe, without an argv/environment override; `run_cli(args: impl IntoIterator<Item=OsString>, input: &mut impl Read, output: &mut impl Write) -> u8` parses before side effects, assembles real available services and handlers, renders once. Initially check/inspect/drain capabilities are absent; Tasks 1–4 prove unavailable transport and process fixtures, not operational business commands. Task 5 connects the real drain capability and preserves absent read/check routes. main.rs passes args_os().skip(1), locked stdin/stdout and converts the returned status to ExitCode. Current-directory lookup/composition errors go through the same envelope in lib.rs, not a second main error writer.

## Execution preflight

- [x] Verify branch TK-11, exact status and this approved specification; keep all changes in the named worktree. Preserve unrelated/other-writer files.
- [x] Retain the unchanged baseline: 172 Rust passes plus fmt/compiler/Clippy, `.artifacts/tk11/start/check-rust.log`. Run a new baseline only if source/dependency state changed since that capture.
- [x] Read rust-skills plus AGENTS; verify current TK-13 integration state read-only. Do not assume its plan or passing branch tests establish a compatible combined tree. Do not install dependencies or write product code before this plan's review/execution selection.

### Task 1: Strict command transport and service handlers

**Files:** Create service/command.rs and cli/{mod,input,output,handlers}.rs; modify service/mod.rs and lib.rs for declarations/reexports and the real public run_cli consumer required by strict non-test builds. Private unit tests remain beside their owner. **Consumes:** existing RepoPath/KnowledgeId/SourceUri/TreeOid/LineRange/Sha256. **Produces:** exact command values, traits, Request/parser, Handlers, single-response writer and run_cli with absent capabilities; no concrete fake business service. This public library entry is the current production consumer of the transport helpers, avoiding dead_code allowances while no binary exists yet; Task 4 adds main and later composition.

- [x] **Step 1: Write failing transport tests.** `parse_check_requires_object_and_rejects_unknown_or_duplicate_fields`: `{}`, `{"paths":[]}` accepted; EOF, null, duplicate paths key, `{"paths":null}`, `{"extra":1}`, trailing second object rejected. `parse_inspect_normalizes_without_resolving_membership`: nonempty equivalent plain paths/anchors/ranges deduplicate; `x.md` retains undecided membership; reject empty IDs, C0/NUL, root escape, absolute/reversed/overflow ranges. `parse_argv_rejects_non_utf8_flags_and_surplus`: reject absent/unknown command, bare embed, duplicate --drain, flags on inspect/check and invalid OS bytes before a counted reader/service is touched. `drain_accepts_only_eof_or_empty_object`: test whitespace EOF and `{}`, reject unknown fields/trailing JSON. Assert exact error kinds.
- [x] **Step 2: Run RED.** `cargo test --locked --offline --lib cli::` and focused service address tests; new symbols or assertions fail. Capture actual diagnostics rather than claiming an unrelated compiler error proves behavior.
- [x] **Step 3: Implement the frozen types/parser and handlers.** Add private strict DTOs, explicit conversions and run_cli invoking the actual parser/Handlers(None,None,None)/writer. Its absent capabilities yield the approved real failure path; do not add an empty concrete service to satisfy generic types. Fake Checks/Inspections/Background only inside tests. Handler tests prove one service call with exact normalized inputs, no adapter calls, expected unavailable failures, no drain recursion, success warnings omitted when empty, source/lines/hash metadata rendered without raw text, error causes/secret sentinel absent. Returned check.errors does not itself turn `ok:true`/exit0 into transport failure: it is X-1's validation report.
- [x] **Step 4: Implement/render and run GREEN.** Test every catalogue error code, omitted optional lines vs explicit null hash, no empty warnings, a writer failing before/partway through output returns1 without second attempt, and success/error exactly one JSON plus newline with exit0/1. Run `cargo test --locked --offline --lib cli::`, address/error tests and `./scripts/check-rust.sh`; strict lint/fmt checks pass. Clearly label handler doubles as transport evidence.
- [x] **Step 5: Commit Task 1's actual paths.** `git commit -m "feat(cli): add strict JSON transport and service handlers"`; record checked behavior and ownership, leaving real command acceptance pending.

### Task 2: Persistent std File lock capabilities

**Files:** Modify filesystem/local.rs, mod.rs and service/mod.rs; preserve existing FileSystem API and current filesystem/process tests. **Consumes:** existing confined openat/statat/open_service_directory and DocumentError types; frozen EmbedLocks. **Produces:** LocalFileSystem implements FileSystem/EmbedLocks with one concrete persistent guard based on File; existing open_filesystem exposes both capabilities (`impl FileSystem + EmbedLocks + use<>`).

- [x] **Step 1: Write failing lock tests.** Test PID write after ownership, busy probe returning None without changing holder PID/bytes, both independent index/embed lock names, drop/death releasing ownership while preserving inode, symlink/FIFO/service-directory rejection, path replacement under contention, macOS concurrent create fallback and std File/rustix flock interoperability. Retain existing 20-schedule create race and normal document publication tests.
- [x] **Step 2: Run RED.** `cargo test --locked --offline --lib filesystem::local::tests`; new missing capability/PID assertions fail while historical safe-file tests remain meaningful.
- [x] **Step 3: Implement the lock conversion.** Convert the safely opened OwnedFd to File, use File::try_lock/File::lock and an explicit File::unlock Drop guard; retry Interrupted only. Parameterize the existing owned-name open/reopen helper for index.lock/embed.lock, retaining exactly one no-CREATE reopen after ENOENT with NOFOLLOW/NONBLOCK and regular-file/path dev+inode checks. Construct the guard before PID/truncation fallible work; verify named inode under acquired ownership, then truncate/seek/write PID. Preserve the contention hook at the actual failed try-lock boundary. Never unlink or infer ownership from PID.
- [x] **Step 4: Run GREEN.** Focused filesystem tests and `./scripts/check-rust.sh`; real cross-process lock tests verify serialization and no double-lock around Documents::refresh. No unrelated file-read/schema/SQL changes.
- [x] **Step 5: Commit owned paths.** `git commit -m "feat(filesystem): coordinate index and embedding with File locks"`; record std-flock platform limits until Task 4 runs both systems.

### Task 3: Background orchestration and detached process implementation

**Files:** Create service/background.rs, process/{mod,detached}.rs, examples/cli_runtime_fixture.rs, tests/cli_process.rs and tests/support/cli.rs; modify service/mod.rs, lib.rs, Cargo.toml/Cargo.lock. **Consumes:** Task 1 CommandResult/problem and EmbeddingTasks/Background/DrainProcess; Task 2 EmbedLocks. **Produces:** exact BackgroundService, DetachedProcess and open_background constructor; real process evidence from an explicitly test-only executor.

- [x] **Step 1: Write failing orchestration/process tests.** Fake task counters prove missing/no eligible work does not probe/spawn, busy probe does not spawn, lease is dropped before spawn, readiness failures are typed errors, spawn/probe failures yield one safe warning, and direct drain calls executor once without acquiring a second lock. A real fixture executable owns a barrier/control connection to the observer and uses production open_background; each child runs exact embed --drain in root cwd with inherited safe sentinel env, acquires the real persistent lock, reports session/standard-fd/inherited-fd facts through its owned control connection and exits on release. No fixture business branch is compiled into the product binary.
- [x] **Step 2: Run RED.** `cargo test --locked --offline --lib service::background` and `cargo test --locked --offline --test cli_process`; establish intended failures. Add pinned libc only when implementing spawn, using `cargo fetch --locked` after the minimal lock update if required; never update unrelated dependencies.
- [x] **Step 3: Implement the frozen concrete components and composition.** Borrow optional task capability; no fake production executor. Start the real child through Command::spawn/setsid/null fds with CLOEXEC inherited resources; no parent wait or child mutex allocation. Add safe static error mapping and documented unsafe justification. Bound waiting/killing belongs only to owned test harnesses, not product deadlines.
- [x] **Step 4: Run GREEN and process schedules.** Explicitly build the example: `cargo build --locked --offline --example cli_runtime_fixture --message-format=json`; resolve its real compiler-artifact executable path, not an assumed target/debug path. The private test support helper uses a supplied, verified `LETOPIS_CLI_FIXTURE` artifact when present; otherwise it builds that exact example locked/offline once per test executable and parses the Cargo JSON result, capturing failure diagnostics. Thus ordinary root cargo test needs no prior manual fixture build and never silently skips these tests. The receipt runner prebuilds once and passes the owned artifact path. Use that path as open_background executable in fixture schedules; production run_cli always uses current_exe. Observer barriers prove two released probes cause exactly one active drain, foreground exits/pipe EOF while child is blocked, owner SIGKILL frees the same lock inode, stale PID does not block, setup/exec failure warns without payload leakage, no cache/corpus writes, and neither database nor lock descriptors survive exec. Prove fd0/1/2 really target null and SID differs from foreground; no timing sleeps as correctness oracle. Then run focused tests and `./scripts/check-rust.sh`.
- [x] **Step 5: Commit Task 3 paths.** `git commit -m "feat(runtime): launch detached embedding drains"`; report process-fixture evidence separately from pending production executor integration.

### Task 4: Assemble the product executable and capture runtime acceptance

**Files:** Create src/main.rs, tests/cli_runtime.rs and tests/cli/{run.py,test_runner.py,Dockerfile,negative_controls.py,README.md}; modify lib.rs and relevant CLI unit tests; create docs/zkit/letopis/checks/TK-11/report.md and update TK-11 card/spec/plan. **Consumes:** Tasks 1–3 signatures and real constructors. **Produces:** `letopis` executable, coherent unavailable-route behavior, reproducible host/Linux transport/lock/spawn receipts; runtime portion ready for TK-12/TK-19/TK-13 consumers.

- [x] **Step 1: Write failing executable checks.** Resolve CARGO_BIN_EXE_letopis, invoke in isolated Git repos, verify one schema-valid JSON and exit0/1 for success via injected handler unit tests vs real unavailable errors via product binary. Actual check/inspect valid requests fail with fixed internal_error until their services exist; malformed inputs/argv fail invalid_request before creating .letopis. Internal drain EOF receives the agreed unavailable error before Task 5. Compare corpus bytes, Git index and user refs before/after repeated/lost-response invocations; do not label a disabled route as real idempotent check/inspect acceptance.
- [x] **Step 2: Run RED and assemble.** `cargo test --locked --offline --test cli_runtime` initially lacks the product target. Add thin main calling the existing Task 1 run_cli, verify real open_background constructor calls in lib.rs and keep all helpers consumed by normal non-test builds. No logging/raw panic on ordinary composition/input failures; serialize all errors once. Handler optional service borrows are wired by lib.rs, never direct adapter calls from CLI. Confirm no test fixture/env switch can enable a fake product backend.
- [x] **Step 3: Add the dedicated receipt runner and run GREEN.** Require a Python3.11+ interpreter (verify the `python3` used below; select an installed newer interpreter if needed). Use stdlib for runner/process management and the existing development `jsonschema.Draft202012Validator` for X-1 validation; this is not a product dependency. Preflight its presence and record its version. Prepare this dev library in the Linux image before offline execution (the existing schema validator already uses it); do not write a partial homegrown JSON Schema interpreter or fetch packages during runtime; `run.py --platform host|linux --evidence <new-path>` rejects existing directories and captures exact source/input hashes, executable artifact/hash, commands/exit codes, environment/platform/tool versions and observer receipts. Unit-test nonzero propagation, timeout ownership cleanup, inherited evidence/env overrides and refusal to reuse evidence. Build Linux from current source with prepared dependencies; runtime uses `--network none`. The new runner owns only its containers/children. Reuse established storage runner patterns without rewriting old receipts or claiming historical suites ran.
- [x] **Step 4: Execute bounded proofs.** Run `./scripts/check-rust.sh`; `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`; `python3 -B -m unittest discover -s tests/cli -p test_runner.py -v`; `python3 -B tests/cli/run.py --platform host --evidence .artifacts/tk11/host-<checkpoint>` and matching `--platform linux --evidence .artifacts/tk11/linux-<checkpoint>`. Runner parses the existing JSON-compatible X-1 catalogue with json.loads, selects its command/error $defs and validates captured outputs with Draft202012Validator; no package fetch at runtime, plus real Task 2/3 schedules. Compare common source hashes across platforms. Compiling controls remove null output redirection and kernel exclusion separately; each must compile, fail the intended observer assertion, restore exact source and pass. Only kill/reap test-owned children; do not signal a recorded arbitrary PID.
- [x] **Step 5: Review architecture and commit.** Inspect physical files, minimal visibility, trait implementations, dependency imports and lib.rs constructor calls. Verify the fixture is not reachable from product dispatch. Record exact outcomes/limits; leave real drain criterion open pending Task 5 and real inspect/check with TK-12/TK-19. `git commit -m "feat(cli): assemble executable and verify process runtime"`. No task/issue done claim yet.

### Task 5: Connect completed TK-13 and prove a real drain

**Gate:** Start only when the completed TK-13 implementation is present in the intended integration tree through an authorized merge/rebase. Its current worktree is not ours to alter. A read-only readiness check is allowed; if absent, stop this task, report the exact dependency and retain the tested Tasks 1–4. Do not implement another executor or waive final TK-11 acceptance. This is a bounded integration task, not permission to take over TK-13 or modify its dirty files.

**Files:** Modify the integrated service/embedding.rs, service/mod.rs, filesystem/local.rs, lib.rs and tests/cli_runtime.rs/support; add real-drain cases to tests/cli runner; update report/spec/plan/card. **Consumes:** TK-13 `Embeddings::read_configuration/drain`, ConfigState/DrainOutcome, StorageRead::read_next_embedding_work and shared FileSystem/EmbeddingFiles contracts. **Produces:** EmbeddingTasks on the existing EmbeddingService, one shared filesystem/lock implementation and an actually wired drain route/trigger; no algorithm fork.

- [x] **Step 1: Verify integration inputs and write RED.** Record the exact combined revision and public TK-13 signatures against its reviewed plan. Confirm no duplicate LocalFileSystem/MarkdownParser/lock helpers survived integration. If its final API differs, update only this bounded integration mapping and its tests/ruling before writing code; preserve accepted behavior and prior evidence. New executable test uses a controlled Ollama or compatible HTTP fixture, valid config, a real indexed knowledge text and empty vectors. Require actual HTTP arrival from a detached product drain and eventual matching-hash vector, then process completion; initial missing route fails. No unit-double success counts here.
- [x] **Step 2: Implement EmbeddingTasks on the existing service.** `has_work` synchronizes readiness through its existing read_configuration; Missing/Invalid yields false, Ready reads one eligible work row for its identity and checked attempt budget, releases the view and returns presence. Identity change returns false; genuine storage/I/O errors remain typed failures. `drain` calls the existing trait method explicitly as `Embeddings::drain(self)` (not a recursive `self.drain()`), mapping Complete/AlreadyRunning/Unavailable/ConfigChanged to clean no-work termination and unexpected failures safely. Do not acquire embed.lock in this wrapper. In EmbeddingFiles::try_lock_embed, delegate to the shared EmbedLocks capability and translate the error, preserving one concrete guard/PID implementation.
- [x] **Step 3: Wire real confined dependencies in lib.rs.** Reuse Git root discovery and TK-10 open_service_directory/open_confined path validation; construct real SQLite storage, existing provider/parser/filesystem and EmbeddingService. Prepare refreshed graph/FTS before ready work checks using Documents::refresh where the triggering service requires it. Connect real task capability into BackgroundService, keep real check/inspect services absent until their own tasks. Product `embed --drain` now uses that same configured service. Avoid implicit unsafe explicit-path openers that bypass confined automatic storage. No SQLite transaction/connection is inherited across exec or held over HTTP by the foreground. Do not use unavailable read commands as secretly successful embedding triggers; direct drain and a test-only foreground consumer can prove the launch/real executor path, with actual public read-command trigger acceptance added by their owners.
- [x] **Step 4: Run real-drain GREEN and race/error cases.** Build/publish a fixture through the real Documents service in test preparation, invoke the actual product drain with empty stdin, and separately trigger it via production BackgroundService/open_background with the resolved product executable path, not the fixture executable path. Controlled provider barriers prove one active embed owner, no request without config or eligible work, exhaustion not respawned, foreground response/EOF independent of HTTP, final eligible backlog empty, parent loss/death lock release, preserved corpus/cache revision and correctly hash-addressed vectors. Check identity change/late result using TK-13's actual publication guard, invalid config and provider errors without secret leakage; configuration formatting reuses existing JCS. Use a fixture foreground consumer only to trigger the production launcher, never as the embedding executor. Rerun the changed combined root gate and dedicated host/Linux runner at new evidence paths; keep Task 4 receipts historical.
- [ ] **Step 5: Complete acceptance and commit.** Map each corrected TK-11 checkbox to an actual command/receipt and the reviewed combined revision. Do not mark real inspect/check complete here; name the owning tasks. Run final architecture/API/privacy review of changed files and the selected execution workflow's whole-branch review. `git commit -m "feat(cli): connect production embedding drain"` for intended paths. Update Engram with decisions/limits/current artifacts; merge/push/issue closure only on user authorization.

## Coverage and completion ledger

### Task 5 execution evidence and integration rulings — 2026-10-22

### Accepted verification policy update — 2026-10-23

The user replaced AGENTS after review of `4030714`. Fix round 1 applies the new
mandatory `make verify` (debug check, strict Rustdoc, release tests), whole-crate
cargo-mutants after storage integration, and host/Linux storage/native acceptance.
The two historical compiling controls remain useful but do not replace mutation
testing. No waiver is granted. Exact survivors must be killed by meaningful tests
or individually justified and narrowly excluded with comments; defaults determine
timeouts, and caught/missed/timeout/unviable are recorded for the actual scope.

Before mutation execution, bounded caller ordering and helper contracts were fixed,
the historical host nested count corrected to eight, and a minimal worktree-owned
Makefile became the single command source. The old script delegates to `make check`.
The primary checkout's other-writer workflow/Makefile was not imported. First
`make verify` passes on the policy tree; final mutation/platform acceptance remains
pending and receives new receipts. Both accepted integration rulings below remain.

Authorized completed TK-13 `06dc8a18` was merged separately as `f5611e2`; product
integration is `521276a`. Steps 1–4 are implemented and verified. Step 5 remains
open only for independent Task 5/whole-branch review and controller acceptance;
the implementer has committed the source and completed its architecture/API/privacy
self-review. Controller owns progress/Engram; no final main merge, push or issue
closure is included.

The [combined report](../../zkit/letopis/checks/TK-11/report.md#task-5-combined-production-drain--2026-10-22)
maps actual APIs, all criteria and raw receipts. Both full host/Linux root gates
pass 348 root tests (four ignores individually explained), plus twelve real-drain
scenarios. Final manifests share 124 input hashes; post-run documentation overlay
preserves every technical input. Historical Task 4 and TK-13 evidence remains pinned.

Accepted integration rulings: reject non-private hardlinked lock descriptors before
PID mutation and after ownership, retaining ordinary hardlinked document reads;
preserve lazy service-directory creation for ordinary/unavailable composition while
embedding construction explicitly pins it. One OnceCell<File> is necessary because
existing &self operations initialize and retain that descriptor. A lock-specific
single absence observation permits legitimate peer creation; CREATE|EXCL, no-follow,
opened identity and existing deletion/replacement checks remain. Config/document
reads retain their stable-observation rules.

Bounded layout adjustments: `tests/cli_drain.rs` and `tests/support/drain.rs` contain
cohesive actual-product scenarios; previous inline filesystem tests were extracted
unchanged to `filesystem/document_tests.rs`. Runner/Docker inventories include all
root tests for combined Linux verification. Deferred Minor M1 is resolved by Cargo
JSON artifact paths and explicit-target compiling controls.

| Spec requirement | Owning task |
|---|---|
| Strict argv/stdin, catalogue envelopes, safe errors, one output, unavailable routes | 1 and actual executable 4 |
| Typed handler calls; no direct technology access, no fake business success | 1/4; actual inspect/check later12/19 |
| Persistent std locks, PID-after-ownership, no unlink, confined inode checks | 2, cross-platform4 |
| Eligible-work/probe/release/spawn sequencing, session/null fds, parent independence | 3/4 |
| Real available configuration, real compute/drain, identity/hash/attempt reuse | Gated5 using completed13 |
| Refresh without nested index.lock, actual component wiring, thin main | 2/4/5 |
| Lost response/idempotence, corpus/user Git state preserved | 4/5, real read semantics12/19 |
| Platform inputs, failure controls, accurate evidence/limits | 4/5 |

The user approved the plan and selected Subagent-driven execution on 2026-10-22; execution is active. Tasks 1–4 can be implemented independently of unfinished read algorithms and TK-13. Task 5 and TK-11's final real-drain checkbox remain pending until its integration gate is satisfied. The earlier Native recommendation is superseded by the user’s explicit Subagent-driven selection. Use fresh implementers, task-scoped spec/quality gates and the final whole-branch review.

## Planning self-review

Completed author self-review against every approved spec section: the coverage table assigns all delivered behavior to Tasks 1–5, and read/check semantics remain explicitly with12/19. Every task contains a concrete RED/GREEN cycle and commit boundary; exact consumers/producers use the frozen names. All five Review Focus conditions have owning tests. Corrected three planning gaps before handoff: Task1 introduces the real public library consumer to satisfy strict dead-code checks; process test support resolves/builds the fixture without hidden manual setup; Task5 passes the actual product executable into the reusable launcher rather than accidentally executing a fixture as the drain. JSON Schema validation reuses the existing development validator with prepared offline prerequisites. No source body is prescribed where a signature/test suffices; this plan is about twice the approved specification's word count. No test in this future plan has been claimed executed; only the unchanged pre-planning baseline exists.

## Execution rulings

1. Reject lock descriptors with link count other than one before PID mutation, including revalidation under acquired ownership; leave ordinary corpus reads unchanged. Independent Task2 review found PID truncation could overwrite hard-linked corpus/external bytes despite NOFOLLOW and matching device/inode. This is a narrow safety correction to the planned lock-specific validation. Cost if wrong: deliberately hard-linked lock setups fail safely and require separate private lock files. Implemented and independently re-reviewed in `1a7df6a`: four behavioral RED failures, 25 focused GREEN and 206 root tests PASS; safe ordinary document hardlink reads remain supported. Linux verification remains Task 4.


### Task 4 execution evidence — 2026-10-22

Task 4 executable/transport/process acceptance is implemented and verified; see
[the runtime report](../../zkit/letopis/checks/TK-11/report.md). Root gate: 224 PASS,
0 failures, 2 parent-driven ignored helpers; rustdoc PASS; Python runner 7 PASS.
Host/Linux ARM64: 88 matching input hashes, 31 CLI unit + 25 filesystem + 5 process
+ 3 executable tests per platform, schema-valid real error outputs and labelled
injected success outputs. Two compiling controls fail the intended observers,
restore exact source and pass. `.dockerignore` was narrowly extended for the new
runner/schema inputs; existing test support emits observer receipts. No fixture
backend exists in product dispatch. Task 5 and real-service idempotency remain
open; TK-11 is partial. Raw commands, hashes, source revision and limits are in the
report. No merge, push or task/issue completion is claimed.


## Independent review and integration gate — 2026-10-22

Task 4 source `a5e2b7319158c707b33f1ab336e203475876d10b` passed independent
specification and code-quality review with no Critical/Important findings. Tasks
1–3 also passed their scoped gates; the final whole-branch review remains pending
with Task 5. Evidence totals and scope above are unchanged.

One Minor finding is deferred to that final review: `tests/cli/negative_controls.py`
currently assumes `target/debug/examples/cli_runtime_fixture`; an inherited
`CARGO_BUILD_TARGET` changes Cargo's artifact directory. Resolve the control's
executable through Cargo JSON. It fails closed and does not invalidate the captured
runs, which used no explicit build target.

Task 5 was not started. The read-only dependency checkpoint found TK-13 at
`00bb62a1e68b6b5e837b2775bdc307b7d8918d18`: its committed card/report still state
`in_progress`, pending its independent Task 5 and final whole-branch reviews.
TK-13 is absent from this integration tree. The approved plan additionally requires
authorized integration and forbids an implicit merge/rebase. Resume after the
completed TK-13 is available through that gate, reconcile the shared filesystem
and service contracts, then execute real-drain and combined-tree acceptance. No
final TK-11 completion or publication is claimed.


## Task 5 fix round 1 — 2026-10-23

Fix review base is4030714; policy/source checkpoint1034d016 retained the completed
TK-13 integration and actual production drain. The immutable original2061-candidate
baseline is complete:550 tool-caught/81 missed/6 timeout/1424 unviable. Its exact
705+1356 disjoint union, metadata, every raw diff and72 unchanged source hashes
are sealed in `.artifacts/tk11/task5-fix1/immutable-baseline-final/`.

All550 tool catches were causally audited.23 have timing-only failure blocks;
four are directly mutation-sensitive (two removed-NONBLOCK FIFO schedules, inverted
setsid success handling, forced unsupported dimension1).19 observer-only cases
(first-stage3+continuation16) retain their original tool classification with causal
credit withheld until final affected validation. The first audit found18; its raw
receipt is preserved and v2 adds the multi-observer-only array-length case.
Four ambiguous tool timeouts each pass every test in isolated single-worker replay
and are MISSED there (0caught/4missed/0timeout/0unviable;270s actual budget).
The original six timeouts remain six; two are causal unbounded initialization loops.

### Accepted fix-round rulings and costs

| Ruling | Accepted decision | Cost / future obligation |
| --- | --- | --- |
| 1 | Require coordination-lock nlink=1 before PID mutation and revalidate under ownership; ordinary corpus hardlinks remain valid. | Intentionally hardlinked locks require private files. |
| 2 | Ordinary/unavailable CLI composition initializes its one service directory lazily; embedding construction explicitly pins it. | Constructor readiness differs; no-directory and replacement regressions remain required. |
| 3 | Cap lints only in disposable mutation copies; production make verify remains strict and whole-scope/time budgets remain explicit. | Mutation copies establish behavioral sensitivity, not production lint cleanliness. |
| 4 | One private generic typed retry helper accepts the operation; production passes File::try_lock and retains WouldBlock hook, PID ordering and RAII. Rerun all final local.rs candidates plus survivor scopes. | One helper and changed inventory require explicit before/after mapping; no byte-identical final-inventory claim. |
| 5 | Checkpoint/interrupt only the owned coordinator, benchmark bounded six-suite modes, and resume the exact unfinished identity partition. Defaults require all12 passes and clear improvement; they were rejected. | Benchmark time and multi-receipt accounting; shared-target timings omit per-copy rebuild cost. |
| 6 | Preserve public Diagnostic spans around the complete offending service comment, without unrelated prefix or multiline truncation. | A future equally useful location change requires intentional contract/test updates. |
| 7 | Isolated integration executable interposes only its own C setsid boundary with async-signal-safe errno failure; prove real warning propagation on both platforms/debug/release. | Platform/linker-specific test coupling; report concrete interception failure rather than excluding the mutation. |
| 8 | Only initialization elapsed<5s to <=5s is contract-equivalent under the per-step retry budget; exact equality may permit one extra retry, and existing1ms sleep can finish beyond5s. | Future strict-cutoff/retry-count requirements need deterministic clock tests and removal of this exact exclusion. |
| 9 | Keep the real five-second SQLite expiration body in an owned30s supervised child, kill/reap only that child on failure, and prove the exact test ran. | Extra child invocation and watchdog maintenance; no clock seam or product behavior change. |
| 10 | Use two mutation workers/two test threads for final affected receipts after actual observer-only noise; preserve exact scope and non-shortening timeouts (actual274s final), isolate ambiguous failures without catch credit. | Longer wall time and replay accounting; no new benchmark or baseline restart. |
| 11 | Use the existing SQLite ffi authorizer on a test-owned connection to ignore one seeded-bad policy setter; validate actual policy, safe failure, non-panicking callback/lifetime and scoped unregister. | Test-only native unsafe/runtime coupling and final strict/platform gates; no new production seam/dependency. |

The interrupted705 terminal outcomes are256caught/39missed/0timeout/410unviable;
continuation1356 is294caught/42missed/6timeout/1014unviable. The benchmark's six
2-thread suites pass348root/0failed/4ignored. Five default suites pass the same;
one default suite fails148passed/1failed/3ignored at the real two-probe observer.
Default mode was only3.2% faster and is rejected. Preserve its original parser
report and use corrected-counts-v2.json; the failed measurement is no mutant catch.
The first non-capped exploratory receipt (6caught/26unviable/6unfinished) remains
separate and provides no union credit. All final unviables are real Rust type or
syntax failures; no resource/link/disk error was silently classified unviable.

### Exact equivalent mutations

The23 decisions below are final current-consumer/source audits, not pending
proposals. Historical proposals remain in survivor-triage.json and the initial
map; final-equivalence-decisions-v3.json records all23 accepted mappings. Only exact
anchored names are excluded in `.cargo/mutants.toml`; no file/function is skipped.
Each disjoint-bit proof applies to the fixed macOS/Linux permission/open constants.
Private diagnostic redaction, URI spelling, read-only extra Git flags and retry
eligibility are contract-equivalent; they are not claimed byte/trace-identical.
Native dependency/build-mode or consumer/contract changes require re-auditing the
relevant exclusion. SQLite3.53.2 comes from libsqlite3-sys0.38.2; the accepted bundled
build excludes system SQLite. Arbitrary build-script/pkg-config overrides are
outside this supported build choice. Pinned source hashes and URI/readonly Git
probes are retained in the fix evidence; canonical Git-root proof includes a
supplied double-leading-separator/PWD alias resolving to the ordinary physical root.

| ID | Exact final mutation | Current-contract equivalence proof |
| --- | --- | --- |
| E01 | `src/utils.rs:65:9: replace <impl Visitor<'de> for JsonVisitor<CHECK_INTEGER_TOKENS>>::expecting -> fmt::Result with Ok(Default::default())` | JsonVisitor is crate-private and consumed only by serde_json deserialize_any; it implements every JSON value kind. expecting is a required visitor formatting hook for unsupported deserializer value kinds, not an application diagnostic. Parsing errors are wrapped by EmbeddingError with fixed public Display/Debug and private causes. No public JSON acceptance, identity, vector, or error category changes. |
| E02 | `src/domain/sha256.rs:23:46: replace \| with ^ in Sha256::parse` | The unchanged precondition accepts exactly lowercase hexadecimal bytes. Each decode result is 0..15, so the shifted first nibble occupies bits4..7 and the second occupies bits0..3. Their intersection is zero for every accepted pair; bitwise OR and XOR produce identical bytes. Rejected inputs return before this expression. |
| E03 | `src/embedding/openai.rs:31:9: replace <impl fmt::Debug for Openai>::fmt -> fmt::Result with Ok(Default::default())` | Openai is crate-private and its custom Debug implementation has no runtime consumer, generic Debug bound, or outward Debug composition (HttpProviders has no Debug implementation). Empty output and the fixed redacted text both satisfy credential nondisclosure; prescribing component-name formatting solely to kill this mutation would invent an incidental contract. |
| E04 | `src/filesystem/local.rs:309:40: replace \| with ^ in LocalFileSystem::finish_lock` | Mode::RUSR and Mode::WUSR are independent POSIX permission bits (0400 and0200). Their bit sets do not overlap, so OR and XOR both produce owner read/write mode0600 on both supported hosts. |
| E05 | `src/filesystem/local.rs:422:42: replace \| with ^ in create_lock_file` | LOCK_FILE_FLAGS contains RDWR/CLOEXEC/NOFOLLOW/NONBLOCK. CREATE and EXCL are distinct nonoverlapping flag bits absent from those base flags; each changed OR therefore has disjoint operands and XOR preserves the exact open flags on supported macOS/Linux. |
| E06 | `src/filesystem/local.rs:422:25: replace \| with ^ in create_lock_file` | LOCK_FILE_FLAGS contains RDWR/CLOEXEC/NOFOLLOW/NONBLOCK. CREATE and EXCL are distinct nonoverlapping flag bits absent from those base flags; each changed OR therefore has disjoint operands and XOR preserves the exact open flags on supported macOS/Linux. |
| E07 | `src/filesystem/local.rs:423:20: replace \| with ^ in create_lock_file` | Mode::RUSR and Mode::WUSR are disjoint owner permission bits; OR and XOR both produce0600. |
| E08 | `src/filesystem/read.rs:31:70: replace \| with ^ in read_regular` | The read flags RDONLY/NOFOLLOW/NONBLOCK have no CLOEXEC bit set. CLOEXEC is a separate open flag, so XOR and OR produce the same descriptor flags on supported macOS/Linux. |
| E09 | `src/filesystem/read.rs:31:51: replace \| with ^ in read_regular` | RDONLY is zero; NOFOLLOW/NONBLOCK/CLOEXEC are separate nonoverlapping open flags. The exact OR expression has disjoint operands, so XOR produces the same flags on supported macOS/Linux. |
| E10 | `src/filesystem/read.rs:31:32: replace \| with ^ in read_regular` | RDONLY is zero; NOFOLLOW/NONBLOCK/CLOEXEC are separate nonoverlapping open flags. The exact OR expression has disjoint operands, so XOR produces the same flags on supported macOS/Linux. |
| E11 | `src/git/process.rs:109:65: replace == with != in GitProcess::build_command` | The first argument for hash-object is itself different from -w, so any != -w is always true. Existing writable hash-object calls still receive exactly the same fsync settings; the only changed call is read-only independent rehash without -w, which receives extra core.fsync settings but never writes an object. The private process adapter has no externally supplied argv or argument observer. |
| E12 | `src/storage/connection.rs:65:33: replace \| with ^ in ConfinedDirectory<'a>::open_database` | The base database flags do not contain SQLITE_OPEN_NOFOLLOW, a distinct bit. The directory RDONLY(0), DIRECTORY, NOFOLLOW and CLOEXEC operands are distinct bits on supported macOS/Linux; OR and XOR produce identical effective flags at each exact expression. |
| E13 | `src/storage/connection.rs:110:67: replace \| with ^ in ConfinedDirectory<'a>::verify` | The base database flags do not contain SQLITE_OPEN_NOFOLLOW, a distinct bit. The directory RDONLY(0), DIRECTORY, NOFOLLOW and CLOEXEC operands are distinct bits on supported macOS/Linux; OR and XOR produce identical effective flags at each exact expression. |
| E14 | `src/storage/connection.rs:110:48: replace \| with ^ in ConfinedDirectory<'a>::verify` | The base database flags do not contain SQLITE_OPEN_NOFOLLOW, a distinct bit. The directory RDONLY(0), DIRECTORY, NOFOLLOW and CLOEXEC operands are distinct bits on supported macOS/Linux; OR and XOR produce identical effective flags at each exact expression. |
| E15 | `src/storage/connection.rs:110:28: replace \| with ^ in ConfinedDirectory<'a>::verify` | The base database flags do not contain SQLITE_OPEN_NOFOLLOW, a distinct bit. The directory RDONLY(0), DIRECTORY, NOFOLLOW and CLOEXEC operands are distinct bits on supported macOS/Linux; OR and XOR produce identical effective flags at each exact expression. |
| E16 | `src/storage/connection.rs:180:42: replace < with <= in wait_for_initialization` | Only exact elapsed==5s differs, permitting one extra retry. Existing per-step budget already sleeps1ms after eligibility and may finish an operation after5s; accepted TK10 contract is not a hard nanosecond cutoff. |
| E17 | `src/storage/connection.rs:327:41: replace \|\| with && in attach_cache` | alphanumeric and unreserved/slash sets are disjoint, so AND percent-encodes all bytes. The only consumer is SQLite URI parsing: locked sqlite3.c decodes %HH before VFS use, mode=ro remains separate, canonical absolute callers prevent authority differences. Same pinned SQLite3.53.2 probe confirms identical filename/value and rejected writes for both encodings, including delimiters/UTF-8. No formatting contract for private URI. |
| E18 | `src/storage/connection.rs:411:35: replace < with == in verify_capabilities` | Cargo pins bundled libsqlite3-sys0.38.2, whose SQLite3.53.2 is strictly above required3.51.3. Both comparisons remain false for the supported runtime. No external/unbundled version consumer; revisit exact exclusion on native dependency/requirement changes. |
| E19 | `src/storage/connection.rs:411:35: replace < with <= in verify_capabilities` | Cargo pins bundled libsqlite3-sys0.38.2, whose SQLite3.53.2 is strictly above required3.51.3. Both comparisons remain false for the supported runtime. No external/unbundled version consumer; revisit exact exclusion on native dependency/requirement changes. |

| E20 | `src/git/process.rs:109:40: replace && with \|\| in GitProcess::build_command` | For current private argv callers, mktree and update-ref retain fsync settings, hash-object -w retains fsync settings, and no other command carries -w. The changed conjunction therefore only adds core.fsync/core.fsyncMethod to the independent read-only hash-object call without -w. That command performs no object write and returns the same independently recomputed OID; SHA1/SHA256 repository state/argv-consumer proof is preserved in hash-object-readonly-equivalence.json. GitProcess has no public arbitrary-argv or argument observer. Reaudit if any future caller passes -w to another command or exposes command formatting. Cost: Read-only command invocation differs; future argv consumers or flags require re-auditing this exact exclusion. |
| E21 | `src/service/embedding_error.rs:77:9: replace <impl fmt::Display for PrivateCause>::fmt -> fmt::Result with Ok(Default::default())` | PrivateCause owns the original error but exposes no payload or source chain. Its only outward path is EmbeddingError::source as dyn Error; current production consumers use the public EmbeddingError kind/Display/Debug, with no PrivateCause formatting consumer. The public type promises a private non-formatable cause, not a marker string; both the fixed redacted marker and empty formatting preserve nondisclosure and source presence. The redaction integration test formats the wrapper only to assert no secret and no nested source. Public EmbeddingError Display and useful Debug classification are unchanged and separately tested. Cost: Wrapper text is contract-equivalent, not byte-identical; a future public marker-text requirement requires removing this exact exclusion. |
| E22 | `src/service/embedding_error.rs:82:9: replace <impl fmt::Debug for PrivateCause>::fmt -> fmt::Result with Ok(Default::default())` | PrivateCause is an opaque cause wrapper with no useful-classification formatting contract. No production Debug composition consumes this private implementation; EmbeddingError Debug independently prints its stable kind and is strengthened by a public positive assertion. Empty PrivateCause Debug preserves the documented non-formatable/private payload contract, the retained source wrapper, and nondisclosure. The sole formatting consumer is the integration redaction assertion; requiring incidental wrapper marker text solely to kill this mutation would add a new contract. Cost: Wrapper debug text changes; future diagnostic-format guarantees or consumers require re-auditing this exact exclusion. |
| E23 | `src/service/embedding_config.rs:41:13: replace \|\| with && in EmbeddingConfig::parse` | JsonValue rejects duplicate keys and ROOT_FIELDS lists exactly the four mandatory provider/model/parameters/max_retries keys. With all four present and no extras both predicates accept; any extra with all mandatory keys has length>4 and unknown=true, so both predicates reject. At length4 an unknown key necessarily replaces a mandatory key; at length<4 or only mandatory-key subsets a mandatory key is missing. The mutated early check may let these inputs reach an individual required-field lookup, but each missing field rejects with the same fixed InvalidConfig and no cause. Before any missing max_retries lookup, parameter validation/read_dimensions also only use the same cause-free rejection. No canonicalization, I/O, result identity, retained cause, or accepted configuration changes. The only difference is private rejection control flow; error text is fixed. Reaudit if fields become optional or a later validation gains side effects or distinct public error classification/cause. Cost: Early rejection path differs; future optional fields or diagnostic/side-effect requirements require removing this exact exclusion. |

### Historical changed-tree verification checkpoint before the259 run

Strict `make verify V=1` passes388root/0failed/4ignored in both debug and release,
with8 emitted nested passes per profile and two additional supervisor-owned child
invocations whose stdout is captured. The interposed setsid failure test passes in
both profiles on macOS; Linux execution remains a final platform gate. One failed
first strict attempt found duplicate test-module loading; the corrected shared
cfg(test) supervisor avoids production coupling. The negative unmatched-filter
regression is RED then GREEN; the approved retry helper API is RED then GREEN.
A stale copied timestamp on the first supervisor GREEN attempt is preserved;
refreshing timestamps proves actual recompilation in supervisor-green2.log.
Python runner checks pass11CLI/10storage, including quiet/V1/failed-step output.

Final unfiltered inventory is2063; all1880 nonlocal mutation metadata, expression
bytes and enclosing-function bytes equal1034d01.44 serialized diff contexts change
only because adjacent cfg(test) content changed. Whole-file hashes are unchanged
for57 source files;15 existing source files differ (14test-only, one production
retry refactor), plus one new test-only module. Final local.rs has183 candidates
versus181 originally. Its full universe is retained, with four exact equivalents;
all179 eligible local candidates are in the259-record affected selection, together
with all meaningful original nonlocal misses, all19 withheld cases and all6timeouts.
The tool's list exactly equals those259 complete final candidate records. Actual
baseline auto-timeout is274s (floor270); two workers/two test threads are running.
Technical hashes are frozen in affected-inputs-before.json. Final mutations,
host/Linux native/CLI evidence, source commits and independent reviews remain open.
No final acceptance, merge, push or GitHub operation is claimed.


Ruling11 accepts narrow real SQLite authorizer fault injection in a test-owned
connection through existing ffi, with no new feature/dependency or production seam.
Seed one bad foreign_keys/synchronous value, ignore only that PRAGMA setter, keep
WAL and the other policy valid, and assert safe failure plus actual observations.
The callback must validate action/non-null arguments, never panic/unwind, retain a
static context with documented unsafe lifetime, and unregister through scoped
cleanup. Cost: native unsafe/runtime coupling plus strict final changed-tree gates.
This addresses the affected-run miss at connection.rs:379:38. The initial triage
incorrectly attributed both OR mutations to the WAL-only scenario; the actual last
OR mutation preserves journal rejection and weakens only sync/foreign rejection.
The active259 run stays unchanged; apply the regression afterward and replay this
exact candidate with minimum timeout274s, retaining the original miss classification.

### Final mutation resolution — 2026-10-23

The sealed259-record affected receipt is102caught/6missed/0timeout/151unviable,
with exact selected metadata/raw diffs and unchanged242storage/131CLI input hashes.
Every unviable has a real Rust type/syntax error. All102 catches have mutation-
sensitive failures: the sole timing-only catch removes O_DIRECTORY and blocks the
controlled FIFO open, so its verified30s owned-child watchdog is directly causal.
Two resource-keyword hits are expected native WouldBlock assertions after removing
unlock/forcing successful lock acquisition; they are not resource exhaustion.
No unrelated observer-only catch or unresolved infrastructure case remains here.

The six actual misses are retained. E20–E23 above resolve four as exact equivalents.
Two meaningful gaps receive tests: a real SQLite authorizer ignores only one
seeded-bad policy setter, then configuration must reject while WAL/other policy
remain valid; a public parse accepts valid nested/empty arrays, preserves execution
parameters and matches independently specified canonical JSON bytes/hash. Each
focused unmutated test passes; an exact two-candidate replay catches both mutations
(2caught/0missed/0timeout/0unviable), with12s build+54s baseline and actual274s timeout.
Its failures are the new policy unwrap_err on Ok and allowed array rejected as
InvalidConfig, not unrelated timing failures. No production seam/dependency changed.

The final tree has2063 unfiltered candidate records byte-identical to the259-run
inventory,23 exact equivalents and2040 default-eligible candidates. Only the two
test-only files and mutation-policy exclusions changed after259. All183 final
local.rs candidates are accounted for:179 executed plus4 narrow equivalents.
The1880 nonlocal production-expression/enclosing-function correspondence to1034d01
remains valid; changed whole-file test bytes are not called identical. Raw259 plus
focused2 receipts cover259 distinct affected identities, resolving104caught/
4equivalent/151unviable; this is not another full-crate execution. The full original
2061 baseline remains550tool-caught/81missed/6timeout/1424unviable.

`final-original-gap-resolution-map.json` preserves each original identity and its
resolution:81 misses→62caught+19equivalent;19 withheld observer-only catches→17caught+
2equivalent;6 timeouts→4caught+2equivalent. The two real init loops remain historical
270s timeouts even though the finite nonretryable test now catches them. Four late
ambiguous timeouts first replayed0caught/4missed/0timeout/0unviable on unchanged1034;
subsequent regressions/equivalence audits resolve them without laundering baseline
classifications. The earlier18-case audit and failed default-harness benchmark are
preserved; corrected receipts say19 withheld and148passed/1failed/3ignored for that
partial benchmark. Later timeout floors preserve actual253→270→274s progression.

Technical commit `c81f5285344fe67b31eb2f17c932fc4f69974a91` contains34 intended paths.
The host native runner captured dirty1034d01; every one of its242 technical hashes
matches the committed blobs exactly (`technical-commit-correspondence.json`). This
content mapping does not relabel the original source-capture log. Its explicit
make verify V=1 exits0 with390root/0failed/4ignored per debug/release profile,
8emitted nested passes each and2additional captured supervisor children. Root
normal ignores remain3parent-driven child entries plus the real provider deadline.
The host runner separately selects the current root-manifest production provider
deadline:1pass/0fail/0ignored,39filtered,60.01s libtest elapsed; production elapsed
is asserted59s<=elapsed<64s against a delayed65s response. Historical standalone
manifests are not substituted or claimed rerun. Final Linux/native and CLI gates
are recorded separately below. Independent fix/whole-branch review
and controller acceptance remain required; no main merge/push/GitHub/Engram write.

### Final platform gates and limits

All four commands below exit0 with PASS manifests. Each explicitly executes
`make verify V=1` (exit0): formatting, all-target compiler/Clippy with -D warnings,
390 root passes/0 failures/4 ignores in each debug/release profile, and strict
Rustdoc. Each profile emits8 nested one-pass summaries; two additional supervised
children (FIFO and real initialization expiration) capture their output and are
reported separately. The child supervisor verifies the exact case ran, not merely
exit0. The executable-local setsid failure interposition passes macOS/Linux in both
profiles. CLI runners additionally pass12 dedicated real-drain scenarios each,
validate handler/product envelopes and preserve real process/null-fd/lock evidence.
Each CLI platform includes five expected product-error exits1; successful driver
exit0 means every command matched its expected exit, not that all child exits were0.
Python workflow checks pass11CLI/10storage; exact quiet/V1/failure receipts prove
one ok line by default and complete failing-step output without full-suite repeats.

```sh
.artifacts/tk11/python-env/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk11/task5-fix1/storage-host-final
.artifacts/tk11/python-env/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk11/task5-fix1/storage-linux-final
.artifacts/tk11/python-env/bin/python3 -B tests/cli/run.py --platform host --evidence .artifacts/tk11/task5-fix1/cli-host-final
.artifacts/tk11/python-env/bin/python3 -B tests/cli/run.py --platform linux --evidence .artifacts/tk11/task5-fix1/cli-linux-final
```

| Runner | Result | Outer manifest SHA-256 |
| --- | --- | --- |
| storage host | PASS; 390/0/4 debug and release; 8 emitted nested each | `b1275861497ae4a9fd4a5fe764c72aa28b3b8fcf72d630ad1619d0ee761c5fcf` |
| storage linux | PASS; 390/0/4 debug and release; 8 emitted nested each | `25bfd108659509ccbbb00d40291664a8b7d7a0f26432d0d3b89138c34e58bd88` |
| cli host | PASS; 390/0/4 debug and release; 8 emitted nested each | `1a2da675410b0e205dfa8511d60df91cdbb9a5595759edb57d1d5ffe6de9f11e` |
| cli linux | PASS; 390/0/4 debug and release; 8 emitted nested each | `9be75ef2481adba787da1c53c4982427480ee5ecac9b78627157ea61ab3b8447` |

Evidence lives under `.artifacts/tk11/task5-fix1/`; Linux checked manifests are
`storage-linux-final/container/manifest.json` SHA-256
`8f468669301f937b5d85fcf87a2a4748a8700d8d8f6b44e75ba720161a111c1a`
and `cli-linux-final/container/manifest.json` SHA-256
`80bbc9fec039c911e03f68bfb5b733a845e882054c9f3824f4eca55ae06d503f`.
Storage host/outer inventories contain242 entries; the container's132 entries are
an exact shared subset, with110 host-only historical/runner inputs. Every common
hash matches, including all new root tests, cfg(test) support/git.rs, production,
Makefile and mutation policy. This is not a claim of242==132 equality. CLI host,
outer and container inventories are exactly equal at131 entries. All technical
bytes still match c81f528; the later four task-document edits are outside those
inventories and do not change source/test/build inputs. Docker image preparation
uses apt/rustup/pip and cargo fetch --locked as applicable; only actual checks are
locked/offline and runtime containers use --network none. Image builds are not
labelled offline.

The storage runners separately execute the current root-manifest command:
`cargo test --locked --offline --test embedding_provider attempt_obeys_real_sixty_second_deadline -- --ignored --exact --nocapture`.
Both pass1/0/0 with39 filtered; libtest elapsed is60.01s host and63.55s Linux.
The test calls the actual open_embedding_provider and asserts product elapsed
59s<=elapsed<64s against a65s delayed body; the printed suite duration is recorded
separately from that assertion. The captured test source SHA-256 is
`690dea276ae596c25d16f2f551b281543e00071378a1b3c1d0c471c92baaaa16`.
This is a new current-product proof, not reuse of a matching historical test name
or separate manifest. CLI runners and normal root tests retain its ignore.
Unchanged historical standalone harnesses were not rerun; no duplicate deadline
execution was added outside the required storage runners.

Limits remain controlled loopback providers (not model quality), macOS/Linux
ARM64 only, no Windows/NFS/x86_64/power-loss/target-resource or handled TK-29 SIGINT
claim. Real inspect/check semantics remain TK-12/TK-19. Independent fix review and
whole-branch review, final acceptance/ledger and Engram are controller-owned and
pending. No main merge, push, GitHub action or other-worktree write was performed.
