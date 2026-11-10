# Issue 108 reproducible commands

Run from `/Users/zdkadiev/.codex/worktrees/51d9/letopis`, with `login:false`.
Canonical builds use `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target`
and `CARGO_BUILD_JOBS=2`. Ordinary samples run alone, after compilation/tests and
before profiles/instrumentation. No user checkout configuration is changed.

## Full shipping verification

```sh
CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target CARGO_BUILD_JOBS=2 make verify V=1
```

Exit 0; complete output retained at
`.superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/verify-initial.log`.
Debug/example: 1,297 passed, 0 failed, 11 ignored. Release: 1,287 passed,
0 failed, 11 ignored. Includes fmt/check/all-target Clippy/fixed-value lint,
`RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`, and doc tests.
Both original FIFO watchdog tests passed without changes.

## Immutable builds

```sh
CARGO_BUILD_JOBS=2 /opt/homebrew/bin/python3.12 -B .artifacts/issue-108/freeze-before.py
CARGO_BUILD_JOBS=2 /opt/homebrew/bin/python3.12 -B .artifacts/issue-108/freeze-after.py
```

The first command was executed by Task 1 before Rust edits; Task 5 ran only the
second. Each wrapper uses unchanged `compare.freeze_build` with its invocation-local
`build_release` target redirected to the canonical target above. This avoids duplicate
dependency builds and retains input/binary/symbol/original-receipt verification.
The actual build command is `cargo build --release --locked --offline --message-format=json`,
with `CARGO_PROFILE_RELEASE_DEBUG=1`. Both freeze commands exited 0.
The wrappers, complete manifests, source archives, binaries and symbols live under
`.artifacts/issue-108/`. Frozen receipts are `before-build/build.json` and
`after-build/build.json`; all campaigns reverify them before every command.

## Ordinary comparisons

For each `nodes` in `10000`, `100000`, and `variant` in `shared`, `distinct`:

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/historical.py \
  --before /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/before-build/build.json \
  --after /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/after-build/build.json \
  --nodes 10000 --variant shared --stage timings \
  --evidence /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/timings-10000-shared
```

Replace both scale/variant flags and the new evidence path together. The exact
four argv arrays, exits and summaries are recorded in `timings-ledger.json`.
The sequential driver is preserved in ignored `task-5-evidence/run-timings.py`;
its output is `.artifacts/issue-108/timings-driver.log`. Per-stage logs use
`timings-<nodes>-<variant>.log`; each evidence directory contains manifest,
fsynced samples, summary and any failure receipt. Defaults remain 600 seconds
per first publication, 120 per later call, 1,200 per stage; two conditioning
calls and three measured alternating pairs per side and command. Failed
series stop without retries. Timeout/partial runs are not speedups.

## Backend evidence

```sh
cargo tree --locked --offline -e features -i sha2@0.10.9
rustc --print cfg
sysctl hw.optional.arm.FEAT_SHA256
sysctl machdep.cpu.brand_string
```

All exited 0; exact outputs and the selected dependency source are retained in
ignored `task-5-evidence/backend/`. The default/std feature set has no `asm`;
sha2 0.10.9 selects software compression on this aarch64 build. CPU SHA256
support does not imply that this shipping backend uses it.

## Supplemental sharded control and bounded continuation

```sh
/opt/homebrew/bin/python3.12 -B tests/performance/historical.py \
  --before /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/before-build/build.json \
  --after /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/after-build/build.json \
  --nodes 100000 --variant distinct-sharded --stage timings \
  --evidence /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/timings-100000-distinct-sharded
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/run-initialized.py \
  --nodes 100000 --variant distinct-sharded --mode status \
  --evidence /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/timings-100000-distinct-sharded-status
```

The first stage intentionally exited 1 after all three successful check pairs,
before any status attempt. A kqueue watcher verified its owned campaign identity,
waited for six valid measured records and post-fsync log writes, then sent SIGINT.
Its interrupted aggregate summary is preserved; no sample is discarded or relabelled.
The second stage exited 0, with fresh status conditioning and three pairs, no
publication/check retry, and the original 120/1,200-second bounds. Its archived
`driver.py` and manifest establish `continuation_of`, exact input/config/ignore hashes,
historical tree identity and endpoint bytes, projection counts and revision 1.
This explicit scheduling change avoids the setup-plus-calls exceeding one stage.

The earlier original flat 100k stage is separate: baseline publication timeout
600.007386 s; candidate setup intentionally interrupted at 278.272005 s after
one authorized native five-second sample of its verified owned PID/executable/ancestry.
Raw sample, identities and cancellation receipts are in
`task-5-evidence/flat-100k-setup-sample/`. The candidate setup is observer-affected;
it is neither an ordinary latency nor a fabricated timeout/product failure.
All original driver/campaign/child PIDs were confirmed exited. No flat input retry ran.

## Shared profiles

For nodes 10000 and 100000, using new evidence paths:

```sh
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/run-initialized.py \
  --nodes 10000 --variant shared --mode profiles \
  --evidence /Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/profiles-10000-shared
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/summarize-profiles.py
```

Both scale stages exited 0: 12 valid captures each, with two fresh conditioning
calls and three alternating ordinary/Samply controls per side/command.
The controller-authorized reuse driver verifies successful initialized shared
fixtures as above and avoids repeated publication. State is warm initialized,
with no cold-cache claim. Ordinary controls are not pooled with original timings.
The underlying recorder uses `samply record --save-only --rate 1000
--unstable-presymbolicate --output <capture> <frozen-binary> <command>`.
Raw gzip, symbols, profile receipts and PID-filtered CLI-only inclusive stacks
are retained; `profile-summary.json` records hashes, PID/library provenance and
pooled observed CPU deltas. Its scope unions count each matching stack once;
inclusive scopes overlap and cannot be added.

## Candidate-only cost instrumentation

```sh
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/prepare-instrumentation.py
# From the generated instrumentation/source directory:
CARGO_TARGET_DIR=/Users/zdkadiev/.codex/worktrees/51d9/letopis/.artifacts/issue-108/instrumentation-target \
  CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_DEBUG=1 cargo build --release --locked --offline
