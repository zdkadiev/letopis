# TK-22 — verified search filters and fallback

Implementation verified on 2026-10-25. Task 1–3 reviews passed, including Task 3 scoped evidence re-review. The final whole-branch review at `f099224` found no product or architecture defects and returned **With fixes** solely for the metadata inconsistency reconciled in this update; a scoped re-review of this metadata update remains pending. This is an implementation acceptance receipt, not completion of deferred TK-28 UI or system gates.

## Captured revision and commands

Product revision: `bee627e19956da317fa45961ab6d4321eb0741d4`. Task 3 started at `1826241a18d17c2e63b0ec049a82e4e9a8133571`; reviewed upstream product was `6076fda1917b586642a1c8b72e9fa9b9beef7e38`. Platform: `macOS-26.6.2-arm64-arm-64bit`, Rust/Cargo 1.95.0. [Machine-readable receipt](verification.json) contains every root executable count and evidence SHA-256.

All commands ran at the task root with `CARGO_TARGET_DIR=/Users/zdkadiev/Repos/letopis/target RUST_TEST_THREADS=2` and locked offline dependencies:

| Command | Actual result |
| --- | --- |
| `cargo test --locked --offline --test search_filter_fallback` | 3 passed, 0 failed, 0 ignored |
| `cargo test --locked --offline --lib service::search::tests::scoped` | 3 passed, 0 failed, 0 ignored |
| `cargo test --locked --offline --test search_consistency` | 7 passed, 0 failed, 0 ignored |
| `make verify V=1` | exit 0; fmt/check/Clippy, root debug tests, strict Rustdoc, root release tests; no warnings |

Final root counts use the **last** summary after each Cargo `Running` header, excluding nested child summaries and Doc-tests. Debug: 620 passed / 0 failed / 5 ignored across 45 executables. Release: 620 passed / 0 failed / 5 ignored across 45 executables. Five ignored tests remain ignored in each profile; this receipt does not claim them run.

Fresh evidence lives under `.artifacts/tk22/task3/`: `final-verify.log`, `final-counts.json`, `final-inputs.json`, focused logs and mutation/restoration logs. Frozen Rust/build/test input manifest SHA-256: `13566d7be1b87833fe8dc45a818907a40e4ceecb712a4ce10f7d88149e7bc760`. Final verify log SHA-256: `9c22eaa125571d4436b3cbdbda11edfb5d096c2529e62340ac55e0331a18a37f`. Private evidence remains untracked; historical receipts were not rewritten. Documentation validation checked 156 relative file links with no missing targets; changed contract prose was checked against the implemented filter and lifecycle rules.

## Acceptance coverage

| Spec item | Concrete evidence in the final root suite |
| --- | --- |
| 1. Strict transport, empty/duplicate selectors | `tests/search_filters.rs`: `empty_and_duplicate_selectors_preserve_literal_ids`, `invalid_filter_shapes_and_addresses_are_invalid_requests`; CLI unit tests |
| 2. AND/OR, recheck modes, exact identities | `source_and_revision_require_one_binding_and_fields_combine_with_and`, `sources_use_exact_normalized_addresses_and_opaque_urls`, `current_changed_missing_and_invalid_notes_drive_all_recheck_modes` |
| 3. Same-binding trap | `source_and_revision_require_one_binding_and_fields_combine_with_and` |
| 4. Equal bytes/different trees, per-note basis, missing/range/remote nonmatches | `revision_equality_uses_per_note_basis_and_complete_selected_bytes` |
| 5. Complete revision preflight and safe typed errors | `every_requested_root_is_validated_even_after_a_matching_tree`; service `revision_preflight_precedes_provider_and_candidate_dependent_exits`, `revision_comparison_errors_preserve_typed_codes` |
| 6. Filter before eligible channel ranks/fusion/limit | service `eligible_channel_ranks_are_recomputed_before_fusion`, `excluded_channel_heads_do_not_consume_limit`, `eligible_ties_and_duplicates_keep_identity_order`; CLI `ineligible_fts_head_cannot_consume_the_final_limit` |
| 7. Live flags, one observation, pinned metadata/bindings | service `changing_observer_is_called_once_and_retained_flags_drive_rendering`; scoped `search_observes_metadata_and_bindings_from_original_pinned_view`; seven `search_consistency` tests |
| 8. Filtered fallback and singleton lifecycle | three `search_filter_fallback` tests: one endpoint returns HTTP 500 then valid 2D embedding under unchanged identity; literal FTS IDs a.md/b.md, then semantic.md appears; exactly two query payloads; no warnings/secrets/text; no pending embedding work or attempts |
| 9. Late failure releases scoped reads | scoped `late_provenance_failure_releases_pinned_view_while_service_stays_alive` covers observation/comparison errors after successful query; no partial result, safe InternalError, WAL checkpoint and publication to revision 2 before explicit service drop. Existing post-query metadata error remains green |
| 10. Physical architecture review | actual files and constructor review below |

All five approved Review Focus cases are covered: missing selector cannot be hidden by OR/empty-query/unmatched-source shortcuts (item 5); equal addresses use each note's basis (item 4); one retained observation drives filtering/rendering (item 7); same-config failure→actual success clears query_error (item 8); excluded corrupt/nonfinite metric remains an integrity error (`excluded_nonfinite_metrics_remain_integrity_errors`).

## Accepted execution adjustments

