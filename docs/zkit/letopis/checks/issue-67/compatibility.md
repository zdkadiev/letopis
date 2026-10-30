# Issue 67: in-process Git library compatibility gate

Date: 2026-10-30. This updates the source-only gate preserved at commit
`52aa7bdff613aea88162a657ba9dabcb3a1b2575`; its original fixed-memory blocker
was superseded by explicit approval of current-object/delta buffers. Task 2 only; product snapshot operations still use the process
adapter. The reader and durability helpers are compiled only in library tests.
This report establishes local library capabilities, independent object identity,
scoped resource measurements and filesystem publication ordering. It does not
establish final product migration, NFR performance or physical power-loss safety.

## Locked build and features

Rust 1.95.0, macOS arm64. Root dependencies are exactly `gix =0.90.0` with
`default-features=false`, features `sha1,sha256,index,excludes`, and `gix-dir
=0.32.0` with `default-features=false`, features `sha1,sha256`. No root `dirwalk`,
`attributes`, command, network, credentials, interrupt or parallel feature bundle
was enabled. The resolved graph nevertheless contains mandatory
`gix -> gix-protocol -> gix-transport -> gix-command` dependencies. Presence of
those crates is not a claim that the selected local object/ref paths execute
subprocesses; zero product helpers remains the binding runtime requirement.

Actual resolved sources include `gix-odb 0.87.0`, `gix-pack 0.77.0`,
`gix-object 0.66.1`, `gix-ref 0.69.1`, `gix-lock 26.0.1`,
`gix-tempfile 26.0.1`, `gix-fs 0.24.1`. These patch releases differ from the
pre-resolution upstream reconnaissance. Root `sha1-checked =0.10.0` and
`sha2 =0.10.9` are unchanged. Additional hash-crate versions coexist for gix.
The only replaced existing locked package is dev dependency `tempfile`, from
3.23.0 to 3.27.0: gix-odb requires `^3.26.0`, incompatible with the old exact pin.
Its declared MSRV is 1.63; gix's is 1.88, both below the pinned toolchain.

Full metadata, features, package-version differences and tool versions are in
`.artifacts/issue-67/task2-20261030/{metadata.json,features.txt,lock-changes.json,toolchain.txt}`.
The four historical Docker contexts now copy `tests/fixtures/git` so their root
library tests can include the collision fixture. This does not mean all four
historical acceptance runners were executed.

## Raw reader and independent integrity

`ObjectReader::new(&Repository, oid_len)` and `read_object(oid, expected,
missing, wrong_type)` preserve the requested private interface. `VerifiedObject`
remains `Blob` or `Tree(Vec<u8>)`; blobs retain no body. The independent original
hasher moved from `batch/hashing.rs` to `hashing.rs`; the process adapter still
uses it unchanged. Hash updates use 64 KiB chunks.

The raw local ODB handle, supplied with `gix::odb::pack::cache::Never`, performs
no filter, hook, lazy/promisor fetch or subprocess call. Repository-level object
and delta payload caches are bypassed. Every reconstructed object's canonical
Git header and bytes are independently hashed, and the full lower-case SHA-1 or
SHA-256 OID must match. Missing/wrong-type classifications remain caller-selected;
decode/integrity failures are internal errors. Decode causes preserve only safe
I/O kinds, never gix's path/object payload diagnostics.

The reader opens one controlled raw `Store::at_opts` at the resolved object path
with `alloc_limit_bytes=None`, no replacements and an uncached handle. Ordinary
trusted gix defaults are uncapped, but repository configuration can impose
`gitoxide.objects.allocLimit` (zero is a zero-byte cap), and reduced-trust opens
default to 16 MiB via `allocLimitIfReducedTrust`. The private reader bypasses both
rather than introducing a new input rejection. A one-byte configured-cap plus
replacement regression failed before the change and passes afterward; a second
read after on-disk corruption proves no stale payload reuse. Source:
gix 0.90.0 `src/config/cache/util.rs`, `src/config/tree/sections/gitoxide.rs`,
`src/open/repository.rs`; gix-odb 0.87.0 `src/store_impls/dynamic/init.rs`.

The raw store is operation-owned and points to the same existing ODB; it creates
no persistent second store. Reuse the reader throughout one closure, never reopen
it per object. As it now owns its handle, `ObjectReader` needs no borrowed lifetime;
removing the draft lifetime is an approved private-signature correction, without
PhantomData or an otherwise unused repository reference.

