# TK-12 combined acceptance

Status: in progress. Task 5 and whole-branch reviews have reported approval; the final ordering fix awaits scoped re-review and controller-owned acceptance-document finalization. This report does not authorize integration or mark TK-12 complete.

## Scope and source

Task 5 started from `9ff939e90fafbdb9ee1c460e17214af1c25b3838` on `TK-12`. Historical receipts stay immutable under `.artifacts/tk12/`. The unrelated primary checkout and untracked task-root `.letopis/` are preserved.

The required acceptance extends the existing CLI/storage runners. No dependency, runtime, schema, provider or configuration change is introduced. `tests/cli_reading.rs` and `tests/cli/negative_controls.py` are controller-confirmed Task 5 scope, although the original Files list omitted them.

Named instruction fixes return per-file captured knowledge before caller aggregation (preserving read/write/drop order), rename search test helpers with verbs, and add redacted public `QueryEmbeddingRequest` Debug. Query text, model and endpoint remain excluded from Debug. The final external-public declaration audit also added safe derived `SnapshotEntry` Debug and a compile-availability assertion in the existing Git snapshot test. Preexisting unchanged `SnapshotFile`/`EmbeddingService` declarations are outside this narrow fix.

## Test chronology

Existing inspection/search baseline: 1 inspection unit, 11 search units, 5 inspection integration and 12 search integration tests passed before the refactor. No fabricated behavioral RED is claimed for these behavior-preserving refactors.

Receipt-parser tests first reported the missing interface, then a stub produced six expected assertion failures. Final Python runner suite passed 18 tests. Product reading observations carry command, classification, exit and actual binary JSON; the Rust observer asserts exactly one JSON line and empty stderr before emitting them. Python selects each command's X-1 schema and rejects wrong shape/exit/classification and injected-handler substitution. Transport also runs the copied executable.

Debug test chronology is explicit: missing Debug first failed compilation; an initial incorrect fixture key (`max_attempts`) caused fixture-parse failures, including a service test run with 59 passed / 1 failed / 2 ignored. These are setup failures, not redaction proof. Corrected `max_retries` plus a compiling leaky derive failed the private-text assertion; the manual redacted implementation then passed (60 service tests / 2 ignored). Full logs are in `task5-development/`.

## Compiling controls

`task5-controls-03` proves captured-config reread, candidate-hash validation bypass and cache reopening after inference: clean compile, intended runtime RED, exact source-byte restoration, same test GREEN. `task5-controls-live-04` proves live inspection reread with the same sequence; the saved `run-live-control.py` selects exactly that descriptor in memory. The product Rust input hashes are identical between these runs; only the control helper setup differs. Final-source changes afterward add only SnapshotEntry Debug in service/mod.rs and its Git-test availability assertion; the four exact control-target source files are unchanged. This is a documented source delta, not a claim of identical complete inventories.

Earlier control receipts remain preserved: run01 anticipated a rank assertion but reopening correctly failed earlier with `InternalError: invalid search index data`; run02 had an invalid Markdown error conversion in the disposable mutation; run03's live mutation failed strict unused-variable compilation before the run04 correction. None counts as runtime proof. Repeated successful controls are counted once. Historical null-output/kernel-exclusion controls were retained, not rerun as new TK-12 proof.

## Specification and Review Focus mapping

| Requirement or focus | Executable evidence |
| --- | --- |
| Literal OR/NEAR/quotes, Cyrillic, accents, punctuation | `literal_query_quotes_unicode_tokens_without_fts_operators`, `fts_literals_work_on_native_tokenizer`, `empty_query_never_calls_match_or_provider`, actual `cli_reading` search |
| Captured model/dimensions, publication/reset during inference | `captured_query_never_rereads_or_resets_configuration`, `captured_vector_configuration_requires_matching_identity_and_native_dimensions`, `search_consistency` interleavings and copied controls |
| Invalid Markdown with retained old SQL | `invalid_live_knowledge_retains_pinned_link_as_stale`, `inspect_rejects_invalid_current_file_instead_of_using_old_sql`, publication consistency test |
| Unsupported filters/path-shaped source selectors | CLI parsing unit tests, `unsupported_filters_fail_before_repository_initialization`, `inspect_distinguishes_referenced_sources_from_unknown_ids` |
| Lost stdout after publication/capture then retry | `lost_output_then_retry_preserves_corpus_user_state_and_revision`, CLI copied-executable transport, retained-tree Git tests |
| Full-channel reciprocal ranking and final limit | Search ranking units, actual Ollama/OpenAI fixture query tests, duplicate-hash and stale-vector tests |
| All-or-nothing retained capture and exact metadata | `knowledge_inspection`, per-file capture ordering unit, actual `binary_inspect_returns_current_retained_tree_metadata` |
| Physical ownership | Service traits own boundaries; concrete search/inspection services orchestrate interface calls; Git owns object/ref argv; `lib.rs` builds actual independent storage/filesystem/provider collaborators; CLI handlers call service interfaces |

