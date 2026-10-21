# Fix round 1 — preserve the discovered repository path

Fix base: `2df19b12e0aeb20ba3ccab030326678ad61adee6`.
Implementation commit: `fa1fff52e459eecc2afb43d460c524320902cc2c` (`fix: preserve newlines in discovered Git root paths`). Evidence is a following local commit. Independent scoped re-review and TK-8 acceptance remain controller-owned.

## Changed behavior and root cause

`rev-parse --show-toplevel` adds one newline delimiter. `trim_end_matches('\n')` also removed newline bytes belonging to the actual directory name. When both `repo` and `repo\n` existed, opening the latter bound to the former and wrote its objects/service ref there. The fix in `src/adapters/git/mod.rs` removes exactly one output delimiter with `strip_suffix('\n')`, rejecting an absent delimiter as an invalid response. No other product behavior or module boundary changed.

Covering regression: `adapters::git::tests::discovers_trailing_newline_repository_without_touching_sibling` in `src/adapters/git/tests.rs`. For each SHA-1/SHA-256 format it initializes sibling repositories, opens the newline-suffixed target through the public factory, creates/pins a snapshot, verifies the sibling object counts and refs remain identical, verifies the ref exists in the target, checks exact readback and checks the discovered root. Setup commands must succeed; setup errors are not accepted as a negative control.

## Behavioral RED/GREEN

Exact focused command for both runs:

```sh
cargo +1.95.0 test --locked --offline --lib adapters::git::tests::discovers_trailing_newline_repository_without_touching_sibling -- --nocapture
```

- `red.log`: exit **101**; assertion failed: `opening repo\n must not write objects into sibling repo`. The previously empty sibling gained two loose objects. Result: **0 passed, 1 failed, 6 filtered out**. This was a compiling behavioral failure before the product fix.
- `green.log`: exit **0**; named regression **ok**, **1 passed, 0 failed, 6 filtered out**. Both formats execute inside that test.

## Host/Linux verification of changed source

All commands and statuses/full logs are in this directory and `verification.json`.

| Exact command | Actual result |
|---|---|
| `cargo +1.95.0 test --locked --offline` | exit 0; **50 tests PASS** (7 private Git + original 30 integration + 5 snapshot + 8 retention), 0 failed/ignored; 0 doc tests |
| `cargo +1.95.0 fmt --all --check` | exit 0 |
| `cargo +1.95.0 clippy --locked --offline --all-targets -- -D warnings` | exit 0, no warnings |
| `docker build -f tests/git/Dockerfile -t letopis-tk8-check .` | exit 0 |
| `docker run --rm --network none letopis-tk8-check` | exit 0; **50 tests PASS**, fmt and Clippy `-D warnings` PASS |
| `git diff 2df19b12e0aeb20ba3ccab030326678ad61adee6 --check -- src` | exit 0 |

Host: Darwin arm64, Git 2.50.1 (Apple Git-155), rustc 1.95.0 (59807616e 2026-04-14), Cargo 1.95.0 (f2d3ce0bd 2026-03-21). Container: Linux aarch64, Git 2.39.5, same Rust/Cargo. New image: `sha256:5df3cfade229b5a574a2c6efd9f65a1d1a9d8d94c746b12f2f3ed6f08fdf29d9`. The container compiles source and runs with no network or host binary mounts.

The original TK-8 logs/report/receipt remain byte-for-byte unchanged. This separate round-1 receipt records updated source SHA-256, new full logs, and the original receipt SHA-256. Raw Cargo logs retain their original whitespace. No extra broad reruns were performed after these checks on unchanged source.

## Self-review and remaining concerns

The production diff changes only delimiter handling in repository discovery; the regression exercises the visible wrong-sibling writes rather than just comparing a string helper. No fixture framework, public API, process policy, roadmap, task acceptance, Engram or unrelated worktree changes were made. No known blocker remains for this fix. Actual Git 2.29 execution, physical durability, x86_64, atomic live capture and resource limits remain the earlier explicitly unverified limits. Independent review is pending.
