# TK-13 product embedding acceptance

## Final combined-fix acceptance — bcd405c0d9c6

Status: all final-review findings I1/I2, M1–M4 and controller C1 are fixed and verified; independent scoped re-review and controller approval remain pending. Runtime/test source is `bcd405c0d9c6cd5d68e4d57f16b2aafebce048fa`, comprising `ca8bfe3` combined fixes plus `bcd405c` request-consuming process fixture. Later documentation changes do not alter receipt inputs. No merge/push, Engram write or TK-10 integration occurred.

Fresh exact commands (run at the TK-13 worktree):

```sh
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk13/host-bcd405c0d9c6
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk13/linux-bcd405c0d9c6
/opt/homebrew/bin/python3 -B .artifacts/tk13/mutations-bcd405c0d9c6/reproduce.py
```

| Final receipt | Inspected results | Input reconciliation |
|---|---|---|
| macOS 26.6.2 ARM64 / Python 3.14.7 / Rust 1.95.0 | PASS; 16/16 commands zero; root **219 passed, 0 failed, 2 ignored**; separately selected real60s **1 PASS, 60.00s** | 207 current input hashes match |
| Linux 7.0.0 glibc 2.36 ARM64 / Python 3.11 / Rust 1.95.0 | PASS; inner 14/14 and outer 8/8 commands zero; root **219 passed, 0 failed, 2 ignored**; separately selected real60s **1 PASS, 62.96s** | 102 inner / 207 outer current input hashes match |

All **102 common inputs** are byte-identical. Explicit inclusion checks cover the three embedding integration targets, their support, private filesystem/process/config tests and the new `src/utils.rs`. The source process test SHA256 is `8591fe5335814560c331e7f42c8ad95e0451daf5729d65bbf84473d0c4af29e4`; utility SHA256 is `9b4ff028c6113517799875fd841d2ce927d61ebd415510ea2bfda05984ec0732`. Cargo.lock remains `0116d9a5353b1aed6435649e6520603870f8a43c079fab4c982ef48a42acbf95`. Implementer and controller independently reconciled manifests against current files.

Both receipts execute `./scripts/check-rust.sh` (fmt, compiler, Clippy, all root tests), strict `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`, nine runner regressions, native probes and the standalone storage executable, all PASS. Each final platform run explicitly executes the real60s case once. Docker acceptance runs with `--network none`; preparation uses the existing build/fetch phase.

Counting: prior source had 211 root passes/2 ignores; final source adds six provider cases, one config unit case and one forced-watchdog case, for 219 root passes/2 ignores. Both counts include two inert older child entrypoints; final remaining ordinary/parent cases number 217. Final root logs contain eight filtered provider-child passes, counted separately and never added to 219. Filesystem nested output is captured. The five process parents still own nine separate process logs (eight successful child completions and one intentionally killed owner). The explicit real60s test is separate from the root ignore and from these child invocations.

### Findings and exact covering tests

| Finding | Final change and executed coverage |
|---|---|
| I1 | Shared explicit **10 MiB** response bound under the unchanged single deadline; BodyExceedsLimit → safe InvalidResponse. `embedding_provider::bounds_both_protocol_bodies_with_and_without_content_length` tests real oversized valid payloads over Content-Length and close-delimited framing for both protocols, one request, no accepted vector, safe diagnostics, plus ordinary bounded success. No new config or chunking. |
| I2 | Private literal JSON visitor prevents RawValue-key object reinterpretation at HTTP and canonical verification boundaries. `preserves_reserved_key_objects_in_unknown_config_parameters`; `rejects_object_wire_components_indices_and_wrapped_responses` (both protocols, whole wrappers and compatible index); `preserves_decimal_and_exponent_values_with_large_integer_jcs_spelling`; `rejects_trailing_response_tokens_and_excessive_depth`; config unit `rejects_trailing_json_after_configuration`. Original recursive duplicates/raw integer overflow/depth/JCS collision cases remain green. Original integer tokens retain strict checking; legal decimal/exponent values may canonicalize to large integer spellings. |
| M1 | Renamed helpers to reject_config, map_config_error, reject_response and updated all call sites. |
| M2 | Private filesystem/mutator/external/integration child output goes into owned temporary captures; completed failures expose at most 64 KiB diagnostics. `embedding_files` (8 passes) and private filesystem (8 passes) cover real subprocess paths; root logs no longer contain their nested summaries. |
| M3 | Dedicated outer process group, inherited by mutator/descendants, is terminated on watchdog cleanup; direct child is reaped. `filesystem::test_support::forced_watchdog_terminates_nested_descendants` initially failed because the descendant-held lock survived, then passed after group cleanup. Existing rustix process feature is dev-only; no package/version/lockfile change. The test establishes descendant-held lease release, not non-parent wait() of orphan zombies. |
| M4 | `service::embedding::tests::process::edit_before_send_skips_stale_hash_without_reservation` counts/serves unexpected requests, drains pending connections and asserts **zero HTTP** after unchanged-state/cache checks. It consumes headers/body before reply to preserve the stored-vector mutation oracle. |
| C1 | Native Ollama response model must be present, a nonempty string; aliases are accepted without request equality. `rejects_missing_empty_and_wrong_type_native_model`; positive alias controls and all current native success fixtures include valid metadata. I1/I2/vector-negative fixtures include metadata so missing model cannot mask their assertions. TK-3/TK-6 historical inputs remain unchanged. |

