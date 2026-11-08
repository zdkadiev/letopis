# Issue #97: discovery ignore configuration

Status: implementation, host/Linux acceptance and independent final review passed. Branch `isssue-97-ignore-config`, product base `52ed491`.

[Approved design](../../../../superpowers/specs/2026-11-06-issue-97-ignore-config-design.md) ·
[Approved plan](../../../../superpowers/plans/2026-11-06-issue-97-ignore-config.md) ·
[Machine-readable results](20261106T140628Z/results.json) ·
[Source hashes](20261106T140628Z/source-hashes.json).

## Implemented behavior and boundaries

- `.letopis/ignore` uses existing pinned gix and repository-root Gitignore
  semantics, including tracked paths, pattern order, directory precedence and
  negation. No crate/runtime/database change was added.
- `discovery.respect_gitignore` defaults to true; false disables standard Git
  sources including nested `.gitignore`, `info/exclude` and explicit/default
  global excludes. The additional patterns filter the resulting union.
- The application reads config/pattern bytes through generic confined filesystem
  methods. Git receives mode/bytes and owns matching/enumeration. Application
  Markdown/private-root/no-follow/scope rules remain in their existing owner.
- Config is a complete value separate from embedding state/identity. Config output
  includes effective defaults, old nested files remain valid, and init does not
  create an ignore file or overwrite existing valid bytes.
- Refresh redetects policy under the lock, rechecks original bytes/presence at both
  no-op paths and before commit, and atomically removes excluded owners. Inspection
  independently checks its captured policy around final strong pin.
- Malformed/unreadable discovery stops refresh and preserves the previous index;
  valid discovery with invalid embedding settings still permits FTS. Existing
  fatal envelopes and status reason `invalid_embedding_config` are preserved.

## Acceptance matrix

| Area | Evidence and result |
| --- | --- |
| Gitignore syntax | PASS: anchors, basenames, directory-only patterns, parent negation/re-inclusion, last-match order, slash boundaries, `*`/`?`/`**`/classes, escaped `#`/`!`/`*`/spaces, comments/CRLF, nonmatching malformed patterns, Unicode/raw bytes and case mode, against independent Git plus literal expectations |
| Git enumeration | PASS: Standard/Disabled against `git ls-files`; SHA-1/SHA-256; tracked, staged, deleted, conflicted/sparse membership; nested, info, explicit/default global sources; preserve user Git state |
| Every command | PASS: fresh per-command/mode actual binary fixtures for check/search (including empty query)/traverse/inspect/index and all flag combinations; nested invocation directory; config/status/init/drain observation behavior; global and all command help/version |
| Lifecycle | PASS: exclude/restore tracked owners, include ignored local `.markdown`, scoped removals, dangling links, excluded source bytes through a surviving binding, hard roots/invalid filtered names, no-op/full revisions and same-stat input edits |
| Failure/safety | PASS: malformed/unknown/wrong-type/duplicate config, missing/empty ignore, symlink/directory/FIFO/unsafe parent, typed permission error and host real permission denial, replacement/race regressions and unchanged prior publication |
| Concurrency | PASS: edit before locked redetection, during preparation, before commit, preliminary/locked no-op; config and ignore inputs; inspection capture and pin; old files/nodes/edges/FTS/revision/vector state retained on abort |
| Embeddings | PASS: retained identity/vector/attempt/query-error state through toggles and shared-owner removal; enumeration/config/status/init produce no provider work; fixtures use synthetic local HTTP only |
| Host Rust gates | PASS: `make verify V=1` inside host storage acceptance, including format, all-target check/Clippy, fixed-value checks, debug/release root tests and strict Rustdoc |
| Linux Rust gates | PASS: the same full acceptance inside the current-source Linux arm64 image with runtime network disabled |
| Native/standalone | PASS: host and Linux canonical-DDL parity, runner tests, native capabilities, copied executable persistence/vector/FTS checks and dynamic linkage; synthetic 60-second deadline fixture ran explicitly on both |
| Runtime helpers | PASS: 47 traced Linux product invocations each had exactly one Letopis execve and no helper/non-thread child; configured modes/patterns included |
| Mutation checks | PASS: 22 deliberate behavioral mutations were detected and restored; exact cases/results are in results.json and raw receipts |
| Schema/docs/knowledge | PASS: 27 schema corpus cases plus 3 runtime-only cases, five independent effective-output cases, affected local link/anchor consistency, 15 exact source bindings checked against recorded tree, and repository `check` returned an empty errors array |

