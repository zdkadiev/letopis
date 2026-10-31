# Explicit Markdown link paths — issue #69 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Execution approval:** 2026-10-31, Subagent-driven with per-task review. Implementation and task reviews complete; final whole-branch review pending.

**Goal:** Use mandatory `path` as the sole graph target, compare local navigation file paths, and persist mismatches for `check`/`status`.

**Architecture:** Extend the existing pure Markdown codec and diagnostic classification. Reuse domain address validation and file-level atomic publication; status remains observation-only. No new service, persistence schema, runtime, dependency or legacy parser.

**Tech Stack:** Rust 1.95.0, pinned pulldown-cmark 0.13.4, existing SQLite/SeaQuery storage and synchronous CLI; Python 3.12 for fixture tools.

**Spec:** [Specification accepted for planning](../specs/2026-10-31-issue-69-markdown-link-path-design.md), including the 2026-10-31 path-only comparison/status refinement. User authorized continuing to planning; implementation awaits separate plan approval.

**Workspace:** `/Users/zdkadiev/.codex/worktrees/issue-69/letopis`, branch `issue-69`, product base `a82565ee8f590b959d2fd1cdf3e50cbe5d800187`.

## Global Constraints

- Title grammar: `letopis:type=TYPE;path=ADDRESS`, TYPE `[a-z][a-z0-9_]*`; path is mandatory and terminal. No legacy fallback, parameter map or metadata percent decoding.
- After `;path=`, semicolons, `=`, `%` and even `;path=` are address content. Existing domain rules decide whether that complete address is valid.
- Canonical IDs stay repository-root-relative; only the visible local navigation reference is resolved against its containing document. Ignore both fragments for comparison. Do not inspect file existence or viewer behavior.
- Navigation path decoding and rejected forms follow the specification exactly. Remote SourceUri targets skip local comparison and remain opaque, with no fetch.
- Mismatch: `LinkPathMismatch`, reason `link_path_mismatch`, message `Markdown link path does not match letopis path`. Store via existing `files.error`; expose through `status.result.problems`/check findings with no address payload.
- Status never refreshes, clears or writes. Findings appear/clear only after publication by a refreshing command; retain the existing last-valid invalid-file policy.
- Upgrade uses explicit full reindex, not cache deletion, a parser-version framework or automatic corpus rewriting. Preserve vectors, attempts, identities and honest source bases.
- Follow [AGENTS.md](../../../AGENTS.md), [structure/review](../../zkit/letopis/architecture.md#program-structure), synchronous I/O, meaningful constants, verb names, focused functions (about 40, maximum 60 lines) and Given/When/Then tests.
- Scope changes go back to the user. No paid/model runs, destructive maintenance of real user caches, historical evidence rewrites, merge, push or issue closure without authorization.

## Review Focus

1. Section IDs include `#anchor`; navigation must use the containing file directory, not the anchor or caller cwd — Task 1.
2. A literal percent filename and an encoded space must not be confused; decode only navigation once, and split its fragment before decoding — Task 1.
3. Matching but missing files and arbitrary navigation fragments must pass this new check; existing dangling/provenance findings must remain independent — Tasks 1 and 3.
4. Cached mismatch classification and read-only status must agree across a fresh process, author edits and subsequent publication — Task 3.
5. Generators, fixed-size fragments, vector reuse and old golden receipts must survive the breaking input change without inventing expected output from production code — Tasks 2 and 3.

## File ownership and order

| Task | Files and responsibility |
| --- | --- |
| 1 | `src/markdown/links.rs`: title decoding/target construction/path comparison; `src/service/model.rs`: diagnostic; `src/service/graph_diagnostics.rs`: cached-message classification and its tests. `tests/markdown_links.rs`, `tests/markdown_elements.rs`, `tests/markdown_search.rs`, `tests/fixtures/markdown/{whole,sections,crlf,unicode,no-final-newline,references,code}.md`, `tests/fixtures/markdown/expected.json`: inspect and update only active positive inputs/affected golden bytes. Create `tests/markdown_link_paths.rs` for the focused new behavior. |
| 2 | Migrate active executable input producers listed below; preserve each scenario's intended targets and assertions. No production changes. |
| 3 | Create `tests/markdown_link_lifecycle.rs`: real CLI/storage mismatch, upgrade and vector-reuse proof. Reuse `tests/support/reading.rs`, `tests/support/documents.rs`, `tests/support/provenance.rs`; add helpers only where at least two actual consumers justify sharing them. |
| 4 | Active X-4/related descriptions, skill and corpus migration; new `docs/zkit/letopis/checks/issue-69/report.md`. Final verification and independent review. |

Task 2's current executable migration inventory:

- `src/commands/construction_tests.rs`, `src/service/inspection/tests.rs`, `src/service/source/tests.rs`, `src/service/document_refresher/tests/process.rs`.
- `tests/document_refresh.rs`, `tests/source_lifecycle.rs`, `tests/cli_traversal.rs`, `tests/graph_diagnostics.rs`, `tests/cli_reading.rs`, `tests/traversal.rs`, `tests/knowledge_inspection.rs`, `tests/maintenance_workflow.rs`, `tests/corruption_recovery.rs`, `tests/git_no_subprocess.rs`, `tests/status_command.rs`, `tests/check_lifecycle.rs`, `tests/source_inspection.rs`, `tests/check_command.rs`, `tests/source_reading.rs`, `tests/knowledge_search.rs`, `tests/read_verification.rs`.
- `tests/support/documents.rs`, `tests/support/provenance.rs`, `tests/fixtures/markdown/thin_slice/cache.md.in`, `tests/fixtures/markdown/thin_slice/flow.md.in`, `tests/cli/tk24_workflow.py`.
- `tests/target_corpus/generate.py`, `tests/target_corpus/validate.py`, `tests/target_corpus/test_generate.py`, `tests/performance/controls.py`, `tests/performance/test_controls.py`.

Inventory with `rg -l 'letopis:type=' src tests examples skills letopis` before editing. Additional real consumers found by that search join the responsible task; record them. Do not modify files merely because a historical/fenced example contains the old token. No runtime/test-helper compatibility rewriter may silently convert legacy inputs.

## Task 1: Implement the single link grammar and pure path comparison

**Interfaces:** Keep public `markdown::parse(&RepoPath, &[u8]) -> Result<Option<ParsedFile>, Vec<Diagnostic>>` and MarkdownCodec unchanged. `links::collect` stays private with its existing signature. Use `element.id` (file part before `#`) to obtain the containing file, avoiding an extra argument through the parser. The private resolver takes the typed source ID and extracts its owning file internally. Private helpers in `links.rs`:

```rust
fn parse_link_title(title: &str) -> Result<(EdgeType, &str), DiagnosticKind>;
fn verify_navigation_path(source: &KnowledgeId, destination: &str, target: &EdgeTarget)
    -> Result<(), DiagnosticKind>;
fn resolve_navigation_path(source: &KnowledgeId, destination: &str) -> Result<RepoPath, DiagnosticKind>;
fn decode_navigation_path(raw: &str) -> Result<String, DiagnosticKind>;
```

Deriving the owner from the already validated element ID is a planning simplification of the spec's document-path plumbing, not a new target-resolution rule. Keep all four helpers private, callers above callees; no new wrapper or module is required.

- [x] **Step 1: Add red parser tests and migrate the focused tests' positive input syntax.**

In `tests/markdown_link_paths.rs`, use existing `build_whole_file`, `parse_file`, `assert_error`. Include tests named `uses_path_as_the_only_target`, `compares_paths_from_the_containing_section_file`, `ignores_navigation_fragments_and_target_existence`, `rejects_legacy_titles`, `preserves_terminal_address_bytes`, and `decodes_only_navigation_paths_once`. Independent positive/negative cases:

| Containing file | Destination | Metadata path / type | Expected |
| --- | --- | --- | --- |
| `letopis/cache/note.md` | `../../src/cache.rs#anything` | `src/cache.rs#L2-L4` / external | Source `src/cache.rs#L2-L4` |
| `letopis/cache/note.md` | `other.md#viewer` | `letopis/cache/other.md#expiry` / related_to | Knowledge target with `#expiry` |
| `letopis/cache/note.md` | `../../src/other.rs` | `src/cache.rs` / external | LinkPathMismatch |
| `letopis/cache/note.md` | `../../src/cache%20key.rs` | `src/cache key.rs` / external | Source with a literal space |
| `letopis/cache/note.md` | `../../src/cache%2520key.rs` | `src/cache%20key.rs` / external | Source with literal `%20` |
| `letopis/cache/note.md` | `../../../escape.rs` | `escape.rs` / external | LinkPathMismatch |
| `letopis/cache/note.md` | `#different` or empty destination | `letopis/cache/note.md#other` / related_to | Knowledge `#other`, provided it is not the owning element |

Place section cases under source anchor `#owner`; the other target need not exist. Also table-test malformed `%`, `%GG`, invalid UTF-8, encoded slash/backslash/control, `/absolute`, `//host`, `https://host/file`, a local query, valid Unicode and a `%3F` filename. Keep metadata bad-address tests distinct from navigation mismatch tests: canonical target parsing happens first. Pin remote URL `https://Host/a%20b?q=x;y;path=z#part` verbatim with an unrelated navigation destination; remote comparison is skipped. Keep literal assertions such as:

```rust
assert_eq!(parsed.edges.first().unwrap().get_target().as_str(), "src/cache.rs#L2-L4");
assert_error(result, DiagnosticKind::LinkPathMismatch);
assert_eq!(DiagnosticKind::LinkPathMismatch.get_code(), "link_path_mismatch");
```

- [x] **Step 2: Run the focused tests and confirm failure for the intended missing behavior.**

Run `cargo test --locked --offline --test markdown_link_paths`. A missing diagnostic/old grammar rejection is expected at this stage; dependency/toolchain failures are infrastructure failures, not a red behavior test.

- [x] **Step 3: Implement the parser/helpers and diagnostic mapping.**

Split once at `;path=`, validate the exact `letopis:type=` prefix/type, require a nonempty terminal address, and parse that whole address with existing domain types. Verify only local file paths. Implement one-pass navigation UTF-8 percent decoding per the spec, without a URL library or filesystem calls; do not decode metadata. Before constructing the edge, run the comparison and map its reason to the inline-link byte range. Add LinkPathMismatch's fixed message/code and include it in `PARSER_KINDS` and the cached-message test table in `graph_diagnostics.rs`. Do not persist raw addresses.

- [x] **Step 4: Run focused green tests, including search rendering and cached-reason mapping.**

Run `cargo test --locked --offline --test markdown_link_paths --test markdown_links --test markdown_elements --test markdown_search` and `cargo test --locked --offline --lib classifies_every_known_cached_message_without_changing_parser_diagnostics`. All selected tests must pass. Extend the search test to assert unchanged literal visible text/hash for equivalent link-only syntax changes and a changed exact element hash. Independently recompute affected exact-byte golden hashes, preserving CRLF and no-final-newline fixtures.

- [x] **Step 5: Mutation-check the behavior and restore the tree.**

Temporarily remove comparison, use the destination instead of path, then percent-decode metadata in separate compiled mutations. Each relevant new test must fail at its expected behavior assertion. Restore the exact production bytes and rerun affected tests; record results. Do not treat compiler errors as proof of sensitivity.

- [x] **Step 6: Commit only Task 1's intended files.**

Run `make fmt`, rerun focused tests, `git diff --check`, then commit as `feat: parse explicit link paths and diagnose navigation mismatches`. Remaining old positive consumers belong to Task 2: a focused green result is not the final root gate.

## Task 2: Migrate executable fixtures and independent corpus tooling

**Interfaces:** Consume Task 1's parser unchanged. Existing fixture helper signatures remain stable unless the actual containing path is required; fix each caller explicitly rather than adding a legacy-to-new conversion layer. Existing generator and manifest APIs stay the same.

- [x] **Step 1: Inventory and classify active declarations.**

For each inventory file, record the containing Markdown path, intended canonical target, and whether the declaration is positive input, deliberately invalid input, visible text in code, or a historical artifact. For positive local links, set path to the existing intended root-relative ID and destination to the appropriate document-relative path. Repair nested thin-slice links explicitly; preserve anchors, line ranges, relation names, labels and missing-target scenarios. Retain legacy syntax only in named negative tests or intentionally quoted content.

- [x] **Step 2: Add red generator/oracle assertions and confirm failure.**

Extend `test_smoke_has_literal_counts_targets_and_exact_utf8_sizes` with literal examples of document-relative source and knowledge links plus canonical path fields. Add a validator-negative case where only the navigation file path disagrees but manifest byte hashes are independently updated, so failure is not merely a stale digest. Run `python3 -B -m unittest discover -s tests/target_corpus -p 'test_generate.py'`; new-format assertions must fail before generator changes.

- [x] **Step 3: Migrate all active positive producers and the independent validator.**

Update Task 2's inventory. In the generator, add path and repair destinations before final fragment padding; retain the exact 2,000-byte profile and deterministic packing. Update the independent validator to check the declared canonical targets and navigation agreement without importing production parser logic. Update performance edge masking for the new title while preserving byte counts and the intended causal variant. Do not regenerate old .artifacts, profiles, baseline hashes or published receipts.

- [x] **Step 4: Run root tests and the affected model-free Python suites.**

Run `cargo test --locked --offline`, `python3 -B -m unittest discover -s tests/target_corpus -p 'test_*.py'`, and `python3 -B -m unittest discover -s tests/performance -p 'test_*.py'`. Also run `python3 -B tests/cli/tk24_workflow.py --binary "$PWD/target/debug/letopis" --evidence /private/tmp/issue69-tk24-NEW` using a fresh evidence path and the prepared schema-validation Python environment (`jsonschema`, `yaml`). Expect pass; report explicit optional skips. Use fresh temporary fixtures, no model/provider calls or full-scale drain.

- [x] **Step 5: Mutation-check the independent fixture oracle.**

Change one generated destination while leaving metadata canonical, update the input digest independently, and confirm the new validator-negative test rejects lexical disagreement. Restore and rerun it. This must detect a path defect beyond a manifest checksum mismatch.

- [x] **Step 6: Commit the fixture/tooling migration.**

Run `make fmt`, rerun changed focused tests and `git diff --check`; commit only classified active consumers as `test: migrate executable fixtures to explicit link paths`. A remaining old token is acceptable only with an explained negative/historical purpose.

## Task 3: Prove durable status diagnostics, upgrade and vector reuse

**Interfaces:** Use `ReadingRepo::{new,write,invoke,open_storage,seed_vectors}` and public `Storage`/`CacheWrite`/`StorageRead` capabilities. `invoke(&["status"], b"{}")` returns a process output; inspect literal JSON fields. Do not change StatusService, SQLite schemas or CLI response types merely to add tests.

- [x] **Step 1: Add lifecycle tests with exact public outcomes.**

Create `tests/markdown_link_lifecycle.rs` with:

- `status_reports_published_path_mismatch_until_a_refresh_repairs_it`: publish a valid nested note, change its destination to a different file, refresh via check, and assert files.error's exact safe message, invalid state and retained old graph. Assert this literal status item:

```json
{"code":"validation_failed","message":"Markdown link path does not match letopis path","details":{"path":"letopis/cache/note.md","reason":"link_path_mismatch"}}
```

- The same test must observe status in a new process, fix only author bytes, show status still reports the finding without a revision change, then refresh and show the reason disappears. Confirm unrelated valid files publish and raw conflicting URLs/titles never appear in diagnostics.
- `legacy_titles_never_publish_into_a_fresh_cache`: legacy type-only input has InvalidLinkTitle and creates no partial edge/knowledge projection.
- `reindex_reparses_unchanged_pre_upgrade_rows`: independently seed a stale FileRow/error (or old valid projection) through CacheWrite with the exact current fixture stat/hash, then prove ordinary refresh can leave it while reindex clears/reclassifies it. Use temporary fixture FileTimes for a non-racy stat precondition; no wall-clock sleeps, old product binary or runtime compatibility mode. Seed cache fields explicitly, not via production title parsing.
- `link_syntax_migration_reuses_vectors_and_preserves_source_basis`: use a root-level fixture with independent static old cache data, same visible label/text, a real verified source tree and a sentinel vector `[1.0, 0.0, 0.0]`. After new-form reindex, assert exact search text/hash, vector, identity, attempts, created_at and source_snapshot remain unchanged while exact element bytes/hash reflect the authored syntax change. Use existing source-lifecycle sentinel patterns; never delete real caches.
- `matching_missing_targets_keep_existing_diagnostics`: lexical agreement with nonexistent knowledge/source paths passes the parser, while existing `dangling_target`/`source_not_found` still appear in check/status. A mismatched path adds its own invalid-file reason instead of replacing those global rules.

Keep each scenario and helper within the project's function-size guidance; split genuinely sequential phases into focused tests instead of hiding assertions inside one large helper.

- [x] **Step 2: Run focused tests and investigate genuine failures.**

Run `cargo test --locked --offline --test markdown_link_lifecycle --test check_lifecycle --test status_command --test source_lifecycle`. These tests may already pass after Task 1 because they verify existing publication contracts; do not manufacture a failing production change merely to get a red run.

- [x] **Step 3: Mutation-check persistence, classification and reindex sensitivity.**

Individually bypass cached mismatch classification, suppress storing that file error, and make reindex use incremental detection. Relevant tests must fail on the exact asserted reason/state, then pass after byte-for-byte restoration. Do not weaken the default invalid-file or read-only status rules to satisfy tests.

- [x] **Step 4: Verify purity and absence of additional I/O.**

Review actual imports/calls in the Markdown helpers; no fs/Git/network/service concrete type is reachable from comparison. Test matching missing targets and ignored malformed navigation fragments. Record that current graph/source checks continue to perform their own existing I/O; this is not a claim that the entire command is I/O-free.

- [x] **Step 5: Commit lifecycle proof.**

Run `make fmt`, rerun focused tests and `git diff --check`; commit as `test: verify link mismatch status and explicit reindex upgrade`. If a real production gap emerges, fix only its owner, rerun required root checks and record that justified change.

## Task 4: Update active contracts/corpus and complete verified handoff

**Files:** `docs/zkit/letopis/contracts/X-4-markdown.md`, affected current link descriptions in `docs/zkit/letopis/contracts.md`, `docs/zkit/letopis/mechanics.md`, `docs/zkit/letopis/decisions.md`, `docs/development.md`, `skills/letopis/SKILL.md`, `skills/letopis/references/operations.md`; active `letopis/architecture/*.md` (enumerated below). Create `docs/zkit/letopis/checks/issue-69/report.md`.

Active corpus: `overview.md`, `component-boundaries.md`, `command-wiring.md`, `source-addresses.md`, `git-snapshots.md`, `markdown-projection.md`, `graph-model-and-traversal.md`, `search-pipeline.md`, `embedding-execution.md`, `cli-observation.md` under `letopis/architecture/`.

- [x] **Step 1: Update current contracts, skill and upgrade instructions.**

Document the one mandatory title grammar, local lexical comparison, no fragment/existence checking, remote exemption, mismatch reason/status timing and explicit reindex. Distinguish literal title paths from percent-encoded navigation. Add a dated decision without renumbering historical decisions. Remove active instructions teaching legacy marked syntax; preserve historical specifications/reports and recorded evidence.

- [x] **Step 2: Migrate the active corpus with honest provenance.**

Add canonical path fields and repair relative destinations. Address the observed stale `git-snapshots.md` reference to removed `src/git/cli.rs`/GitCli by reading the current GitRepository implementation and updating the claim/reference. Verify any changed source basis against exact tree bytes as required by the Letopis skill; do not swap OIDs blindly. Preserve created_at. Other existing source-change findings must be reported or resolved by actually reviewing their source, never hidden by rewriting a basis.

- [x] **Step 3: Run required final checks on the actual changed tree.**

Run `make verify` once after the complete migration; it includes make check, strict Rustdoc and release tests. Run the affected Python suites from Task 2 once after their final changes. Use an isolated copied active corpus with the real new CLI, disabled embeddings and copied/available source bases to exercise reindex → check → status; inspect any pre-existing provenance limitations and report them rather than claiming an unassessed clean corpus. Preserve user cache, config, Git/index and author bytes outside the intended edits. If Linux is available, run focused parser/lifecycle tests there; otherwise explicitly record that platform as unrun. No native storage harness is needed unless its owned behavior/inputs change.

- [ ] **Step 4: Perform independent whole-branch review and write the report.**

Use superpowers:requesting-code-review after implementation. Review source boundaries, path decoding, title grammar, private APIs, diagnostics persistence, fixture fidelity and active/historical documentation. Resolve material findings and rerun affected checks after actual fixes. Report source revision, platforms, exact verification/mutation commands and outcomes, remaining findings and unrun suites. No claim of viewer compatibility, provider performance or full pilot acceptance.

- [x] **Step 5: Commit docs/report and hand off the branch.**

Check local links, `git diff --check`, actual changed-file scope and restored mutations. Commit intended docs/corpus/report as `docs: document link paths diagnostics and upgrade`. Use superpowers:finishing-a-development-branch only after verification, and wait for the user's integration instructions. Do not merge/push or close issue #69 as part of executing this plan without that authorization.

## Plan self-review and execution gate

- Coverage: title/domain rules → Task 1; active inputs and independent generation → Task 2; persistence/read-only status, upgrade and vectors → Task 3; current documentation, source corpus and final gate → Task 4.
- Type consistency: no new public parse signature or new storage/service trait; the private helper signatures above are the shared parsing contract. Error message and reason match the specification exactly.
- Proportion: four testable tasks, with fixed decisions and commands; implementation bodies remain with the implementer. All five review-focus conditions have named owning tests.
- Planning checks passed before execution; no product tests were run during planning. User approved this plan and Subagent-driven execution on 2026-10-31; implementation and per-task reviews are complete; the final whole-branch gate is pending.
