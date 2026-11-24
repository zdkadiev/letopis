# Letopis operations

Read for setup, configuration, diagnostics, full audits, rebuilds, or cancellation.

## Source-snapshot audits

Search/traverse/inspect automatically refresh the database from Markdown without
editing Markdown. Routine refresh retains source roots/refs and verifies addressed
objects; it does not audit unrelated descendants. Run `check` with omitted or empty
`paths` for a full source-snapshot audit. Any nonempty `paths` selects scoped findings
without that audit. `index --verify` audits complete closures incrementally; `index --full` reparses the selected corpus and fully audits effective source
snapshots before publication. A full audit failure preserves the previous cache.
A successful routine read or an existing retention ref is not proof that every
source object is intact. Inspect fully verifies its own newly captured reading tree.

## Discovery settings

Letopis discovers tracked `.md` and `.markdown` files and untracked Markdown
admitted by standard Git ignores. Marked documents can be stored throughout the
repository without corpus registration.

Add `"discovery":{"respect_gitignore":false}` beside `embedding_model` in
`.letopis/config.json` to admit Git-ignored untracked Markdown. The default is
true; omitting the section/member uses that default. False disables all standard
ignore sources, including `info/exclude` and global Git excludes. `config` returns
the effective section, and `init` includes it in newly generated config while
preserving existing valid file bytes.

Create `.letopis/ignore` when you need additional rules; `init` writes a
commented default when the file is absent and never replaces an existing file.
Its Gitignore patterns use the repository root as their base and filter tracked
paths too. For example:

```gitignore
/vendor/
notes/*
!notes/keep.md
```

The parent must remain eligible for a child negation to work. `notes/` plus
`!notes/keep.md` still excludes the child. Matching directories are pruned during
the worktree walk, so their contents are not read. No pattern can admit
`.git`/`.letopis` components or a path missing from the Git candidate union. A
missing ignore file means no additional rules; indexing does not create it.

Refreshing commands use the same policy, including all index modes. An exclusion
removes cached owner rows/projections at the next relevant refresh. Scoped index
leaves outside owners untouched; `check.paths` only filters findings. External
source bindings and retained historical snapshots are separate from discovery.
Config/status/init/drain do not refresh membership. A policy edit during prepared
publication or inspection causes a safe failure/retry; it cannot publish a mixed
policy. Discovery changes never enter embedding identity or reset retained state.

## Initialize, configure, and observe

Interactive commands use an empty request without waiting for stdin. An open pipe
with no data after 250 ms also supplies an empty request. For commands that need
fields, pipe a JSON object; once data arrives, it is read until EOF.

Run `letopis init` once in a Git work tree to create `.letopis/`, both database
schemas and the editable configuration:

```sh
printf '%s\n' '{}' | letopis init
```

It succeeds with `{"ok":true,"result":{}}`. It does not index the corpus or contact
a provider. Repeating it preserves a valid edited configuration and existing data.
Outside a non-bare Git work tree it fails with `invalid_request` and creates
nothing.

`config` observes `.letopis/config.json` without changing it:

```sh
printf '%s\n' '{}' | letopis config
```

Before setup it succeeds with `{"ok":true,"result":null}` and creates no service
files. Otherwise `result` is the complete nested configuration. There is no config
setter: edit the file yourself, then observe it again. A valid enabled example is:

```json
{"embedding_model":{"enabled":true,"provider":"ollama","model":"code-model","parameters":{"base_url":"http://127.0.0.1:11434/api"},"max_retries":2}}
```

Only this nested form is accepted. An old flat file is rejected with a migration
message; move its four settings under `embedding_model` and add `enabled` yourself.
`provider` and `model` are both null (with `enabled:false` and empty `parameters`)
or both configured. `max_retries` is required, and `0` means one attempt. Retained
provider settings are validated locally even when `enabled:false`.

Malformed content makes `config` fail with exit 1 and
`error.code:"embedding_failed"`; it never echoes the file. Valid discovery with
invalid embedding settings still allows FTS. Malformed JSON, invalid discovery
settings and unsafe/unreadable policy files stop refresh; `status` reports its
existing `invalid_embedding_config` problem.
A valid `enabled:false` also searches with FTS but preserves vectors and retry
state, reports zero embedding backlog/failed counters and hides stored execution
failures; enabling the same settings reveals them without resetting vectors.
`config` does not reset vectors, perform inference, launch a drain, or create
service state.

`status` reads the last published revision and its current diagnostics without
refreshing:

```sh
printf '%s\n' '{}' | letopis status
```

