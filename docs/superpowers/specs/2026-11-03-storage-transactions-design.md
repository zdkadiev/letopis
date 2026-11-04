# Shared storage and closure transactions

Status: design approved in conversation on 2026-11-03; implementation plan pending approval.

## Intent and accepted decisions

Simplify the storage implementation to two concrete storage structs, `CacheStorage`
and `VectorStorage`. Each implements the application interfaces it needs. Remove
`SqliteStorage`, `SqliteStatusStorage`, `SqliteBootstrap`, `SqliteRead`,
`SqliteCacheWrite`, `SqliteVectorWrite` and `ConfinedDirectory` as concrete storage
components. Database identifier enums and ordinary data values are not components.
Library-owned `rusqlite::Connection` and `Transaction` remain implementation details.
Do not replace the removed types with equivalent wrapper structs under new names.

The user explicitly chose closure scopes without a transaction argument, shared
storage instances across services, automatic joining when a transaction already
exists, and rollback-only behavior after an inner error even when caught outside.
Directory confinement belongs to filesystem, while storage owns database schemas,
queries, native capabilities and transaction behavior.

## Proposed service API

The following names and signature are accepted design choices:

```rust,ignore
fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
where
    F: FnOnce() -> Result<T, E>;
```

Provide read and write scopes on the application-owned cache/vector interfaces in
`service/mod.rs`. Callbacks receive no argument; services use their injected storage:

```rust,ignore
self.cache.with_write_tx(|| {
    self.first_service.update()?;
    self.second_service.update()?;
    Ok(())
})
```

Return ordinary typed callback values and errors directly. No `Any`, boxed unknown
payload, output parameter, or universal enum of all command responses is introduced.
`TransactionError<E>` is a shared service error value enum distinguishing
`Operation(E)` from `Storage(StorageError)`. It is not another storage component.
The calling service preserves its existing command-specific error mapping; no
blanket `From<StorageError>` erases the context of search versus traversal errors.
An original callback error takes precedence over a cleanup error; retain useful
cleanup diagnostics internally without exposing payloads or fabricating success.

These generic closure/result methods are not object-safe. Use application-owned
trait bounds for the storage parameters of affected services, rather than pretending
the methods work through `dyn Storage`. This is a concrete need for generics, not
generic dispatch solely to choose an injected implementation. Other collaborators
retain their existing dispatch unless the migration requires a change.

Services share `Rc<C>` / `Rc<V>` where C/V implement those application interfaces.
The composition root creates each required shared instance and injects clones of
the same `Rc` into participating services. Services never name concrete SQLite
types. Synchronous execution on one thread makes `Rc` sufficient; no locks, async
runtime, thread-local transaction context or process-global registry are added.

## Implementation mechanism and ownership

The pinned rusqlite 0.40.2 exposes
`Transaction::new_unchecked(&Connection, TransactionBehavior)`. It starts a real
transaction from a shared connection reference, rejecting an existing transaction
at runtime. Despite its name it is a safe Rust API. Only the outermost storage
scope calls it. The returned library guard stays on the scope's stack and supplies
rollback on drop. Storage operations execute on the same connection via `&self`.

Each storage records its active transaction mode and rollback-only state using
private interior-mutable values such as `Cell` and a finite state enum. A connection
is not placed in a `RefCell` borrowed across callback execution. No guard that owns
a borrow into its own containing struct is constructed. No unsafe code is needed.
Exact field layout is left to the builder; it must retain only necessary state.

State is associated with the storage instance and connection role. This is scoped
to the synchronous call tree, not an execution context propagated to arbitrary
threads. Concurrent unrelated requests must not share these instances.

An outer scope starts the transaction, calls the closure once, decides its outcome,
and clears active state on all return paths. A joined scope calls its closure once
without begin/commit/rollback or a savepoint. An inner scope error, or any failed
database write, marks the transaction rollback-only. A later `Ok` cannot clear it.
The outer scope returns a typed storage error if its closure succeeds after the
transaction has been marked rollback-only. Cache revision advances once, only on
a successful outer cache-write commit.