The focused Linux tracing gate uses image
`sha256:608cdd218a3aa72a40164885e4dcfba9eb5268dd9f75f5d2bfe79940c57475e0`.
Host is macOS arm64; Linux acceptance is arm64. 228 common inputs in the host and
Linux native manifests match byte-for-byte. Permission failure is injected on both
platforms; the real mode-000 file observer is applicable on the non-root host,
while root containers can bypass that filesystem restriction.

All completed commands, exact argv/exits, failed first attempts, full outputs,
source inventories, process receipts and mutations are preserved locally under
`.artifacts/issue-97/final-20261106T140628Z` (ignored). Concise source-hash/results summaries are committed here,
following tests/storage/README.md. No pre-existing evidence directory was reused.

## Compatibility findings resolved during acceptance

The first full run identified old tests expecting malformed config to permit FTS
or config output to omit discovery defaults. Drain, filtered-search, maintenance
and source-preservation fixtures now distinguish invalid embedding settings from
malformed discovery and keep their original preservation/no-provider assertions.
The Git test image also lacked existing storage SQL fixtures needed to compile
library tests; its COPY list was corrected, then host/Linux storage acceptance
was run in new directories. Initial failed receipts remain available.

The cache used for an initial installed-binary knowledge query was schema 2;
this checkout expects schema 4. The inactive task-worktree cache was preserved at
`.artifacts/issue-97/cache-from-installed-binary`; a fresh built-binary projection
validated the edited knowledge. Main-worktree state was not used or replaced.

## Limits

The evidence covers the specified behavior on the two recorded platforms and
synthetic providers. It does not assert real-model quality, physical power-loss,
other hardware/platform acceptance, or execution of historical proof manifests.
The external target-corpus observation requiring a supplied request was not run;
private ignored child entries were driven by their parent tests, and the isolated
60-second provider deadline entry was explicitly run by native acceptance.
One unchanged symbolic Markdown example target (`цель`) in historical decision
prose remains outside the affected-link check. These limits describe the original
acceptance run; PR #98 was subsequently opened without merging.

## Independent review

[One fresh whole-branch review](review.md) found no Critical, Important or Minor
findings and no declined behaviors. All five focus areas and ledger rulings were
accepted. The review range ends at `ad2770185ec7331aaaffb14e48f4c0483c0a3af3`; subsequent edits record
completion only, through `9757061`. The subsequent organizational follow-up is
recorded below and preserves the original evidence against its captured inputs.

## PR follow-up: inline modules only for tests

The user replaced the former inline production-helper convention after PR #98
was opened. Five existing modules (`basis`, `projection`, `ranking`, `comparisons`,
`observations`) were flattened into their owning three service files. Helpers
are now private items; the flat service layout, behavior, test namespaces and
selectors are preserved. AGENTS.md, development guidance and architecture now
state the updated rule.

[Follow-up results and changed source hashes](inline-modules.json):

- PASS: syntax scan of all 99 Rust source files found zero production inline
  modules and 79 test-only module roots.
- PASS: all 77 production function/method bodies match the pre-refactor baseline
  after normalizing formatting, trailing call commas and trivial closure blocks.
- PASS: before/after library inventories are identical: 620 tests.
- PASS: `cargo check --locked --offline --all-targets`, `make fmt`, `make verify`
  and `git diff --check`, on macOS arm64. The verification includes full root
  debug/release tests, strict Rustdoc, all-target compilation and Clippy.
- PASS: focused independent review found no issues with behavior, visibility,
  cfg gating, imports, ownership, test selectors or guidance.

Exactly three of the original 267 product/test inputs changed; the other 264
remain byte-identical. Linux/native acceptance and behavioral mutation receipts
remain historical evidence for the original tree and were not repeated for this
organizational follow-up. No new behavior or test assertions were introduced.
Raw follow-up receipts are in `.artifacts/issue-97/inline-module-followup`.
