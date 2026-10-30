# Issue 67: in-process Git library compatibility gate

Current integrated product acceptance is recorded in the Task 7 section below;
the original Task 2 report remains the historical capability gate.

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
3. `retain_ref(repo, oid)` acquires the exact private ref lock before it may
   acknowledge any existing value. Loose confirmation syncs the actual ref file
   and its parent chain under that lock. Packed-only confirmation additionally
   holds an immediate, owned `packed-refs.lock`, reloads the exact direct target,
   syncs `packed-refs` and the common Git directory, then checks identity again.
   Backing opens use `O_NOFOLLOW`. A new publication still syncs its writable lock
   before rename; since rename releases that lock, it reacquires an owned marker
   and enters the same confirmation protocol, selecting the then-current backing
   if packing intervened. No equality-only success branch remains.
4. Ref and packed-ref acquisition are nonblocking, so this fixed lock order never
   waits on an inverted packer's order. Contention returns a safe `RefLockBusy`
   cause under the unchanged `InternalError` classification, recognized by
   `is_ref_lock_busy(&SnapshotError)`. Task 4 must retry this condition through the
   entire durable protocol within its command budget, preserving ordinary
   concurrent exact-target idempotence at the public boundary. Sync/integrity
   failures are not contention and must not be swallowed. A long-held foreign
   lock remains an error and is never opened, synced or removed by confirmation.

The four-thread fixture now requires **all** ordinary matching creators to
complete through bounded, test-only retries. That test's five-second deadline is
not a new product setting. Deterministic confirmation regressions cover retry
while a post-rename sync fault remains active, both concurrent acquisition
outcomes, packed file/directory sync failures, held foreign packing locks and
independent create-new probes showing both required locks remain held at sync.

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
interruptions during creation, write/sync and promotion before product adoption. This includes the
new packed-ref and post-publication ref markers, not just the writable ref lock.
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

Original Task 2 `make check`/`make verify` receipts under
`.artifacts/issue-67/task2-20261030/` apply to commit
`1b755a7cc5ca973bb70e273b74dd342c1c95f895`; they **predate the I1 fix** and are not
current-tree full-verification evidence. The original reader/resource receipts
remain valid for that unchanged reader implementation. M1's maintained per-size
delta assertion remains assigned to later final review; this fix does not change
that matrix.

I1 evidence is separate: `.artifacts/issue-67/task2-fix1-20261030/`.
Four deterministic regressions failed on the original equality-only paths.
The restored focused durability suite passes 20 tests. Five semantic mutations
(visibility bypass, swallowed sync errors, prematurely released packing lock,
erased contention classification, followed packed backing symlink) fail through
runtime assertions. Removing the actual backing sync or the actual parent sync
is detected by the syscall oracle in each of loose, packed and concurrent
confirmation. Restored traces require backing sync before all parent syncs, and
for the concurrent case after the winner's rename; they record no foreign-lock
sync. All source mutations were restored byte-for-byte.

Current-tree `make check` passed (`make-check.log`: `ok`), and
`cargo test --locked --offline --release --lib git::durability` passed 20 tests
(`release-focused.log`). Full `make verify`, Linux/native acceptance
and handled-SIGINT integration must be checked on the eventual adopted tree;
this fix does not relabel the original full run as current evidence.

Exact commands, results and scoped commit are appended to
`.superpowers/sdd/2026-10-30-issue-67-product-git-optimization/task-2-report.md`.

## Task 7 integrated acceptance — 2026-10-31

This section updates the product state; Task 2 sections above remain evidence of
their captured revision. `GitRepository` now owns in-process discovery, raw object
reads/writes, index/ignore listing and exact retention. Product public interfaces,
Git object formats, snapshot membership and strong/weak verification boundaries
are unchanged. Source-only declarations are supplemented by the current platform
receipts recorded in the Task 7 report; do not apply old measured resource values
to an uncaptured platform or release.

### Executable reachability audit

The resolved `gix -> gix-protocol -> gix-transport -> gix-command` chain is present.
The claim is about executed call paths, not dependency names:

- `src/git/repository.rs` uses local `discover_opts` with isolated Git routing,
  `permissions.config.git_binary=false`, `config.env=false` and replace refs off.
  gix 0.90.0 `config/cache/init.rs::source_path` tests this permission before
  resolving Git-installation config; `open/permissions.rs` also defaults the
  attribute Git-binary permission off.
- `src/git/corpus.rs` uses `index_or_empty`, a newly constructed `excludes` stack,
  pathspec matching and `gix_dir::walk`. gix's index `recent_snapshot` rereads on
  changed on-disk mtime. The maintained SHA-1/SHA-256 same-handle regression stages
  an ignored file, removes a tracked entry and changes `.gitignore` between calls.
  Deliberately restored index mtimes are outside that regression's evidence.
