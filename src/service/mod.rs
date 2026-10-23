//! Application contracts.
mod background;
pub(crate) use background::BackgroundService;
mod retained_tree;
mod snapshot_error;

use crate::domain::{BlobOid, RepoPath, TreeOid};
pub use retained_tree::RetainedTree;
pub use snapshot_error::{SnapshotError, SnapshotErrorKind};

/// One captured regular file. The adapter never reads its live path.
pub struct SnapshotFile<'a> {
    /// Root-relative path in the resulting tree.
    pub path: &'a RepoPath,
    /// Exact captured bytes, without Git filters or normalization.
    pub bytes: &'a [u8],
}
/// One validated path and previously written blob in a snapshot request.
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
pub(crate) use document::DocumentService;
mod model;
pub use error::{StorageError, StorageErrorKind, StorageResult};
pub use model::{
    Diagnostic, DiagnosticKind, EmbeddingFailure, EmbeddingWork, FileRow, KnowledgeMetadata,
    NodeMetadata, ParsedElement, ParsedFile, QueryError, SourceMetadata, StoredNode,
    StoredVectorKey, TextMatch, VectorMatch, VectorOutcome,
};

mod storage;
/// Scoped storage capabilities for cache, vector and pinned read operations.
pub use storage::{CacheWrite, Storage, StorageRead, VectorWrite};
mod search;
/// Consistent search assembly used only by the composition root.
pub(crate) use search::{SearchDependencies, SearchService};
mod inspection;
pub(crate) use inspection::InspectionService;

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

/// Command requests, safe results and public error classifications.
pub use command::{
    CheckReport, CommandError, CommandErrorKind, CommandProblem, CommandResult, ElementAddress,
    InspectedElement, Inspection, SearchFlag, SearchLink, SearchRequest,
};

/// Application-owned corpus validation capability.
pub trait Checks {
    /// Validate the given paths, or the whole corpus when empty.
    ///
    /// # Errors
    /// Reports command, refresh or infrastructure failures.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
/// Consistent metadata-only hybrid knowledge search.
pub trait Searches {
    /// Refresh and search one pinned revision, observing baseline live knowledge freshness.
    ///
    /// # Errors
    /// Reports refresh, storage, integrity, or non-configuration filesystem failures.
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>>;
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
pub use model::{ConfigState, DrainOutcome, QueryEmbedding, QueryEmbeddingRequest};

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
    /// Compute one transient query using immutable captured inputs without rereading or resetting configuration.
    ///
    /// Rejects a changed committed identity and incompatible captured or current dimensions.
    /// No write unit or embedding lock spans inference; document attempts are untouched.
    /// Only the captured identity's guarded safe diagnostic may be persisted or cleared.
    ///
    /// # Errors
    /// Reports storage or clock failures; invalid provider policy and provider failures are unavailable.
    fn embed_query_with_config(
        &mut self,
        request: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding>;
}