`result` has `revision` (null before the first publication), `embedding_backlog`,
`embedding_failed`, held `locks` and `problems`. Problems are current observations,
not history: an invalid file, a dangling link, an includes cycle, a missing Git
`source_snapshot` or invalid configuration disappears when the condition is
repaired and the next command observes it. Exhausted embedding attempts clear only
with a changed configuration identity or a vector reset, not merely on the next
publication. `status` does not rebuild a missing or stale cache and does not make a
subsequent live-file read atomic.

## Review check and status findings

Read `result.problems` on both commands. An empty array means no findings.
The commands return the first 100 sorted, deduplicated findings. For `check`,
this replaces the former `result.errors` field. A successful report exits
`0` with `ok:true`, including when findings are present. A command failure
exits `1` with `ok:false` and a top-level `error`.

The `source_changed` reason identifies a difference in addressed bytes
between a declared `source_snapshot` and the working tree. Each affected
element gets one diagnostic with all unique changed addresses in
`details.targets`. `status` checks the last published bindings.
`source_not_found` identifies a missing source, `missing_source_snapshot`
identifies an absent basis, and `source_snapshot_unavailable` identifies a
declared basis that cannot be used. The `stale` flag on read results indicates
that the working copy of the knowledge element could not be matched to its
indexed projection.

## Mandatory-path upgrade (issue #69)

Marked links require `letopis:type=TYPE;path=ADDRESS` with the complete terminal
address as the sole graph/source target. There is no legacy fallback, automatic
rewrite or parser-version migration. For this breaking binary upgrade:

1. Wait for prior Letopis calls and detached drains using this repository to finish.
2. Install/use the new binary consistently; do not mix versions on the same cache.
3. Add the intended canonical root-relative `path` to every marked title. Repair
   local destinations relative to their containing document; fragments remain
   independent. Preserve created_at and checked source_snapshot bases.
4. Run `printf '%s\n' '{}' | letopis index --full`, then the same empty request to
   `letopis check`. Ordinary incremental refresh can skip unchanged files.
5. Resolve markup findings and review other provenance findings before trusting
   the migration. Reindex retains last-valid projections of invalid files, so its
   success alone does not prove every declaration migrated.

Preserve cache.db, vectors.db, configuration, locks, Git and authored documents.
`status` never reparses or upgrades the cache. A local path mismatch is published
through existing `files.error`; check/status expose `validation_failed`, reason
`link_path_mismatch`, message `Markdown link path does not match letopis path`,
and only the containing file in details.path. A fresh cache publishes no partial
projection of an invalid file. Status shows a new mismatch only after refresh or
reindex publishes it; an author fix alone leaves the cached finding until the
next successful refresh/reindex. Other valid files continue to publish. There is
no fragment/existence/viewer validation or lookup; HTTP/HTTPS sources are exempt
from local-file comparison. This does not replace independent graph/provenance checks.

## Cancel your own foreground call

SIGINT is handled for your own `search`, `traverse`, `inspect`, `check` and
`index --full` invocation. The process writes exactly one response and exits 1:

```json
{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}
```

If a normal result was already printed, a later SIGINT adds no second JSON. If the
cancellation response won, the process cannot also exit 0 as a successful call.
An uncommitted SQLite transaction rolls back as after a crash and the kernel
releases `index.lock`. A publication that had already committed is not rolled
back, so cancellation promises your caller terminates, not that no publication
survived. Treat the outcome as uncertain, read `status` or retry the same
idempotent command, and do not claim that SIGINT edited authored knowledge.

There is no cancel subcommand and no PID-based cancel API. Signal only a process
you started; never signal another writer. A detached `embed --drain` is an
independent process in its own session. It has no cancellation response, keeps
its durable attempt reservation, may finish after your cancelled caller, and never
edits authored Markdown. Leave it alone; the next foreground command observes its
work.

## Current boundaries

Invalid files retain their previous valid projections with current diagnostics.
A fresh cache can publish only valid Markdown. A rebuild can reuse existing
vectors keyed by unchanged search hashes. Physical cache corruption requires its
own recovery procedure. Full-text search works without a configured provider.

## Explicit index publication

Ordinary edits need the next refreshing command. Use `letopis index` for explicit
publication, `--verify` for an audit of all effective source snapshot closures,
and `--full` to reparse and publish even unchanged files. `--full` implies
`--verify`, preserves authored `source_snapshot` bases, and returns after graph
and full-text publication. Verify changed claims against their sources in Markdown.

```sh
printf '%s\n' '{"paths":["letopis/cache"]}' | letopis index
```

Optional `paths` limits owner-file observations and removals. An omitted or empty
list selects the corpus. Scoped runs preserve out-of-scope rows and source closure
audits remain global. The result reports `revision`, `published`, `files_updated`,
`files_removed`, and `invalid_files_updated`. Use `check` for validation findings.
Its `paths` field filters reported findings while refresh still covers the corpus.
