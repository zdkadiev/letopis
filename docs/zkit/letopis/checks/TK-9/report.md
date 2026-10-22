# TK-9 storage acceptance evidence

Implementation and platform checks passed; fresh Task 6 and whole-branch reviews
remain parent-owned and pending. No integration, push or issue closure was done.

## Sources and receipts

Final platform source: `b0a5aeaa0cd5c51bfbdb008cf1f3870b28c49b16`.
Executable/runner/native-test code: `eac2e38c9e20445f84b31d463f0ba4fa4ead7766`.
The followup changes only Docker context allowlisting. Receipts capture the actual
source commit and dirty documentation status; this later evidence commit was not
executed. Exact product/build/vendor/test input SHA-256 values are in each manifest.

- [macOS manifest](macos-arm64/manifest.json), [commands and root results](macos-arm64/rust-check.log), [native assertions](macos-arm64/native.log), [standalone](macos-arm64/standalone.log), [linkage](macos-arm64/dynamic.log).
- [Linux outer manifest](linux-arm64/manifest.json), [image metadata](linux-arm64/image.json), [runtime manifest](linux-arm64/container/manifest.json), [root results](linux-arm64/container/rust-check.log), [native assertions](linux-arm64/container/native.log), [standalone](linux-arm64/container/standalone.log), [linkage](linux-arm64/container/dynamic.log).
- [Negative controls](negative-controls/manifest.json), [successful mutant compilation](negative-controls/registration-compile.log), [registration runtime rejection](negative-controls/registration-reject.log), [checksum rejection](negative-controls/checksum-reject.log).
- [Runner RED](history/tk9-task6-runner-red.log), [runner GREEN](history/tk9-task6-runner-green.log), [missing-example RED](history/task-6-example-red.txt), [isolated example GREEN](history/tk9-task6-oracle-green.log).
- [Initial Linux build failure](linux-arm64-initial/docker-build.log): `.dockerignore` excluded required COPY inputs. Minimal allowlist correction enabled the planned source build. [Initial host receipt](macos-arm64-initial/manifest.json) is retained.
- [Tasks 1–5 reports and original RED/GREEN logs](history/): archived before ignored SDD scratch removal; no source/vendor/target duplication. Task 4's report includes its captured command output.

## Exact execution and results

`/opt/homebrew/bin/python3 -B -m unittest discover -s tests/storage -p test_runner.py -v`:
3 passed. The tests catch swallowed exit 42, evidence overwrite, and a timed-out
child still alive/unreaped. `run_command` starts owned process groups and kills and
waits on timeout or interruption; Docker cleanup additionally removes its named
container in `finally`. Timeout cleanup was executed; external interruption and
Docker-daemon failure cleanup were code-reviewed, not separately fault-injected.

`/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence docs/zkit/letopis/checks/TK-9/macos-arm64` and the same command with
`--platform linux --evidence docs/zkit/letopis/checks/TK-9/linux-arm64`: PASS.
Every subprocess command and exit is recorded. Each platform ran
`./scripts/check-rust.sh`, strict `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`,
the focused native capability test with `--nocapture`, Cargo JSON example build,
dynamic inspection, and isolated executable execution. Root counts are
12+7+11+5+10+6+8+14+18+7+8 = **106**, 0 failures, 0 doc tests.
The original **53** scenarios remain (see archived Task 1 scenario comparison).
Storage adds 53 root scenarios; Task 6 adds three Python checks and one executable
oracle, and extends an existing native test without increasing root test count.

macOS 26.6.2 ARM64 used Python 3.14.7, Rust 1.95.0. Linux glibc ARM64 used Python
3.11.2 and Rust 1.95.0. Image ID:
`sha256:48afb0f6f32b72bf39ff155b53563c843cc880fd6673fbc66c7c72ada8e5fd55`.
Pinned base digest is in `tests/storage/Dockerfile`. Runtime command explicitly
uses `--network none`; dependencies are fetched at image build, and Cargo runtime
commands are locked/offline. No host target mount or extension path is accepted.
Both platforms assert SQLite **3.53.2**, vec1 **0.8 (NEON, multi-threaded)**,
WAL, synchronous=2 and foreign_keys=1; macOS additionally asserts fullfsync=1.
The both-database policy test checks writable cache/vector and attached connections.
The copied Mac executable links libSystem only; Linux links ordinary glibc/libm/
libgcc libraries, with no SQLite/vec1 shared dependency.

## Five acceptance criteria