Exact execution rulings 4/5 are appended verbatim to the durable [plan](../../../../superpowers/plans/2026-10-22-tk-13-embeddings.md), retaining rulings 1–3; the [specification](../../../../superpowers/specs/2026-10-22-tk-13-embeddings-design.md) clarifies transport policy, literal types, integer semantics and schema precedence over old omitted fixture fields.

### Fresh guard controls and development evidence

New `.artifacts/tk13/mutations-bcd405c0d9c6/manifest.json` records both controls: compile exit **0**, exact behavioral test exit **101**, byte restoration, same exact test exit **0**. Both RED assertions observe `Current(Some([1.0, 2.0, 3.0]))` instead of `Current(None)`. Guard file before/restored/shared/current hashes match the original hashes in the historical control table below; altered fixture/process source is separately pinned by the final receipts. The new reproduction script is an owned copy; original `mutations/` remains unchanged.

`.artifacts/tk13/final-fix-development/` retains provider RED (34 pass, four behavioral failures, one ignore), provider GREEN, forced-watchdog RED (`descendant survived watchdog`), focused filesystem/integration/process GREEN and full check logs. Two intermediate full-check attempts caught new-test Clippy issues (match_same_arms and semicolon_if_nothing_returned), fixed without allowances. Final platform checks cover the corrected tree.

Preliminary `mutations-ca8bfe382d52/` is **not accepted**: its hash control failed on an attempt count because the fixture closed an unread HTTP request, rather than on the requested stored-vector assertion. That prompted `bcd405c` and both fresh controls above. Preliminary `host-ca8bfe382d52/` and `linux-ca8bfe382d52/` are likewise excluded from final acceptance because the fixture changed during that wave. No earlier receipt is relabeled as proving current source.

### Final physical review and limits

Reviewed concrete provider implementations, configuration parsing, shared private JSON utility, module visibility/import direction and unchanged `lib.rs` constructor assembly. Public interfaces stay frozen; protocol rules stay in embedding, config rules in service, and utility owns only literal JSON parsing. No SQL/native identifiers enter service/domain, no technology adapter orchestrates another, and no runtime/registry/schema/batching/process product feature was added. The existing rustix process feature is used only by test supervision. Changed covering files respect the 60-line function limit. Independent scoped re-review remains a controller gate.

Existing limits below still apply: no merged TK-10, x86_64, live model/quality/corporate-CA, performance/RAM, physical-power-loss, hostile atomic filesystem confinement or downstream CLI/lifecycle/search acceptance. The documented cap deliberately rejects larger valid responses; nonconforming native shims omitting model are rejected, aliases accepted. Query vector validation at the future search read remains TK-12.

## Historical Task 5 evidence — source 8d39c11ca5ad

The following earlier receipt/results/matrix are retained as provenance for their captured source. They do **not** prove amended source or supersede the final results above. Their previous claim of bounded HTTP responses was incorrect until I1 was fixed. Deferred findings listed below are historical and are resolved by the final fix dispositions above; independent final approval remains pending.

