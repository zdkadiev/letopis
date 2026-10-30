# Issue #67 — compatibility gates

Date: 2026-10-30. Source inspection only; no dependency resolution, compiled
allocation probe, durability drill or optimization benchmark has passed yet.

## Original fixed-memory condition: superseded by explicit user approval

Inspected upstream gitoxide commit:
`8644bf3a05ec10a9fcf6bfa461b79d0378825aad`, whose manifests identify
`gix 0.90.0` and `gix-dir 0.32.0`. These version labels do not establish the
future locked transitive dependency graph or platform compilation.

The original specification required full-audit blob payload working memory
independent of blob size, including transient allocations inside the library.
It separately permitted complete blob bytes for addressed reads.

The raw object interface accepts a complete output vector. The normal packed
decoder resizes it to the decompressed object size. Delta reconstruction grows
it to two maximum-sized base/result regions plus decompressed instructions;
turning off caches does not remove those working allocations.

- [Object lookup interface](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-object/src/traits/find.rs#L16).
- [Complete-object decode](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-pack/src/data/file/decode/entry.rs#L192).
- [Delta working buffers](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-pack/src/data/file/decode/entry.rs#L354).

Read-only reconnaissance also checked the pack streaming iterator, lookup and
delta-tree verification callbacks, loose verification and low-level zlib APIs.
Streaming pack input does not supply a reconstructed delta-body stream; traversal
callbacks run after payload materialization. Low-level fixed-buffer inflation
does not itself provide delta reconstruction. A custom seekable-spool pack/delta
engine would be additional work explicitly excluded by the approved plan.

Main-agent verification independently inspected the decisive non-delta resize
and two-region delta allocation in the pinned source. This is a source-established
compatibility gap, not a measured RAM number or proof that no future library could
provide the required mechanism. Mapping compressed input is distinct from these
explicit decompressed heap allocations. Spooling after allocation cannot satisfy
the approved bound.

**User decision, 2026-10-30:** allow per-object decoding buffers, including delta
buffers, with bounded caches and unchanged snapshot, integrity and durability
requirements. This explicitly supersedes the original fixed-memory condition;
the source findings remain valid and must inform measured resource reporting.
The updated plan keeps payload caches disabled and drops reusable decoding
capacity above 4 MiB after each object. The reuse threshold does not reject large
objects. Compilation, large-object allocation/RSS observations, checksum parity
and durability probes are still pending; this decision is not a passing gate.

## Write/ref durability gate: unresolved

The current adapter requests `core.fsync=all` and `core.fsyncMethod=fsync` for
object/tree/ref writes. Inspected gix loose-object finalization persists a temporary
file without an explicit file or directory sync in that path. Ref transactions
also require a closer inspection of lock-file sync and publication ordering.

- [Loose-object finalization](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-odb/src/store_impls/loose/write.rs#L151).
- [Ref preparation](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-ref/src/store/file/transaction/prepare.rs#L174).
- [Ref publication](https://github.com/GitoxideLabs/gitoxide/blob/8644bf3a05ec10a9fcf6bfa461b79d0378825aad/gix-ref/src/store/file/transaction/commit.rs#L105).

These findings do not prove write parity or physical power-loss safety. Reopening
published files for sync is not automatically equivalent to pre-publication
ordering. Required adapter sync boundaries and race/failure behavior need an
explicit implementation and disposable probes before this gate can pass.
The proposed memory decision does not authorize weakening durability.

Read-only follow-up identified candidate mechanisms for the implementation probe:
write through the loose-store API into operation-owned private same-filesystem
staging, synchronize the completed object file before no-clobber promotion, then
sync affected directories. A lower-level owned library lock exposes a mutable
file for pre-commit `sync_all`. Neither is a proven implementation yet.

Prefer preserving the library ref transaction where its semantics fit; any
prepared-lock synchronization must prove file identity and ownership. In
particular, preparation may accept an already equal direct ref and release its
lock as a no-op; blindly opening a later `.lock` can touch another writer's file.
Reflog behavior and commit ordering also need explicit parity checks. These are
concrete probe requirements, not a request to relax durability or add a permanent
alternative object store.

## Evidence and limits

Read-only research report is retained in the plan-scoped execution workspace as
`streaming-recon.md`. Load-bearing source references are reproduced here because
that workspace is ignored and temporary. No Rust source/Cargo changes, production
library migration, performance improvement or acceptance completion is asserted.