- `src/git/reader.rs` calls raw `Store::at_opts` and `try_find_cached` with `Never`;
  `gix-odb 0.87.0` dynamic find resolves local packs/loose stores. It does not enter
  remote connection or filter APIs. Replacements/alternates are disallowed and
  missing promisor objects fail locally. Raw writes use the loose store; refs use
  lock/file/packed-ref readers, without identity discovery or reflog generation.
- gix's `filter` module is behind the disabled `attributes` feature. Transport
  command preparation/spawn lives in `gix-transport 0.62.0`
  `client/blocking_io/file.rs`; `client/mod.rs` gates it on `blocking-client`, which
  is not resolved. No product caller enters remote/fetch/checkout/filter APIs.
- `src/process/detached.rs` is the sole production process-spawn site: the already
  selected absolute Letopis executable with `embed --drain`. The signal listener
  spawns a Rust thread. Other command/spawn matches under `src/` are test fixtures,
  under test modules or `cfg(test)`; they are not product fallbacks.

Exact resolved feature tree, source-spawn inventory and SHA-256 fingerprints of
the audited dependency files are under `.artifacts/issue-67/task7-20261031/`.
Linux process tracing complements this audit; PATH poisoning alone cannot
establish absence of absolute helpers. The network-disabled model-free matrix
has no eligible detached work, so its traced intervals require exactly one
Letopis exec and no non-thread fork/clone. Nonempty direct drain uses only a
fixture-owned loopback provider. Existing detached-drain and cancellation suites
remain part of full platform verification.

### Maintained acceptance and limits

The command matrix checks literal public results for init/config/check/status,
FTS search, inspect/traverse, automatic refresh/reindex, disabled and nonempty
direct drain, safe failures and help/version outside a repository. Fixtures and
Git oracles execute outside the product tracing intervals. Source inspection
retains both unrequested marked knowledge and declared local source files;
existing exact-byte/range/hash, captured marked-source, unknown-batch and final
strong-pin tests remain binding.

An owned test-only reader counter checks one validation per distinct supplied
blob OID in assembly. Corrupt supplied bytes and post-retention closure damage
still fail. The delta fixture now requires a real delta separately in each 1 MiB
and 64 MiB pair, for both formats. Preflight independently asserts last observation
before retention before SQL publication.

The actual-resource cleanup test delivers repeated SIGINT while cleanup is
acknowledged and paused after stage detachment; the child must emit one cancelled
envelope and preserve the foreign lock inode/bytes. A missing-parent filesystem
fault proves residual ownership is retained, other resources are still released,
and retry after availability is restored leaves no stage. This is not every
filesystem failure or every kernel interleaving; prior dirfd-model boundaries
remain as recorded in Task 4. No physical power-loss, NFS or x86_64 claim is made.

The Docker build-context omission for the collision input was fixed with an exact
`.dockerignore` exception for `tests/fixtures/git/sha-mbles-1.bin`. Existing Docker
COPY lines alone did not establish availability, and no previous Linux success
is inferred from them. Historical receipts remain immutable.


### Platform failures and explicit oracle decision

The initial Linux native build failed because `tests/fixtures/config/corpus.json`
was absent from the Docker context/COPY despite a root unit-test `include_str!`.
The exact fixture was admitted and copied; its contents were unchanged. The next
Linux gate reached the tests and exposed eight differential failures with Debian
Git 2.39.5. Those failed receipts remain at `task7-linux-native-20261031` and
`task7-linux-native-final-20261031`, respectively; neither is a Linux PASS.

