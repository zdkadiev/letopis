# TK-11 — CLI process, locks and JSON

Status: complete written design approved by the user on 2026-10-22: «Да, утверждаю. Продолжай к плану реализации TK-11 по Superpowers». The branch/worktree, ownership split, corrected task cards and two labelled builder calls are accepted. The [implementation plan](../plans/2026-10-22-tk-11-cli-runtime.md) is prepared and author-reviewed; the user approved it and selected Subagent-driven execution on 2026-10-22. Product implementation is authorized within that plan.

## Intent and starting state

Expose the existing synchronous Letopis application through a short-lived CLI process, preserving the hexagonal boundaries and the X-1 JSON contract. A command refreshes published document metadata where required, calls an application service, emits one JSON envelope and exits. A detached invocation of the same executable drains embeddings without making the foreground command wait for inference. There is no server, persistent worker, process registry or async runtime.

- Branch: `TK-11`.
- Native managed worktree: `/Users/zdkadiev/.codex/worktrees/tk-11/letopis`.
- Base: `12c5ad0c9f25e9a13f41a072fcac9570ff42ae75`, equal to fetched `origin/main` at start.
- Existing product: Markdown/domain, immutable Git snapshots, SQLite storage and document refresh/publication. No product CLI or inspect/check service implementation exists at this base.
- Baseline: `./scripts/check-rust.sh`, exit 0; formatting, compiler, Clippy and 172 passing Rust tests, zero failures. One outer process-test child is ignored by the default harness and explicitly invoked by its parent tests.
- Baseline receipt: `.artifacts/tk11/start/check-rust.log`, SHA-256 `d1d0af4382ddd010ed95bb401cbb502b0f7496774cd4163f494551b58ef1c909`.
- The primary checkout's existing `.gitignore` edit (`skills-lock.json`) and the active TK-13 worktree remain untouched.

## Design authority

Read with:

