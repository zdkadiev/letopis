<!-- letopis: {"created_at":1793278392,"source_snapshot":"2f8c5badf7c901f677e7498ee5ea2fdab9b1287d"} -->
# Git snapshot storage

`GitRepository` discovers a Git work tree in process through the pinned gix library and implements snapshot operations for creating and reading trees and blobs and retaining snapshot trees. It rejects object stores configured with Git alternates and rechecks this condition at snapshot operation entry.

[Git snapshot implementation](../../src/git/repository.rs "letopis:type=external;path=src/git/repository.rs")