Status: implementation and implementer verification recorded below; independent Task 5 review and controller final whole-branch gate remain pending. The task card remains `in_progress`. This report does not mark TK-10 or any downstream task accepted.

## Source, commands and receipt ownership

Acceptance source: `8d39c11ca5ad` (`test(embedding): prove process exclusion and guarded publication`), on the TK-13 worktree's own main line, after reviewed Tasks 1–4 (`8c8bf785cf28c33d735a2376485ac4ec8c336c65`). The evidence/documentation commit after this source changes no hashed runtime input. Large raw logs remain in ignored `.artifacts/tk13/`; they are local evidence, not committed portable artifacts.

Exact platform commands from the worktree root:

```sh
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform host --evidence .artifacts/tk13/host-8d39c11ca5ad
/opt/homebrew/bin/python3 -B tests/storage/run.py --platform linux --evidence .artifacts/tk13/linux-8d39c11ca5ad
```

The runner records every argv/exit in `manifest.json`, compiler/source/native input SHA256 values, tool versions, native capability output, dynamic dependencies and isolated executable hash. The host receipt is `.artifacts/tk13/host-8d39c11ca5ad/`; Linux's outer receipt is `.artifacts/tk13/linux-8d39c11ca5ad/`, with actual product receipts in `container/`. Both root commands execute `./scripts/check-rust.sh`: fmt, locked offline all-target compiler/Clippy checks and `cargo test --locked --offline`. Strict docs use `RUSTDOCFLAGS='-D warnings' cargo doc --locked --offline --no-deps`.

The independently recorded `provider-real-deadline.log` executes exactly:

```sh
cargo test --locked --offline --test embedding_provider attempt_obeys_real_sixty_second_deadline -- --ignored --exact --nocapture
```

Its assertion requires one HTTP request, `Timeout`, and elapsed time in `[59s,64s)`. The root suite's ignored result does not count as execution of this test. Docker prepares dependencies with `cargo fetch --locked` during build; product acceptance runs with `--network none` and loopback fixtures. The existing Dockerfile already copies `tests/*.rs` and `tests/support`; the necessary change was its build-context allowlist, adding all three TK-13 embedding integration files. This is execution ruling 3, not a change to runtime policy.

## Platform results

| Receipt | Outcome / inspected commands | Root suite | Explicit real60s | Input verification |
|---|---|---|---|---|
| macOS 26.6.2 ARM64, Rust 1.95.0, Python 3.14.7 | PASS, 16/16 exits zero | 211 passed, 0 failed, 2 ignored | 1 PASS, 60.01s test duration | 206 current input hashes match |
| Linux 7.0.0 glibc 2.36 ARM64, Rust 1.95.0, container Python 3.11 | PASS, 14/14 inner exits zero; 8/8 outer orchestration exits zero | 211 passed, 0 failed, 2 ignored | 1 PASS, 62.28s test duration | 101 inner current hashes match; 206 outer hashes match |

All 101 common inputs are byte-identical. Required Linux inputs include `tests/embedding_provider.rs`, `tests/embedding_service.rs`, `tests/embedding_files.rs`, all product `src/` and shared `tests/support/` files. Source commit is `8d39c11ca5ad3b58ac9435792429a8f593b34a93`. Cargo.lock SHA256 is `0116d9a5353b1aed6435649e6520603870f8a43c079fab4c982ef48a42acbf95`; process test SHA256 is `1183307b690346cfb8b2a8df723a518049b0a24adf722a2e237ef73896382ee6` on both platforms.

The 211 root passes include two inert historical Task 3 child entrypoints: 209 remaining parent/ordinary cases. The two root ignores are the real60s case and the private process child. Root logs additionally contain 27 filtered child pass entries from older harnesses; these are not added to 211. New process logs are separate: nine invocations, eight successful children and one intentionally killed owner, on each platform. The process parents themselves contribute five of the root passes. The explicit deadline contributes one separately executed test per platform.

