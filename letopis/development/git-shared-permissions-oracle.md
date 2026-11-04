<!-- letopis: {"created_at":1793790634,"source_snapshot":"1c24df703adfbe1c196583a36ddcb19cea7bfeb5"} -->
# Shared Git object-directory permission oracle

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

[Durability test](../../src/git/durability.rs "letopis:type=external;path=src/git/durability.rs")

[Object publication](../../src/git/cleanup.rs "letopis:type=external;path=src/git/cleanup.rs")

[Git fixture](../../tests/support/git.rs "letopis:type=external;path=tests/support/git.rs")

[Git snapshots](../architecture/git-snapshots.md "letopis:type=related_to;path=letopis/architecture/git-snapshots.md")
