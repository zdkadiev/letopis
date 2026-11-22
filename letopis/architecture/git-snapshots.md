# Git snapshots and write cleanup

## Git snapshot storage {#snapshot-storage}
<!-- letopis: {"created_at":1793278392,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->
`GitRepository` discovers a non-bare worktree in process through the pinned gix
library. Product snapshot operations do not invoke the Git executable. The adapter
rejects object stores configured with Git alternates and rechecks this condition
at snapshot operation entry.

The snapshot interface writes blobs, assembles trees and reads a blob by tree and
repository-relative path. Trees identify immutable Git content; a snapshot tree is
not necessarily a commit or the current Git index. `inspect` uses this interface
to construct a tree containing captured knowledge and addressed local sources.
Its discovery observation is checked around final pin, so an author policy edit
during capture cannot return a successful snapshot selected under mixed policy.
A surviving external binding can still address an excluded knowledge-owner path.

Git stores each object either as a loose zlib-compressed file under
`.git/objects/ab/cdef…`, named by its object ID, or inside a pack file. A loose
object appears as soon as it is written and disappears when Git packs the store.
A snapshot write computes the object ID and reuses an existing loose object after
verifying its kind and identity and syncing its file and parent directories;
packed or absent objects take the ordinary staging path.
[Loose-object reuse](../../src/git/durability.rs#L183-L222 "letopis:type=external;path=src/git/durability.rs#L183-L222")
[Blob write entry](../../src/git/repository.rs#L190-L193 "letopis:type=external;path=src/git/repository.rs#L190-L193")

`ref_update_by_oid` validates one exact root and updates its direct ref without
auditing descendants; a later GC therefore protects healthy reachable objects but
not against hidden corruption. `tree_sync_by_oid` verifies the root's complete
local closure before updating the same ref, and that closure ignores gitlinks.
Routine work retains roots with the direct ref; full audits use the
closure-verifying path. `SnapshotVerifier::tree_verify_by_oid` only reports the
state of one exact root and never creates, writes or retains objects.

[Git repository](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")
[Snapshot interface](../../src/service/mod.rs "letopis:type=external;path=src/service/mod.rs")

[Inspection capture](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")

[Shared-permission test oracle](#shared-permissions "letopis:type=related_to;path=letopis/architecture/git-snapshots.md#shared-permissions")

## Git write cleanup ownership {#write-cleanup}
<!-- letopis: {"created_at":1793816837,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->
Composition shares one `Arc<GitWriteCleanup>` across the writing Git adapters of
a command. It owns the actual temporary resources independently of the foreground
stack, so a handled interruption can release them before process exit.

The state is a `cancelled` atomic flag plus `state_mutex: Mutex<WriteState>`. The
mutex guards the next lease id and the owned resources: a temporary stage
directory, a writable ref lock or a packed-ref marker holder. Each acquisition
admits or rejects the write and registers ownership under that gate.

Admission fails after cancellation, so a resource cannot be registered behind a
drain. The mutex makes the cancellation check and registration one step relative
to the cancelling thread; an atomic flag alone would allow a stage to be
registered after the map was emptied. A poisoned gate is recovered instead of
unwrapped, because cleanup must remain possible during unwinding.

The gate stays short. It is not held during payload writes, compression, hashing
or syncing; it is held for admission, ownership registration and removal, and for
publication such as creating the object directory, hard-linking a staged object,
or committing a ref lock.

Release is by identity, never by a remembered pathname. `WriteLease::drop`
removes and drops its resource under the same gate, normal completion removes and
commits under the gate, and object publication verifies that the lease still owns
a stage.

Cancellation drains the map, detaches each stage onto a fresh owned sibling,
retries `DirectoryNotEmpty`, and re-registers a stage it could not remove. It
reports cleanup failure but still attempts every resource, so an arbitrary
filesystem failure can leave residue.

[Git write cleanup](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Durable object and ref writes](../../src/git/durability.rs "letopis:type=external;path=src/git/durability.rs")

[Foreground listener wiring](../../src/lib.rs "letopis:type=external;path=src/lib.rs")

[Git snapshot storage](#snapshot-storage "letopis:type=related_to;path=letopis/architecture/git-snapshots.md#snapshot-storage")

## Shared Git object-directory permission oracle {#shared-permissions}
<!-- letopis: {"created_at":1793790634,"source_snapshot":"331ae0dd190b2aa304e1476fe8726bd016f3d4d8"} -->
The linked-worktree durability test compares the mode of the object directory
created by Letopis with a directory containing an object written by Git after
`core.sharedRepository` is set to `group`. The fixture initializes a repository
and writes other objects before it sets that option. A two-character loose-object
prefix can therefore identify a directory that existed before the policy change.

In a macOS arm64 full test run on 2026-11-04, that directory comparison failed:
Letopis's directory had mode `0775` and Git's oracle directory had mode `0755`.
The same test passed when rerun alone. The failed test did not record the
history of either directory, so the cause of that specific failure and the
correctness of the product's permission behavior remain unproven.

A separate Git-only experiment under umask `022` demonstrated why directory
history matters: create one loose-object prefix before enabling group sharing,
then write one object into that prefix and another into a fresh prefix. The
pre-existing directory remained `0755`, while Git created the fresh directory
as `02775`. A deterministic oracle should compare equivalent creation histories;
the experiment alone does not establish that the failing test used this case.

The current durability test selects two distinct loose-object prefixes that do
not yet exist, verifies both directories are absent, and compares publication
against the independently invoked Git oracle. This controls the relevant
creation history in the comparison; it does not establish the cause of the
historical failure or prove physical power-loss behavior.

[Durability test in the recorded snapshot](../../src/git/durability.rs "letopis:type=external;path=src/git/durability.rs")

[Current linked-worktree mode comparison](../../src/git/durability_test.rs#L459-L517)

[Object publication](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Git fixture](../../tests/support/git.rs "letopis:type=external;path=tests/support/git.rs")

[Git snapshots](#snapshot-storage "letopis:type=related_to;path=letopis/architecture/git-snapshots.md#snapshot-storage")