- [TK-11 task](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-11-cli-runtime.md).
- [Program structure](../../zkit/letopis/architecture.md#program-structure) and [structural review](../../zkit/letopis/architecture.md#program-structure-review).
- [Decisions](../../zkit/letopis/decisions.md), especially D-93 through D-96 and D-98 through D-101.
- [Mechanics](../../zkit/letopis/mechanics.md), especially MC-2 publication and MC-5 process/drain ownership.
- [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md), its [JSON schema](../../zkit/letopis/contracts/schemas/X-1-cli.yaml), and [I-11 diagnostics](../../zkit/letopis/contracts/I-11-diagnostics.md).
- [I-12 snapshot](../../zkit/letopis/contracts/I-12-snapshot.md), [X-3 Git](../../zkit/letopis/contracts/X-3-git.md), and [X-4 Markdown](../../zkit/letopis/contracts/X-4-markdown.md).
- [TK-4 findings](../../zkit/letopis/checks/TK-4/findings.md), [TK-6 review](../../zkit/letopis/checks/TK-6/review.md), and [TK-10 specification](2026-10-22-tk-10-change-detection-design.md).
- [TK-12](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-12-search.md), [TK-13](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-13-embeddings.md), [TK-14](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-14-thin-slice.md), [TK-19](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-19-check-renames.md), and [TK-24](../../zkit/letopis/roadmap/epics/EP-2-knowledge/tasks/TK-24-inspect.md) for ownership boundaries.
- [Agent rules](../../../AGENTS.md) and [development checks](../../development.md).

The canonical design wins over implementation convenience. Keep historical evidence pinned to its original source; baseline tests are not evidence for a new CLI.

## Accepted ownership split

The user accepted: TK-11 builds CLI, JSON and detached process launching; TK-13 owns configuration and the real embedding executor. Connect the completed TK-13 service for actual drain acceptance. Do not duplicate its configuration validation/JCS implementation, HTTP clients, work selection, attempts, vector publication or provider logic in TK-11.

The corrected TK-11 card explicitly reuses config identity from TK-13. Treat this as reuse/integration, with one implementation and one exact dependency version. Do not cherry-pick, merge, rebase or edit the other worktree without corresponding authorization. Main may receive its completed work independently; any combined tree needs its own verification.

A process-fixture test can establish detachment, descriptor isolation and lock exclusion before the real executor is available. It cannot establish that production embedding work drains. Preserve the latter acceptance item as pending until real integration succeeds.

## CLI boundary

`src/main.rs` is a thin entry point into the assembly in `src/lib.rs`. Parse argv with `std`, not clap: command names and their documented flags are argv, the command-specific object is JSON on stdin. Unknown commands, flags, surplus positional arguments, malformed input, extra JSON values, wrong types and unknown object fields fail as `invalid_request` with exit 1. Strict DTOs use serde `deny_unknown_fields`; do not deserialize a general command wrapper absent from X-1.

Implement the current CLI boundary for check/inspect and the internal drain command. Their consumer-driven interfaces and typed transport tests are a TK-11 deliverable; no dummy business service is introduced. Search/traverse, full reindex/status and future lifecycle commands retain their roadmap owners. Do not return empty fake inspection/check results or claim a command is operational merely because its parser accepts it.

Accepted builder call: until a concrete read/check service is wired by its owning task, a recognized and valid check/inspect request fails closed with the existing `internal_error` envelope and a fixed safe message that the operation is unavailable in this build. It never returns `ok:true`, runs a fake business service or creates a new error code. Parsing failures still return `invalid_request` before service dispatch. This is an incremental-build limitation, not a change to the final X-1 contract or acceptance of the missing command. Remove the unavailable route when TK-12/TK-19 supplies the real service. The real embedding route must similarly remain unavailable until TK-13 is connected; a fixture child is test-only.

For public commands, require the documented JSON object; `{}` is the empty request. Do not invent numerical payload limits, a global execution deadline, environment config loading or a help/version protocol as part of TK-11.

Accepted builder call: the internal `embed --drain` invocation accepts empty stdin as the empty internal request because its fd 0 is `/dev/null` by the accepted runtime design. If an explicit JSON body is supplied, validate it as the strict empty object. This exception does not extend to public command payloads.

## Responses and diagnostics

Normal completion emits one complete X-1 JSON value plus newline, with no progress or subprocess output on stdout. `ok:true` has `result` and exits 0; `ok:false` has one `error` with `code`, `message`, `details` and exits 1. Optional nonempty `warnings` contains safe per-call diagnostics; a drain spawn failure is a warning, does not undo the foreground result and is not persisted.

Use the existing error catalogue: `invalid_request`, `not_found`, `validation_failed`, `embedding_failed`, `internal_error`, `cancelled`. Do not add unsupported-command or conflict error codes. Map typed service errors at the CLI boundary; retain useful internal causes without serializing raw knowledge, subprocess/provider payloads, environment keys or authorization headers. No catch-all mapping may turn a real storage/Git failure into a successful empty result.

The response writer owns normal stdout emission. It should be usable by the later TK-29 once-guard/cancellation implementation without introducing a second writer or success-exit path after cancellation. Handled SIGINT and its signal-hook thread remain TK-29; this task does not claim `cancelled` behavior from default signal death. The X-1 guarantee after forced termination remains explicitly limited.

## Refresh, locks and detached execution

Commands whose accepted semantics require refresh call `Documents::refresh` before serving. Keep its locked redetection and atomic publication intact. CLI handlers do not acquire index.lock around a service that already owns it; double locking is not extra safety. Check/inspect output must be produced by services, never by direct CLI access to SQLite, filesystem adapters or Git subprocesses.

Extend the concrete filesystem capability only as required for the two persistent lock files. Use the chosen `std::fs::File::lock`/`try_lock` semantics; retain descriptor-relative safe opening, no-follow/nonregular checks, persistent inode identity, restrictive creation modes and the macOS create-race regression from TK-10. Write the owner's PID only after acquiring the kernel lock. File existence or PID text is not ownership. Release through RAII and never unlink the lock file. Do not change unrelated current file-read safety behavior while replacing the flock call.

For a command eligible to trigger embedding work:

1. Obtain configuration readiness and whether eligible current work exists through the application service boundary, reusing TK-13.
2. If work is absent or configuration is unavailable, do not create useless detached processes or make inference requests.
3. Probe embed.lock with try-lock. If busy, a drain is already running. If free, release the probe before spawn.
4. Launch the current executable with exact argv `embed --drain`, repository-root cwd and inherited environment. Use process APIs, never shell interpolation or a PID registry.
5. In the child, create the detached session using `libc::setsid` and redirect fd 0/1/2 to `/dev/null`. Any pre-exec operation must obey Rust's post-fork safety requirements; report setup/spawn failure safely to the parent.
6. The foreground command does not wait for HTTP or draining. The child independently reopens service dependencies, tries embed.lock, exits successfully if another drain won, otherwise processes work and exits when eligible work is exhausted.

Two successful probes may race; child lock acquisition resolves that race. Do not inherit the parent's SQLite connections or a live lock guard into a drain. Close-on-exec handling and detached process tests must prove the foreground can exit independently and its output pipes reach EOF.

TK-13 owns the drain's lock for its entire execution. Agree the shared capability during integration; do not acquire the same lock again in the CLI handler. No queue/seq table, process daemon, retry scheduler or async runtime is introduced.

## Physical responsibility boundaries

- `domain/` remains pure; do not put JSON/process/I/O state there.
- `service/mod.rs` owns small consumer-driven interfaces and necessary typed requests/results. Concrete orchestration belongs beside its own struct/impl in a responsibility-named file.
- `cli/` owns argv/JSON DTOs, dispatch, handlers and rendering. Handlers call service interfaces. Split files by actual responsibility, avoiding a file for every trivial helper.
- `filesystem/` owns confined paths and persistent lock implementations.
- A process implementation file, placed by its actual runtime responsibility during planning, owns spawn/setsid/null-fd behavior and implements the application interface. It does not invoke storage or provider adapters directly.
- `lib.rs` constructs the concrete adapters, services and handlers; declarations/reexports alone do not count as assembly.
- `main.rs` forwards process I/O/argv into that assembly and returns the selected exit status.

Use the pinned Rust toolchain and project Rust conventions: explicit imports, verb-form methods, meaningful local constants, focused functions aiming at 40 lines and no more than 60, typed failures, minimal visibility and Arrange/Act/Assert tests. Add no speculative future interfaces or generic plugin/framework abstraction. The exact signatures and smallest useful physical file set are builder choices to freeze in the implementation plan; the task ownership below is settled.

## Accepted delivery boundaries

The independent builder check identified a circular ownership statement: TK-11 demanded real inspect acceptance while assigning its algorithm to TK-12, which depends on TK-11. On 2026-10-22 the user explicitly approved correcting the cards and continuing Superpowers with this resolution:

- TK-11 owns CLI/JSON/processes/locks and contract tests that call the service interfaces with clearly labelled test doubles. This does not establish real read/check business behavior.
- TK-12 owns the real basic inspect service and product-binary acceptance: refresh, common Git snapshot, metadata-only output and failures. Full batch/source inspect remains TK-24.
- TK-19 owns full check and its real product-binary acceptance after refresh. Earlier parsing/publication remains TK-7/TK-10 and graph rules remain TK-18. Do not silently move the full checker into TK-11 or claim it complete here.
- TK-13 owns config/JCS/providers/executor. TK-11's real drain acceptance remains pending until the completed service is connected and the combined path is verified. It is not waived or replaced by process fixtures.

All three task cards carry the matching work, acceptance and handoff text. TK-19 explicitly consumes TK-11. No TK-11 dependency on unfinished read algorithms is introduced. The final X-1 wire/behavior contract is unchanged; subsequent owners satisfy the missing production routes rather than inheriting fabricated successful behavior.

## Verification and acceptance

| Area | Required evidence |
|---|---|
| Input/output | Actual executable, one parsed JSON value, correct exit 0/1, malformed/unknown/type/trailing input and argv rejection; output validated against X-1 |
| Service dispatch | Typed requests reach service interfaces; no CLI direct-adapter calls; distinguish injected interface checks from real service execution |
| Real check/inspect | Not a TK-11 success claim: TK-12 performs real basic inspect acceptance; TK-19 performs full check acceptance. TK-11 proves strict parsing, service dispatch, safe envelope and explicit unavailable-route failure before integration |
| Idempotence | Repeat after losing a response preserves corpus bytes and Git user refs/index; unchanged refresh does not republish |
| Locks | Real multi-process index/embed exclusion, occupied probe, owner death, stable lock inode and stale PID, std File lock interoperability on macOS/Linux |
| Spawn | Same executable, setsid/session isolation, null fds 0/1/2, parent output EOF/exit independent of child, spawn failure warning, no shell |
| Real drain | Pending TK-13 integration: missing vectors trigger a real drain; a competing drain exits; queue empties and process exits; no success inferred from a fixture-only child |
| Composition | Actual lib.rs constructor calls, thin main/handlers, inward imports, minimal API, one concrete implementation per responsibility |

Run `./scripts/check-rust.sh` on the changed product tree and focused executable/process checks. Exercise the changed path on macOS and Linux glibc ARM64 using isolated repositories/databases and controlled provider fixtures after TK-13 integration. Prepare dependencies before an offline Linux run. Pin inputs, lockfile, commands, output/exit results and platform limits in `docs/zkit/letopis/checks/TK-11/report.md` and a separate ignored evidence directory. Existing TK-4/TK-6/TK-10 evidence informs the design but is not rerun or relabelled automatically.

Meaningful negative controls should compile and fail at the intended behavioral oracle, then restore exact original source and pass. Prioritize an inherited-output-fd/detachment control and loss of kernel exclusion. No x86, target RAM/latency, actual model quality, physical power loss or full TK-29 cancellation evidence is promised here.

No task is marked done with fake or pending acceptance. Record the actual dependency and remaining work explicitly. The user-approved card correction resolves the builder check's only question. The user approved this full design, including the two labelled builder calls (internal drain EOF and unavailable incremental routes). The implementation plan fixes concrete interfaces/files, RED/GREEN checks and integration evidence, then receives its own review/execution-method selection before implementation. No product code, dependency installation, merge, push or issue publication happens at this brainstorming stage.


## Task 4 implementation checkpoint — 2026-10-22

The executable and real background composition now implement the accepted runtime
boundary. [Acceptance report](../../zkit/letopis/checks/TK-11/report.md): root gate
224 PASS / 0 failures / 2 parent-driven child helpers; host and Linux ARM64 receipts
have 88 identical input hashes; both compiling controls fail their intended oracle
and pass after exact restoration. Actual check/inspect/drain requests still return
explicit unavailable errors; successful injected-handler output is separately
labelled. Malformed input precedes composition; cwd lookup failure uses one safe
envelope. No product fixture switch or duplicate executor was introduced.

TK-11 remains partial: Task 5 real drain integration/acceptance and real-service
idempotency remain open. The runtime fixtures do not waive those gates; real
inspect/check acceptance remains with TK-12/TK-19. Historical baseline receipts
above retain their original revision and meaning.


Task 4 independent review approved specification and quality at `a5e2b73`; one
non-blocking control-runner artifact-path finding is retained in the
[report](../../zkit/letopis/checks/TK-11/report.md#independent-review-and-integration-gate--2026-10-22).
Tasks 1–4 are accepted as scoped increments. Task 5 and the whole-branch gate remain
open: the latest read-only TK-13 checkpoint is not finally reviewed/completed and
is absent from this tree. The accepted integration gate remains unchanged.

## Task 5 implementation checkpoint — 2026-10-22

The historical gate above was satisfied by the user's authorized integration of
completed TK-13 `06dc8a18`; merge `f5611e2` preserves both histories. Implementation
`521276a` binds the existing EmbeddingService to the real drain and uses confined
automatic storage. The [combined report](../../zkit/letopis/checks/TK-11/report.md#task-5-combined-production-drain--2026-10-22)
records the exact API mapping, conflict resolutions and source/evidence hashes.

Both macOS/Linux ARM64 full root gates pass (348 root tests; nested helper counts
separate), plus twelve real-product drain scenarios and compiling controls. No
second configuration/provider/executor or lock implementation was introduced.
Hardlinked coordination files still fail before PID mutation; ordinary hardlinked
documents remain readable. Ordinary composition remains lazy, while embedding
construction explicitly pins its service directory; one OnceCell retains that
descriptor for existing &self operations. These controller rulings preserve both
accepted source contracts. Deferred Task 4 Minor M1 is addressed by Cargo artifact
JSON resolution and an explicit-target control run.

Final Task 5 and whole-branch review remain pending; this is implementation evidence,
not final task acceptance. Real inspect/check remain TK-12/TK-19. The unchanged
historical 60-second deadline proof and separate-manifest harnesses were not rerun.


## Task 5 fix-round verified contract checkpoint — 2026-10-23

The private-lock/lazy-construction contracts above remain unchanged. One private
typed helper preserves Interrupted-only retry, WouldBlock contention, private
causes, PID-after-ownership and RAII unlock. Public invalid-comment coordinates
retain the complete offending comment and omit unrelated prefix text. Public
errors keep useful safe classification/context; opaque private cause wrappers
remain non-formatable. Valid nested-array parameters retain their execution values
and independently specified JCS identity. SQLite policy tests use a scoped native
authorizer, with no production seam, to prove each ineffective setting fails closed.
The real five-second initialization-expiration body is supervised in an owned30s
child; the helper verifies exactly the requested test ran. The retry budget remains
per-step, allowing the documented exact-equality equivalent rather than a strict
nanosecond cutoff. All eleven rulings/costs and23 exact equivalent proofs are in
[the implementation plan](../plans/2026-10-22-tk-11-cli-runtime.md#task-5-fix-round-1--2026-10-23).

Technical commit c81f528 is verified by host/Linux storage/native and CLI runners:
strict make verify V=1 passes390 root tests/0failures/4ignores per debug/release
profile,8emitted nested passes each plus2additional captured supervisor children.
Real setsid failure interposition passes both platforms/profiles; dedicated CLI
real-drain tests pass12 each. Storage runners separately pass the current root
production provider's real deadline test (60.01s host/63.55s Linux libtest elapsed,
product assertion59<=elapsed<64s); normal roots retain its ignore. Separate-manifest
historical harnesses were not rerun. Full original mutation baseline2061 remains
550tool-caught/81missed/6timeout/1424unviable; final259 affected plus focused2 receipts
resolve every meaningful miss/withheld case, with23 narrow equivalents and2063
unfiltered/2040eligible final candidates. These scopes are not conflated.

Independent fix review, whole-branch review and controller acceptance remain
required. Earlier evidence retains its captured revisions/commands, including the
host native dirty1034 capture explicitly content-mapped to c81. No task acceptance,
main merge, push, GitHub action or Engram update is claimed by this implementer.


## Final review tooling checkpoint — 2026-10-23

M1 retargets the compiling kernel-exclusion control to the current real lock
closure while retaining retry_lock usage; every declared source anchor now has a
lightweight exact-match regression. Python12/12 and both isolated compile→intended
RED→byte-exact restore→same-oracle GREEN cycles pass on macOSARM64. No product Rust,
build/native/schema input or accepted runtime contract changed. Two Python tooling
hashes differ from32c9914;241 other prior runtime input hashes remain identical.
The prior runtime receipts are preserved and combined with the new focused receipt,
not relabelled as byte-equal complete current maps. The plan/report retain exact
old/new hashes and scope. All11rulings/costs and23equivalents remain; one scoped
final re-review and controller acceptance are pending, with no broad suite rerun.

## Final acceptance — 2026-10-23

TK-11 is complete and accepted on branch `TK-11` at reviewed implementation
`62f2356143a485852ecb392f35d51c901e7f7536`; runtime Rust/build source is
`c81f5285344fe67b31eb2f17c932fc4f69974a91`. Completed TK-13 was integrated by
authorized merge `f5611e253baa6a506b41f059f66dfcd6e597354d`; no TK-11 merge into
main, push, or GitHub issue update is claimed.

All five plan tasks passed their task reviews. The full branch review
`12c5ad0..32c9914` found no Critical/Important issue and one Minor stale control
anchor. Its single fix wave `32c9914..62f2356` passed the independent scoped
re-review: M1 addressed, no new breakage or out-of-scope observations. All eleven
recorded rulings and their costs remain accepted; no new waiver was introduced.
The twelve explicitly declined broad-review subjects retain their previously
approved roadmap/contract boundaries, including actual check/inspect semantics
in TK-12/TK-19 and handled cancellation in TK-29.

Accepted verification combines the unchanged runtime inputs with the focused
Python correction; it does not relabel historical raw manifests as fresh runs:

- Host and Linux ARM64 storage/native and CLI drivers ran `make verify V=1`: each
  debug and release profile reports 390 passed, zero failed, four ignored entries.
  Eight emitted nested results per profile and two captured supervisor children
  remain separate. Dedicated real-product drain coverage passes 12 cases per platform.
- Native SQLite/vec1, standalone linkage and actual provider-deadline cases passed
  in their retained platform receipts. Evidence covers macOS/Linux glibc ARM64,
  controlled providers and process death; other platforms, live-model quality and
  physical power loss are not claimed.
- The original complete 2061-candidate mutation union remains
  **550 caught / 81 missed / 6 timeout / 1424 unviable**. Final affected 259-candidate
  execution remains **102 / 6 / 0 / 151**, followed by exact two-target replay
  **2 / 0 / 0 / 0**. Distinct affected resolution is 104 caught, four equivalent,
  151 unviable; all original gaps resolve through catches or the 23 individually
  justified narrow equivalents. This is composite evidence, not one final 2063 run.
- Only two Python tooling files changed after the full runtime receipts; all other
  241 of the prior 243 inputs are identical, including every Rust/build/native/schema
  input. New focused evidence has 12/12 Python passes and both actual compiling
  controls: intended RED 101, exact restoration, same-case GREEN 0. This focused
  execution is macOS ARM64 only. Old input maps and source captures remain intact.

The final-fix correspondence remains at
`.artifacts/tk11/final-fix/focused-validation-and-correspondence.json`
(SHA-256 `7e30f54c8d29a13739488d0172ad39648927b74fab05cb10ef69215a3d54585a`).
Final review/ledger/package records are retained in the verified plan-only archive
`.artifacts/tk11/sdd-final-2026-10-23/`; persistent task documents preserve decisions,
verification, equivalences and limits. Review report SHA-256 values:

- `task-5-rereview-1.md`: `0f269739a5d0e3a906c2d7f4f124efcae1420cb85f837be15552c092f91e6435`.
- `final-review.md`: `93d762d85808681fc267a4774f23a4470d0c264359ebef3a660f7abbcb166624`.
- `final-rereview.md`: `542a06aff71df52fbf098c104629b9dd4ab3ee74320ba7e99fa71f816f7e0547`.