Storage computes the pending publication revision under the owning immediate
write transaction. Publication logic can obtain that same pending revision before
returning its value; outer commit publishes it exactly once. Joined publication
work shares that revision, and its returned value is provisional until the outer
scope succeeds. Do not return a public successful command response before that
scope finishes, or reopen/re-read after commit to discover the revision (another
publisher could have committed by then). The builder may expose the pending value
through a narrowly named storage accessor rather than another result wrapper.

Unwinding must also mark a joined scope failed, release the outer library guard,
and clear state before a caught panic allows the instance to be reused. A local
`catch_unwind`/`resume_unwind` boundary is acceptable for cleanup; it must rethrow
the original panic. Process abort relies on SQLite crash recovery, not Rust Drop.
If rollback leaves the connection unusable, reject subsequent operations rather
than reuse uncertain transaction state.

## Read/write joining and database boundaries

Cache reads join an active cache read or cache write. Inside a write they use its
connection and see that write's changes. Starting a cache write inside an active
cache read is rejected; do not upgrade, reopen or silently change the pinned view.
Vector scopes follow the equivalent rule. A scope error poisons its owning write.
An inner error also marks an owning read scope failed, so catching it cannot make
the outer scope report a successful result.

Normal service database operations require an active compatible scope. A request
outside a scope fails explicitly; it must not execute a write in implicit autocommit
or assemble a purported snapshot from separate implicit reads. Opening, schema
initialization and the explicitly atomic vector reset are documented exceptions
whose own storage implementations establish their required transaction boundaries.

CacheStorage retains the connections required for cache publication and a pinned
cache/vector read. VectorStorage owns its vector-writer connection with a read-only
cache attachment for pruning. Cache connections that need vector reads attach the
vector database; only VectorStorage writes it. The number of connections is not
artificially limited to the number of structs. Connection-specific busy policies
and read-only attachment permissions remain unchanged.

Search/traverse/check/status must retain one scoped read view, original revision,
configuration identity, membership and source bindings throughout their operation.
Vector reads made as part of that view route through CacheStorage's attached read
connection. They must not silently read through VectorStorage's writer instead.
Readiness/reset stays before pinning. Search may retain a read view across query
inference; no write transaction spans provider HTTP. Query-error recording uses
the independent vector writer and can occur while the cache read is active.

Joining applies to the same storage transaction. It does not create a distributed
transaction across `cache.db` and `vectors.db`. Preserve the existing separation
of cache publication, attempt reservation, provider calls and vector publication.
Vector expected-identity checks remain inside the owning write scope before its
callback; a mismatch does not run the callback and preserves `ConfigChanged`.
Nested vector scopes may only join with a compatible expected identity.
A mismatch returns `ConfigChanged` without invoking the callback. It is a normal
outcome, not an SQL error; no inner scope may reset the active identity or start a
replacement transaction to satisfy it.
This mismatch alone does not poison the outer transaction; its compatible work
may still commit if all other conditions succeed. A caller that treats the outcome
as an error and returns it from its scope triggers the ordinary rollback-only rule.

## Filesystem, opening, initialization and status

Move directory/leaf identity, symlink, regular-file and cache/vector alias checks
out of storage into filesystem. They remain real guarantees, including checks
around SQLite open/attach; a one-time path check is not an equivalent replacement.
The composition root coordinates filesystem checks with database opening. Storage
does not import rustix directory traversal or independently inspect directories.
SQLite-specific open flags, URI encoding, connections and attachments stay in
storage. The filesystem API must permit pre/post verification around those
operations without depending on concrete storage implementations. Exact method
signatures are a builder choice, with the existing race tests as acceptance evidence.

