# Storage transaction refactor execution report

Plan: [2026-11-03-storage-transactions.md](2026-11-03-storage-transactions.md).
Design: [storage-transactions-design.md](../specs/2026-11-03-storage-transactions-design.md).
Branch: `rafactor`. Implementation base: `264262b`.

Completed on `rafactor`. Final product commit: `95823b8`. Independent review findings were addressed by TDD; both final native acceptance receipts report PASS for the exact final input tree.

## Resulting structure

`CacheStorage` and `VectorStorage` are the two production storage components.
`SchemaDefinition` is static schema data; `TestDatabases` and fixture records are
compiled only for tests and implement no application storage interface.
The old sqlite/status/bootstrap/read/cache_write/vector_write components and
boxed transaction interfaces have been removed. Ordinary SQL remains in the
storage query modules; directory and file identity policy belongs to
`filesystem/database.rs`, coordinated before/after opening in `lib.rs`.

`CacheDatabase` and `VectorDatabase` live in `service/mod.rs`. Their zero-argument
closures use the injected owner. Composition shares the same `Rc<C>` / `Rc<V>`
with participating services. Nested scopes join that instance, and only the outer
scope finalizes. Cache and vector owners remain separate transaction boundaries.
No dependencies, database schemas, CLI responses, async runtime, pools, connection
wrapper components or process-global transaction context were added.

Cache reads pin revision and attached vector identity before callbacks. Reads
inside writes use the writer and see pending changes. Rejected read upgrades,
caught nested errors, failed writes and caught nested panic prevent outer commit.
Failed cleanup disables reuse if SQLite remains in a transaction. Publication
returns its own provisional revision after the outer commit, without rereading a
later publisher's revision. Vector identity mismatch skips callbacks, preserving
compatible outer work. Reservations commit before provider HTTP; query diagnostics
use the independent vector writer while search retains its pinned read.

Status observes through a CacheStorage with no writer. Absent storage opens no
connection, creates no durable files and skips the callback. Initialization keeps
its existing validation-before-creation ordering through delayed composition.

Embedding entry checks are explicit storage operations using
`TransactionRequirement::Inactive` / `ReadOnly`. Query accepts an inherited cache
read and requires an inactive vector owner; drain requires both owners inactive.
Rejected active scopes become rollback-only before configuration/reservation/HTTP.
This preserves normal pinned search while preventing inherited writer latency and
stale-snapshot drain loops.

## Acceptance evidence

| Exact command | Result and retained receipt |
| --- | --- |
| `python3 -B tests/storage/run.py --platform host --evidence .artifacts/storage-transactions/host-20261104-final-04` | PASS, macOS arm64; manifest and complete logs in that new directory. |
| `python3 -B tests/storage/run.py --platform linux --evidence .artifacts/storage-transactions/linux-20261104-final-05` | PASS, Linux aarch64 in the pinned Docker image; inner host-style receipt in `container/`. |
| `python3 .artifacts/storage-transactions/check-final-receipts.py` | PASS: every command succeeded, both outer input inventories equal the final tree, and every product source copied into Linux matches it. |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | PASS before final runners, then again inside both `make verify` runs. |
| `cargo check --locked --offline --all-targets` | PASS, including the Task 5 completion gate and both final runners. |
| Letopis `check` / `inspect` and source-tree comparison | Clean check; four knowledge elements and 24 source bindings verified against retained Git bytes. |
| Markdown local destination check and `git diff --check` | PASS: 624 file destinations before report finalization; no missing paths or whitespace errors. |

Each final runner completed `make verify V=1`: formatting, all-target check,
Clippy, root debug tests, strict Rustdoc and root release tests. Both also passed
schema parity, ten runner tests, pinned native SQLite/vec1 probes, the copied
standalone executable oracle with exact vector bytes/reopen/revision, and dynamic
linkage checks excluding shared SQLite/vec1 dependencies. The explicitly ignored
real sixty-second provider deadline test was run separately and passed on both
platforms. Other intentionally ignored private process entrypoints are exercised
by their owning parent tests; unrelated ignored tests are not represented as run.