The optional collaborator trait objects preserve existing composition. Search uses concrete/generic collaborators; no production unwrap/expect was introduced. External new public-type coverage is checked; the previously reported oversized Task 2 test file remains explicitly tracked for final reviewer triage. The line scan found no function over 60 lines among 30 changed Rust files; it is supplementary, not an architecture proof.

## Superseded platform evidence

`final-a16864d58444/cli-host` and `cli-linux` passed on the source before SnapshotEntry Debug. The host had 41 expected nested exits and 12 real product reading observations. The owned serial driver was paused; Linux had already begun at that boundary and was allowed to finish. Both PASS receipts are preserved, then the owned parent was terminated; storage suites never started. All four final suites are rerun on `final-ddf6ef0fe779` after the narrow API fix.

## Linux fixture failure and unchanged-source retry

The first final `storage-linux` receipt failed root debug test `git::cli::tests::hardens_tree_and_ref_writes_with_git_fsync_settings` at `src/git/cli/tests.rs:455`: `Git spawn failed`, `ExecutableFileBusy` (180 passed / 1 failed / 3 ignored). It failed before the argv assertions. Both the test and `TempRepo::write_script` are unchanged from main. No product or fixture workaround is made, and the cause beyond the observed spawn error is not claimed proven.

The exact failing test passed 20/20 focused repetitions in retained image `sha256:576dbe0890379e5d4e6ceb980d80765bf06f9360e53f97cca2590da6f998d64a`, with `--locked --offline --lib git::cli::tests::hardens_tree_and_ref_writes_with_git_fsync_settings -- --exact --test-threads=2`, in a single owned network-disabled container. Evidence: `storage-linux-repro-01/repeated-test.log`. The fresh full unchanged-source runner at `final-ddf6ef0fe779/storage-linux-retry-01` passed; the original failure remains retained and is not relabelled PASS.

## Final-source gates and remaining acceptance

The committed source is `facad76bddf3f02b7c206cd152070e8a447929b7`. The four final platform runners were invoked while HEAD was `9ff939e90fafbdb9ee1c460e17214af1c25b3838` and Task 5 source/runner changes were dirty; manifest and dirty-path hashes verify those inputs are byte-identical to `facad76`. No product rerun was needed. The runners passed: `final-ddf6ef0fe779/{cli-host,cli-linux,storage-host,storage-linux-retry-01}`. Root debug and release profiles each reported 454 passed / 0 failed / 4 ignored. Their nested runner counts are kept separate. CLI host/Linux receipts have matching full input maps; the storage Linux container map is the exact Docker COPY subset, omitting 110 historical/CLI test paths and missing no changed Rust, root Rust or support input. Controller hash/command accounting is in `.artifacts/tk12/sdd-final-a38f3f7/final-platform-receipt-check.json`.

Changed-scope mutations completed on `facad76`: 247 candidates, 87 caught / 0 missed / 0 timeout / 160 unviable, exit 0 in 1599.8997 seconds. The audit found zero marker-bearing logs for the named Git executable-busy/spawn cases. Exact outcome and driver hashes are in `.artifacts/tk12/sdd-final-a38f3f7/task-5-changed-receipt-check.json`.

The initial whole-crate process was interrupted after 913/2208 outcomes (415 caught / 498 unviable / 0 missed / 0 timeout); that raw leg remains INTERRUPTED and is preserved under `.artifacts/tk12/mutation-whole-facad76-recovery-01/interrupted-original/`. The version-pinned `cargo-mutants --iterate` recovery ran the remaining 1295 candidates on the same source and finished with exit 0: 247 caught / 1048 unviable / 0 missed / 0 timeout, baseline success, minimum timeout 274s and automatic test timeout 351s. The two receipts cover the exact 2208-candidate manifest: the legs are disjoint, with no missing, extra or duplicate identities. Aggregate outcome: 662 caught / 1546 unviable / 0 missed / 0 timeout / 0 successful survivors. The infrastructure audit found no runtime executable-busy, text-file-busy or fixture-spawn failures. Four `Git spawn failed` occurrences were in mutation-diff text only; three corresponding candidates were caught by the expected Git version guard and one candidate failed compilation as unviable. Detailed candidate, timing and log accounting is in `.artifacts/tk12/mutation-whole-facad76-recovery-01/whole-recovery-accounting.json` and `whole-infrastructure-audit.json`.