Runner regressions: 9 PASS on each platform, including evidence-path propagation, exact once-per-platform deadline argv, no overwrite, failing exit preservation, preparation receipts and owned-process timeout cleanup. Strict Rustdoc, fmt/compiler/Clippy, native probes and isolated storage executable all PASS. Native probes report SQLite 3.53.2, vec1 0.8 NEON, WAL, synchronous=2 and foreign_keys=1 (macOS also fullfsync=1). Standalone executable checks FTS/shared vector/exact bytes/reopen/revision=1 with no native SQLite/vec1 shared dependency. No accepted-run failure occurred; earlier development RED receipts remain separate.

Implementer and controller both reconciled source hashes/command exits and common platform inputs. This reconciliation is not the pending independent Task 5 or final branch review.

## Acceptance matrix

Each row below is executed on both macOS ARM64 and Linux glibc ARM64 through the root check unless a separate command is stated. Test names identify the exact assertions in the indicated target. Platform totals and inspected exit/hash results are recorded in the platform result section.

| Specification area | Target and exact observable assertions |
|---|---|
| Configuration | `embedding_provider`: `ignores_object_order_for_identity`, `changes_identity_when_retries_or_unknown_parameters_change`, `rejects_missing_required_fields_and_forbidden_root_fields`, recursive duplicate/integer-rounding tests, endpoint/parameter rejection. `embedding_service`: `missing_and_invalid_configuration_preserve_corpus_and_prior_vectors`, `valid_shape_with_invalid_endpoint_does_not_reset_or_compute`. Missing/invalid config keeps corpus/vectors and performs no HTTP. |
| Credentials | `embedding_provider`: `reorders_compatible_response_and_sends_only_supported_parameters`, `keeps_ollama_available_without_compatible_credentials`, `rejects_invalid_compatible_credentials_before_network_io`, `disables_environment_proxies_and_native_authorization`, `redacts_cause_bearing_errors_and_config_debug`. Synthetic authorization and knowledge canaries are absent from Display/Debug and the entire exposed cause chain; the safe private source terminates the chain. |
| Providers | `embedding_provider`: request construction and reordering, `rejects_malformed_native_vectors`, `rejects_compatible_index_and_vector_failures`, `rejects_compatible_error_payload_status_and_non_json`, `refuses_redirects`, connection failures and unconfigured-dimension validation. Separate real60s command above checks the production timeout on each platform. |
| Dimensions | `storage_embedding_work`: `first_committed_dimension_wins_between_competing_connections`, `last_vector_cleanup_preserves_dimension_across_reopen`, `rollback_of_first_insertion_does_not_pin_dimensions`, `reset_rollback_restores_native_dimensions_and_commit_releases_them`, `unsupported_native_dimensions_poison_the_unit_without_keys`; `embedding_service::configured_dimensions_reject_wrong_size_without_graph_changes` and `query_rechecks_dimension_established_during_http`. Actual pinned vec1 metadata, not a model lookup. |
| Work selection | `storage_embedding_work::cursor_orders_shared_hashes_and_excludes_sources_and_stored_hashes`, `absent_and_null_error_rows_remain_eligible_until_four_attempts`, `nullable_failure_hash_does_not_hide_unrelated_work`; `embedding_service::stale_first_same_hash_does_not_starve_valid_second_representative`, `stale_and_missing_elements_skip_without_reservations_or_requests`; private service skipped-ID read counters and `embedding_files::markdown_capability_preserves_existing_parser_projection`. |
| Attempt lifecycle | Private process `killed_reservation_keeps_budget_and_releases_stable_lock` observes one committed attempt before SIGKILL, then a new process returns Complete without HTTP and still has one attempt/no vector. `embedding_service::retries_are_additional_and_exhaustion_survives_restart`; private service `credential_failures_use_four_durable_attempts_and_three_one_second_waits`; malformed/read-failure cases persist safe failures. |
| Transactions | Private process `reset_during_http_discards_late_document_success_and_error` writes through another real connection while HTTP is paused, then checks absent stale vector/attempt, retained current referenced vector and unchanged current query error. `embedding_service::reservation_precedes_http_and_new_work_is_found_after_compute`; `storage_vectors::text_hash_shares_one_vector` checks idempotent storage. |
| File edits and FTS | Private process `edit_before_send_skips_stale_hash_without_reservation` edits after work selection/before live parsing; asserts no vector/attempt and unchanged cache/FTS tables. `embedding_service::edit_during_compute_stores_captured_hash_and_keeps_new_hash_pending` asserts captured old hash and new work pending; malformed/provider/storage failures compare corpus, graph/FTS and cache revision. |
| Drain exclusion | Private process `two_drains_have_one_owner_and_one_stored_vector` observes one durable reservation, contender AlreadyRunning and no HTTP before release; one vector after owner completes. `killed_reservation_keeps_budget_and_releases_stable_lock` also checks unchanged lock inode and successful reacquisition after death. Temporary independent databases; process children are owned and reaped. |
| Query boundary | `embedding_service::query_failure_is_single_attempt_without_document_reservation_or_vector`, `query_success_clears_error_without_caching_or_document_attempts`, `query_rechecks_dimension_established_during_http`; private process `reset_during_http_preserves_current_query_error` checks old HTTP success/error cannot replace new diagnostic or insert a document vector. Unavailable is the future caller's FTS-fallback signal; no product search command is claimed. |
| Cleanup | `embedding_service::cleanup_retains_skipped_and_exhausted_references_and_removes_orphans`; `storage_embedding_work::cleanup_retains_referenced_hashes_and_diagnostics_without_cache_publication`, `late_old_identity_cannot_cleanup_current_vectors_or_query_error`, pinned-cache/rollback/shared-representative cases. Process config-race tests retain current referenced vectors. |
| Architecture | Implementer physical review below; independent final review remains a required controller gate. Passing tests alone do not close this row. |

