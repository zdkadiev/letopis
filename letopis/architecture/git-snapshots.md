<!-- letopis: {"created_at":1793278392,"source_snapshot":"91961631e000754f6a0e1e589a54042248f540ee"} -->
# Git snapshot storage

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
[Write entry](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

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

[Shared-permission test oracle](../development/git-shared-permissions-oracle.md "letopis:type=related_to;path=letopis/development/git-shared-permissions-oracle.md")
