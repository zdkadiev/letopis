// Git wire values shared by object parsing, writing, retention and discovery.
pub(super) const HASH_CHUNK_BYTES: usize = 64 * 1024;
pub(super) const BLOB_KIND: &str = "blob";
pub(super) const TREE_KIND: &str = "tree";

pub(super) const OBJECT_INTEGRITY_MESSAGE: &str = "Git object integrity failed";

// Shared diagnostics for object identities that do not match the repository hash format.
pub(super) const FOREIGN_OID_FORMAT: &str = "OID does not match repository object format";