The integration test builds the private library-test executable and launches
owned children with one exact test selector. A required `READER_RECEIPT` prevents
an empty/mistyped selector from falsely passing. It never calls
`open_git_snapshots`, which would exercise the old adapter. Git independently
creates expected OIDs and `verify-pack -v` proves real delta entries. Both formats
cover 1 MiB and 64 MiB base/modified blobs, loose and packed reads, wrong types,
missing objects with a configured promisor remote, valid-zlib/wrong-identity
corruption, corrupted packed bases, corrupted deltas and affected delta reads.
The child PATH contains no executables. This, together with source inspection,
is local-reader evidence; final whole-product exec tracing belongs to migration
acceptance.

The 640-byte SHA-mbles fixture comes verbatim from locked sha1-checked 0.10.0.
Its SHA-256 is `3ead211681cec93d265c8ac123dd062e105408cebf82fa6e2b126f4f40bcb88c`;
provenance and MIT permission notice accompany it. A private hash test feeds it
**unprefixed** to SHA-1 and checks collision failure propagation. It does not
claim that prepending a Git header preserves an arbitrary published collision.

## Scoped resource receipts

Each object runs in a fresh library-test process. The global test allocator counts
requests only around `read_object`; `largest_allocation_bytes` is the largest
single requested allocation/reallocation and `total_allocation_bytes` is cumulative
requested volume, not simultaneously live bytes. `getrusage` measures whole-child
peak RSS, including runtime, mappings and library metadata. The caller's fixture
creation allocations are in another process. Returned reader capacity is measured
after the object, on success and error.

Representative successful macOS debug receipts, identical allocation sizes for
SHA-1/SHA-256:

| Object | Largest allocation, bytes | Retained decode capacity, bytes |
| --- | ---: | ---: |
| 1 MiB loose | 1,048,589 | 1,048,589 |
| 64 MiB loose | 67,108,878 | 0 |
| 1 MiB packed base | 1,048,576 | 1,048,576 |
| 1 MiB packed delta | 2,097,233 | 2,097,233 |
| 64 MiB packed base | 67,108,864 | 0 |
| 64 MiB packed delta | 134,221,096 | 0 |

The largest successful child RSS was approximately 140 MiB in the captured run.
These numbers are current-object/delta working allocations, **not constant audit
RSS**. Delta reconstruction uses base/result workspace and instructions; it is
not an object payload cache. After every read/error, capacities over 4 MiB are
dropped; smaller buffers are cleared. The threshold never rejects input sizes.
No whole-closure payload cache, custom decoder or audit disk spool was added.
Exact per-object receipts are in `readers-final.log` under the evidence directory.
`pack-proof.json` archives Git's complete per-format listings and input OIDs;
`capture-pack-proof.py` temporarily instrumented the test, asserted a real delta
for each 1/64 MiB pair, passed, then restored the test file byte-for-byte.

## Durable object and direct-ref capability

The ordinary gix object writer and ref transaction do not supply the required
sync ordering automatically. The gate proves these narrower adapter-owned paths:

1. `StagedObject::new(repo, kind, bytes)` creates one operation-owned 0700
   directory beneath the real resolved object directory. gix's lower loose store
   writes only there. Independent hashing checks its OID; final shared-file mode
   is applied, then completed contents are synced before live publication.
2. `StagedObject::publish(self)` uses a same-filesystem hard link, never replacing
   an existing name. An existing loose object must independently verify; its
   inode is synced too, since a concurrent winner may not have synced yet.
   Fanout and object-root directories are synced. Ordinary error/drop removes the
   owned staging directory. A post-link failure can leave a complete orphan and
   must still return failure. No unsynced pack is accepted as a durable write.
3. `retain_ref(repo, oid)` uses the exact private ref name and an owned lower-level
   `gix::lock::File`. It rechecks a fresh direct ref while holding the lock,
   rejects malformed/symbolic/conflicting targets, writes OID plus newline,
   syncs the actual owned descriptor, commits the rename, syncs ancestor
   directories through the common Git directory, then confirms the target.
   A lock-acquisition failure can succeed only after a matching direct winner
   is confirmed. Sync failures are never swallowed as CAS races.

The source and probes cover SHA-1/SHA-256, normal publication, corrupt preexisting
objects, matching concurrent creators, packed-ref reads, foreign locks,
equal-ref no-ops, dangling symbolic refs, malformed/conflicting values,
filesystem symlinks, linked-worktree common paths and shared permissions.
Injected I/O failures cover object sync, before object promotion, directory sync
after linking, ref sync, before ref rename and directory sync after ref rename.
A post-ref-rename sync failure explicitly reports failure even though a correct
ref is already visible. These are controlled failure probes, not disk-failure
or physical power-cut experiments.

