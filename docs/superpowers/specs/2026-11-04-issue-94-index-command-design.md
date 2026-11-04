# Issue #94 — explicit indexing modes and scoped publication

> **Decision 2026-11-05:** Remove the public `reindex` command entirely. Use `index --full` for full rebuilds. Earlier alias references below record the superseded design.

Status: approved by the user on 2026-11-04 and implemented on the issue-94
feature branch. Verification findings and platform limits are recorded in the
companion implementation plan. Independent builder review of the proposed design
returned “nothing important left”.

## Intent and sources

Implement [issue #94](https://github.com/zdkadiev/letopis/issues/94): expose cache
publication directly, with incremental, verified incremental, and full modes,
including optional JSON `paths`. Reuse the existing synchronous document refresh
pipeline, atomic cache publication, snapshot verification, and `index.lock`.

Design sources: [X-1 CLI](../../zkit/letopis/contracts/X-1-cli.md),
[MC-2 and MC-5](../../zkit/letopis/mechanics.md),
[architecture](../../zkit/letopis/architecture.md#program-structure), and
[TK-25](../../zkit/letopis/roadmap/epics/EP-3-maintenance/tasks/TK-25-reindexing.md).
This issue extends the historical full-corpus-only CLI; captured TK-25 evidence
remains historical and is not rewritten.

## Approaches and decisions

The issue requests one `index` command with argv mode flags. Reusing the existing
`Refresher` methods is the appropriate implementation: a separate indexing service
would duplicate ownership of the same publication pipeline. JSON mode fields and
separate `refresh`/`reindex` commands were considered in the issue and are not the
requested interface.

Ship scoped input with the modes, rather than postponing an acceptance criterion.
Scope selection belongs to the existing application service and candidate helper;
Git and filesystem interfaces retain their generic responsibilities.

The user selected **always rebuild** for full mode on 2026-11-04. Consequently,
`index --full` always reparses selected candidates and publishes a new revision,
including unchanged and empty selections. Only incremental modes promise an
unchanged revision and `published:false` when there is no file delta after initial
publication. This preserves the existing ability to repair missing FTS entries.

Implementation proposal: preserve `reindex`'s existing `{revision}` success result
and empty-object input while marking it deprecated. It uses the same full-mode
pipeline as `index --full`; `index` exposes the complete publication outcome.
This avoids breaking existing consumers of the deprecated command's JSON.

## CLI contract

| Invocation | Publication | Snapshot assurance |
| --- | --- | --- |
| `index` | incremental | retain declared roots |
| `index --verify` | incremental | verify and pin effective closures |
| `index --full` | force reparse and publish | verify and pin effective closures |
| `index --full --verify` | same as full | same as full |
| `reindex` | deprecated, unscoped full-mode alias | same as full |

Accept both orders of the two distinct flags; reject unknown flags, duplicate
flags, extra positional arguments, and malformed JSON before service construction.
Stdin remains an object without a command wrapper: `{}` or
`{"paths":["letopis/architecture","letopis/development"]}`. Existing empty-input,
terminal-input, timeout, and strict unknown-field rules remain in force.

Normalize and deduplicate selectors using `RepoPath`; invalid absolute, escaping,
or malformed paths are `invalid_request`. A valid unmatched path is not an error.
Omitted or empty `paths` selects every candidate. A nonempty selector selects that
exact file or descendants at a component boundary, never a lexical prefix sibling.

`index` returns the existing success envelope with all five `RefreshOutcome`
fields: `revision`, `published`, `files_updated`, `files_removed`, and
`invalid_files_updated`. Invalid Markdown contributes its current observation and
diagnostic, retains its last valid projection, and does not fail indexing merely
because it is invalid. Findings remain the responsibility of `check`.

## Scoped publication and invariants

Apply the same scope to Git candidates and previously cached owner files before
detecting changes or removals. Filter raw candidate names before per-file probes
so an unrelated owner is not observed or rejected by a scoped run. Reapply scope
during detection under the existing lock; never publish a pre-lock delta.

Keep the complete effective graph available for cross-file declarations, source
reference accounting, and closure verification. Unselected owner file rows,
knowledge nodes, outgoing edges, and FTS rows remain untouched. Only source nodes
affected by selected owners are reconciled; a source is a shared derived entity,
so its URI need not lie under an owner-file selector. A scoped full run must not
mark unrelated sources for refresh merely because the mode is full.

First publication may initialize the project revision while selecting only the
requested owners. A later unscoped refresh discovers all remaining candidates.
Scope never changes the closed-world basis of snapshot verification: verified and
full modes still audit all effective declared source closures before publication,
including retained projections outside the selected owners.

Incremental mode retains the existing no-lock fast path only when its present
preconditions hold. Verified and full modes take the existing index lock. No new
lock, schema, runtime, dependency, worker, or publication retry is introduced.
No foreground provider inference or direct foreground vector writes are added.
Successful indexing runs the existing background completion step. Register valid
`index` argv forms for the existing foreground SIGINT cancellation behavior.

Existing implicit refreshes remain unscoped. In particular, `check.paths` still
filters findings rather than publication; its difference from `index.paths` is
documented explicitly.

## Implementation boundaries and verification

Typed argv/request/output changes stay in `src/cli/`; command wiring stays in
`src/commands.rs`; constructors and signal selection stay in `src/lib.rs`.
Extend the existing refresh service and application-owned contract only as needed
for operation scope. Preserve domain purity and technology boundaries. Exact
private helper signatures and test organization are left to implementation.

Update X-1 prose, schema, examples/fixtures, help, architecture/mechanics, and
Letopis skill/operations guidance. Preserve unrelated changes already present in
`src/markdown/parser.rs`.

Cover argv permutations and rejection-before-I/O; mode assurance; changed, unchanged,
removed, invalid and empty corpora; omitted/empty/file/directory/unmatched scopes;
scope boundaries and untouched outside owners; scoped full rebuild repair; legacy
response compatibility; and two real publishers serializing with re-detection.
Test snapshot failures and publication atomicity using the existing fixtures and
checkpoints. Mutation-check new behavior tests by hand and restore every mutation.

Run `make check` and final `make verify`, plus affected contract/link checks. Run
the native storage runner only if storage/native/build inputs actually change.
Record exact verification results and platform limits in the implementation plan
or report. No merge, push, release, or GitHub issue mutation is included.