Final source receipt is `95823b8`; the report commit changes Markdown only.
`source-dirty.log`, full SHA-256 input inventories and image/executable receipts
are retained. Earlier failed, interrupted and pre-review PASS directories remain
historical evidence and are not substituted for these final receipts. The final
runners use the same source bytes; no stale control build is counted as acceptance.

| Design acceptance group | Concrete coverage |
| --- | --- |
| 1. Two components and shared composition | Physical module/visibility/import audit; real confined and ordinary openers; same-Rc graph-service snapshot regression. |
| 2. Standalone B and joined A/B | `standalone_scope_commits_once`, `joined_scope_does_not_commit_and_caught_error_prevents_commit`, joined publication and embedding reservation tests. Independent observers detect premature commits. |
| 3. Errors and failed finalization | Real FTS SQL failure restores metadata, DDL and revision; vector SQL poisoning; nested publication rollback; deferred FK commit failure; authorizer-denied rollback and reset cleanup. |
| 4. Panic and reuse | Caught nested panic rolls back and permits a fresh scope; existing SIGINT/SIGKILL and SQLite-step cancellation tests remain active under cache ownership. |
| 5. Pinned reads and upgrades | Revision/identity pinning before callbacks, membership and paging across concurrent publication, read-in-write visibility, caught write-in-read rejection, observation-only read poisoning and release/checkpoint tests. |
| 6. Vector identity and HTTP boundaries | Incompatible nested identity skips its callback without discarding compatible work; durable reservations; old-identity late responses; dimensions established during HTTP; independent query-error recording; inherited-writer/query and inherited-cache-scope/drain entry regressions, plus a supported pinned-query regression. |
| 7. Status and filesystem policy | Missing/zero revision, schema/read-only/missing-vector observations; symlink, directory replacement, leaf replacement and alias races through actual composition. |
| 8. Layering and explicit scope ownership | No legacy handle types or begin_* transaction APIs in active src/tests/examples; no service transaction parameters or SQLite control; filesystem policy removed from storage. |

All active fixture mutation-operation call counts were compared against `bbec9fb`
with Rust strings/comments masked: reset, reservation, failure, query-error,
vector/prune, node/edge/text/file operations each retain identical counts in both
`tests/` and `src/service/`. This supplements behavioral tests; it is not a proof
of semantic equivalence by itself.

## Mutation checks

Hand mutations were restored before final acceptance. Logs are retained under
`.artifacts/storage-transactions/execution-20261104/`:

- Cache/vector poisoning, read upgrade poisoning, publication revision increment,
  status callback skipping and absent-status no-open behavior.
- Filesystem post-open verification, leaf identity and alias checks.
- Shared-instance read reuse between graph services.
- Premature inner vector commit, vector identity bypass and cleanup Broken-state
  removal after reset failure.
- Parent publication rollback and post-commit revision rereading. The independent
  observers detected respectively rows `1` instead of `0`, and revision `2`
  instead of the publishing scope's `1`.
- New FTS rollback regression: disabling cache write poisoning makes its assertion
  fail. New observed-reader regression: disabling missing-writer poisoning makes
  its assertion fail. The restored cache suite passes.

Additional final-review mutations disabled query/drain entry checks, removed
rollback-only marking on an incompatible scope, bypassed the vector-read entry
requirement, and made query's cache entry requirement too strict. Each regression
failed at its intended behavioral assertion (unexpected inference, bounded repeated
work selection, parent commit, provider call count or rejected supported read).
The restored embedding suite passed **24 tests, 1 ignored child entrypoint**.

## Evidence corrections during final acceptance

The first Linux image omitted existing `tests/fixtures/storage` files from its
Docker context. The initial Linux receipt contains two concrete include_str
missing-file compiler errors. `.dockerignore` and the acceptance Dockerfile now
include those fixtures. The next Linux run was interrupted after the host run
found the fixture issue below; neither receipt is counted as successful evidence.

The first host run caught a migrated pinned-view test whose vector reset call had
landed on a comment line. Restoring the call makes the focused test pass. A scan
of actual comment lines found no second occurrence; the mutation-operation call
inventory also matches the original tests and service code. Final host/Linux runs
use new evidence directories and do not overwrite these failed receipts.

