//! Application contracts.
mod retained_tree;
mod snapshot_error;

use crate::domain::{RepoPath, TreeOid};
pub use retained_tree::RetainedTree;
pub use snapshot_error::{SnapshotError, SnapshotErrorKind};

/// One captured regular file. The adapter never reads its live path.
pub struct SnapshotFile<'a> {
    /// Root-relative path in the resulting tree.
    pub path: &'a RepoPath,
    /// Exact captured bytes, without Git filters or normalization.
    pub bytes: &'a [u8],
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
    /// Read a complete blob from an exact tree, without live-file fallback.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root or path,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format,
    /// non-tree root or non-blob path, and [`SnapshotErrorKind::InternalError`]
    /// for repository policy, Git I/O or integrity failures.
    fn read_blob(&self, tree: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError>;
    /// Validate the complete local closure and retain it through Git GC.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format or
    /// non-tree root, and [`SnapshotErrorKind::InternalError`] for an incomplete
    /// or corrupt closure, repository policy, Git I/O or ref publication failure.
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError>;
}

mod error;
use crate::domain::{Edge, EdgeTarget, KnowledgeId, Sha256};
mod model;
pub use error::{StorageError, StorageErrorKind, StorageResult};
pub use model::{FileRow, KnowledgeMetadata, NodeMetadata, SourceMetadata, StoredNode, TextMatch};

/// Synchronous storage with exclusive scoped capabilities.
pub trait Storage {
    /// An owned read transaction borrowing this storage exclusively.
    type Read<'a>: StorageRead
    where
        Self: 'a;
    /// An atomic cache publication capability.
    type CacheWrite<'a>: CacheWrite
    where
        Self: 'a;
    /// Begin an exclusive cache publication transaction.
    ///
    /// # Errors
    /// Returns an internal error on contention or database failure.
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>>;
    /// Begin and pin a read of cache revision and vector identity.
    ///
    /// # Errors
    /// Returns an internal error if the snapshot cannot be opened or decoded.
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>>;
}
/// Observations pinned within one read transaction.
pub trait StorageRead {
    /// Read typed metadata by normalized identity.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>>;
    /// List all declared edges, optionally restricted to their source.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted edges.
    fn list_edges(&mut self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>>;
    /// Read a candidate file's change-detection state.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn read_file(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>>;
    /// Execute an already-prepared FTS expression without a candidate limit.
    ///
    /// # Errors
    /// Returns an invalid request for malformed expressions, or an internal error for I/O.
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>>;
    /// Publication revision observed by this read.
    fn get_revision(&self) -> u64;
    /// Vector configuration identity observed by this read.
    fn get_config_identity(&self) -> Option<Sha256>;
}

/// Caller-owned atomic cache publication; any failed mutation prevents commit.
pub trait CacheWrite: Sized {
    /// Insert or replace metadata while preserving its row ID.
    ///
    /// # Errors
    /// Rejects unsupported numeric values or database failures.
    fn upsert_node(&mut self, node: &NodeMetadata) -> StorageResult<i64>;
    /// Insert a declared edge, including a dangling target.
    ///
    /// # Errors
    /// Rejects duplicate edges and missing sources; reports database failures.
    fn insert_edge(&mut self, edge: &Edge) -> StorageResult<()>;
    /// Remove all declarations owned by a knowledge element.
    ///
    /// # Errors
    /// Reports database failures.
    fn delete_outgoing(&mut self, source: &KnowledgeId) -> StorageResult<()>;
    /// Delete a node and its FTS entry, preserving incoming edges.
    ///
    /// # Errors
    /// Rejects deletion while outgoing edges remain; reports database failures.
    fn delete_node(&mut self, id: &EdgeTarget) -> StorageResult<()>;
    /// Replace a knowledge element's contentless FTS entry.
    ///
    /// # Errors
    /// Returns not found for absent knowledge, or a database failure.
    fn replace_text(&mut self, id: &KnowledgeId, text: &str) -> StorageResult<()>;
    /// Replace a Markdown candidate's state.
    ///
    /// # Errors
    /// Rejects unsupported numbers and inconsistent validity; reports database failures.
    fn upsert_file(&mut self, file: &FileRow) -> StorageResult<()>;
    /// Remove a candidate's state.
    ///
    /// # Errors
    /// Reports database failures.
    fn delete_file(&mut self, path: &RepoPath) -> StorageResult<()>;
    /// Publish all mutations and increment the revision once, including empty units.
    ///
    /// # Errors
    /// Rejects failed units, revision overflow, and database failures, rolling back.
    fn commit(self) -> StorageResult<u64>;
}