The controller explicitly retained the approved strict validator and fixed the
independent Linux oracle to upstream Git 2.50.1, matching the frozen host version
(Apple Git 2.50.1). [Upstream 2.39.5 object-file.c](https://raw.githubusercontent.com/git/git/v2.39.5/object-file.c)
only walks tree descriptors in `check_tree`; [upstream 2.50.1 object-file.c](https://raw.githubusercontent.com/git/git/v2.50.1/object-file.c)
sets `opts.strict=1` and runs `fsck_buffer`. The approved reference is strict tree
acceptance; cross-version malformed acceptance is explicitly not claimed. No
validator rule or differential assertion was relaxed or skipped.

Both Dockerfiles invoke `tests/git/install-oracle.sh`. The upstream archive is
verified against SHA-256 `7e3e6c36decbd8f1eedd14d42db6674be03671c2204864befa2a41756c5c8fc4`,
from [kernel.org checksum listing](https://www.kernel.org/pub/software/scm/git/sha256sums.asc).
Tag `v2.50.1` is annotated object `f92a7aa85a6c7fe9c45d944ec3b37598ede47274`,
commit `d82adb61ba2fd11d8f2587fca1b6bd7925ce4044`. This is fixture/build tooling,
not a Letopis dependency or fallback. Installed `/opt/git-oracle/SOURCE` and image
IDs preserve its identity; no claim of independently verified PGP signatures is made.

The first host gate passed debug but failed two release cancellation observations.
The saved native trace shows the correct main-thread gix ignore read; release
optimization inlined the free corpus-listing function into the concrete
`GitRepository` implementation of `CorpusLister::list_candidates`. The macOS-only
test observer now accepts either evidenced frame while still requiring the main
thread, gix ignore reader and counted native read. Independent positive debug/
release-frame cases and negatives for wrong thread, absent caller, absent ignore
reader and absent native read cover the predicate. Real release cancellation
suites pass 14/14 after the fix; the failed native receipt remains unchanged.
This was observer sensitivity to optimization, not demonstrated product failure.


### Completed platform receipts

- Host native runner PASS: `.artifacts/issue-67/task7-host-final-20261031/`;
  macOS arm64, Rust 1.95.0, Apple Git 2.50.1. Root debug and release each:
  945 passed, 0 failed, 10 ignored, 70 executables. The separate real 60 s fake
  provider deadline and copied native standalone/linkage checks also pass.
- That full host snapshot precedes only the final `cli_help` test-fixture fix,
  Git README additions and Docker-only build flags. Current-tree CLI help passes
  4 debug + 4 release tests, scoped Clippy and fmt. All `src/`, Cargo/toolchain,
  native/vendor/schema/example/script bytes are unchanged; exact differences
  and source fingerprints are in `task7-20261031/final-gates.json`.
- Fresh final Linux native runner PASS:
  `.artifacts/issue-67/task7-linux-complete-20261031/`; Linux aarch64, Rust 1.95.0,
  upstream Git 2.50.1. Root debug and release each: 939 passed, 0 failed,
  10 ignored, 70 executables; native/standalone/linkage, strict docs and 60 s
  fake-provider deadline pass. Outer and copied inner input hashes match every
  current captured input. Image `sha256:94ab00f9ccc23d130ceaa9ab1d4119458ded17ffd5f2ad39381de15aa2a8a990`.
- Final focused Linux tracing: 4 tests, 39 product invocations, no unexpected
  exec/fork/clone. Image `sha256:743a1b00351979f80d213fd73092433b04f28435cfc2187bd242fd95ccee5098`;
  `linux-trace-final-receipt/` and `tracing-summary.json` retain per-invocation
  argv, traces and hashes. Full Linux root suites are supplied by the native
  image, not redundantly claimed from the focused Git-image run.

Both Linux verification paths disable incremental/debug-info build overhead via
`CARGO_INCREMENTAL=0`, `CARGO_PROFILE_DEV_DEBUG=0`, `CARGO_PROFILE_TEST_DEBUG=0`;
debug assertions remain enabled and release configuration is unchanged. These
are verification settings, not timing settings for the future before/after
performance campaign. One earlier pinned-oracle run exhausted Docker disk before
tests; a later release run exposed the CLI fixture BrokenPipe race. All failed
attempts remain in separate directories. The final CLI fixture keeps stdin open
without a body or EOF, checks the same error/exit and is mutation-sensitive to an
incorrect stdin read before argv rejection.


### Task 8 frozen performance scope (2026-10-31)

The measured optimized product is committed revision
`ba4a11db26705999018187e8301a51e9942fe1ba`, executable SHA-256
`18288faf715310d6ceccd03bedd35adc21e1ab1654c4765c30eff1f3a85a2870`.
The baseline is the unchanged Task 1 frozen release at
`881cc6417073110e852fbebf2b9bce5ed1ea3d29`, with base-equivalent product inputs
from `32ca0af282a1321d0137b4aba115c6fddc086259`. Both use Rust 1.95.0,
`cargo build --release --locked --offline --message-format=json` and
`CARGO_PROFILE_RELEASE_DEBUG=1`; sources, Mach-O/debug identities, receipts and
executable hashes are independently preserved and reverified.

Task 8 changes no Rust/native/dependency input. The accepted Task 7 host/Linux
source and platform scopes above remain the product verification evidence. Its
comparison-harness timeout scheduling correction is covered by focused Python
RED/GREEN and mutation evidence, separately pinned from the unchanged binaries.
The original check60/reads1k campaigns retain the earlier captured harness; later
stages capture the corrected harness. No unchanged root Rust/native checks were
rerun solely to attach a newer timestamp.

The performance campaign runs on the same arm64 macOS host, with disabled
embeddings and unchanged synthetic external declarations, at 60 and 1,000
elements. No larger-scale, real-provider, x86_64 or Linux timing claim is made.
Owned-PID sampled RSS uses macOS libproc in separate instrumented calls; OS wait4
lifetime RSS/accounted descendants remain separately labeled. Neither proves
whole-stack concurrent peak or replaces the per-object allocation/capacity gate.
See [report.md](report.md) for the actual outcomes and budgets,
[review.md](review.md) for unresolved review scope and
[evidence-index.json](evidence-index.json) for the canonical local hash index.
