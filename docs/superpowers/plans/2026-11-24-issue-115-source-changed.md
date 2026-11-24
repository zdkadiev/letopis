# Issue #115 Source Changes in Check and Status — Implementation Plan

> **For agentic workers:** Use `superpowers:executing-plans` or `superpowers:subagent-driven-development` for the approved execution method. Track progress with the checkboxes below.

**Goal:** Report changed source bytes in `check.result.problems` and `status.result.problems`, with at most 100 diagnostics in each response.

**Architecture:** Record changed addresses at the existing byte comparison. Build one diagnostic per affected knowledge element in the shared check/status collector. Each service applies the output limit once it has all findings for its command.

**Tech stack:** Pinned Rust toolchain, existing synchronous services, Serde JSON and the X-1 JSON Schema.

**Spec:** [Agreed design](../specs/2026-11-23-issue-115-source-changed-design.md). The decisions in this design supersede the earlier count and strict-mode proposals in [issue #115](https://github.com/zaurkadiev/letopis/issues/115).

## Global constraints

- Keep the existing comparison, Git reads, refresh scope, snapshot audit, batching and transaction boundaries. Use existing services and traits.
- A completed command returns `ok:true` and exit 0, including when it reports problems. Command failures use top-level `error` and a nonzero exit. Preserve the existing command-line arguments and operational warnings.
- The change is local to the working tree. Leave unrelated edits, Git history and storage schemas intact.

## Review focus

| Case | Expected result |
|---|---|
| Missing or unavailable snapshot | Existing snapshot diagnostic. The broader search/traversal recheck flag alone cannot establish a byte difference. |
| Two elements in one file, multiple addresses per element | Group by full element ID. Preserve all unique changed targets of each element, including line ranges. |
| A missing source and a changed source on the same element | Emit both applicable diagnostics. |
| More than 100 mixed findings | Filter, sort and deduplicate before limiting. Include status-only findings in the final ordering. |
| Existing consumers of `check.result.errors` | Migrate active consumers to `problems`, including scripts where a missing key would silently produce an empty report. |

## 1. Capture the exact changed addresses

- [x] In `src/service/provenance_test.rs`, extend the existing batch tests for edited live bytes and unavailable snapshots. Assert exact addresses for changed ranges and an empty set for equal bytes, missing addresses, unavailable bases and remote addresses. Retain independent assertions for flags and issues.
- [x] In `src/service/types.rs`, add `changed_sources: BTreeSet<SourceUri>` to `ProvenanceObservation`. In `src/service/provenance.rs`, initialize the set empty and insert the current address in the existing branch that compares two available hashes and sets `SearchFlag::SourceChanged`. Update the trait documentation in `src/service/mod.rs` and existing observation literals in test doubles.
- [x] Run `cargo test --locked --offline --lib service::provenance::`. Confirm existing live-read counts, bounded batch reuse and flag tests pass alongside the address assertions.

## 2. Build shared diagnostics and finalize each report

- [x] In the sibling service tests, cover the collection cases from the review table. Include an anchored element with two changed targets, another element in the same file, snapshot-only findings and mixed missing/changed sources. Assert explicit IDs, paths, targets and reasons. Keep the existing 257-owner test for complete observation across batch boundaries.
- [x] Extend `CheckObservations::find_source_problems` in `src/service/utils.rs`. For each observed knowledge element, emit its existing issues and, for a nonempty `changed_sources`, one `CommandProblem` with code `ValidationFailed`, message `knowledge source changed`, and details `element_id`, owner `path`, ordered `targets` and `reason: "source_changed"`. Use the set's order and the existing owner-path extraction.
- [x] Rename `CheckReport.errors` to `problems` in `src/service/types.rs` and update `Checker` documentation and service callers. Keep `StatusReport.problems` as the status collection.
- [x] Use one private shared finalizer in `src/service/utils.rs`: `get_reported_problems(Vec<CommandProblem>) -> Vec<CommandProblem>`. Move the existing sort and deduplication into it, then truncate to a single named limit of 100. Keep `collect_check_problems` uncapped. Call the finalizer in `src/service/check.rs` after scope filtering and in `src/service/status.rs` after adding embedding and configuration problems. Keep internal collection tests distinct from final report ordering tests.
- [x] Test 0, 100 and 101 findings, duplicates near the cutoff, and an out-of-scope owner that becomes visible when selected. In `status_test.rs`, include a status-only finding that sorts before the cutoff. A returned element retains all its targets even when it has more than 100. Run `cargo test --locked --offline --lib service::check::` and `cargo test --locked --offline --lib service::status::` after adapting the renderer in step 3 so the crate builds.

## 3. Publish the CLI contract and migrate its readers

- [x] In `src/cli/output.rs`, render `ResultBody::Check { problems }` from the renamed field. Update `src/cli/cli_test.rs` for clean, nonempty and warning-bearing successes and the existing failure envelope. Review `response.rs`, `commands.rs` and `lib.rs` for end-to-end propagation. Their existing report forwarding should suffice.
- [x] Update `src/cli/help.rs`, `docs/zkit/letopis/contracts/X-1-cli.md`, `docs/zkit/letopis/contracts/schemas/X-1-cli.yaml` and the current contract examples. Require `problems` in each successful result, set `maxItems: 100` on both problem arrays, and document the per-element diagnostic, empty array, filter and exit semantics. Exercise both schema definitions with 100 and 101 diagnostics independently of the service tests.
- [x] Update `README.md`, `skills/letopis/references/operations.md` and the Letopis, capture and dream skill instructions that reference `result.errors`. Explain `stale`, the snapshot-specific reasons, the output limit and the breaking field rename. Update the `check_links.py` reader under `skills/letopis/scripts/` and verify it recognizes a changed-source problem in a successful response.
- [x] Migrate active Rust assertions and Python readers/fixtures of the check wire format. Include `tests/cli/run.py`, `tests/cli/tk24_workflow.py` and `tests/performance/{contention,report,scenarios}.py`. Inspect repo-wide matches for `result.errors`, quoted `errors` keys and `CheckReport` field access. Preserve unrelated error fields and captured historical evidence. For tools that intentionally compare two binary revisions, interpret each revision's recorded format explicitly.

## 4. Verify through the binary and publish the issue summary

- [x] Run `cargo test --locked --offline --test provenance_commands`. Use the four existing draft tests for mixed findings and repair, scoped filtering, the combined cap and a clean corpus. Extend the shared-range fixture so that a covering `search` with `needs_recheck:true`, traversal from a linked start, `check` and `status` each match explicit expected IDs on the same revision and working tree. Include the case where a missing snapshot legitimately adds a search flag but yields its specific report diagnostic.
- [x] Run `python3 -B docs/zkit/letopis/checks/contracts/check_contract_examples.py`, then run `python3 -B -m unittest discover -s tests/cli` and `python3 -B -m unittest discover -s tests/performance` for the updated Python consumers. Confirm `check --help` and `status --help` describe the same problem limit. Record any environment prerequisites and actual pass/fail results.
- [x] After the tests pass, manually disable recording a changed address. Run `cargo test --locked --offline --test provenance_commands check_and_status_report_changed_elements_alongside_existing_problems -- --exact` and confirm the changed-element assertion fails. Restore that edit. Change the cap to 101, run `cargo test --locked --offline --test provenance_commands check_and_status_limit_the_combined_problems_to_one_hundred -- --exact`, confirm 101 versus 100, then restore the limit and rerun both tests.
- [x] Run `make fmt`, `make verify` and `git diff --check` on the restored tree. Report exact commands and outcomes, including strict Rustdoc and release tests. The storage runner applies only if its storage/native inputs change.
- [x] Comment on issue #115 with the final wire format, the 100-entry cap, exit behavior and verification results. Identify the earlier acceptance criteria superseded by the design. Keep the issue open and report that the changes are local.
