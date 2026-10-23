// Git wire values shared by object parsing, writing, retention and discovery.
pub(super) const BLOB_KIND: &str = "blob";
pub(super) const TREE_KIND: &str = "tree";
pub(super) const COMMIT_KIND: &str = "commit";
pub(super) const GITLINK_KIND: &str = "gitlink";
pub(super) const TREE_MODE: &str = "040000";
pub(super) const REGULAR_FILE_MODE: &str = "100644";
pub(super) const EXECUTABLE_FILE_MODE: &str = "100755";
pub(super) const SYMLINK_MODE: &str = "120000";
pub(super) const GITLINK_MODE: &str = "160000";
pub(super) const CAT_FILE_COMMAND: &str = "cat-file";
pub(super) const HASH_OBJECT_COMMAND: &str = "hash-object";
pub(super) const MKTREE_COMMAND: &str = "mktree";
pub(super) const REV_PARSE_COMMAND: &str = "rev-parse";
pub(super) const UPDATE_REF_COMMAND: &str = "update-ref";
pub(super) const MIN_GIT_VERSION: (u32, u32) = (2, 29);
pub(super) const SNAPSHOT_REF_PREFIX: &str = "refs/letopis/snapshots/";
