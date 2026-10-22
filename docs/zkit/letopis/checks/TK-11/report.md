# TK-11 Task 4 runtime acceptance — 2026-10-22

**Status: Task 4 implemented and verified; TK-11 remains partial.** Real drain
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
