# TK-24 Mixed Inspection Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete mixed knowledge/source batch inspect and the portable agent reading/writing skill.

**Architecture:** Extend existing InspectionService behind Inspector, retaining current synchronous Git/filesystem/codec dependencies. Derive membership from captured Markdown and metadata from the exact blobs of one returned tree; reuse ElementLocation for local paths/opaque URLs. CLI remains a service caller; lib.rs remains the composition root.

**Tech Stack:** Pinned Rust 1.95.0, existing Git CLI, confined FileSystem, MarkdownParser, serde_json, existing root tests. No new runtime, library, database schema or migration.

**Spec:** [TK-24 inspection design](../specs/2026-10-26-tk-24-inspection-design.md).

**State:** Approved by user “go” on 2026-10-26; explicit “go subagent” then selected Subagent-driven, superseding Native. User previously selected «по карточке» to skip design questions. Task 1/2 implementation and the Task 2 review fix are verified; independent Task 2 rereview and final whole-branch review remain pending. Builder check: `/root/tk24_builder_check`, “nothing important left”.

## Global Constraints

- Worktree `/Users/zdkadiev/.codex/worktrees/tk-24/letopis`, branch `tk-24`, base `0f8f92b205f6db48a188cc130c5a6e1190512e3d` equals refreshed main/origin/main. Preserve concurrent TK-23 edits.
- Follow [AGENTS.md](../../../AGENTS.md) and [program structure](../../zkit/letopis/architecture.md#program-structure), including actual import/visibility/composition review.
- Keep Rust I/O synchronous, existing interfaces and JSON shapes; no source HTTP requests, query embeddings, new process, cache schema, or migration.
- No text, flags, revision, created_at, file_hash or edges in inspect. Knowledge/source ID is derived, never a UUID field in Markdown.
- Unknown/removed knowledge fails the entire batch; referenced source with absent bytes returns null hash. Source without current declarations is unknown.
- Metadata must match captured tree bytes. No SQL fallback for current hash/lines/membership, live reread of an already captured marked source path, or dependence on source_snapshot for current inspect bytes.
- Retain one working file's bytes plus metadata/path/OID/reference collections, not whole corpus bytes. This is not a fixed RSS guarantee.
- Keep functions near 40/max 60 lines, explicit AAA when useful, independent expected literals, named meaningful constants, verb names, smallest necessary visibility/generics/dispatch. KISS/DRY/YAGNI.
- Preserve historical proof revisions; new evidence paths only. No paid pilot/model runs, main merge/push or GH closure in this plan without subsequent authorization. TK-17/G-1/G-2 and later task limits remain.

## Review Focus

1. A source path also captured as marked Markdown: knowledge and ranged-source hashes must use the identical blob despite a subsequent live edit; Task 1 test.
2. Multiple aliases/ranges of one file, including CRLF/binary/empty/no-final-LF bytes: independent literal hashes, unique tree path and IDs; Task 1 tests.
3. Source remains referenced while file disappears versus final declaration removed: null hash versus whole-batch not_found; Task 1 lifecycle tests.
4. Pin/assembly/filesystem failure after capture: no successful snapshot or secret payload, no index/HEAD mutation; Task 1 fault and CLI tests.
5. Agent records a basis after local changes: verify bytes and isolate temporary Git index, preserve user index/HEAD; Task 2 executable documentation scenario.

## File Map

| File | Responsibility |
| --- | --- |
| `src/service/inspection.rs` | Inspector implementation and capture state; split private helpers/tests by responsibility only when size requires |
| `src/service/command.rs` | Change InspectedElement.path from RepoPath to existing ElementLocation, update docs |
| `src/service/command_traits.rs` | Document Inspector membership/null-hash/retention contract |
| `src/lib.rs` | Existing actual wiring stays; document mixed inspection |
| `src/cli/tests.rs`, `tests/knowledge_inspection.rs` | Adapt path constructors/assertions; replace known-source InternalError placeholder expectation |
| `tests/source_inspection.rs` (new) | Real mixed-tree/source lifecycle service regressions |
| `tests/cli_reading.rs` | Real executable mixed inspect output/error cases |
| `skills/letopis/SKILL.md` | Full current reading/writing/recheck guidance |
| `tests/cli/README.md` | Current inspection and reproducible smoke instructions |
| `docs/zkit/letopis/checks/TK-24/report.md`, `verification.json` (new) | Actual verification and architectural handoff |
| TK-24 task card, this plan/spec | Actual completion evidence and limits |

`cli/output.rs` already calls path.as_str(); change only if required by compilation/contracts. `ElementLocation` remains its existing exported type; no unrelated module reshuffle. Do not modify storage/native/build sources for this scope.

### Task 1: Implement mixed canonical inspection

**Interfaces consumed:** `Refresher::refresh`, `CorpusLister::list_candidates`, `FileSystem::read_file`, `MarkdownCodec::parse_file`, `GitSnapshots::{write_blob,create_tree_from_blobs,read_blob,pin}`, `LocalSource::{get_path,get_lines,select_bytes}`.

**Produces:** existing `Inspector::inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>` with full TK-24 behavior. `InspectedElement { path: ElementLocation, lines: Option<LineRange>, hash: Option<Sha256> }`. No new public service interface.

- [x] **Step 1 — Add failing behavior tests.** In `tests/source_inspection.rs`, add `inspect_mixed_batch_uses_current_tree_bytes`, `inspect_preserves_source_bytes_and_deduplicates_ranges`, `inspect_distinguishes_missing_bytes_from_missing_membership`, and `inspect_includes_unrequested_declared_source_files`. Use ReadingRepo and literal fixture bytes/hashes; existing knowledge fixtures remain independent codec goldens. Example independent SHA for `b"abc"`: `ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad`.
  Assert mixed knowledge + whole file + range + opaque URL metadata; URL keeps query/fragment, hash None/no lines. Use normalized duplicates, LF/CRLF, binary source, empty whole file, incomplete ranges and no final LF. Assert all requested metadata matches bytes read through GitSnapshots::read_blob of returned tree, and unrelated files/.letopis are absent.
- [x] **Step 2 — Prove RED.** Run `cargo test --locked --offline --test source_inspection` with shared target and `RUST_TEST_THREADS=2`; require a runtime failure demonstrating current known-source InternalError, not a setup/compile failure. Existing public path type can be checked with as_str before its change. Keep output in a new `.artifacts/tk24/task1-<unique>/` directory.
- [x] **Step 3 — Implement minimal capture.** Apply spec's seven capture steps in InspectionService. Use sets/maps of typed SourceUri and path/blob identities; assemble an unretained corpus tree, read overlapping marked sources from that tree, and capture other source files once per path. Compute requested source hashes from those exact bytes, reuse LocalSource::select_bytes, retain complete available files, and pin only final output. Keep metadata collections private; helpers named for their work and placed after callers. Update DTO/path constructors/trait docs and lib.rs docs in the same change; retain existing actual wiring.
- [x] **Step 4 — Add failure/overlap/lifecycle coverage.** Extend existing in-module RecordingGit/RecordingFiles tests for `inspect_reuses_captured_marked_source_after_live_edit` and `inspect_fails_when_final_pin_fails`. Existing knowledge-only bounded file-read test remains. Cover a source path referenced at several ranges, marked-source path overlap, safe unsafe-path/I/O failure, absent known file, range beyond EOF, lost final declaration, deleted indexed knowledge/anchor, invalid current markup and unknown mixed-batch ID. Use real Git GC retention in service integration tests; assert real Git index and HEAD preserved before/after inspect.
- [x] **Step 5 — Verify GREEN and sensitivity.** Run `cargo test --locked --offline --test source_inspection --test knowledge_inspection --test source_reading --test cli_reading`, then `make check`. Temporarily break source selection hash, current-membership validation, overlap blob reuse and final pin behavior separately; each named behavioral test must fail at runtime after compiling. Restore original bytes exactly and rerun focused GREEN; retain commands/results, no live mutations.
- [x] **Step 6 — Commit Task 1.** Stage only actual intended Rust/test paths. Commit `feat: complete mixed knowledge and source inspection`. Record commit/evidence and any findings in Engram; do not close issue or declare final acceptance yet.

### Task 2: Verify executable contract and complete agent skill

**Consumes:** Task 1 Inspector contract, unchanged production `letopis inspect` stdin `{"element_ids":[...]}`, existing `check`, `search`, `traverse`, Git standard tree reads and SourceUri rules.

**Produces:** schema-compatible current CLI behavior, portable skill, reproducible workflow and task report with actual limits. No new CLI read/snapshot/apply command.

- [x] **Step 1 — Add actual CLI scenarios.** Extend `tests/cli_reading.rs` with `binary_inspect_returns_mixed_metadata_without_text_or_flags` and `binary_inspect_rejects_unknown_member_without_partial_result`. Invoke production binary via ReadingRepo::invoke, assert exact exit, empty stderr, one JSON line, exact element keys/paths/hash/null/lines. Include a known missing source and opaque URL; errors expose no fixture secrets and no successful result. Add deletion of knowledge/last source declaration after initial publication. Check JSON against existing InspectOutput command schema with the prepared CLI Python validator, not the catalogue's union alone.
- [x] **Step 2 — Verify transport.** Run `cargo test --locked --offline --test cli_reading`. Adapt src/cli tests affected by DTO, preserving strict serialization and existing normalization tests. Do not add flags/text fields or bypass Inspector in handler. Change CLI output only if a failing contract requires it.
- [x] **Step 3 — Update existing skill and reproducible example.** In `skills/letopis/SKILL.md`, implement all scenarios listed in spec's agent-skill section. Show actual JSON for check/search/traverse/mixed inspect and `git show <tree>:<path>`; distinguish captured reading snapshot from verified source_snapshot and literal live reads. Agent chooses folders, anchors and custom edge types; includes/external keep system semantics. Replace obsolete task/deferred table with current boundaries; status and handled cancellation remain later. Update `tests/cli/README.md` full-source inspection claims and instructions, preserving historical evidence descriptions explicitly as historical.
- [x] **Step 4 — Execute documented workflow in disposable repo.** Fixed source bytes and Markdown only, no paid model. Run write → check → search → traverse → mixed inspect → Git reads; edit source → observe needs-recheck → reread → update verified basis → check; rename file/anchor → observe dangling link → repair → check. For uncommitted source basis use a temporary separate GIT_INDEX_FILE, snapshot real index/HEAD before/after, and compare captured addressed bytes. Ordinary live reads are shown without a snapshot guarantee. Capture literal commands, responses, tree/hash checks and old/new index/HEAD bytes or identities; never use this fixture as a real-agent or pilot-quality claim.
- [ ] **Step 5 — Final verification and review.** Freeze actual source map and run `make verify` once on final product tree, collecting exact argv/exit/log and counts. Run relevant Python CLI runner tests and validate recorded inspect responses with its actual command schema. Choose CLI platform runner where applicable; run native storage runner only if actual storage/native/build inputs changed. Record every omitted platform/historical suite honestly. Review physical files/imports/visibility/trait implementations/lib.rs assembly, and obtain independent final whole-branch review through selected execution workflow; address findings and reverify affected code. Keep all evidence/reviews.
- [ ] **Step 6 — Publish local report and commit.** Write report.md/verification.json: criterion→test/evidence, source hashes, base and implementation revisions, baseline, independent expected hashes, manual controls/restoration, schema results, architecture review and actual platform limits. Update TK-24 card from actual outcomes, check completed plan steps and leave unmet criteria open. Check document links and `git diff --check`; commit intended skill/docs/test files. Save useful findings/commits in Engram. No merge/push/GH closure without subsequent user instruction.

## Plan Self-review

Spec requirements map to Task 1 for capture/current membership/metadata/memory/retention, Task 2 for real transport/schema/agent workflow/architecture/report. All five Review Focus cases have named checks above. Existing DTO/interface names and aliases match current main. Paths and constructors affected by ElementLocation must be adapted within Task 1 so its commit compiles. The internal corpus tree uses existing GitSnapshots, not a new API or filesystem path. No criterion here is a claim that paid pilots, G-1/G-2, all native targets or handled SIGINT have passed.

## Execution Handoff

Historical preparation recommended **Native** because the tasks share one service and skill. The user subsequently approved the plan with “go” and explicitly selected **Subagent-driven** with “go subagent”; that choice superseded the recommendation. Task 1 implementation and rereview are approved. Task 2 implementation was committed at `094c174accfe16d246645d780cd7717b3862dc9d`; workflow review fix `5801e6b86227520e76008e71dfdd2c879de857a3` and focused proof are recorded in the [durable TK-24 report](../../zkit/letopis/checks/TK-24/report.md). Independent Task 2 rereview, final whole-branch review and Engram write remain controller-owned at this checkpoint; Steps 5/6 stay open until those actions finish.