## Final ordering-only test fix

After the approved whole-branch review, the final test in `tests/knowledge_inspection.rs` was moved above the fixture helper section in commit `a38f3f764d9fe5a61622faa9c496489ce256fafd`. No test/helper/type names, attributes, bodies or fixture bytes changed; the moved test block and helper block were byte-checked against the pre-move file. The file SHA256 changed from `e5951ce3cc7dfc3da7e09b5c20a062246f4cedca3ecd3ca6320a07d796dae7cd` to `ebad62daefff384e83cda56f6c5eb042c0fba10e3784d0ddfd39f757221b3248`; product `src/` remains identical to `facad76`.

Fresh focused `cargo test --locked --offline --test knowledge_inspection` passed 5/5. Fresh `make verify V=1` exited 0; each root debug and release profile reported 454 passed / 0 failed / 4 ignored (nested filtered helper totals excluded). Full logs and hashes are in `.artifacts/tk12/final-ordering-fix-01/`. No platform runner or mutation rerun was performed because this was a verified declaration-order-only test move; the historical platform and mutation receipts remain tied to `facad76` inputs. Final scoped re-review is approved; the acceptance-document commit records this completed status.

The reviewer’s M1 remains deferred maintenance: before further embedding-test expansion, split cohesive captured-query cases from the oversized integration test file with shared helpers. This fix does not address or claim to close M1.

## Limits

Acceptance targets macOS ARM64 and Linux glibc ARM64 with controlled HTTP providers. It does not prove live model usefulness, minimal-hardware performance, x86, power loss or handled SIGINT. Baseline knowledge stale is implemented; full source flags/provenance/filtering remain TK-20/TK-21/TK-22. Complete source/mixed inspection remains TK-24, full check TK-19, agent workflow TK-14.

## Final review and disposition

The single broad review covered all 47 physical files and found no Critical or Important issue. Task 5 independently passed specification and quality review. The final scoped review confirmed the ordering-only fix and found no new breakage. Full reports, review packages, the ledger and controller checks are preserved in `.artifacts/tk12/sdd-final-a38f3f7/`.

| Finding | Final disposition |
| --- | --- |
| Task 5 M1: stale derivative accounting digest | Corrected; final whole-branch review independently verified the stable raw digest. |
| Task 5 M2: platform commit chronology | Corrected: platform runs captured `9ff939e` plus dirty bytes later committed as `facad76`. |
| Final M1: embedding tests at 522/877 lines | Deferred, not fixed: extract cohesive captured-query scenarios before further embedding-test expansion. This remains a nonblocking maintenance item in the task card. |
| Final M2: inspection helper above the last scenario | Fixed in `a38f3f7`; byte-identical test/helper relocation, focused 5/0/0 and fresh host `make verify V=1` 454/0/4 per profile, then scoped approval. |
| Original Linux Git fixture ETXTBSY | Failure retained; cause and permanent fix unproven. Twenty focused replays and one unchanged-source full retry passed. |
| Historical Task 1 TDD gap and intermediate mutation survivors | Retained as historical exceptions/results, not rewritten into full TDD compliance or fabricated clean runs. |

All five roadmap acceptance criteria and all five implementation tasks are accepted for the approved baseline. Delivered behavior is real metadata-only CLI search with literal FTS, optional captured query embedding and reciprocal-rank fusion on one read view, plus canonical knowledge inspection through a retained Git tree. Future filtering/source inspection, traversal, full diagnostics, cancellation, performance and pilot work retain their roadmap owners.

## Controller disposition of final-review boundaries

Every Declined to judge item was evaluated; none extends the acceptance claim.

