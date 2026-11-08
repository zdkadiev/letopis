<!-- letopis: {"created_at":1793278392,"source_snapshot":"b3d2f67dce3dfb7bddbcd5e90545b1a475d8e5fb"} -->
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

`tree_retain_by_oid` validates a tree root and durably retains its exact ref without
walking all descendants. `tree_pin_by_oid` additionally validates the reachable closure on each call
before retaining the root. Gitlinks are not traversed by that closure walk. This
distinction lets routine work retain roots while full audits use stronger checks.

[Git repository](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")

[Retention and closure](../../src/git/refs.rs "letopis:type=external;path=src/git/refs.rs")

[Inspection capture](../../src/service/inspection.rs "letopis:type=external;path=src/service/inspection.rs")

[Shared-permission test oracle](../development/git-shared-permissions-oracle.md "letopis:type=related_to;path=letopis/development/git-shared-permissions-oracle.md")
