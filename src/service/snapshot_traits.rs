#[cfg(doc)]
use super::SnapshotErrorKind;
use super::{RetainedTree, SnapshotError};
use crate::domain::{BlobOid, RepoPath, TreeOid};

/// Read-only classification of one exact snapshot root object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    /// The exact tree exists and its own bytes match its object identity.
    Available,
    /// The root object does not exist in the repository.
    Missing,
    /// The object has a different type or an incompatible repository OID format.
    Invalid,
}

/// Synchronous verification of exact snapshot root objects without side effects.
///
/// Implementations verify the tree object's own bytes and identity, without
/// checking its child closure, retaining objects, changing refs or reading live
/// source files. Commits and tags must not be peeled into trees.
pub trait SnapshotVerifier {
    /// Classify an exact root without creating, writing or retaining Git objects.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::InternalError`] for repository policy, Git I/O
    /// or object integrity failures; these failures must not become a state.
    fn verify_snapshot(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError>;
}

/// One captured regular file. The adapter never reads its live path.
#[derive(Debug)]
pub struct SnapshotFile<'a> {
    /// Root-relative path in the resulting tree.
    pub path: &'a RepoPath,
    /// Exact captured bytes, without Git filters or normalization.
    pub bytes: &'a [u8],
}
/// One validated path and previously written blob in a snapshot request.
#[derive(Debug)]
pub struct SnapshotEntry {
    /// Root-relative path in the resulting tree.
    pub path: RepoPath,
    /// Exact captured bytes' blob identity.
    pub oid: BlobOid,
}
/// Synchronous immutable snapshot operations. Creation alone does not retain.
pub trait GitSnapshots {
    /// Create a tree from exactly these files, preserving captured bytes.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::InvalidRequest`] for invalid, duplicate or
    /// colliding paths, and [`SnapshotErrorKind::InternalError`] when repository
    /// policy, Git I/O or object integrity checks fail.
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError>;
    /// Write exact captured bytes without filters or normalization; does not retain.
    ///
    /// # Errors
    /// Reports repository policy, Git I/O or object integrity failures.
    fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError>;
    /// Assemble exactly these paths after validating all blobs; does not retain.
    ///
    /// # Errors
    /// Invalid paths, collisions, missing/wrong-type blobs or foreign formats are
    /// invalid requests; policy, I/O and corruption failures are internal errors.
    fn create_tree_from_blobs(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError>;
    /// Read a complete blob from an exact tree, without live-file fallback.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root or path,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format,
    /// non-tree root or non-blob path, and [`SnapshotErrorKind::InternalError`]
    /// for repository policy, Git I/O or integrity failures.
    fn get_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError>;
    /// Validate the exact root and retain its direct ref without auditing descendants.
    /// A retained ref protects healthy reachable objects from normal GC, not corruption.
    ///
    /// # Errors
    /// Missing roots are not found; wrong types/formats are invalid requests.
    /// Root corruption, policy, Git I/O and ref conflicts are internal errors.
    fn retain(&self, tree: &TreeOid) -> Result<(), SnapshotError>;
    /// Validate the complete local closure and retain it through Git GC.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format or
    /// non-tree root, and [`SnapshotErrorKind::InternalError`] for an incomplete
    /// or corrupt closure, repository policy, Git I/O or ref publication failure.
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError>;
}