On macOS, a DYLD interposition probe records actual `F_FULLFSYNC`, hard-link and
rename calls. Four object promotions have prior completed-stage syncs; both
new private-ref renames have prior owned-lock syncs and subsequent parent syncs.
The successful child and ordered trace are recorded in `trace-test-v2.log`,
`sync-order-v2.log`, `sync-order-summary.json`; `trace.c` preserves the probe.
Rust's `File::sync_all` uses the macOS full-sync path here. The first probe only
hooked fsync/link/rename and missed fcntl/linkat; its incomplete trace is retained
as `sync-order.log`, not used as passing evidence.

### Explicit private-reflog scope ruling

The task controller checked X-3, snapshot traits, architecture, mechanics,
resilience, decisions and the approved specification. They require durable exact
private refs/CAS/retention, not automatic reflogs or Git's OS-identity fallback.
The agreed Task 2 implementation therefore **does not create or append private
reflogs**, including under `core.logAllRefUpdates=always`; it leaves existing
private/user reflog bytes and all user refs untouched. Tests cover both absent
and existing logs with `always` and `false`. This is an intentional internal side
effect change from the old `git update-ref` path, not a claim of complete Git
reflog parity. No fabricated identity or OS/DNS identity subsystem was added.

The prepared-transaction alternative was rejected: equal existing values can
cause preparation to release its ref lock, while its lock fields are private.
Reopening a named `.lock` cannot prove ownership. The owned lower-level handle
avoids this ambiguity and makes sync-before-rename explicit.

## Resolved source pointers and limits

Paths below refer to exact registry packages in `metadata.json`:

- gix-odb 0.87.0: `src/cache.rs` (cache dispatch),
  `src/store_impls/dynamic/find.rs` (local ODB/delta lookup),
  `src/store_impls/loose/{find,write}.rs` (full-body decode/private loose writer).
- gix-pack 0.77.0: `src/data/file/decode/entry.rs` (base/result delta workspace).
- gix-ref 0.69.1: `src/store/file/transaction/{prepare,commit}.rs`
  (no-op lock release and unsynced transaction publication),
  `src/store/file/{find,packed}.rs` (direct lookup/fresh packed snapshot).
- gix-lock 26.0.1: `src/{acquire,file,commit}.rs` (owned descriptor and rename).
- gix-fs 0.24.1: `src/lib.rs`, `src/dir/create.rs` (shared modes).

The gate establishes API/source/build and local filesystem capability. Linux
native acceptance and final product helper-denial/exec tracing remain Task 7;
no historical receipt is overwritten or relabeled. Physical power loss, disk
controller caches, unsupported filesystems and final NFRs remain unassessed.
SIGKILL can leave the private staging directory or a stale `.lock`; ordinary
RAII cleanup is not claimed to run after arbitrary process death. This gate does
not introduce unsafe cleanup of other processes' files.

### Required handled-SIGINT integration work

`src/process/signal.rs:52` currently exits the process after its callback; this
bypasses ordinary Drop too. Therefore the helper's normal/error RAII tests do
**not** establish cleanup during handled SIGINT. Tasks 4/7 must connect explicit
command-owned stage/ref-lock ownership to that cancellation path and test
interruptions during creation, write/sync and promotion before product adoption.
A stale private ref lock must not become permanent contention after a handled
cancellation. Do not remove foreign/other-process files.

gix-tempfile 26.0.1 exposes `gix::tempfile::registry::cleanup_tempfiles()` and
`cleanup_tempfiles_signal_safe()` without enabling automatic signal setup. Both
are best-effort and current-PID scoped; the signal-safe variant may skip busy
registry entries. More decisively, `src/handle.rs:200-218` documents that
`with_mut` temporarily removes a tempfile from the registry while its callback
runs, so either cleanup path can miss a ref lock during write/fsync. These hooks
also do not own the external TempDir. They are available integration primitives,
not a complete cancellation solution or a passed Task 2 claim.

## Validation

`make check` and `make verify` passed on the accepted product tree, including
strict Rustdoc and release tests. The restored focused Git suite passed 44 tests.
Three required semantic mutations (full digest comparison, oversized capacity
release and collision propagation) failed as expected. Eleven further durability
mutations were rejected by their targeted tests. Removing the actual staged-file
sync produced four missing pre-publication syncs in the independent syscall
oracle; restoring it produced zero. All mutated/instrumented files were restored
byte-for-byte. Evidence is in `mutations.json`, `durable-mutations.json` and
`sync-mutation.json`, with each command's log retained.

Validation completion and exact final commands are recorded in the task report
`.superpowers/sdd/2026-10-30-issue-67-product-git-optimization/task-2-report.md`.
