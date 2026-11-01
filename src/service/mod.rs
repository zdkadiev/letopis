//! Application contracts.
pub mod error;
pub mod types;
use crate::domain::{
    BlobOid, Edge, EdgeTarget, Graph, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid,
};
pub use error::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, Diagnostic, DiagnosticKind,
    DocumentError, DocumentErrorKind, DocumentResult, EmbeddingError, EmbeddingErrorKind,
    EmbeddingFailure, EmbeddingResult, QueryError, ServiceError, ServiceErrorKind, ServiceResult,
    SnapshotError, SnapshotErrorKind, StorageError, StorageErrorKind, StorageResult,
};
use std::{collections::BTreeSet, num::NonZeroUsize, os::fd::OwnedFd, path::Path, time::Duration};
pub use types::{
    CapturedFile, CheckReport, ConfigState, Configuration, DrainOutcome, ElementAddress,
    ElementLocation, EmbeddingConfig, EmbeddingStatusPolicy, EmbeddingStatusSummary, EmbeddingWork,
    FileRow, FileStat, GraphNeighbor, GraphNodeKey, GraphNodeRef, HeldLock, InspectedElement,
    Inspection, KnowledgeMetadata, LockName, NeighborCursor, NeighborDirection,
    NeighborPageRequest, NodeMetadata, ParsedElement, ParsedFile, ProvenanceIssue,
    ProvenanceObservation, ProvenanceRequest, ProviderKind, QueryEmbedding, QueryEmbeddingRequest,
    RecheckFilter, RefreshOutcome, RetainedTree, SearchFilters, SearchFlag, SearchLink,
    SearchRequest, SnapshotEntry, SnapshotFile, SnapshotState, SourceAvailability, SourceMetadata,
    SourceObservation, SourceRevisionComparison, SourceRevisionRequest, StatusReport, StoredNode,
    StoredVectorKey, TextMatch, Traversal, TraversalDirection, TraversalLink, TraversalMode,
    TraversalRequest, VectorMatch, VectorOutcome,
};
pub(crate) use types::{GENERATED_CONFIG, MAX_DIMENSIONS, MIN_DIMENSIONS, NEIGHBOR_PAGE_QUANTUM};
mod background_runner;
mod check;
mod configuration;
mod document_refresher;
mod embedding;
mod graph;
mod graph_diagnostics;
mod init;
mod inspection;
mod provenance;
mod search;
mod source;
mod status;
mod traversal;
mod utils;

pub(crate) use background_runner::BackgroundRunnerService;
pub(crate) use check::CheckService;
/// Read-only configuration observation over non-creating files and local provider validation.
pub(crate) use configuration::{ConfigurationService, map_invalid_configuration, map_read_error};
/// Cache refresh implementation and its caller-owned collaborator bundle.
pub(crate) use document_refresher::DocumentRefreshService;
pub(crate) use embedding::EmbeddingService;
pub(crate) use graph::GraphService;
pub(crate) use graph_diagnostics::GraphDiagnosticsService;
pub(crate) use init::InitService;
pub(crate) use inspection::InspectionService;
pub(crate) use provenance::ProvenanceService;
pub(crate) use search::SearchService;
/// Concrete source observation assembly, kept inside the crate.
pub(crate) use source::SourceService;
pub(crate) use status::StatusService;
pub(crate) use traversal::TraversalService;

/// Maximum number of already available bindings submitted in one observation batch.
const PROVENANCE_BATCH_SIZE: usize = 256;

/// Read-only live configuration and nonblocking kernel-lock observations.
/// Implementations never create service state or write an owner PID while observing.
pub trait StatusFiles {
    /// Read stable raw configuration bytes, or absence.
    ///
    /// # Errors
    /// Reports unsafe paths or filesystem failures without exposing file contents.
    fn get_config(&self) -> EmbeddingResult<Option<Vec<u8>>>;