| Criterion | Actual proof |
|---|---|
| Agreed schema/keys/CHECKs, graph/source/FTS/files/vectors/failures; no jobs/history | `storage_bootstrap` 14, `storage_cache` 18, `storage_vectors` 8, `storage_failures` 7 and private schema/vector tests in both root logs. `duplicate_edges_and_invalid_checks_are_rejected`, `source_missing_bytes_and_declared_targets_round_trip`, `source_projection_can_be_recomputed_and_removed`, `valid_invalid_and_unmarked_file_rows_round_trip`, `query_error_replaces_and_clears_without_history` inspect actual behavior/schema. |
| WAL/FULL/fullfsync/FK | `both_database_connections_enforce_policy` plus asserted/printed `native_capabilities_match_pinned_stack`; fullfsync is macOS-only. |
| Native vec1 in one executable, pinned source and no loading | `storage_check` publishes a.md/b.md shared text, checks FTS/revision 1, reserves/stores a three-float vector, checks nearest IDs and exact LE bytes, reopens and checks persistence/revision. Cargo artifact path is parsed rather than guessed; only executable is copied to isolated directory. Native test rejects SQL extension loading. Both negative controls below prove checksum/registration checks are non-vacuous. |
| No raw document/source copies | `contentless_fts_does_not_return_original_text`, schema tests and `query_error_replaces_and_clears_without_history`; metadata-only interfaces, contentless FTS. Tokens/postings remain expected; no claim that text markers cannot occur in SQLite bytes. |
| Responsibility modules/dependency boundaries | Physical review: domain is pure; service/mod.rs owns traits and reexports typed values/errors; storage/sqlite.rs owns SqliteStorage and Storage impl; cache_write/vector_write/read own scoped implementations; SQL/native private; git/cli.rs owns GitCli and GitSnapshots impl; markdown/parser.rs implements parser; lib.rs calls real GitCli/SqliteStorage constructors and returns interfaces. No future service/CLI/adapters/ports skeletons introduced. |

## Review Focus mapping

1. Aliases/literal paths: `same_database_path_is_rejected`,
   `symlink_and_hardlink_aliases_are_rejected`,
   `literal_unicode_quote_and_question_mark_paths_open`.
2. Partial schemas/competing initializers: `unknown_or_partial_schema_is_not_recreated`,
   `vector_schema_and_unversioned_user_data_are_not_recreated`,
   `parallel_initializers_leave_a_complete_schema`, `busy_bootstrap_returns_typed_error_without_retry`.
   Deferred minor: concurrent initializer test permits any InternalError; parent
   broad review will assess whether that assertion should be narrowed.
3. Caught mutation/late failures: `caught_mutation_error_poisoned_unit_cannot_commit`,
   `late_fts_error_rolls_back_all_changes`, `dropped_unit_rolls_back`,
   `caught_vector_error_cannot_commit_key_or_prior_write`,
   `success_deletes_failure_atomically_and_corrupt_key_poisoning_rolls_back`,
   overflow and reset-late-failure tests.
4. Interleaved model reads/reset: `old_reader_survives_reset_and_late_reader_rejects_captured_old_identity`,
   `vectors_require_matching_identity`, `dimension_reset_and_rollback`,
   `current_node_hash_excludes_old_vector`, `cache_and_vector_writers_are_independent`.
5. Restart/exhaustion/privacy: `reservation_is_durable_only_after_commit_and_survives_reopen`,
   `maximum_persisted_attempt_is_not_incremented_or_wrapped`,
   `stale_identity_cannot_reserve_or_record_or_store`,
   `malformed_stored_observations_return_safe_typed_errors`,
   `malformed_persisted_values_return_internal_errors` and safe-display unit tests.

## Non-vacuous controls and limits

`/opt/homebrew/bin/python3 -B tests/storage/negative_controls.py --evidence docs/zkit/letopis/checks/TK-9/negative-controls` passed at eac2e38.
Both copies come from `git archive`, with independent fresh targets. Real tracked
vec1 stayed SHA-256 `a38c62274093bec34631831371ad433703d6dde7fa9ecd6088da4ed33d4a61f9`.
Appending one newline produced SHA `345b37e88c32220e427d090e4f928c307c4e4cff9f504b1f00fa34fe52bc44e7`
and the build-script checksum assertion failed before its cc compilation call.
Disabling registration while retaining typed symbol references produced native.rs
SHA `027b022ec6966fa1c8fdaa53aa851f6c99779bbbc1b34a66fd9744881eaa33c6`.
`cargo test --locked --offline --no-run` first succeeded; the exact native test then
failed with `no such function: vec1_info`. These are expected rejections, not failed
acceptance runs. Isolated example behavior was GREEN on first implementation run;
no false behavioral RED is claimed.

Unclaimed: physical power loss, x86_64, old minimum Git, resource capacity, model
quality, full CLI/publication/ranking/provider/drain, historical proof-harness reruns.
The final fresh reviews and Engram outcome recording are parent-owned and pending.
