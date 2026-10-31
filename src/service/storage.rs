use super::{
    EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary, EmbeddingWork, FileRow,
    GraphRead, NodeMetadata, QueryError, StorageResult, StoredNode, StoredVectorKey, TextMatch,
    VectorMatch, VectorOutcome,
};
use crate::domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid};
use std::{collections::BTreeSet, num::NonZeroUsize};

/// Synchronous storage with exclusive scoped capabilities.
///
/// Every implementation hands out at most one scoped capability at a time (hence
/// `&mut self`). A write capability commits atomically or not at all, and a dropped
/// or failed capability leaves the stored data unchanged.
pub trait Storage {
    /// An owned read transaction borrowing this storage exclusively.
    type Read<'a>: StorageRead + GraphRead
    where
        Self: 'a;
    /// An atomic cache publication capability.
    type CacheWrite<'a>: CacheWrite
    where
        Self: 'a;
    /// An atomic vector-file write capability.
    type VectorWrite<'a>: VectorWrite
    where
        Self: 'a;
    /// Begin a write only when the pinned vector identity matches.
    ///
    /// # Errors
    /// Reports contention or database failures.
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>>;
    /// Atomically synchronize vectors using a prior observed identity, including absence.
    ///
    /// An already current `desired` identity returns `Current(false)` without changes.
    /// Otherwise a matching observation resets vectors and attempts, returning `Current(true)`.
    /// A superseded observation returns `ConfigChanged` without changing any data.
    ///
    /// # Errors
    /// Reports contention, schema or database failures, rolling back all changes.
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>>;
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
/// Observation-only access to an optional published cache.
///
/// Implementations never create durable databases, bootstrap or repair schemas,
/// change configuration or vector identity, or acquire a database writer. Ordinary
/// read-side journal/shared-memory files may still be managed by the database engine.
/// Missing publication is distinct from read failure.
pub trait StatusStorage {
    /// One exclusively borrowed consistent database view.
    type Read<'a>: StorageRead
    where
        Self: 'a;
    /// Pin the current publication, returning none only when no publication exists.
    ///
    /// # Errors
    /// Reports inaccessible, malformed or incompatible persisted state.
    fn begin_status_read(&mut self) -> StorageResult<Option<Self::Read<'_>>>;
}

/// Observations pinned within one read transaction.
///
/// Every implementation answers all methods from one consistent view, so results
/// never mix revisions, and never modifies stored data.
pub trait StorageRead {
    /// Count distinct current knowledge hashes under the captured policy.
    /// Sources and orphan vector/failure rows never count. A usable vector takes
    /// precedence over failures; unavailable or mismatched identities count only backlog.
    ///
    /// # Errors
    /// Rejects invalid attempt budgets, malformed persisted values and database failures.
    fn get_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary>;
    /// Read the first eligible knowledge ID strictly after the optional cursor.
    ///
    /// # Errors
    /// Rejects zero or unrepresentable attempt budgets; reports I/O or invalid data.
    fn get_next_embedding_work_by_id_and_max_attempts(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        id: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;
    /// Observe the native vector dimension under the pinned configuration identity.
    ///
    /// # Errors
    /// Reports malformed native metadata or database failures.
    fn get_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>>;
    /// Read typed metadata by normalized identity.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>>;
    /// List distinct non-null author-declared source roots without reading node metadata.
    ///
    /// # Errors
    /// Reports database failures or malformed persisted tree identities.
    fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>>;
    /// List all published node metadata in ascending normalized identity order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn get_nodes(&mut self) -> StorageResult<Vec<StoredNode>>;
    /// List all declared edges, optionally restricted to their source.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted edges.
    fn get_edges_by_source_id(
        &mut self,
        source_id: Option<&KnowledgeId>,
    ) -> StorageResult<Vec<Edge>>;
    /// Read a candidate file's change-detection state.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn get_file_by_path(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>>;
    /// List all candidate file states in ascending exact path order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn get_files(&mut self) -> StorageResult<Vec<FileRow>>;
    /// Execute an already-prepared FTS expression without a candidate limit.
    ///
    /// # Errors
    /// Returns an invalid request for malformed expressions, or an internal error for I/O.
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>>;
    /// Read a text-addressed vector under the expected pinned identity.
    ///
    /// # Errors
    /// Reports malformed stored vectors or database failures.
    fn get_vector_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>>;
    /// Find native cosine matches joined to current node search hashes.
    ///
    /// # Errors
    /// Rejects invalid query vectors or reports database/integrity failures.
    fn find_nearest_vectors(
        &mut self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>>;
    /// Read durable attempts and the latest failure under the pinned identity.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn get_failure_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>>;
    /// Read the last query embedding failure, independent of model resets.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn get_query_error(&mut self) -> StorageResult<Option<QueryError>>;
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

/// Caller-owned atomic vector write; any failed mutation prevents commit.
pub trait VectorWrite: Sized {
    /// Remove vectors and failures absent from this transaction's cache observation.
    /// Concurrent publication can leave recomputable missing vectors until next drain.
    ///
    /// # Errors
    /// Reports database failures and prevents commit of a failed unit.
    fn prune_unreferenced(&mut self) -> StorageResult<()>;
    /// Reserve one durable attempt within the caller's total budget.
    /// Commit before calling the provider; an unsent reservation still counts.
    /// Returns `true` when an attempt was reserved and `false` when the budget is exhausted.
    ///
    /// # Errors
    /// Rejects zero or budgets above `i64::MAX` and reports database failures.
    fn reserve_attempt(&mut self, hash: &Sha256, max_attempts: u64) -> StorageResult<bool>;
    /// Replace an existing reservation's error with the caller's safe message.
    /// Returns false when no reservation exists; never creates an attempt.
    ///
    /// # Errors
    /// Reports database failures.
    fn record_failure(&mut self, hash: &Sha256, safe_error: &str) -> StorageResult<bool>;
    /// Replace the singleton query failure with an already-safe message.
    ///
    /// # Errors
    /// Rejects timestamps above `i64::MAX` and reports database failures.
    fn set_query_error(&mut self, at: u64, safe_error: &str) -> StorageResult<()>;
    /// Clear the singleton error after a successful query embedding.
    ///
    /// # Errors
    /// Reports database failures.
    fn clear_query_error(&mut self) -> StorageResult<()>;
    /// Store one text vector, preserving an existing key and vector.
    ///
    /// # Errors
    /// Rejects empty, nonfinite or mismatched vectors and reports database corruption/I/O.
    fn store_vector(
        &mut self,
        hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey>;
    /// Commit all writes atomically.
    ///
    /// # Errors
    /// Rejects poisoned units or reports database failures, rolling back.
    fn commit(self) -> StorageResult<()>;
}