    /// Return occupied index/embed locks and their best-effort recorded positive PIDs.
    ///
    /// # Errors
    /// Reports unsafe paths, failed probes, or invalid/unreadable PIDs on busy locks.
    fn observe_locks(&self) -> CommandResult<Vec<HeldLock>>;
}

/// Current graph diagnostics derived exclusively from the supplied consistent view.
///
/// Implementations inspect the entire knowledge graph before filtering by exact
/// owner file paths. An empty selector includes all files; repeated selectors do
/// not duplicate findings. Results are sorted by path, reason, element ID and target,
/// with absent optional fields sorting as empty strings. No refresh, write, network
/// call or separately opened read transaction is permitted.
pub trait GraphDiagnostics {
    /// Find safe invalid-file, dangling-target and includes-cycle findings.
    ///
    /// Each cycle contributes one finding per owning file with its minimum local
    /// element ID; each dangling pair retains its source element ID and target.
    /// Every finding has `ValidationFailed` classification and static safe text.
    ///
    /// # Errors
    /// Returns an internal error with a private storage cause if any read fails;
    /// partially collected findings are never returned as success.
    fn find_problems(
        &self,
        view: &mut dyn StorageRead,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>>;
}

/// Current source observations constrained by membership in a caller-pinned view.
///
/// Implementations read only the requested source, retaining its derived identity
/// and creation time. Unknown addresses and knowledge nodes return `None` without
/// filesystem I/O. Local bytes are captured live beneath the configured root;
/// remote addresses are never fetched. Reads do not refresh, reopen storage,
/// publish metadata, change vector state or create a Git snapshot.
pub trait SourceReader {
    /// Observe current bytes or typed availability for one referenced source.
    ///
    /// A local observation has no cross-file or historical snapshot guarantee.
    /// Missing files and incomplete ranges return `SourceNotFound`; remote
    /// availability remains `Unfetched`. Results never contain source bytes.
    ///
    /// # Errors
    /// Returns a safe document error with a private cause when storage or confined
    /// filesystem access fails; such failures never become `SourceNotFound`.
    fn get_source(
        &self,
        view: &mut dyn StorageRead,
        uri: &SourceUri,
    ) -> DocumentResult<Option<SourceObservation>>;
}

/// Read-only source comparison using caller-owned consistent metadata.
///
/// Implementations never refresh, open storage, retain refs, write or fetch URLs.
/// Callers gather all request fields from the same pinned view or validated projection.
/// Declared roots are checked even without sources. Missing/invalid bases require
/// rechecking; missing local addresses are reported independently. Remote bytes
/// remain unknown and are never classified as changed or missing by comparison.
pub trait SourceProvenance {
    /// Validate every unique requested root independently of source bindings.
    ///
    /// Checks roots in their ordered set order; an empty set succeeds. This is
    /// read-only and must run before candidate-dependent exits or query inference.
    /// The first invalid root rejects the request without partial results.
    ///
    /// # Errors
    /// Missing trees are `NotFound`, wrong type/format is `InvalidRequest`, and
    /// repository or object integrity failures remain `InternalError`.
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError>;

    /// Observe source flags and current diagnostics without returning source bytes.
    ///
    /// # Errors
    /// Returns safe errors for filesystem, Git policy, I/O or corruption failures.
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation>;
    /// Observe ordered bindings with one result per request and fresh live captures.
    ///
    /// Stops at the same first demanded error as sequential observation. Implementations
    /// may reuse bounded immutable historical successes for this call only; no persistent
    /// cache or retention is allowed. The default preserves existing test doubles and
    /// implementations by calling `observe_sources` sequentially.
    ///
    /// # Errors
    /// Returns the first filesystem, Git policy, I/O or corruption failure.
    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        requests
            .iter()
            .map(|request| self.observe_sources(*request))
            .collect()
    }