A later Linux release run exposed an existing process-fixture race: invalid CLI
arguments can close stdin before the parent writes. The fixture previously
panicked on BrokenPipe before checking the child's safe JSON error. A deterministic
regression reaps the early-rejecting child before sending input: RED, GREEN after
the fixture fix, and RED again when that fix is hand-mutated away. The restored
CLI runtime suite passes all five tests. Only BrokenPipe is accepted; other write
errors still panic, and callers must validate response and exit status. The
superseded host run was interrupted; final receipts use the same corrected tree.

## Decisions made during execution

1. Retained the user-selected branch and workspace instead of creating another
   worktree. Cost if wrong: branch-level isolation requires staging only task paths.
2. Kept old/new interfaces temporarily together during the migration to preserve
   compilable checkpoints. Cost: temporary duplicate contracts, now removed.
3. Corrected the preexisting OpenAI missing-data classification from Credentials
   to InvalidResponse when its existing malformed-response test exposed it.
   Cost if wrong: malformed-response classification changes; the existing test
   verifies the intended category. User HTTP/init whitespace edits remain in
   the independent user commit `4f9f975`.
4. Grouped early filesystem observation work into the first coherent checkpoint
   because real-opener regression coverage depended on it. Cost: commit grouping
   differs from task boundaries; final module responsibilities are unchanged.
5. Implemented the existing delayed InitStorage capability with a composition
   function pointer rather than a replacement storage struct. This preserves
   config validation before database creation. Cost if wrong: initialization
   ordering must remain covered by the init tests.
6. Included existing legacy SQL fixtures in the Linux acceptance context.
   Cost if wrong: extra test fixture bytes in that image, with no product change.

7. Fixed the process-fixture BrokenPipe race revealed by Linux release tests.
   Cost if wrong: the helper permits a closed input pipe; callers still assert
   the child's exact response/status, so unexpected rejection remains a failure.
   Product request handling is unchanged.

## Scope of verification

Historical proof harnesses and their captured source/evidence were preserved,
not rewritten or claimed rerun. Physical hardware power-loss behavior is not
established by process-termination tests. Shared-instance joining remains
synchronous and scoped to one call tree; it is not a distributed transaction or
cross-thread context.

## Independent final review

A fresh, read-only `gpt-6-astra` reviewer examined the whole branch range
`f0d5b8b..c8606f0` against the plan/spec and all five Review Focus cases. It found
no Critical finding and confirmed those five storage cases and the two-component
architecture. Two Important embedding boundary findings were reproduced and fixed
in the single Native TDD fix pass, committed as `95823b8`:

1. Inherited cache/vector writers could remain active across query HTTP. The
   real-owner regression failed by returning a vector where rejection was expected;
   explicit entry verification now rejects before the provider and poisons the
   incompatible parent even when the service error is caught.
2. Drain inside a retained cache snapshot could repeatedly select already-published
   vector work. A bounded provider reproduced the loop by exceeding one call.
   Drain now rejects inherited scopes before work, while a later standalone drain
   completes with exactly one provider call.

Normal query inference inside a cache read remains tested. An inherited vector
read also rejects before inference because diagnostic publication cannot upgrade
it. No new runtime, storage component or transaction object is introduced. Fixes
are verified by failed-before/passed-after regressions, hand mutation and final
platform suites; a second reviewer was not dispatched under the Native workflow.

### Deferred Minor findings

- When both callback and rollback fail, the callback error correctly wins and
  uncertain connection state is disabled, but the secondary cleanup cause is not
  retained as a separate private diagnostic.
- Some migrated preexisting tests still use bare Given/When/Then markers. New
  entry-boundary regressions have described phases; broad phase-comment cleanup
  remains deferred.

### Review items set aside, and executor rulings

- Hardware power-loss behavior was not assessed. The agreed evidence covers
  process termination and SQLite behavior, so no hardware durability claim is
  made. Cost if wrong: hardware faults may affect durability beyond this evidence.
- Lifetime protection against path replacement after completed verification was
  not assessed. The design promises pre/post observation around open/attach,
  which the tests verify, rather than continuous path locking. Cost if wrong:
  later path replacement is outside this detection guarantee.

