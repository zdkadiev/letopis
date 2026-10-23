//! Application contracts.
mod background;
pub(crate) use background::BackgroundService;
mod retained_tree;
mod snapshot_error;

use crate::domain::{RepoPath, TreeOid};
pub use retained_tree::RetainedTree;
pub use snapshot_error::{SnapshotError, SnapshotErrorKind};

/// One captured regular file. The adapter never reads its live path.
#[derive(Debug)]
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

mod document;
mod error;
mod projection;
use crate::domain::{Edge, EdgeTarget, KnowledgeId, Sha256};
pub(crate) use document::DocumentService;
mod model;
pub use error::{StorageError, StorageErrorKind, StorageResult};
pub use model::{
    Diagnostic, DiagnosticKind, EmbeddingFailure, EmbeddingWork, FileRow, KnowledgeMetadata,
    NodeMetadata, ParsedElement, ParsedFile, QueryError, SourceMetadata, StoredNode,
    StoredVectorKey, TextMatch, VectorMatch, VectorOutcome,
};

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
    /// Atomically clear vectors for a changed identity; return whether changed.
    ///
    /// # Errors
    /// Reports contention, schema or database failures, rolling back all changes.
    fn reset_vectors(&mut self, identity: &Sha256) -> StorageResult<bool>;
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
    /// Read the first eligible knowledge ID strictly after the optional cursor.
    ///
    /// # Errors
    /// Rejects zero or unrepresentable attempt budgets; reports I/O or invalid data.
    fn read_next_embedding_work(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        after: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;
    /// Observe the native vector dimension under the pinned configuration identity.
    ///
    /// # Errors
    /// Reports malformed native metadata or database failures.
    fn read_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>>;
    /// Read typed metadata by normalized identity.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>>;
    /// List all published node metadata in ascending normalized identity order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn list_nodes(&mut self) -> StorageResult<Vec<StoredNode>>;
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
    /// List all candidate file states in ascending exact path order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn list_files(&mut self) -> StorageResult<Vec<FileRow>>;
    /// Execute an already-prepared FTS expression without a candidate limit.
    ///
    /// # Errors
    /// Returns an invalid request for malformed expressions, or an internal error for I/O.
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>>;
    /// Read a text-addressed vector under the expected pinned identity.
    ///
    /// # Errors
    /// Reports malformed stored vectors or database failures.
    fn read_vector(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
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
    fn read_failure(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>>;
    /// Read the last query embedding failure, independent of model resets.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn read_query_error(&mut self) -> StorageResult<Option<QueryError>>;
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

pub use error::{DocumentError, DocumentErrorKind, DocumentResult};
pub use model::{CapturedFile, FileStat, RefreshOutcome};

/// Synchronous document publication operations.
pub trait Documents {
    /// Detect and atomically publish changed file projections.
    ///
    /// # Errors
    /// Reports unsafe paths, concurrent changes, or Git, filesystem and storage failures.
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome>;
}
/// Repository-owned discovery of Markdown candidate paths.
pub trait CorpusFiles {
    /// List normalized, deduplicated candidates including deleted tracked files.
    ///
    /// # Errors
    /// Reports unsupported candidate paths or Git failures.
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>>;
}
/// Confined synchronous file observations and publication exclusion.
pub trait FileSystem {
    /// Exclusive kernel lock released when its guard is dropped.
    type IndexLock;
    /// Observe a regular file without reading its body; stable absence is `None`.
    ///
    /// # Errors
    /// Rejects symlinks, nonregular paths, observed changes and I/O failures.
    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>>;
    /// Capture exact regular-file bytes, checking identity and stat around the read.
    ///
    /// # Errors
    /// Rejects unsafe paths, observed replacements or edits, clock and I/O failures.
    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>>;
    /// Observe wall-clock nanoseconds since the Unix epoch with checked conversion.
    ///
    /// # Errors
    /// Reports clocks before the epoch or values outside the supported range.
    fn get_time_ns(&self) -> DocumentResult<i64>;
    /// Acquire the persistent exclusive index lock; never removes its file.
    ///
    /// # Errors
    /// Reports unsafe service paths, lock replacement or filesystem/locking failures.
    fn lock_index(&self) -> DocumentResult<Self::IndexLock>;
}
mod embedding_config;
mod embedding_error;
pub use embedding_config::{EmbeddingConfig, ProviderKind};
pub use embedding_error::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
use std::num::NonZeroUsize;

/// Synchronous texts-to-vectors boundary shared by documents and queries.
pub trait EmbeddingProvider {
    /// Validate the complete local endpoint/protocol configuration without inference.
    ///
    /// # Errors
    /// Returns invalid configuration when the endpoint or protocol is unsupported.
    fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;
    /// Compute one attempt and validate every vector before returning it.
    ///
    /// # Errors
    /// Reports invalid config/input, credentials, transport or malformed output.
    fn embed(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>>;
}

/// Confined synchronous file, clock and drain-lock operations.
pub trait EmbeddingFiles {
    /// Exclusive drain ownership, released when dropped.
    type EmbedLock;
    /// Read configuration bytes without decoding them; stable absence is `None`.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>>;
    /// Read a current regular document beneath the pinned checkout.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>>;
    /// Try persistent kernel drain ownership without waiting; busy is `None`.
    ///
    /// # Errors
    /// Reports unsafe service paths, observed replacement or I/O failure.
    fn try_lock_embed(&self) -> EmbeddingResult<Option<Self::EmbedLock>>;
    /// Read Unix seconds from the system clock.
    ///
    /// # Errors
    /// Reports a system clock before the Unix epoch.
    fn get_time_secs(&self) -> EmbeddingResult<u64>;
    /// Wait synchronously for a caller-selected retry delay.
    ///
    /// # Errors
    /// Reserved for environment failures while waiting.
    fn wait(&self, delay: std::time::Duration) -> EmbeddingResult<()>;
}

/// Pure shared Markdown projection and search-text codec.
pub trait Markdown {
    /// Parse a whole file atomically; unmarked Markdown produces `None`.
    ///
    /// # Errors
    /// Returns safe local diagnostics for invalid UTF-8, metadata or graph markup.
    fn parse_file(
        &self,
        path: &RepoPath,
        bytes: &[u8],
    ) -> Result<Option<ParsedFile>, Vec<Diagnostic>>;
}

mod command;

pub use command::{
    CheckReport, CommandError, CommandErrorKind, CommandProblem, CommandResult, ElementAddress,
    InspectedElement, Inspection,
};

/// Application-owned corpus validation capability.
pub trait Checks {
    /// Validate the given paths, or the whole corpus when empty.
    ///
    /// # Errors
    /// Reports command, refresh or infrastructure failures.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
/// Application-owned metadata inspection capability.
pub trait Inspections {
    /// Inspect normalized addresses in one snapshot.
    ///
    /// # Errors
    /// Reports invalid, missing or unreadable elements and refresh failures.
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>;
}
/// Narrow readiness and execution capability of the embedding executor.
/// Implementations reuse the executor's configuration, durable attempts and publication
/// guards. Readiness never performs inference or retains a read transaction for launch.
pub trait EmbeddingTasks {
    /// Determine whether configuration is ready and eligible durable work exists.
    ///
    /// # Errors
    /// Reports failures inspecting durable work.
    fn has_work(&mut self) -> CommandResult<bool>;
    /// Drain eligible work under the executor-owned lock.
    /// Missing/invalid configuration, changed identity and an occupied lock finish cleanly.
    ///
    /// # Errors
    /// Reports unexpected storage, coordination or embedding infrastructure failures.
    fn drain(&mut self) -> CommandResult<()>;
}
/// Nonblocking persistent embedding exclusion.
pub trait EmbedLocks {
    /// Owned kernel lock released through RAII.
    type EmbedLock;
    /// Probe exclusive embedding ownership without waiting.
    ///
    /// # Errors
    /// Reports unsafe lock paths or filesystem/locking failures.
    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>>;
}
/// Detached drain process launch capability.
pub trait DrainProcess {
    /// Start the internal drain command without waiting for inference.
    ///
    /// # Errors
    /// Reports process setup or launch failures.
    fn spawn_drain(&self) -> CommandResult<()>;
}
/// Foreground triggering and direct execution of background work.
pub trait Background {
    /// Start eligible work, returning a safe nonfatal launch warning if needed.
    ///
    /// # Errors
    /// Reports failures inspecting durable readiness.
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>>;
    /// Delegate once to the embedding executor.
    ///
    /// # Errors
    /// Reports an unavailable executor or execution failures.
    fn drain(&mut self) -> CommandResult<()>;
}

mod embedding;
pub use embedding::EmbeddingService;
pub use model::{ConfigState, DrainOutcome, QueryEmbedding};

/// Synchronous embedding application operations.
pub trait Embeddings {
    /// Reread, validate and synchronize ready configuration.
    ///
    /// # Errors
    /// Reports storage failures; unreadable configuration is a typed invalid state.
    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState>;
    /// Drain current eligible documents using durable bounded attempts.
    ///
    /// # Errors
    /// Reports storage or environment failures that prevent safe coordination.
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome>;
    /// Compute one transient query vector, persisting only a safe diagnostic.
    ///
    /// # Errors
    /// Reports storage or clock failures; ordinary provider errors are unavailable.
    fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding>;
}