    /// Compare one address across a declared basis and an explicit requested tree.
    ///
    /// Validates the requested root before returning any unavailable/unknown state.
    /// Both trees use the same local path and complete inclusive line range.
    ///
    /// # Errors
    /// Requested missing trees are `NotFound`, wrong type/format `InvalidRequest`;
    /// repository and object integrity failures remain `InternalError`.
    fn compare_revision(
        &self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError>;
}
/// Source retention and audit before publication; the caller holds index.lock.
/// Missing/invalid author roots remain findings; available root/ref failures are errors.
pub trait BasisRetainer {
    /// Validate unique roots and retain direct refs without traversing descendants.
    ///
    /// # Errors
    /// Reports root corruption, policy, Git I/O and ref failures.
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;
    /// Audit every available unique root's complete local closure and retain its ref.
    /// Existing refs must not bypass repeated corruption checks.
    ///
    /// # Errors
    /// Reports incomplete/corrupt closures, policy, Git I/O and ref failures.
    fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;
}

/// Bounded graph reads from one consistent caller-owned view.
///
/// Handles belong to this view only. Pages sort by (edge type, reached identity),
/// preserve original edge orientation and exclude missing endpoints before the
/// limit. No complete node metadata is decoded for neighbor enumeration.
/// A cursor sequence fixes node, direction, mode and types. Only an empty page
/// ends a stream; a short nonempty page does not guarantee exhaustion.
pub trait GraphRead {
    /// Resolve a normalized identity using its actual stored classification.
    ///
    /// # Errors
    /// Reports database failures or invalid persisted identities/kinds.
    fn get_node_ref_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>>;
    /// Read at most the requested number of eligible neighbors strictly after a cursor.
    ///
    /// The handle must originate in this view; the positive limit may not exceed 32.
    ///
    /// # Errors
    /// Rejects a limit above the internal quantum; reports database or decoding failures.
    fn get_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>>;
}

/// Incremental graph traversal within the caller's existing pinned read view.
///
/// Implementations validate all starts, preserve MC-6 BFS ordering, bound neighbor
/// buffers and selected data, and never read unrelated complete node metadata.
/// Starts are excluded from selected elements; edges preserve declaration orientation.
pub trait GraphTraversal {
    /// Discover bounded graph data without refreshing or opening another view.
    ///
    /// # Errors
    /// Rejects empty or absent starts and propagates safe storage/integrity failures.
    /// No partial graph is returned on failure.
    fn traverse(
        &self,
        read: &mut dyn GraphRead,
        request: &TraversalRequest,
    ) -> CommandResult<Graph>;
}

/// One pinned transaction exposing both metadata and bounded graph reads.
/// Implementations obey both parent contracts for the same captured database view.
pub trait ReadTransaction: StorageRead + GraphRead {}

impl<T: StorageRead + GraphRead> ReadTransaction for T {}

/// Synchronous storage with exclusive scoped capabilities.
///
/// Every implementation hands out at most one scoped capability at a time (hence
/// `&mut self`). A write capability commits atomically or not at all, and a dropped
/// or failed capability leaves the stored data unchanged.
///
/// A live transaction prevents another mutable borrow of its storage:
///
/// ```compile_fail,E0499
/// use letopis::service::Storage;
/// fn cannot_borrow_storage_twice(storage: &mut dyn Storage) {
///     let tx = storage.begin_read_tx().unwrap();
///     let _other = &mut *storage;
///     let _ = tx.get_revision();
/// }
/// ```
///
/// Storage cannot be dropped while a transaction is still used:
///
/// ```compile_fail,E0505
/// use letopis::service::Storage;
/// fn cannot_drop_borrowed_storage<S: Storage>(mut storage: S) {
///     let tx = storage.begin_read_tx().unwrap();
///     drop(storage);
///     let _ = tx.get_revision();
/// }
/// ```
pub trait Storage {
    /// Begin a write only when the pinned vector identity matches.
    ///
    /// # Errors
    /// Reports contention or database failures.
    fn begin_vector_write_tx(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>>;
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
    fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>>;
    /// Begin and pin a read of cache revision and vector identity.
    ///
    /// # Errors
    /// Returns an internal error if the snapshot cannot be opened or decoded.
    fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>>;
}
/// Observation-only access to an optional published cache.
///
/// Implementations never create durable databases, bootstrap or repair schemas,
/// change configuration or vector identity, or acquire a database writer. Ordinary
/// read-side journal/shared-memory files may still be managed by the database engine.
/// Missing publication is distinct from read failure.
pub trait StatusStorage {
    /// Pin the current publication, returning none only when no publication exists.
    ///
    /// # Errors
    /// Reports inaccessible, malformed or incompatible persisted state.
    fn begin_status_read_tx(&mut self) -> StorageResult<Option<Box<dyn StorageRead + '_>>>;
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
    fn get_text_matches_by_expression(&mut self, expression: &str)
    -> StorageResult<Vec<TextMatch>>;
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
    fn get_nearest_vectors(
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
pub trait CacheWrite {
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
    fn commit(self: Box<Self>) -> StorageResult<u64>;
}

/// Caller-owned atomic vector write; any failed mutation prevents commit.
pub trait VectorWrite {
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
    fn reserve_attempt(&mut self, search_hash: &Sha256, max_attempts: u64) -> StorageResult<bool>;
    /// Replace an existing reservation's error with the caller's safe message.
    /// Returns false when no reservation exists; never creates an attempt.
    ///
    /// # Errors
    /// Reports database failures.
    fn record_failure(&mut self, search_hash: &Sha256, error: &str) -> StorageResult<bool>;
    /// Replace the singleton query failure with an already-safe message.
    ///
    /// # Errors
    /// Rejects timestamps above `i64::MAX` and reports database failures.
    fn set_query_error(&mut self, at: u64, error: &str) -> StorageResult<()>;
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
        search_hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey>;
    /// Commit all writes atomically.
    ///
    /// # Errors
    /// Rejects poisoned units or reports database failures, rolling back.
    fn commit(self: Box<Self>) -> StorageResult<()>;
}

/// Synchronous document publication operations.
///
/// Every implementation publishes a refresh atomically: readers see either the previous
/// projection or the complete new one, never a partial update.
#[cfg_attr(test, mockall::automock)]
pub trait Refresher {
    /// Detect and atomically publish changed file projections.
    ///
    /// # Errors
    /// Reports unsafe paths, concurrent changes, or Git, filesystem and storage failures.
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome>;
    /// Refresh incrementally and audit all resulting source closures under index.lock.
    /// Verification precedes SQL publication even when the file delta is empty.
    ///
    /// # Errors
    /// Reports capture, integrity, retention and storage failures without partial publication.
    fn refresh_verified(&mut self) -> DocumentResult<RefreshOutcome>;
    /// Reparse every candidate under the index lock and publish once atomically.
    /// Audits resulting source closures before SQL. Invalid files retain their last valid projection. Vectors are not awaited.
    ///
    /// # Errors
    /// Reports capture, provenance or storage failures without a partial publication.
    fn reindex(&mut self) -> DocumentResult<RefreshOutcome>;
}
/// Repository-owned discovery of Markdown candidate paths.
///
/// Every implementation reports only normalized root-relative paths and never reads file bodies.
#[cfg_attr(test, mockall::automock)]
pub trait CorpusLister {
    /// List normalized, deduplicated candidates including deleted tracked files.
    ///
    /// # Errors
    /// Reports unsupported candidate paths or Git failures.
    fn get_candidates(&self) -> DocumentResult<Vec<RepoPath>>;
}
/// Confined synchronous file observations and publication exclusion.
///
/// Every implementation stays beneath one pinned root, never follows symlinks out of it, and
/// fails rather than returning bytes it observed changing.
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
    fn get_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>>;
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

/// Pure shared Markdown projection and search-text codec.
///
/// Every implementation is deterministic and performs no I/O: equal bytes give equal results.
pub trait MarkdownCodec {
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

/// Snapshot operations and root verification for the same repository.
///
/// Implementations satisfy both [`GitSnapshots`] and [`SnapshotVerifier`] contracts.
/// Provenance uses this combined boundary as one owned trait object; every type
/// implementing both component traits implements it automatically.
pub trait VerifiedGitSnapshots: GitSnapshots + SnapshotVerifier {}

impl<T: GitSnapshots + SnapshotVerifier + ?Sized> VerifiedGitSnapshots for T {}

/// Synchronous texts-to-vectors boundary shared by documents and queries.
///
/// Every implementation validates what it returns and exposes no credentials or response
/// bodies in its errors.
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
///
/// Every implementation stays beneath one pinned root and fails rather than returning bytes
/// it observed changing; at most one drain lock is held at a time.
pub trait EmbeddingFiles {
    /// Exclusive drain ownership, released when dropped.
    type EmbedLock;
    /// Read configuration bytes without decoding them; stable absence is `None`.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn get_config(&self) -> EmbeddingResult<Option<Vec<u8>>>;
    /// Read a current regular document beneath the pinned checkout.
    ///
    /// # Errors
    /// Reports unsafe paths, observed replacement or I/O failure.
    fn get_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>>;
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
    /// Reports an environment failure that prevents the wait; none is currently produced.
    fn wait(&self, delay: Duration) -> EmbeddingResult<()>;
}

/// Synchronous embedding application operations.
///
/// Every implementation keeps document vectors consistent with the committed configuration
/// identity and persists only safe diagnostics.
pub trait Embedder {
    /// Validate current configuration before any vector synchronization. Valid disabled
    /// or unconfigured documents return `ConfigState::Disabled` without observing or
    /// resetting vector identity. A superseded enabled observation returns
    /// `ConfigState::Changed` without retrying or resetting.
    ///
    /// # Errors
    /// Reports storage failures; unreadable or invalid configuration is a typed invalid state.
    fn get_configuration(&mut self) -> EmbeddingResult<ConfigState>;
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

/// Application-owned current corpus validation capability.
///
/// Implementations refresh the entire corpus before opening one pinned cache read.
/// All cache-derived findings share that view. Exact owner selectors filter findings
/// after full graph validation; they never restrict refresh or graph membership.
/// Source bases and bindings come from that view; live byte observations are read-only.
/// Missing sources or bases produce findings without removing knowledge.
pub trait Checker {
    /// Validate the given paths, or the whole corpus when empty.
    ///
    /// # Errors
    /// Reports refresh or infrastructure failures with private causes; no partial report
    /// is returned on failure. Validation findings are successful reports.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}
/// Read the latest published diagnostics without refreshing or mutating state.
pub trait StatusReader {
    /// Observe one pinned database view, current configuration and busy locks.
    ///
    /// # Errors
    /// Reports storage, provenance or unsafe lock failures without a partial report.
    fn get_status(&mut self) -> CommandResult<StatusReport>;
}
/// Filesystem capability required to initialize private service state.
///
/// Implementations create the service directory only when explicitly asked,
/// publish configuration without replacement and report durability work separately.
pub trait InitFiles: StatusFiles {
    /// Atomically publish complete bytes when `config.json` is absent.
    ///
    /// # Errors
    /// Rejects unsafe or nonregular destinations and reports write, link or sync
    /// failures. Existing files and another writer's winner are never replaced.
    fn install_config_if_absent(&self, bytes: &[u8]) -> DocumentResult<()>;
    /// Open or create the pinned private service directory.
    ///
    /// # Errors
    /// Reports unsafe/replaced paths, creation or permission failures.
    fn open_service_directory(&self) -> DocumentResult<OwnedFd>;
    /// Sync the existing service directory and its Git-root parent.
    ///
    /// # Errors
    /// Reports absent/replaced state or durability-sync failures without creating entries.
    fn sync_service_state(&self) -> DocumentResult<()>;
}

/// Storage capability required to initialize both service schemas.
pub trait InitStorage {
    /// Open or validate cache and vector schemas beneath one pinned directory.
    ///
    /// # Errors
    /// Reports unsafe paths, incompatible schemas, contention or database I/O failures.
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()>;
}

/// One repository initialization operation.
pub trait Initializer {
    /// Prepare service schemas and publish generated configuration without
    /// replacing existing valid state.
    ///
    /// # Errors
    /// Reports unsafe repository/config/storage state, invalid retained settings or
    /// an environment failure before success is observable.
    fn initialize(&mut self) -> CommandResult<()>;
}

/// Read the current validated embedding configuration without changing state.
///
/// Implementations observe only the non-creating configuration source and perform local
/// provider validation. They never expose raw bytes, identity, dimensions-derived data or
/// credentials, and never start readiness, vector resets, storage bootstrap or inference.
pub trait ConfigReader {
    /// Observe the current configuration, or its stable absence.
    ///
    /// # Errors
    /// Returns the complete nested document on success, or stable absence. A legacy
    /// flat document and every malformed or provider-rejected value return
    /// `EmbeddingFailed`; unreadable or unsafe files return `InternalError` with
    /// `cannot read embedding configuration`. Causes stay private and no partial
    /// configuration is returned.
    fn get_config(&self) -> CommandResult<Option<Configuration>>;
}
/// Consistent metadata-only hybrid knowledge search.
pub trait Searcher {
    /// Refresh and search one pinned revision, observing live knowledge and source freshness.
    ///
    /// Source bindings and metadata share the ranking view; live bytes are observed separately.
    /// Flags are deduplicated X-1 values and links contain no source text or excerpts.
    ///
    /// # Errors
    /// Reports refresh, storage, integrity, or non-configuration filesystem failures.
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>>;
}
/// Application-owned metadata inspection from one retained Git tree.
///
/// Membership derives from current canonical Markdown, with duplicates returned once.
/// Knowledge hashes/lines and available source bytes match that tree. Known sources
/// with absent files or incomplete ranges have no hash; opaque URLs are never fetched.
/// Unknown or removed knowledge/source declarations fail the entire batch.
/// The final tree is retained before success; working files, Git index and HEAD stay intact.
pub trait Inspector {
    /// Inspect normalized addresses in one snapshot.
    ///
    /// # Errors
    /// Reports unknown elements, invalid current markup, unsafe paths, refresh or Git failures.
    fn inspect(&mut self, ids: &[ElementAddress]) -> CommandResult<Inspection>;
}
/// Narrow readiness and execution capability of the embedding executor.
/// Implementations reuse the executor's configuration, durable attempts and publication
/// guards. Readiness never performs inference or retains a read transaction for launch.
#[cfg_attr(test, mockall::automock)]
pub trait EmbeddingExecutor {
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
///
/// Every implementation grants ownership to at most one holder at a time across processes
/// and releases it when the returned lock is dropped.
pub trait EmbedLocker {
    /// Owned kernel lock released through RAII.
    type EmbedLock;
    /// Probe exclusive embedding ownership without waiting.
    ///
    /// # Errors
    /// Reports unsafe lock paths or filesystem/locking failures.
    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>>;
}
/// Detached drain process launch capability.
///
/// Every implementation returns without waiting for the launched process to finish.
#[cfg_attr(test, mockall::automock)]
pub trait DrainLauncher {
    /// Start the internal drain command without waiting for inference.
    ///
    /// # Errors
    /// Reports process setup or launch failures.
    fn spawn_drain(&self) -> CommandResult<()>;
}
/// Foreground triggering and direct execution of background work.
///
/// Every implementation reports launch problems as safe, nonfatal diagnostics where the
/// foreground result is already complete.
#[cfg_attr(test, mockall::automock)]
pub trait BackgroundRunner {
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

/// Consistent metadata-only traversal with live freshness observations.
///
/// Implementations refresh, synchronize embedding readiness before pinning one read
/// view, and build selected links without inference or fetching remote sources.
pub trait Traverser {
    /// Traverse and observe selected elements from one pinned publication revision.
    ///
    /// # Errors
    /// Returns safe refresh, storage, start-membership or observation errors without
    /// partial output. The pinned view is released on every return path.
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal>;
}
