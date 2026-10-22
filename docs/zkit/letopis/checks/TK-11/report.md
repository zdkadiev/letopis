# TK-11 runtime acceptance — 2026-10-22

Current implementation checkpoint: [Task 5 combined production drain](#task-5-combined-production-drain--2026-10-22)
is implemented and verified on macOS/Linux ARM64; independent Task 5 and whole-branch
reviews remain pending. The Task 4 material below is historical and retains its
original source revisions and meaning.

**Historical Task 4 status: implemented and verified; TK-11 was partial.** Real drain
acceptance stays open for Task 5 after authorized integration of completed TK-13.
Real inspect/check semantics remain TK-12/TK-19. No task/issue completion, merge,
push, or integration of the parallel worktree is claimed.

## Captured source and implementation

Source base/HEAD before this Task 4 commit:
`1fdd1f36e745416aaa3f12c5dd7b06a1e432bfd2`, branch `TK-11`, worktree
`/Users/zdkadiev/.codex/worktrees/tk-11/letopis`. The receipts describe the actual
modified source, not just that commit: both platform manifests and the controls
inventory contain the same **88 input SHA-256 values**. Source remained unchanged
during each run; the inventories include the X-1 schema, build inputs, test
support and markdown fixtures. `source-dirty.log` retains the working-tree status.

- `src/main.rs`: thin args/stdin/stdout entry point returning `ExitCode`.
- `src/lib.rs`: validates input before looking up cwd/current executable; constructs
  real `LocalFileSystem`, `DetachedProcess`, `BackgroundService` through
  `open_background`, then borrows `Background` into the handlers. Composition
  errors pass through the same response writer exactly once.
- CLI handlers use the existing application traits. No CLI adapter calls,
  fixture executor, environment-selected backend, extra lock, config implementation,
  provider client, daemon or async runtime was added. `current_exe` is the only
  product executable source. Existing hardlink rejection remains unchanged.
- `tests/cli_runtime.rs` checks executable argv/input errors, unavailable commands,
  deleted-cwd errors, and repeated/discarded-response preservation of corpus bytes,
  raw Git index and user refs. Malformed requests do not create `.letopis`.
- CLI unit test rendering and process socket observations now expose explicit raw
  receipt markers under `--nocapture`; production output is unaffected.
- `tests/cli/` owns the focused runner, lifecycle/schema tests, prepared Linux image,
  isolated compiling controls and reproducibility instructions. `.dockerignore`
  adds only its source/schema inputs. No historical evidence was rewritten.

## Verification

All commands ran from the TK-11 worktree. Python below means the isolated
`.artifacts/tk11/python-env/bin/python` (host 3.12.0, jsonschema 4.26.0).

| Command | Result |
|---|---|
| `cargo test --locked --offline --test cli_runtime` before main existed | Expected RED, exit 101: `CARGO_BIN_EXE_letopis` missing |
| Same command after executable/composition | GREEN, 3 passed, exit 0 |
| `./scripts/check-rust.sh` | exit 0; formatting/compiler/Clippy pass; 224 passed, 0 failed, 2 ignored child entry helpers driven by parent tests |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` | exit 0 |
| `python -B -m unittest discover -s tests/cli -p test_runner.py -v` | exit 0; 7 passed |
| `python -B tests/cli/run.py --platform host --evidence .artifacts/tk11/host-task4-b` | exit 0; PASS |
| `python -B tests/cli/run.py --platform linux --evidence .artifacts/tk11/linux-task4-a` | exit 0; PASS; owned container removed |
| `python -B tests/cli/negative_controls.py --evidence .artifacts/tk11/controls-task4-a` | exit 0; both controls compile, fail behavioral oracle, restore exact bytes, then pass |

Both platforms run 31 CLI unit tests, 25 filesystem tests (one parent-driven child
entry helper), five process schedules and three executable tests. Six additional
captured product invocations yield schema-valid errors and exit 1. Successful
check/inspect output and exit 0 come from **injected handler unit tests**, labelled
in `handler-responses.json`; no successful product check/inspect/drain is claimed.
The runner parses X-1 via `json.loads` and uses Draft202012Validator with the
specific command's success/error `$defs`, not the broad catalogue union.

Host is macOS 26.6.2 ARM64. Linux is glibc 2.36 ARM64, kernel 7.0.0-22-generic,
Python 3.11.2, jsonschema 4.26.0. Both use Rust 1.95.0. Linux builds from the current
source with dependencies prepared during image construction; runtime invokes
`docker run --network none`. The image is pinned to the Dockerfile base digest;
`image.json`, Docker build logs and all version logs are retained.

Product artifact SHA-256:

- host: `69e948ae160cf32202b03c47b9f578247b7890d100ffada8cd3846f6a911c16e`;
- Linux: `89397c7a59b777b90ed3a0d9b802a755e52baaaa00981b0476bc35616018392b`.

Each platform receipt copies both actual Cargo-resolved executables and records
their hashes. Exact argv/cwd, input bytes/hash, stdout/stderr and exit codes are
in `manifest.json.commands`; `observers.json` and `runtime.log` preserve native
session/null-fd/descriptor and ownership observations. Platform source inventories
were compared for exact equality, including all 88 keys and hashes.

## Compiling controls and restoration

Controls ran on macOS in a disposable copy of the same input inventory. They never
modified the product worktree. Each mutated fixture builds successfully and the
integration target compiles with `--no-run` before its behavioral test is executed.

1. Replacing null stdout/stderr with inherited descriptors fails
   `detached_child_has_exact_context_null_fds_and_no_service_descriptors` at
   `stdout EOF before child release` (exit 101). This catches an inherited pipe
   held open by a live drain independently of foreground exit.
2. Removing `File::try_lock` exclusion fails
   `two_released_probes_yield_exactly_one_active_drain`: second ownership is
   `Bool(true)` when `false` is required (exit 101).

After each control, original bytes/hash and the full source inventory match
exactly, the fixture is rebuilt and the same observer passes (exit 0). Original,
mutated, restored and fixture hashes, compile logs, RED/GREEN commands and output
are retained in the controls receipt. Only owned peers are terminated/reaped;
lock-file PID text is never used to choose a signal target.

## Receipt locations and hashes

All paths in this table are relative to the ignored `.artifacts/tk11/` directory
in the TK-11 worktree. New evidence directories reject reuse. Earlier
`host-task4-a` is a development checkpoint before the final composition regression
and README; it is preserved but not used as the final source checkpoint.

| Receipt | SHA-256 |
|---|---|
| `host-task4-b/manifest.json` | `3bf32eb92a76d18d811f441cd1d58d5bb34fa092593177b905aeb8c8e72d5391` |
| `linux-task4-a/manifest.json` | `704f70e0c5258aa1e50d407878b83c4cd282a8619f80feb3077514df126fc1c8` |
| `linux-task4-a/container/manifest.json` | `9ceb3a46742c0e12bd73bee9adc92ad04a28d2e446e5f91d0e63e51645671e11` |
| `controls-task4-a/manifest.json` | `33c090d77930b585e0b038e4c1e9c1552ac03f4b84a03c8ea09ba1db56f95399` |
| `task4/check-rust.log` | `51e93295165c6589237abcbb2e79a3d9695651d0adbc0f07f9a1aac974cf9059` |
| `task4/rustdoc.log` | `2a7422589af683c58244a2142407de62306bd3fdda9702b23fdbb2642f8f723c` |
| `task4/runner-tests-final.log` | `3e4a316bdcc7000c1602e4bbb0e82cd2b96b343d1db21b26f2f0c607ae559694` |
| `task4/red.log` | `600d2ca4590c8a25b59676c4063cfed88f54df94ad2c4dbcf6f4243ecf74be27` |

The first extra deleted-cwd assertion used an incorrect expected static input
message; it failed in `task4/composition-tests.log`, was corrected to the existing
`invalid command input`, and passed in `task4/composition-green.log`. This was a
test expectation correction, not a product diagnostic change.

## Architectural review and remaining acceptance

Self-review checked actual imports/visibility and constructor calls: pure domain
unchanged; application interfaces remain service-owned; concrete process and
filesystem behavior stays in its modules; CLI handles typed requests via service
traits; `lib.rs` assembles them; `main.rs` contains no business behavior. Optional
borrows require no new allocation/shared ownership. Ordinary composition errors
retain internal causes and expose only fixed safe messages. The existing bounded
lock acquisition after SIGKILL is retained; socket EOF alone is not treated as
lock release. No scoped correctness findings remain from self-review.

The TK-11 card's transport, process and structural criteria are supported by these
receipts. Its real drain criterion remains unchecked. Repeat/discarded responses
preserve data for the **unavailable routes only**; this does not complete the real
service idempotency/refresh acceptance. Task 5 must verify the real executor,
competing drain and queue exhaustion after TK-13 integration. Handled SIGINT,
x86_64, Windows/NFS, target RAM/latency, real model quality, power loss and historical
standalone proof harnesses were not exercised by this task.


## Independent review and integration gate — 2026-10-22

Task 4 source `a5e2b7319158c707b33f1ab336e203475876d10b` passed independent
specification and code-quality review with no Critical/Important findings. Tasks
1–3 also passed their scoped gates; the final whole-branch review remains pending
with Task 5. Evidence totals and scope above are unchanged.

Historical Task 4 review follow-up: one Minor finding was deferred to final review:
`tests/cli/negative_controls.py` assumed `target/debug/examples/cli_runtime_fixture`;
an inherited `CARGO_BUILD_TARGET` changes that directory. It failed closed and did
not invalidate those no-explicit-target receipts. Task 5 below resolves M1.

At that historical checkpoint Task 5 had not started. TK-13 was
`00bb62a1e68b6b5e837b2775bdc307b7d8918d18`, still pending its independent reviews,
and absent from the integration tree. Authorized integration of its completed
implementation was required. That dependency was subsequently satisfied below;
the earlier evidence is not reclassified.

## Task 5 combined production drain — 2026-10-22

**Implementation verified; independent Task 5 and whole-branch acceptance pending.**
The final TK-11 checkboxes remain open until those reviews. Product inspect/check
are still unavailable and remain owned by TK-12/TK-19; the fixture foreground is
an integration consumer of the real launcher, never the embedding executor.

### Source and integration mapping

- TK-11 pre-merge base: `382dad5f986bedc43247c8de512728752eb01bce`.
- Completed, authorized TK-13 input: `06dc8a18b117759d15507dd49f9922387488768d`;
  its reviewed runtime is `bcd405c`. TK-13's historical receipts are unchanged.
- Separate merge: `f5611e253baa6a506b41f059f66dfcd6e597354d`, preserving both parents.
  The merge baseline passed the full host gate: 333 root tests, zero failures,
  four ignored entries; eight additional filtered nested passes are separate.
- Product implementation: `521276a5bf1fb67c04d744413a0f7368ca2d5d2b`
  (`feat(cli): connect production embedding drain`). Evidence names the merge
  base plus the exact dirty source inventory; every tested technical input is
  identical to this implementation commit. Documentation overlay is detailed below.

The integrated public contracts remain `Embeddings::read_configuration() ->
EmbeddingResult<ConfigState>`, `Embeddings::drain() -> EmbeddingResult<DrainOutcome>`
and `StorageRead::read_next_embedding_work(identity, max_attempts, after) ->
StorageResult<VectorOutcome<Option<EmbeddingWork>>>`. `EmbeddingTasks` is implemented
on that same `EmbeddingService`: readiness synchronizes the existing configuration,
reads one eligible durable row and releases its view; missing/invalid configuration
and changed identity mean no work. Storage failure remains a safe typed error.
The drain wrapper explicitly invokes `Embeddings::drain(self)` and maps Complete,
AlreadyRunning, Unavailable and ConfigChanged to clean completion without acquiring
another lock. No config/JCS, HTTP, retry, selection or vector-publication algorithm
was copied into the CLI runtime.

`open_embedding_tasks` discovers the Git root and uses the pinned service directory
with `SqliteStorage::open_confined`. `run_cli` binds only the real drain; unavailable
read commands never refresh or launch. The foreground test consumer refreshes via
`Documents::refresh`, then invokes real readiness and `open_background` with the
Cargo-resolved product executable. Product executable discovery remains exclusively
`current_exe`; the fixture override is test-only. The direct drain consumes the
already published projection and owns no index lock. HTTP executes outside SQL
transactions, demonstrated by publishing text and resetting identity while the
provider response is blocked.

### Conflict resolutions and accepted rulings

All eleven conflicts were reconciled by responsibility, not by choosing one branch
wholesale: both dependency/runner input sets; both application model/trait sets;
one Markdown codec with TK-13's public parser type; both constructors in `lib.rs`;
TK-10's confined SQLite initialization with TK-13's read-only cache attachment and
work/pruning operations; one concrete filesystem and one persistent guard.

The filesystem retains TK-11 std lock/try_lock, EINTR handling, PID-after-ownership,
no unlink, one-shot macOS creation fallback, no-follow/nonblocking/CLOEXEC flags,
inode checks and hardlink protection. TK-13 confined config/document reads,
replacement/deletion checks and restrictive service modes remain. Its
`EmbeddingFiles::try_lock_embed` delegates to `EmbedLocks`, preserving one guard.

Two controller rulings are carried into this persistent record:

1. Reject lock descriptors with link count other than one before PID mutation,
   including revalidation under acquired ownership. Ordinary hardlinked corpus
   reads remain valid. A deliberately hardlinked lock setup fails safely and needs
   separate private lock files.
2. Ordinary/unavailable CLI composition retains lazy service-directory creation;
   the embedding constructor explicitly establishes its pinned directory before
   returning. One `OnceCell<File>` retains the directory because the existing
   filesystem operations initialize through `&self`; this is descriptor lifetime
   state, not a second filesystem/cache subsystem. No-directory and replaced-directory
   regressions both pass.

The merged explicit storage fixture initially left cache WAL sidecars because its
read-only cache attachment outlived the last writable cache connection. Field drop
order now closes that attachment first. New regressions verify confined vector
pruning, read-only cache access, final sidecar cleanup and a symlink introduced
immediately before cache ATTACH. The symlink safety oracle was retained unchanged.

The first concurrent-creation test hung because an error before its second barrier
stranded the peer. Assertions now occur after the barrier while retaining the guard.
A later host gate exposed the exact error: the config-read helper required two
stable absence observations, so legitimate first-lock creation between them failed.
Lock opening now uses one no-follow stat, followed by CREATE|EXCL and opened-inode
checks. Observed existing deletion/replacement still fails; config/document stable
reads are unchanged. A deterministic peer-creation schedule and both original
mutation/deletion suites pass. This correction belongs to the implementation commit,
not the earlier merge baseline.

Auto-review rejected the initial broad scripted rewrite and two larger filesystem
resolution patches because preservation was not sufficiently visible. No rejected
write executed. Small field/method patches first preserved both contracts, then
removed exact superseded definitions. A module patch was also rejected for apparently
removing `mod local`; showing the complete retained declaration resolved it. No
human permission workaround or out-of-worktree writes were used.

### Product behavior and validation

The twelve `cli_drain` scenarios use the actual product executable and real Documents,
SQLite, provider/parser/filesystem and background composition:

- Detached HTTP arrival, matching-hash stored vector, empty eligible backlog and
  process completion; competing product exits while the first HTTP call is blocked.
- Foreground response/EOF before HTTP completion; killed foreground does not cancel
  detached work. Direct EOF drain exits 0, and repeating after discarded stdout
  preserves corpus bytes, Git index/refs, cache revision and vectors.
- Missing/invalid configuration or no eligible rows suppresses the lock probe/spawn;
  exhausted attempts do not restart. Formatting preserves existing JCS identity.
- Provider failure consumes the checked durable budget, persists only a safe error
  and does not leak fixture secrets into headers/output. Config reset during HTTP
  discards stale results through the actual TK-13 publication guard.
- New text can publish during HTTP and requires its own hash/vector; old unreferenced
  vectors are pruned. Stale live text is skipped before HTTP/reservation. SIGKILL
  releases the persistent lock and retains the reserved attempt.
- Unsafe automatic database paths fail without modifying external files; unavailable
  check/inspect never trigger otherwise-ready embeddings.

Two additional unit regressions verify readiness performs no provider/document work,
releases its read before another storage unit, and preserves a durable-read error.
The separate `cli_drain`/`support/drain` files keep these scenarios cohesive instead
of enlarging transport-only tests. Prior local filesystem tests were extracted
unchanged into `filesystem/document_tests.rs` under the same module paths; incoming
TK-13 tests remain in `local_tests.rs`. Production filesystem code is about 500 lines.

All commands ran in the TK-11 worktree; `python` below is the isolated
`.artifacts/tk11/python-env/bin/python` (3.12.0, jsonschema 4.26.0).

| Command / receipt | Result |
|---|---|
| `cargo test --locked --offline --test cli_drain -- --nocapture`, before binding | RED 101: actual detached product never reaches provider barrier |
| Same executable oracle after binding and conforming fixture response | GREEN, 1 passed; later expanded to 12 real-product scenarios |
| `python -B -m unittest discover -s tests/cli -p test_runner.py` | 8 passed |
| `python -B tests/cli/run.py --platform host --evidence .artifacts/tk11/task5-host-final` | PASS, exit 0 |
| `python -B tests/cli/run.py --platform linux --evidence .artifacts/tk11/task5-linux-final` | PASS, exit 0; container removed |
| `CARGO_BUILD_TARGET=aarch64-apple-darwin python -B tests/cli/negative_controls.py --evidence .artifacts/tk11/task5-controls-final` | Both compile; intended RED 101; exact restoration; GREEN 0 |
| `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps` | PASS, exit 0 |

Both platform runners execute the complete `./scripts/check-rust.sh` on the actual
combined product: **348 root tests passed, zero failed, four ignored** on each.
Three ignores are explicitly parent-driven child entrypoints (filesystem lock,
document publication, embedding process). The fourth is
`attempt_obeys_real_sixty_second_deadline`: the unchanged TK-13 real 60-second proof
is deliberately not rerun or claimed for this combined revision. Host logs also
contain six successful filtered nested runs; Linux contains eight. They are not
added to the 348 root total. Dedicated observers additionally pass 31 CLI unit,
25 lock, five process, three executable and twelve actual-drain tests per platform.
Successful injected check/inspect output is labelled separately. Actual drain
success envelopes are validated against the exact X-1 command schema.

Host: macOS 26.6.2 ARM64. Linux: glibc 2.36, kernel 7.0.0-22-generic, ARM64,
Python 3.11.2, Git 2.39.5. Both use Rust 1.95.0; Linux image dependencies are prepared
before `docker run --network none`. Each receipt contains exact argv/stdin/exit,
stdout/stderr, versions and executable copies/hashes. The final host, Linux outer,
Linux inner and controls manifests have **124 identical input hashes**.

Product SHA-256: host `5ba9167f76ce61d34decaf6f001bf0a90366185c4dfeb323f2c9a1b1d13cd494`;
Linux `11f666606cea435ed756119b28f525b587ba9407931c9dabaf133f2377aaec87`.
Controls retain their Task 4 behavioral oracles: inherited pipes fail foreground EOF;
removed kernel exclusion admits a second owner. Deferred Minor M1 is addressed:
both normal/negative-control runners resolve fixture paths from Cargo JSON, proven
with an explicit build target and a Python artifact-path regression.

### Receipts, development failures and limits

Paths below are relative to ignored `.artifacts/tk11/`; all raw receipts remain
separate from historical Task 4/TK-13 evidence.

| Receipt | SHA-256 |
|---|---|
| `task5-merge/check-rust-3.log` | `7bd1fc35dc2aff5ef49dcddfc6ff33968f20c6b1a329dc907fb2ba2f087f6409` |
| `task5/real-drain-red.log` | `8851659c7787d5d8ee396fa84514af0ab04a927f96cb8faa0597f6358345e5af` |
| `task5/real-drain-green-2.log` | `29e2965457dc3ec187fa945535faa10e9f3014df8a994f50308b8962bd577bd7` |
| `task5-host-final/manifest.json` | `a3b4acc1f9e2a9f5c4e7d91a6a061e8de4be16ba555987c1c850ef07f1dc7440` |
| `task5-host-final/root-check.log` | `80016d5bb74ddc563c98b881e14dfa900cfec9db3a5aec55574ef8695ab2b498` |
| `task5-linux-final/manifest.json` | `e98f1a7561311a8540fa47cf283587dd5543091e32e2d99bf12f6b103ae7b41c` |
| `task5-linux-final/container/manifest.json` | `d5b7050920d0f0860f503f334e885484aea1a2a03dd2add54637f0bd1fdc0029` |
| `task5-linux-final/container/root-check.log` | `3c8e746f082af49ee3fa4516cbc7c5fed6d465f625f73c4987ce036d9cc08357` |
| `task5-controls-final/manifest.json` | `01182204854fb36852686dab992781286fea94942c7fa278a9af5a1c091bc785` |
| `task5/python-runner.log` | `71a537aef125596c506fc198fe2106030138a73970195a66e4f780d99cfe0e4f` |

Development failures are preserved, not acceptance: merge `check-rust.log` caught
WAL lifetime; `check-rust-2.log` was terminated when the old barrier assertion hung;
`task5-host` caught the actual lock-observation race. Pre-fix `task5-linux` and
`task5-controls` passed but are superseded by final receipts. The initial HTTP
fixture omitted required Ollama `model`; the existing provider correctly persisted
`invalid embedding response` (`real-drain-observation.log`). Correcting that fixture,
not provider behavior, produced `real-drain-green-2.log`. Development Clippy findings
(unused shared test helpers/import and needless owned Output) were corrected before
the final platform gates. No failing/partial receipt is counted as acceptance.

After all four final snapshots completed, documentation alone was updated.
`task5/documentation-overlay.json` records old/new hashes for this report, spec,
plan, card and `tests/cli/README.md`; only that README differs among the 124 captured
inputs. All remaining 123 inputs, including every product/test/build/runner input,
remain identical. Original manifests/hashes were not rewritten. Documentation
consistency/link checks and the overlay verification cover this prose-only delta.

Self-review checked actual modules, visibility, imports, one parser/filesystem/guard,
confined storage and `lib.rs` constructor wiring, thin CLI handlers/main, error/privacy
mapping and synchronous execution. No known scoped correctness issue remains.
Independent Task 5 and whole-branch review are still required. Engram/progress are
controller-owned; no graph mutation, main merge, push or GitHub operation was done
by this implementer. The primary checkout and exact TK-13 worktree were untouched.

Limits: product provider tests use controlled loopback Ollama, not real model quality;
no x86_64/Windows/NFS, target RAM/latency, physical power loss or handled TK-29 SIGINT
claim. Detached completion allows a terminated Linux zombie pending PID 1 reaping;
direct product calls verify exit 0. Unchanged historical harnesses with separate
manifests and the historical 60-second provider deadline were not rerun.