| Reviewer boundary | Controller decision |
| --- | --- |
| Source freshness, provenance, filters | Accepted boundary: TK-20/TK-21/TK-22; knowledge stale and fail-closed unsupported restrictions are covered. |
| Complete source/mixed I-12 capture | Accepted boundary: TK-24; current multi-ID inspection is knowledge-only. |
| Full check, traverse, status | Accepted boundary: TK-19/TK-23/TK-28 respectively; baseline is not complete v1 CLI. |
| Agent workflow and corpus preparation | Accepted boundary: TK-14 and subsequent target-corpus work; not inferred from fixtures. |
| Live model usefulness and resource measurements | Accepted boundary: later pilot, TK-34 memory and TK-35 latency; controlled providers establish correctness only. |
| Other platforms/filesystems | Accepted evidence limit: macOS ARM64/Linux glibc ARM64; Windows/x86/NFS/other distributions unverified. |
| Physical power loss and handled SIGINT | Accepted boundary: TK-38 and TK-29; current process/error tests do not prove these. |
| Atomic live multi-file instant or freshness after observation | Accepted contract: immutable captured tree and pinned cache, no whole-corpus atomic-instant guarantee. |
| First-ever search inference without native dimensions | Accepted structural eligibility: no search HTTP; dimension establishment covered via captured-query API. |
| Avoid HTTP for all-obsolete stored vector hashes | Accepted structural eligibility: one provider attempt may yield an empty current-hash join; no O(n) preflight added. |
| Permanent ETXTBSY fixture fix/root cause | Accepted limit: cause unproven; original failure retained, 20 focused passes and one full unchanged retry establish nonreproduction only. |
| Retrospective TDD and all historical proof harnesses/equivalences | Accepted limit: historical exceptions retained; current acceptance does not rewrite chronology or imply unrelated suites were rerun. |
| Unrelated preexisting Debug and reading-order cleanup | Accepted scope: apply current instructions to new/touched code; current local M1/M2 tracked separately. |

## Rulings I made

Chronological decisions and their costs; complete verbatim ledger paragraphs are retained in `.artifacts/tk12/sdd-final-a38f3f7/rulings-exhaustive.md`.

| # | Decision | Reason | Cost if wrong |
| --- | --- | --- | --- |
| 1 | Use approved Subagent-driven execution | The recommended method was accepted | Additional execution/review contexts |
| 2 | Use 2 mutation jobs, 2 test threads, minimum 274s | Reuse measured stable settings | Longer wall time |
| 3 | Correct the Cargo/libtest argument separator | Original baseline rejected argument routing | One failed setup baseline retained |
| 4 | Run one full mutation gate on combined final source | Avoid a superseded intermediate full run | Interactions discovered later |
| 5 | Accept Task 1 with its historical TDD exception visible | Current behavior has independent evidence | Reduced historical RED confidence |
| 6 | Add four compiling query controls | Most automatic candidates were unviable | Additional focused validation time |
| 7 | Use a private search test module | Keep production ownership focused | One navigation step/file |
| 8 | Choose structural vector-channel eligibility | Avoid an O(n) hash preflight | One unnecessary HTTP attempt on obsolete hashes |
| 9 | Strengthen the malformed-vector message oracle | Different public diagnostics are observable | Tests couple to diagnostic wording |
| 10 | Apply current instructions without copying primary changes | Preserve unrelated user work | A local rank-helper refactor |
| 11 | Update obsolete unavailable-inspect tests | Inspect is now a real route | Old unavailable-case coverage is replaced |
| 12 | Guard serde sequence-shaped options | An empty array otherwise deserializes as a struct | One bounded additional token parse |
| 13 | Fix output-parameter and noun-helper gaps before freeze | Follow current instructions before expensive gates | Refactor accepted code and revalidate |
| 14 | Use explicit exhaustive CLI root selection | Future routes need an explicit root policy | Extra match arms and verification |
| 15 | Include actual reading observations in Task 5 ownership | The runner needs product executable receipts | Narrow test instrumentation |
| 16 | Add redacted public query-request Debug | Public Debug must not expose query/config payloads | Manual formatting maintenance |
| 17 | Extend the existing control helper | Reuse disposable compile/RED/restore/GREEN machinery | Control-descriptor maintenance |
| 18 | Add safe SnapshotEntry Debug before final gates | Close a late public-type audit gap | Superseded platform receipts and rerun time |
| 19 | Allow one unchanged-source full Linux retry | Focused replays did not reproduce ETXTBSY | Unfixed fixture flake and retry time |
| 20 | Recover with supported --iterate | Preserve 913 classified outcomes and run remaining 1295 | Cross-leg accounting risk; exact union verified |
| 21 | Fix final test ordering; defer larger embedding test split | Bound the final correction and retain explicit maintenance | Navigation debt and host-only reordered-test validation |
| 22 | Accept all 13 explicit review boundaries | Keep baseline and later/unmeasured guarantees separate | Unsupported expectations if boundaries are overlooked |
| 23 | Prioritize public-before-private for a private accessor call | Resolve conflicting ordering rules without duplicate access | One upward jump to a trivial accessor |
