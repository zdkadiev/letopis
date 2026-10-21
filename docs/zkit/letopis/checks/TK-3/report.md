# TK-3 — vec1 read snapshots, stale text and durable attempts

2026-10-21. Retained scratch harness in `tests/embedding/`, based on main `2ea7e581bff5388f8199bd996669aff5d939426f`. Independent review and fix verification are recorded in [review.md](review.md). This proves the bounded integration scenarios, not a production executor or G-0 completion.

## Criteria and observed behavior

| Criterion | Tests / expected result | Observed |
|---|---|---|
| ATTACH, dimension reset and old readers | `attached_vec1_round_trip_and_dimension_rejection`, `pinned_reader_survives_dimension_reset`: real 3-float vector round trip through attached native vec1; old bytes/identity/prepared query preserved across committed reset to 4 floats; fresh reader sees only new config | PASS in platform logs |
| Reset interruption and independently pinned databases | `reset_rollback_preserves_old_and_fresh_views`, `late_vector_snapshot_checks_identity`: rollback preserves committed state; a reader first touching vectors after reset detects identity mismatch and uses FTS-only | PASS in platform logs |
| Edit before computation | `stale_before_compute_spends_no_attempt`: fresh file hash mismatch skips old hash without request/attempt; other valid work completes; publication makes new hash eligible | PASS |
| Edit during computation | `late_result_never_represents_new_text`: old vector commits under the independently calculated old hash, never joins the updated node; same drain discovers and stores new text; idle cleanup retains old hash only if another node references it | PASS for shared and unshared old text |
| Work from data | `same_text_is_shared_and_sources_are_not_work`: duplicate text has one request/vector, sources excluded, cache rebuild preserves usable vectors | PASS |
| Config change during drain | `config_change_rejects_stale_reservation_success_and_error`: resets at selection, before request and after response; late errors included; same/different dimensions; exact new vector database stays unchanged by old drain | PASS for 8 schedules |
| Restarts preserve attempts | `restart_respects_attempt_budget`: real SIGKILL over eight checkpoints with max_retries 0/2; exact durable attempts and request counts; no orphan key/vector and no work after exhaustion | PASS for 30 kills / platform |
| Attempt primitives and invalid outputs | `reservation_is_durable_and_exhaustion_excludes_work`, `errors_and_invalid_vectors_consume_budget`: reservation visible from another connection, limit enforced; error/empty/nonfinite/wrong-dimension responses exhaust budget; new config clears it | PASS |

Full command: `python3 -B tests/embedding/run.py --evidence NEW_DIRECTORY`. See [README](../../../../../tests/embedding/README.md) for Docker reproduction, fixture modes and checkpoint protocol. Exact commands, environments, output and input hashes are in each platform manifest and its referenced logs.

## Stack and platform receipts

Rust 1.95.0, rusqlite 0.40.2, libsqlite3-sys 0.38.2 / SQLite 3.53.2. Native vec1 0.8 source pinned to SQLite commit `a27bcfe21c9b4e0989e86eec8fb34a3832cd23c7`, SHA-256 `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`. The build uses TK-1's verifier and resolved matching SQLite headers. Each final run compiles a fresh native library and creates fresh temporary databases.

- macOS ARM64: `macos-arm64/manifest.json`.
- Linux ARM64 Docker: `linux-arm64/manifest.json`; image `letopis-tk2:rust-1.95.0`, ID `sha256:aebf49e1f99b1406d6db6e5c02ed44585f2174fde88fce56e985265e515c9bf7`. Read-only host Cargo registry sources are built for Linux offline; no host binaries are mounted. Explicit toolchain `1.95.0-aarch64-unknown-linux-gnu` selects the image's installed compiler/components.

Each runner checks 11 TK-3 Rust integration tests and two Python process-group lifecycle tests, fmt and Clippy with warnings denied; the complete unchanged TK-2 runner (15 tests / 25 cuts), complete TK-1 runner (10 Rust + 4 Python and capability rejection probes), and roadmap consistency are nested regressions. No measurements or real provider calls occur.

## Negative controls

Initial RED: 2 snapshot assertions failed before hash-joined retrieval existed; all 6 drain scenarios failed before the scratch drain was implemented. GREEN records follow. Native SQLite rollback/snapshot behavior that already passed before adding retrieval was kept as an observed primitive, not artificially made to fail.

[Mutation manifest](mutations/manifest.json) records four independently compiled mutations, actual exit 101/assertion failures, exact test commands and restored source SHA-256. Source was restored byte-for-byte before the next mutation and final platform runs:

| Removed protection | Failing oracle |
|---|---|
| Live search_hash comparison | stale text provider-call list |
| Current-node/hash join | new node must not join old vector |
| Config identity checks in write transactions | exact post-reset vector database must remain unchanged |
| Durable reservation commit | attempt must be visible from an independent connection |

## Decisions and limits

- Reuse TK-2 publication/reset and bounded string/integer config fixtures. No full Markdown codec, general JCS, production config validation or migrations.
- Old vector results keep their captured text hash; there is no second live file read before storage. The node/hash join is the association guard. Config identity is rechecked inside reserve/store/error transactions.
- Dedicated vectors.db writers keep BEGIN IMMEDIATE from locking the ATTACHed main cache. Read views of separate databases may be established at different times; mismatched vec identity yields FTS-only.
- Fake retries are immediate. Real one-second retry timing/backoff, HTTP and credentials belong to TK-6/runtime tasks. A reserved-but-unsent request can spend an attempt; exactly-once model invocation is not promised.
- Idle cleanup snapshots referenced hashes and then writes only vectors.db; concurrent orphan reclamation versus publication is not proved. No production drain launch/flock tests (TK-4), bulk work/DR-6 (TK-37), arbitrary in-place live-file snapshot guarantee, physical power loss (TK-38), x86-64, capacity or retrieval-quality claim.
- Engram was initialized locally in this worktree at the user's explicit request; the task and verified findings are recorded there. No existing graph was overwritten.

## Independent review

[Review record](review.md): three Important findings, no Critical or Minor. Strengthened two non-vacuous oracles (native provider bytes and late errors against a populated new-config row) and fixed command-group timeout/interruption cleanup. Two additional narrow compiling mutations are caught and restored; process regressions are RED then GREEN. All three findings are addressed in one fix pass; final platform receipts are refreshed afterward. No second independent review is claimed.

The six negative controls consist of the original four plus [two targeted review controls](review-fixes/manifest.json). The runner now starts command sessions and kills/reaps the command group on timeout or interruption; deliberately independently daemonized sessions are outside that guarantee. See the two Python regressions and their retained RED/GREEN logs.
