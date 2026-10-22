# TK-10 — Change detection and atomic per-file publication

Status: approved by the user on 2026-10-22; implementation, platform checks and independent final review complete.
Base: main@bf9b6b9c46ddc9af0864a3a3924e8d0826da5abb.
Workspace: separate native worktree on branch TK-10.

## Intent and scope

Implement the production refresh service that subsequent CLI handlers call before
search, traverse, inspect and check. TK-7 supplies the actual X-4 parser, TK-8
supplies the Git process implementation, and TK-9 supplies SQLite units. TK-10
connects these implementations according to MC-2. No CLI command is claimed to
exist yet: argument handling and command integration belong to TK-11/TK-14.

Success means a call publishes completed valid Markdown edits, keeps the last
valid projection of an invalid file, and exposes either the entire old publication
or the entire committed new publication after interruption. The corpus is never
written by Letopis.

Sources:

- [Task and acceptance](../../zkit/letopis/roadmap/epics/EP-1-thin-slice/tasks/TK-10-change-detection.md).
- [Program structure and review](../../zkit/letopis/architecture.md#program-structure).
- [MC-1/MC-2 and concurrency](../../zkit/letopis/mechanics.md).
- [Publication interface I-2](../../zkit/letopis/contracts/I-2-projections.md).
- [X-4 Markdown](../../zkit/letopis/contracts/X-4-markdown.md).
- [D-94](../../zkit/letopis/decisions.md#D-94),
  [D-95](../../zkit/letopis/decisions.md#D-95),
  [D-97](../../zkit/letopis/decisions.md#D-97),
  [D-98](../../zkit/letopis/decisions.md#D-98),
  [D-99](../../zkit/letopis/decisions.md#D-99),
  [D-100](../../zkit/letopis/decisions.md#D-100).
- [TK-2 process-death evidence and limits](../../zkit/letopis/checks/TK-2/report.md).

## Existing decisions retained

Use synchronous Rust I/O, the pinned toolchain, real kernel flock and the existing
SQLite/rusqlite/SeaQuery stack. Do not introduce Tokio, a daemon, an event watcher,
a custom ignore parser, another database, a publication job table or Git trees on
publication. Vectors and vector failure state remain outside cache publication.
Store all declared graph edges; do not recalculate incoming-edge activity, reject
cross-file includes cycles on publication, or persist a graph-problem lifecycle.

G-0 is recorded as continue in the roadmap. TK-2/TK-7/TK-8/TK-9 are done; the
latest storage query-builder refactor is merged locally. The main checkout has
unrelated edits to storage/files.rs and storage/fts.rs and an untracked
skills-lock.json. TK-10 starts at the committed main revision and does not move,
stash or incorporate those edits. Publication of existing commits is a separate
action; starting TK-10 does not implicitly push main.

## Service and dependency boundaries

Introduce one concrete DocumentService with a refresh operation and a small
application-owned service interface for future command handlers. It owns candidate
diffing, lock/redetect sequencing, file projection and transaction completion.
Application-owned technology interfaces live in service/mod.rs, splitting private
interface files only if their size warrants it. Concrete implementation bodies
stay outside mod.rs.

- service/document.rs: DocumentService and its inherent/trait implementations;
  cohesive private algorithm helpers may share a responsibility-named file.
- filesystem/local.rs: LocalFileSystem, root-relative stat/read, checked clock
  observations, and an RAII index-lock guard; filesystem/mod.rs only declares and
  reexports required names. No graph or SQLite orchestration here.
- git/cli.rs: existing GitCli also implements the candidate-listing interface,
  reusing its argv-based Git process implementation. No new Git process wrapper.
- markdown/parser.rs: existing pure parse function remains the X-4 implementation;
  expose it through a small application-owned parsing interface and concrete
  MarkdownParser only as required to keep service independent of its implementation.
- storage/: add narrowly scoped typed reads needed by refresh, including enumeration
  of file state and existing nodes per affected path; use bound SeaQuery and retain
  the existing write-unit ownership. SQL and builder values stay private.
- lib.rs: construct real Git, filesystem, Markdown and SQLite implementations and
  pass them into DocumentService. Preserve existing public open helpers.
- domain/: retain pure identities, hashes and graph rules; no filesystem paths,
  SQLite, subprocesses, locks or CLI orchestration.

Avoid interfaces for every helper, generic repositories, forwarding services,
ports/adapters/contracts directories, speculative subsystems and async machinery.
Use trait plus concrete struct/impl, explicit imports, verb-based methods, useful
constants and focused functions (about 40 lines, maximum 60).

## Candidate discovery and change detection

Run git ls-files -z --cached --others --exclude-standard at the repository root.
Parse NUL-delimited paths, normalize through RepoPath, deduplicate and select
.md/.markdown paths; exclude Git metadata and Letopis service files. Follow Git's
actual semantics: ignore rules exclude untracked candidates; tracked files remain
candidates even if their path matches an ignore rule. No separate check-ignore
filter may silently exclude tracked files.

Compare candidates and existing files rows. A stored path no longer selected is
a deletion. Git still lists tracked paths deleted from disk: stat NotFound also
means deletion, including a missing new tracked path for which no row exists.
The latter alone is not a publication delta. A later reappearance is an addition.

Compare size, mtime_ns and inode. Read and hash new/stat-changed files and racy
files. As an approved implementation choice, use the conservative one-second racy window
already exercised by TK-2: recorded mtime >= recorded checked_at minus one second.
Use checked arithmetic and handle clock regression conservatively by hashing.
Do not claim correctness for timestamp resolution coarser than one second or edits
that deliberately restore an old non-racy timestamp; record these limits.

If bytes have not changed but observed stat data changed, update only the file
observation in the same cache unit; do not reparse or replace its graph/FTS.
An unchanged racy rehash does not rewrite checked_at merely to age out the racy
condition, does not advance revision and does not take index.lock. Unchanged,
non-racy marked and unmarked files are not read. A genuinely empty initial cache
still takes the full initial-build path and records one publication, even if the
candidate set is empty; an existing empty publication is then a no-op.

Read from a confined root-relative regular file. Check identity/stat before and
after reading and fail the refresh on detected concurrent replacement or I/O
failure. An unavailable candidate must not silently become an invalid Markdown
file or wipe its old knowledge. Missing files observed consistently are deletions.
Live edits are not an atomic corpus snapshot (D-77).

Approved path policy: reject candidate symlinks,
nonregular files and paths traversing a symlink with a safe typed error, preserving
the old cache. Do not follow a link outside the root or inherit the proof harness's
fixture-only skip policy. Unsupported/non-UTF8 candidate paths likewise fail
explicitly. Exact Unix no-follow primitives and any narrowly justified dependency
are left to the implementation plan; no dependency is approved by this document.

## Locking and publication

First detect without holding index.lock. A nonempty delta acquires the persistent
.letopis/index.lock using flock. Then discard the preliminary delta/read view and
re-detect using current files and a fresh SQLite read view while holding the lock.
A competing publisher may make the second delta empty: release and return the
current revision. Always release guards on errors and normal exit; process death
releases the kernel lock. Do not delete/recreate the lock file to unlock it.

One caller-owned cache write unit performs publication and one commit increments
revision. File observations are never committed before their projections. Process
files sequentially and retain only transient per-file bytes/search text; do not
build or persist a raw-text copy of the corpus. Preparation and per-file application
do not open or commit nested transactions. A fatal I/O/storage error rolls back
the whole unit. A parser validation error is a per-file result and does not abort
valid updates to other files.

For each changed valid file, use its actual ParsedFile: derive kind, preserve
knowledge row_id by UPSERT, replace changed outgoing declarations and FTS, remove
old elements absent from the new file, then update its files row. Contentless FTS
text comes from the parser's transient search_text; hashes/addresses alone cannot
populate FTS. Keep declared edges from other files, including those pointing to
removed knowledge. A valid unmarked file removes its old knowledge and receives
a valid files row with zero elements.

For an invalid file, change only its files observation/hash and safe parser error;
keep all previous nodes, outgoing edges and FTS for that file. A new invalid file
has no previous projection. Do not parse it again while its bytes remain unchanged.
Deleting any file removes its knowledge, outgoing edges and files row in the same
unit. An explicit later full rebuild must use the same per-file validity behavior;
public reindex command and corruption recovery remain later tasks.

Reconcile source rows from the resulting retained-plus-new external declarations:
shared source created_at is the minimum referring knowledge created_at; missing
source bytes are represented through the existing nullable source metadata; remove
a source only when no referring declaration remains. Read only affected addressed
source bytes, preserve external links from invalid files, and use current normalized
source addresses without creating a source registry. Source freshness on unrelated
external-file edits and full provenance validation are TK-20/TK-21 responsibilities.
No snapshot creation or embedding provider call occurs in publication.

Approved I-2 clarification: its illustrative PreparedProjection.search
currently lists only id/uri/search_hash, although its example also requires FTS
text. Keep prepare/publish and caller-owned unit/commit semantics, but make the
per-file search entry explicitly carry transient search_text until synchronous
publish completes. Prepare/apply one file at a time in the enclosing publication
unit; do not accumulate the corpus text. This is an approved internal contract
clarification, not a change to SQLite schema or CLI wire formats. Align I-2 with
this wording before implementation; do not silently invent a second
parser pass, a SQL body column or a callback from storage to Markdown.

Return a typed refresh outcome with committed/current revision and safe file
findings or counts needed by later handlers. Exact result field names belong to
the plan. Do not add a CLI JSON schema, new public error codes or promise that
status/check handlers already exist.

## Verification and evidence

Use Arrange/Act/Assert tests of behavior through the service interfaces, plus
production integration with real Git, filesystem flock and SQLite/X-4 parsing.

1. Initial/empty build, additions, same-ID edits with stable row_id, removal,
   anchored-element replacement, valid-to-unmarked and unmarked-to-marked files.
2. Tracked/untracked/ignored paths, tracked missing files, filenames with spaces,
   candidate deduplication, safe unsupported path and symlink behavior.
3. Non-racy no-read, same-tick equal-size/inode/mtime edit detected by hash,
   unchanged racy no-op and stat-only refresh without unnecessary parsing.
4. Invalid/new-invalid/unchanged-invalid/fixed/deleted files and a mixed batch:
   old invalid-file knowledge and FTS retained, valid peers published atomically.
5. Declared dangling edges, cycles retained, incoming edges survive target removal,
   source reference sharing/minimum creation time/last-reference removal.
6. Real two-process duplicate-delta and newer-edit ordering; deterministic pipe
   handshakes prove redetection, one publisher, and a no-op not waiting on index.lock.
7. Port TK-2 publication SIGKILL cuts to the production path: detected, locked,
   redetected, begun, cleared, projections, FTS, fingerprints, before/after commit.
   Check exact old or complete new nodes/edges/FTS/files/revision; restart converges
   without lock repair. Instrumentation stays test-only or in private hooks, not
   an arbitrary public fault-injection API or environment switch.
8. A pinned WAL reader retains its old publication while the new one commits;
   before-commit failure rolls back every files field and leaves vectors unchanged.

Run ./scripts/check-rust.sh and relevant platform integration on macOS ARM64 and
source-built Linux ARM64. Preserve separate receipts with input/source hashes;
never overwrite historical TK-2 evidence or call it production validation.
Physical power loss, x86_64, performance/capacity, unrelated historical harnesses,
provider quality, full CLI, cancellation JSON and drain launch are not established
by these checks. Architecture review must inspect actual files, visibility,
imports, trait implementations and lib.rs constructor wiring.

## Approval and next step

The user approved the full specification on 2026-10-22, including symlink
rejection, the conservative one-second racy window and the I-2 transient-text
clarification. The fresh independent builder check found only the I-2 question;
that clarification was made explicit before approval. No other consequential
unanswered questions were reported.

Baseline ./scripts/check-rust.sh passed on macOS ARM64: formatting, compiler,
Clippy and 112 Rust tests; log: /tmp/letopis-tk10-baseline.log. Linux has not been
rerun for the documentation stage. See the [implementation plan](../plans/2026-10-22-tk-10-change-detection.md)
for concrete interfaces, file ownership and verification. Plan review/execution
method selection precede implementation under the requested Superpowers workflow.
All four implementation stages are now independently approved; see the final-fix verification state below.


## Implementation and Verification

The approved service is implemented through Tasks 1–4 of the plan. Production
source checkpoint: `15fcc196b554c5a77b8b83818ddc2274d916fb70` (initial Task 4 checkpoint `2378731` is retained as historical evidence). The
[acceptance report](../../zkit/letopis/checks/TK-10/report.md) maps each criterion
to real integration/process tests and separate macOS ARM64/source-built Linux
ARM64 receipts: 172 Rust tests and nine Python runner tests pass per platform.
The two publication negative controls remain pinned to the initial checkpoint;
final-fix schema-recheck and lock-reopen controls also compile, fail at intended
assertions, and pass after byte-exact restoration. All 86 common platform input
hashes match.

Tasks 1–4 have independent approval, including scoped fixes for raw metadata
exclusion ordering and automatic service-database symlink protection. The final whole-branch review found initialized-schema startup contention and
a dependency-description error. Both are corrected, including deterministic
first-initialization/recheck and macOS lock-creation regressions. Scoped re-review
confirmed both findings addressed and no new breakage; the final gate is complete. Execution rulings and their costs are preserved in the
[plan](../plans/2026-10-22-tk-10-change-detection.md#execution-rulings).

CLI command integration remains TK-11/TK-14. The report retains the one-second
timestamp/live-read limits, hostile ancestor-rename residual, and the limits
of process-death checks versus physical power loss. At the initial acceptance gate, no integration had occurred. The user has since
authorized integration, and `TK-10@352eace` is merged into `main` as `64b2188`.
The merged root gate passes all 172 Rust tests; see the plan integration record
and GitHub issue #10 for publication provenance.