## Process harness and guard controls

`src/service/embedding/tests/process.rs` defines five parent tests and one ignored `run_child` entrypoint. Parents explicitly invoke the current unit-test executable with `--ignored --exact --nocapture`, real `open_storage`/`open_embeddings`, argv/environment scoped to that child, and isolated corpora/databases. Only `#[cfg(test)]` calls before live preparation and after committed reservation reach the private checkpoint. Production builds contain no fault environment or transport alternate. UnixStream readiness/continue barriers have 15-second I/O bounds; owned children have 15-second exit watchdogs and unconditional kill/wait on parent panic. These filtered children spawn no descendants. The outer Python receipt command owns a process group and kills/reaps it on interruption. Child output goes to separate receipt-local files via `LETOPIS_TEST_EVIDENCE`; it does not interleave with root test output.

There are nine child invocations per process-suite run: eight successful child test completions and one intentional killed reservation owner. These are separate from the five parent assertions, and from the two older Task 3 inert root entrypoints. A killed child's partial log is expected; the parent verifies unsuccessful waited exit and durable state. Full cache table snapshots include nodes, edges, files, project revision and FTS shadow data; vector/failure/query-error assertions use the application storage interfaces.

Development RED: `.artifacts/tk13/development/process-red.log`, from `cargo test --locked --offline --lib service::embedding::tests::process -- --nocapture`. Before checkpoint calls, three intended barrier readiness checks failed; this first run also exposed an accepted TCP stream inheriting nonblocking mode on macOS, corrected with explicit blocking mode and read/write timeouts. An earlier compilation rejected the unused checkpoint; that compiler failure is not behavioral guard evidence. GREEN: same command, five parent PASS/one ignored child, in `development/process-green.log`. `development/root-check.log` records the complete required root verification before source capture.

Sensitive controls ran in disposable copied trees, never in the primary/TK-10 tree. `.artifacts/tk13/mutations/reproduce.py` records their reproduction code; `manifest.json` contains exact commands, hashes and exits. For each control, `cargo test --locked --offline --lib --no-run` exited 0; its exact process test exited 101 with stored state `Current(Some([1.0, 2.0, 3.0]))` instead of `Current(None)`; byte restoration followed by the same check exited 0.

| Control | Mutated file / exact parent oracle | Original = restored = current SHA256 | Mutated SHA256 |
|---|---|---|---|
| Remove pre-HTTP search-hash comparison | `src/service/embedding.rs`; `service::embedding::tests::process::edit_before_send_skips_stale_hash_without_reservation` | `0763b14f2fd9fa72888f0e66e9171577688dce12e4662462dde9b5b7e1a79e92` | `282d13b2919dcae5ad1030d2c00b7b1c7ff58276265bd11efa0be2bbcf232c79` |
| Bypass late-write identity comparison | `src/storage/vector_write.rs`; `service::embedding::tests::process::reset_during_http_discards_late_document_success_and_error` | `9c21310e8a43f6733325197a75e04ffe966e919b7aacf0e7cdefa16265ab2604` | `77fe6dc31bc0bc0f7db7ad476cffa4dd2321b70042fbca11bdaddbda4c7f5b06` |

