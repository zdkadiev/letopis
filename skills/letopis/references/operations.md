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

## Initialize, configure, observe, and reindex

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
`error.code:"embedding_failed"`; it never echoes the file. Search stays on FTS while
the configuration is invalid, and `status` reports `invalid_embedding_config`.
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

`index --full` is explicit maintenance, not the normal edit path:

```sh
printf '%s\n' '{}' | letopis index --full
```

It rebuilds graph and FTS from the corpus under `index.lock` and returns
`{"ok":true,"result":{"revision":N}}`. It does not wait for vectors, does not
confirm changed sources, and preserves author-declared `source_snapshot`. Ordinary
edits need only the next refreshing command; use `index --full` after cache loss or
a deliberate full rebuild. A physically corrupt cache is a separate recovery
decision, not something this command silently repairs.

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

The CLI has `init`, `check`, `config`, `search`, `status`, `traverse`, mixed
`inspect`, and `index --full`. Cancellation is a signal to your own foreground
process, not a command. Invalid files retain their previous valid projections
with current diagnostics; a fresh cache cannot recover a previous projection of
currently invalid Markdown. Reindex preserves author-declared `source_snapshot`
and never confirms changed sources for the agent. It also preserves existing
vectors keyed by unchanged search hashes when rebuilding a deleted cache. It does
not repair physical cache corruption or provide selective paths.
Embeddings are optional; FTS remains usable without a configured provider.
Packaging, installation and operational procedures remain TK-43/TK-44 work.

## Explicit index publication

`letopis index` publishes changed documents without returning validation findings. `--verify` audits all effective snapshot closures; `--full` always reparses and publishes, including unchanged files, and implies the audit. Provide optional JSON `paths` to limit owner-file observations and removals; omitted/empty means all. Out-of-scope rows remain untouched, and source closure audits stay global. Use `check` for diagnostics; its `paths` filter findings, not the refresh.