Implement initialization and observation roles on the two storage structs. Status
opens existing databases without bootstrap/repair, identity reset or durable file
creation. Missing cache/publication remains distinct from malformed/inaccessible
storage. A missing vector database retains the existing empty-vector observation
behavior. An instance opened for observation cannot later write. Neither storage
struct needs an always-open connection merely to represent absent status data.

## Migration and scope

Update central service interfaces, their consumers/mocks, `lib.rs` composition,
filesystem handoff, storage code and relevant tests together. Keep schema contents,
CLI contracts, synchronous I/O, SeaQuery, rusqlite and vec1 unchanged. Do not add an
ORM, pool, generic transaction framework, savepoint API or new runtime dependency.
Database rows and query behavior remain in their existing responsibility modules.

Update AGENTS.md's borrowed-box transaction rule and naming guidance to describe
closure scopes, along with architecture, D-98/D-99 clarifications and I-8 where the
old representation is normative. Preserve historical records against their revisions.
Refresh source-linked storage knowledge after the final implementation is verified.

Current branch: `rafactor` (renamed at the user's request); design commit
`0e8db61` builds on `d5cfc30`. Existing uncommitted
edits to `src/embedding/http.rs` and `src/service/init.rs` are unrelated and must be
preserved. The user selected this branch for the refactor; retain that selection
when choosing an execution workspace after plan approval. Available disk space has recently been below 1 GiB; do not start another
full build until capacity permits, and do not delete user data to make room.

## Embedding entry requirements (final review clarification)

Use cases that require fresh observations enforce their entry requirements before
configuration, reservation or inference. Storage exposes
`verify_transaction_scope(TransactionRequirement)` with `Inactive` and `ReadOnly`:
healthy inactive owners satisfy either, and ReadOnly also permits an active read.
Rejecting an incompatible active scope marks that owner rollback-only even when
its caller catches the error; failed/disabled owners remain rejected.

Query inference permits an inherited cache read but requires an inactive vector
owner, because query completion publishes or clears diagnostics. It rejects an
inherited cache writer and every inherited vector scope before provider calls.
Drain requires both owners inactive so each iteration can observe vector
publication and terminate; it must not join a caller's pinned work-selection view.
These are entry preconditions on the existing shared owners, not new components,
transaction wrappers, runtime context or cross-database atomicity.

## Acceptance and verification

Prove, with real SQLite behavior and an independent observer where relevant:

1. Exactly two concrete storage components remain; application services depend on
   application-owned interfaces and share the intended instance through composition.
2. Standalone B starts/commits its transaction; A calling B joins one transaction.
   Neither an inner success nor a nested scope prematurely commits.
3. A or B error rolls back all writes; a caught B error or caught SQL write error
   still prevents outer commit. Failed commit cannot report success.
4. Panic unwinding, including an inner panic caught by the outer callback, releases
   the transaction correctly and cannot permit partial commit or stale active state.
5. Read scopes preserve the existing pinned view and release it on success/error;
   writes cannot upgrade a read. Reads inside a write see its uncommitted changes.
6. Vector identity mismatch skips callbacks; attempts remain durable before HTTP,
   and old-identity results never publish after a reset.
7. Status absence/read-only/schema behavior and filesystem open/attach race,
   symlink, alias and directory-replacement protections remain covered.
8. SQLite transaction control remains inside storage and path policy in filesystem;
   no service receives a database transaction object or transaction parameter.

New/modified tests use described Given/When/Then phases and hand mutation checks.
Run the storage host runner with a new evidence directory (it includes root
verification); run the Linux runner when the environment supports it. Report exact
platform limits. Documentation-only review uses link/consistency checks. The design
must receive the Superpowers builder check and user approval before a separate
implementation plan is prepared and approved.

## Design review

The independent Superpowers builder check confirmed the shared-reference guard,
zero-argument closure, shared ownership and generic application-interface direction
against the pinned rusqlite API. It requested explicit pending-revision and nested
vector-identity outcomes; those contracts are specified above. This is a design
check, not compiled implementation evidence or a passing storage test run.