The plan-scoped ledger records six accepted adjustments, in order: (1) Task 1 used a temporary narrow `#[expect(dead_code)]` on `SearchFilters::matches_flags` until Task 2 consumed it, costing one temporary lint exception that was removed; (2) Task 2 used a private borrowed `SearchEligibility` context because the storage GAT guard conflicts with a whole-`&self` borrow, costing one private context/helper-signature deviation with no public or runtime change; (3) Task 2 updated an obsolete unavailable-filter scenario in `tests/cli_reading.rs`, costing one extra test file; (4) Task 2 replaced an obsolete valid-filter row in `tests/cli_runtime.rs` with malformed input, costing a second extra test file; (5) Task 3 added two production-path error-propagation controls after initial controls were classified as fixture sensitivity, costing two focused mutation/restoration cycles and receipt updates; (6) the plan-specific ignored review workspace was retained to preserve historical evidence, costing local disk space.

## Runtime controls and restoration

- Omitting `clear_query_error` in `service/embedding.rs` failed the same-identity sequence at the successful-query cleared-row assertion (exit 101), after actual successful semantic inference. Exact source bytes restored; target passed.
- Historical fixture/oracle-sensitivity checks replaced injected late observation/comparison outcomes with success and failed `unwrap_err` (exit 101), then restored and passed. They changed test collaborators, so they are preserved as two fixture-sensitivity checks, not production error-handling mutation proof.
- Added production-path controls in `src/service/search.rs` while keeping the injected fixture errors unchanged: observation failure was swallowed as empty flags/issues success; comparison failure was swallowed as Equal. Each failed the covering scoped late-failure test at `unwrap_err` on successful a.md links (exit 101); each restored exact production bytes and passed the same target (1 passed, 0 failed). New diffs, logs and receipts are separate under `.artifacts/tk22/task3/evidence-strengthening/`.
- `verification.json` preserves the original three receipts and records their classification, plus both new production-path receipts with original/mutated/restored SHA-256 and exact commands. These hashes capture the mutation-time files; the final frozen manifest also includes the later helper-order/documentation cleanup, covered by final make verify. The initial fixture-sensitivity observation attempt removed the sole error-constructor use and was rejected by unused-import lint; it was not counted as runtime proof. The valid fixture-sensitivity control retained construction before returning empty success.
- Initial fallback runs corrected test-only assumptions (safe diagnostic excludes HTTP status; dimensions belongs inside parameters; Ollama adds truncate:false). These were fixture mistakes, not production defects or invented RED evidence. No production fix was required.

The independently checked aggregate is **13 production-path controls**: Task 1 five, Task 2 five, Task 3 three (query-error clearing plus the two new production error-propagation controls), with **two separate fixture-sensitivity checks**. Task 1 has six historical mutant/stub receipts; corrected-corrupt-fixture RED and its later post-GREEN control cover one behavior and are counted once. Raw historical receipts/logs are retained. `control-classification.json` enumerates the controls and verifies their runtime failure/restored-success logs.

The original input manifest contains **299 inputs** and remains unchanged, with every hash still matching the restored product `bee627e`. It omitted `clippy.toml`; the separately labeled `clippy-supplemental.json` records a later hash and exact byte equality with the tracked file at main base `cafd60b54032ca878bbca7fdda9a1839c4ffc658`. This supplemental check is **not** claimed to belong to the earlier pre/post frozen299 map. The historical tracked receipt is preserved at docs commit `503813f` and copied into the new evidence directory. No full suite rerun was needed for this evidence-only update; the earlier combined verify result remains tied to its original inputs/logs, and both new restored targeted runs passed.

## Files, API and architecture

Task 3 changes only `tests/search_filter_fallback.rs`, `tests/support/embedding.rs`, and `src/service/search/tests/scoped.rs`, plus acceptance/contract documentation. `HttpFixture::start_sequence(Vec<FixtureReply>) -> Self` is test-only, preserves `start`, consumes one reply per accepted HTTP request, rejects empty/exhausted sequences, and joins its owned worker on drop. No product trait/schema/runtime/dependency was added.

Reviewed the actual branch's `service/search_filters.rs`, `service/mod.rs`, `service/search.rs`, `service/search/ranking.rs`, `service/provenance.rs`, `cli/input.rs`, CLI handler path and `lib.rs::wire_searcher`. Domain remains I/O-free; application traits remain service-owned; eligibility only borrows filesystem/codec/provenance while storage owns the pinned reader. Services import domain/traits rather than concrete technology; concrete SQLite/Git/filesystem/embedding implementations are constructed in lib.rs and passed through SearchDependencies. CLI converts strict JSON to typed filters and invokes Searcher. No adapter orchestration, new layer, broad fallback catch or production visibility change was introduced by Task 3. Query diagnostics use the executor's separate guarded connection; the scoped read itself stays read-only. Synchronous I/O and X-1 link/envelope/error names remain unchanged.

## Limits and handoff

TK-28 owns status/problems presentation; this task proves the existing storage/executor lifecycle directly. No native storage runner was required because schema/storage/build/vendored/native inputs were unchanged. No fresh Linux run, historical isolated proof harness, real deadline, paid model, large-corpus or pilot/resource/G-1/G-2 certification is claimed. Existing historical evidence stays attached to its captured revision. All implementation-task reviews passed. The whole-branch review had one P3 metadata finding and no product/architecture findings; the actual verdict was **With fixes**, addressed by this documentation update. Scoped re-review of the metadata update remains pending. Merge/push/issues were not performed.
