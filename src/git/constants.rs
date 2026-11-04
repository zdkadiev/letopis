// Git wire values shared by object parsing, writing, retention and discovery.
pub(super) const HASH_CHUNK_BYTES: usize = 64 * 1024;
pub(super) const BLOB_KIND: &str = "blob";
pub(super) const TREE_KIND: &str = "tree";
pub(super) const GITLINK_KIND: &str = "gitlink";
pub(super) const SNAPSHOT_REF_PREFIX: &str = "refs/letopis/snapshots/";

pub(super) const OBJECT_INTEGRITY_MESSAGE: &str = "Git object integrity failed";