# From the worktree:
/usr/bin/dsymutil .artifacts/issue-108/instrumentation-target/release/letopis \
  -o .artifacts/issue-108/instrumentation-target/release/letopis.dSYM
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/run-instrumentation.py
```

The source copy starts from the immutable candidate archive and is the only
location patched. It uses a dedicated target, never the canonical target.
Final preparation/build/symbol generation exited 0. The single actual probe
campaign exited 0 with six valid calls: check/status for 10k shared, 100k shared
and 100k sharded. Each call retains a 120-second bound and no retry. Instrumented
latency is excluded from ordinary results; CPU/wall scope and empty-timer overhead
are explicit in the report. `instrumentation/{build.json,inputs.json,instrumentation.patch,
samples.jsonl,summary.json}` retains exact binary/source/patch identities and counters.
Pre-call source-formatting/optional-symbol setup errors and their corrections are
retained; no failed or timed-out product measurement was retried.

## Harness tests

```sh
# From tests/performance:
/opt/homebrew/bin/python3.12 -B -m unittest test_historical
# From the root, once after final harness code:
PATH=/opt/homebrew/bin:$PATH /opt/homebrew/bin/python3.12 -B -m unittest discover -s tests/performance -p 'test_*.py' -q
/opt/homebrew/bin/python3.12 -B .superpowers/sdd/2026-11-10-issue-108-historical-provenance/task-5-evidence/sharded-mutations.py
```

RED: unknown fixture variant and rejected CLI choice. GREEN: 10 tests;
full suite: 94 tests in 25.475 s, OK with 8 optional skips. Four mutants all exited 1;
restored shipping harness: 10 tests in 6.793 s, OK. Exact logs use
`task-5-evidence/{sharded-red.log,sharded-green.log,python-suite-final.log,
sharded-mutant-*.log,sharded-restored-green.log}`. Later README wording correction
only clarifies that the CLI control measures planning, not fallback.

## Knowledge verification

The frozen candidate initialized only this worktree's absent `.letopis/` cache.
Its local configuration was set to disabled embeddings with null provider/model,
empty parameters and max_retries=0 before any search; original checkout and
configuration were untouched. Search first found the existing concept file.
A separate temporary Git index produced a complete source tree, after exact
addressed-byte comparisons; the real index was verified unchanged.

Requests to the frozen candidate were `check {}`, search with
`{"query":"\"Historical range reuse\"","limit":10}`, and inspect of the new
knowledge ID plus its report/code source addresses. All exited 0/ok:true;
check errors were empty and the new search result flags were empty. Five descriptors
were read through `git show <returned-inspect-tree>:<path>`, sliced by their returned
inclusive lines, and SHA-256 checked. Requests, complete responses, source-basis
hashes, exact returned-tree bytes and CLI ledger are retained in
`task-5-evidence/knowledge/`. The source basis uses report lines 1–77, allowing later
review notes below the stable-results marker without changing the new binding.

## Final compliance verification

From the actual corrected task worktree, with no source-copy build into the canonical target:

```sh
CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target CARGO_BUILD_JOBS=2 make verify V=1
```

The corrected Rust diff contains only API documentation and described test phases; no operations or assertions change. Current Rust file hashes differ from the frozen measured candidate manifest for those two files only. Frozen before/after archives and all original timings, profiles and probes remain unchanged; none were rerun for these corrections. Exact fresh output and totals are in `final-fix-evidence/verify.log`, `verify.exit` and `verify.summary.json` beside the SDD ledger.

The existing new knowledge element does not bind `src/domain/source.rs`. Its report lines 1-77, provenance file, diagnostic batch-size line and batch-owner range were compared byte-for-byte with existing basis `cd688426bf88773c9d3fc3fb3f658a0a31302cd9`; all matched. The entire knowledge file, including its previous dirty prefix and old metadata, remains unchanged, so no basis OID was changed.

Using the immutable candidate executable verified against its binary SHA-256 and only this worktree's existing disabled-embedding configuration, fresh `check {}`, search for `"Historical range reuse"` with and without `needs_recheck:true`, and the original five-ID inspect request all exited 0/ok:true. Check returned no errors; the new element had no flags and was absent from the needs_recheck search. Inspect returned tree `1335e90eb6ed3b77df1ffc5f1b1336389647cf35`; all five descriptors were read from that tree and their exact addressed-byte SHA-256 hashes matched. Full requests/responses, basis comparisons and returned-tree byte proofs are in `final-fix-evidence/knowledge/`.

Final result: exit 0. Debug plus example 1,297 passed / 0 failed / 11 ignored; release 1,287 passed / 0 failed / 11 ignored. Fmt, all-target check/Clippy, fixed-value lint, strict Rustdoc and doc tests passed; FIFO watchdog passed unchanged in both profiles.
