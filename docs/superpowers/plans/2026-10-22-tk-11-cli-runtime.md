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
