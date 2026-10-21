//! Immutable Git snapshots, independent of the concrete Git transport.
use crate::domain::{RepoPath, TreeOid};
use std::{error::Error, fmt};

/// One captured regular file. The adapter never reads its live path.
pub struct SnapshotFile<'a> {
    /// Root-relative path in the resulting tree.
    pub path: &'a RepoPath,
    /// Exact captured bytes, without Git filters or normalization.
    pub bytes: &'a [u8],
}
/// Stable classification for snapshot failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotErrorKind {
    /// Invalid path, object format or object type.
    InvalidRequest,
    /// The requested root or path does not exist.
    NotFound,
    /// Git, repository integrity or retention failed.
    InternalError,
}
/// A safe diagnostic, retaining I/O or exit causes without subprocess payloads.
#[derive(Debug)]
pub struct SnapshotError {
    kind: SnapshotErrorKind,
    message: &'static str,
    cause: Option<Box<dyn Error + Send + Sync>>,
}
impl SnapshotError {
    pub(crate) fn new(kind: SnapshotErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            cause: None,
        }
    }
    pub(crate) fn with_cause(
        message: &'static str,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: SnapshotErrorKind::InternalError,
            message,
            cause: Some(Box::new(cause)),
        }
    }
    /// Classification suitable for application error mapping.
    pub fn kind(&self) -> SnapshotErrorKind {
        self.kind
    }
    /// Static operation context, without repository paths or file bytes.
    pub fn message(&self) -> &str {
        self.message
    }
}
impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)?;
        if let Some(c) = &self.cause {
            write!(f, ": {c}")?;
        }
        Ok(())
    }
}
impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_ref().map(|c| c.as_ref() as &dyn Error)
    }
}
/// A tree whose complete local closure has a confirmed permanent service ref.
#[derive(Debug)]
pub struct RetainedTree {
    oid: TreeOid,
}
impl RetainedTree {
    pub(crate) fn new(oid: TreeOid) -> Self {
        Self { oid }
    }
    /// The exact retained tree OID.
    pub fn oid(&self) -> &TreeOid {
        &self.oid
    }
}
/// Synchronous immutable snapshot operations. Creation alone does not retain.
pub trait GitSnapshotPort {
    /// Create a tree from exactly these files, preserving captured bytes.
    fn create_tree(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError>;
    /// Read a complete blob from an exact tree, without live-file fallback.
    fn read_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError>;
    /// Validate the complete local closure and retain it through Git GC.
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError>;
}
