# Issue 61 — initialization and disabled embeddings verification

**Status:** implementation and review fixes verified; final host storage receipt pending.

**Branch:** `issue-61-init` at `/private/tmp/letopis-issue-61`
**Plan:** [2026-10-29 issue 61 plan](../../../../superpowers/plans/2026-10-29-issue-61-init.md)
**Spec:** [2026-10-29 issue 61 design](../../../../superpowers/specs/2026-10-29-issue-61-init-design.md)
**Base:** `4624d91` (`docs: adopt revised issue 61 specification and plan`)

## Acceptance matrix

| Area | Evidence |
| --- | --- |
| Nested config, defaults, validation | `tests/config_command.rs`, `src/service/configuration/tests.rs`, `src/service/configuration_value.rs` corpus test |
| Schema agreement | `tests/config_schema_agreement.py`; shared `tests/fixtures/config/corpus.json` |
| Disabled execution and status | `tests/embedding_service.rs`, `tests/status_command.rs`, `tests/search_filter_fallback.rs`, `tests/storage_status.rs`, status/embedding unit tests |
| Atomic publication and recovery | `src/filesystem/init_tests.rs`, `src/service/init.rs` ordering/sync tests |
| CLI initialization | `tests/init_command.rs`; root/subdirectory/outside-Git/bare, empty repo, preservation, concurrency, retry |
| Contracts and delivery | X-1/X-2 prose/schemas/examples, CLI help, skill, decisions D-104, architecture/mechanics/data text |

## RED / GREEN

- `cargo test --locked --offline --test config_command` failed 6 new nested/disabled/migration cases against the old flat parser; after implementation it passed 10/10.
- New filesystem and init service APIs were first compiled only after their behavioral tests existed; focused suites then passed.
- Full Rust suite before review: `cargo test --locked --offline` → **895 passed, 11 ignored**. Final `make check` and `make verify` after review fixes → **ok**.
- `make check` → `ok`.
- The host storage runner result is recorded below after it finishes.

## Mutation checks

All mutations were temporary; each file was restored and its SHA-256 compared with the saved pre-mutation copy.

| Mutation | Test that failed | Observed failure | Restored |
| --- | --- | --- | --- |
| Allow provider-only partial config | `configuration_value::tests::shared_corpus_agrees_with_local_parse_and_validation` | `provider without model`: true, expected false | `1a2d4c80…` |
| Skip local provider validation | `configuration::tests::validates_configured_disabled_settings_locally` | returned `Ok(Disabled(..))` | `2dd8d384…` |
| Disable legacy-flat detection | `configuration_value::tests::reports_legacy_flat_document_for_migration` | `InvalidConfig` instead of `MigrationRequired` | `1a2d4c80…` |
| Change identity projection | `configuration_value::tests::identity_excludes_enabled_and_the_root_wrapper` | different SHA-256 | `1a2d4c80…` |
| Disable zero disabled-status counts | `storage_status::disabled_policy_returns_zero_without_touching_execution_state` | `(2, 0)` instead of `(0, 0)` | `ec827104…` |
| Show stored failures while disabled | `status::tests::disabled_configuration_hides_execution_failures_and_reports_zero_counts` | execution problems present | `023d3afd…` |
| Remove file-sync fault gate | `filesystem::init_tests::file_sync_failure_leaves_absence_and_retry_publishes` | first call returned `Ok` | `1f1d73d2…` |
| Accept an unsafe config leaf | `filesystem::init_tests::rejects_symlink_and_nonregular_configuration_leaves` | symlink destination accepted | `1f1d73d2…` |
| Map missing Git to `internal_error` | `init_command::init_accepts_empty_repository_and_rejects_non_repository_or_bare` | returned `internal_error` instead of `invalid_request` | `20981af2…` |
| Bootstrap before config validation | `service::init::tests::validates_existing_config_before_opening_state_or_storage` | trace `open, storage, config` | `948424be…` |
| Skip winner validation | `service::init::tests::invalid_publication_winner_prevents_success_and_sync` | invalid winner returned `Ok` | `948424be…` |
| Ignore incompatible storage | `init_command::init_preserves_incompatible_storage_and_retry_completes_after_repair` | init returned success and replaced the failure path | `98740cb7…` |
| Reset vectors while disabled | `embedding_service::disabled_configuration_preserves_vectors_attempts_and_skips_execution` | `unexpected configuration change` | `0dfad63b…` |
| Reorder config before stored identity | `service::embedding::tests::process::configuration::configuration_change_after_storage_observation_never_resets_newer_state` | stale observation destroyed newer state | `84fefc7d…` |
| Bypass 0700 protection | `init_command::init_protects_a_preexisting_permissive_service_directory` | mode `0755` instead of `0700` | `f9af6933…` |

## Commands and results

- `python3 -B tests/config_schema_agreement.py` → 20 schema cases checked, 3 runtime-only, 0 errors.
- `python3 -B docs/zkit/letopis/checks/contracts/check_contract_examples.py docs/zkit/letopis` → 82 CLI examples, 113 assertions, 0 errors.
- `make check` → `ok`.
- `make verify` → `ok` (strict Rustdoc, root tests in release profile).
- `python3 -B tests/storage/run.py --platform host --evidence .artifacts/issue-61/2026-10-29-host-1` → pending.

## Platform limits

- `python3.12 -B -m unittest discover -s tests/final_pilot -p 'test_*.py'` → 173 tests OK, 2 skipped (opt-in Docker proofs), after updating the pilot config writer and service probe to the nested format; the full pilot was not rerun.
- Host platform is macOS arm64; no Linux/Docker run was performed for this issue.
- No physical power-loss drill, live provider call, performance measurement, or full pilot rerun was performed.
- macOS file durability uses `F_FULLFSYNC` for the published configuration; directory entry durability uses `fsync` through the existing filesystem boundary.
- The initial full baseline run had one unrelated temporary-fixture collision; stale `letopis-git-*` directories were removed, and the rerun passed `make check`.

## Review

A fresh read-only whole-branch review was run by GPT-6.1 Sol (high) over `4624d91..72fc908`.

**Critical:** none.

**Important findings and fixes:**

1. `src/service/embedding.rs` read configuration before observing stored identity. A concurrent config publication between those reads could make the stale observation reset newer vectors/attempts. Fixed by observing the stored identity first, then reading/validating config; disabled configs still return before `reset_vectors`. Added a deterministic process regression `configuration_change_after_storage_observation_never_resets_newer_state`; reverting the ordering makes it fail.
2. Initialization reused an already-pinned permissive `.letopis` directory without applying mode 0700. Fixed with `open_private_service_directory`, used only by `InitFiles`; observational reads remain non-mutating. Added filesystem and real-binary tests for preexisting 0755 directories with absent and valid config; bypassing the chmod makes them fail.

**Minor findings:**

- Git exit code 128 is still mapped to `invalid_request`/“Git repository required” for any fatal discovery failure. This can mislabel corrupt/ownership failures; deferred because it is outside the agreed init behavior and no test currently distinguishes those causes.
- X-1 prose cited D-103 and said “nonempty”; corrected to D-104 and “non-empty work tree” wording is now “Git work tree, not bare”.

**Declined to judge:** fresh runtime results (the reviewer was read-only), physical power-loss durability, Linux/Docker behavior, live-provider compatibility/performance, hostile same-UID isolation, and uncommitted report/plan edits.

**Ready to merge after fixes:** yes, subject to the rerun gates below.