## Physical architecture and classifications

Reviewed `src/service/mod.rs`, `embedding_config.rs`, `embedding_error.rs`, `model.rs` and `embedding.rs`: application-owned traits/typed states, synchronous orchestration, domain values and interface dependencies. Concrete storage/HTTP/filesystem imports appear only in test modules, not product service bodies. No SQL builder or native vec1 identifiers enter the product service/domain. Config JCS/validation belongs to application configuration, transport validation to provider; finite dispatch is `ProviderKind`/`HttpProviders`.

Reviewed `src/embedding/{mod,http,ollama,openai,provider}.rs`: Ollama, Openai and HttpProviders each implement `EmbeddingProvider` with implementation bodies beside their structs; private HTTP policy owns no proxies/redirects/retries, 60s attempt limit; the earlier bounded-response claim was disproved by I1 and is corrected only in the final source above. `src/filesystem/{mod,local,confined,read}.rs`: LocalFileSystem implements EmbeddingFiles, owns confined handles/regular reads and stable explicitly-unlocked lock guard. `src/markdown/parser.rs`: MarkdownParser implements Markdown using the shared codec and application-owned models. `src/storage/{mod,sqlite,read,vector_write,work,vectors,query}.rs`: concrete Storage/StorageRead/VectorWrite implementations retain explicit transactions and typed/bound ordinary SQL; native dimension metadata stays private. No technology adapter orchestrates another adapter.

`src/lib.rs` contains actual constructor calls: `open_storage` constructs SqliteStorage; `open_embedding_provider` constructs Ollama/Openai/HttpProviders and captures compatible credentials; `open_embeddings` passes caller storage, LocalFileSystem, MarkdownParser and that provider into EmbeddingService. Visibility is limited to required exports; no new CLI/main/daemon/async runtime, model registry, dimension column, jobs/seq table or interface change was introduced by Task 5. Test-only process fixtures may import concrete technology to inspect real integration.

Missing/invalid configuration returns Missing/Invalid and preserves prior derived state. Source parse/read failures consume the normal durable preparation budget; changed/missing valid elements skip. Provider dimension mismatch is InvalidResponse; storage InvalidRequest on insertion is rolled back before safe provider-failure recording. Corrupt/I/O storage failures remain InternalError, not false provider failures. Lock contention is AlreadyRunning; unsafe lock/filesystem failures remain typed errors. Late identity mismatches yield ConfigChanged/Unavailable rather than writes. Query dimension recheck is the accepted post-HTTP pinned observation, not a guarantee that a future consumer's index cannot change.

## Deferred review findings and scope limits

Carry into independent final review: Task 1 M1 helper verb naming, M2 serde_json raw_value reserved-key object compatibility; Task 3 T3-M1 inherited child output interleaving and T3-M2 descendant cleanup after forced outer termination. Their prior reports and `.superpowers/sdd/2026-10-22-tk-13-embeddings/progress.md` retain provenance. Task 5's separate child output and no-descendant ownership do not silently resolve the older harness findings.

TK-10 remains unmerged/unmodified. Future integration must reconcile service/mod.rs, lib.rs and concrete filesystem/storage capabilities, preserve the single shared Markdown codec and rerun the combined tree. TK-12 must validate query vectors against its pinned index observation. Historical TK-3/TK-6 evidence is preserved against its captured revision; those separate proof manifests were not rerun. In particular TK-6's prior system-TLS evidence is not a fresh TK-13 corporate-CA/real-model/system-drill result.

No x86_64, real inference/model quality, performance/RAM, physical power loss, hostile ancestor-rename atomicity, full CLI/TK-27/TK-41 or merged-TK-10 acceptance is claimed. SIGKILL verifies process death, not power loss. The five parents combine reservation-death and stable-lock-release in one case. The existing Dockerfile needed no edit because its COPY inputs already cover the allowlisted files. These are explicit implementation choices and evidence limits, not omitted acceptance gates.
