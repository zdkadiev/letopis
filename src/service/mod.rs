//! Application contracts.
pub mod error;
pub mod types;
use crate::domain::{
    BlobOid, Edge, EdgeTarget, Graph, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid,
};
pub use error::TransactionError;
pub use error::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, Diagnostic, DiagnosticKind,
    DocumentError, DocumentErrorKind, DocumentResult, EmbeddingError, EmbeddingErrorKind,
    EmbeddingFailure, EmbeddingResult, FileError, FileErrorKind, FileResult, GitError, GitResult,
    ProcessError, ProcessResult, QueryError, ServiceError, ServiceErrorKind, ServiceResult,
    SnapshotError, SnapshotErrorKind, StorageError, StorageErrorKind, StorageResult,
};
use std::{
    collections::BTreeSet,
    num::{NonZeroU32, NonZeroUsize},
    os::fd::OwnedFd,
    path::Path,
    time::Duration,
};

/// Synchronous wall-clock observations and bounded waits.
/// Implementations report failed or unrepresentable time observations as I/O errors.
pub trait Clock: std::fmt::Debug {
    /// Observe nanoseconds since the Unix epoch.
    /// # Errors
    /// Reports clock or signed nanosecond representation failures.
    fn clock_get_time_ns(&self) -> std::io::Result<i64>;
    /// Observe seconds since the Unix epoch.
    /// # Errors
    /// Reports clock failures.
    fn clock_get_time_secs(&self) -> std::io::Result<u64>;
    /// Wait synchronously for the requested duration.
    /// # Errors
    /// Reports an interrupted or otherwise unsuccessful wait.
    fn clock_wait(&self, delay: Duration) -> std::io::Result<()>;
}

/// Confined, observational file operations rooted at one pinned directory.
///
/// Paths are relative and may not escape the root. Reads never create entries
/// or change permissions. Missing regular files return `None`; stable nonregular
/// leaves return `NonRegular`, while unsafe parents and failed I/O remain errors.
/// Explicit directory observations pin identities for subsequent operations;
/// ordinary per-file traversal retains descriptors only for that operation.
#[cfg_attr(test, mockall::automock)]
pub trait FileReader {
    /// Observe final symlink presence without following any component or reading bytes.
    /// Missing entries return false; directories and other nonsymlinks return false.
    /// Verify the leaf identity and ancestry before returning, including absent entries.
    ///
    /// # Errors
    /// Reports unsafe parents, replacements, invalid relative paths or I/O failures.
    fn symlink_is_exist(&self, path: &Path) -> FileResult<bool>;

    /// Capture regular bytes, verifying identity and content observations around I/O.
    /// Verify leaf stability and ancestry before classifying nonregular exclusions;
    /// replacing an observed regular leaf always remains an integrity failure.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changes, clock or I/O failures.
    fn file_get_by_path(&self, path: &Path) -> FileResult<Option<CapturedFile>>;
    /// Read regular file bytes with the same identity, content and ancestry checks as capture.
    /// This operation does not sample the clock or require representable index metadata.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changes or I/O failures.
    fn file_get_bytes_by_path(&self, path: &Path) -> FileResult<Option<Vec<u8>>>;
    /// Observe a regular file's metadata without reading its body.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changing observations or I/O failures.
    fn file_get_stat_by_path(&self, path: &Path) -> FileResult<Option<FileStat>>;
    /// Observe stable regular-file identity without reading bytes or comparing content.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, identity changes or I/O failures.
    fn file_get_identity_by_path(&self, path: &Path) -> FileResult<Option<FileIdentity>>;
    /// Return regular-file presence using metadata only; absence is the only false result.
    ///
    /// # Errors
    /// Preserves nonregular, unsafe-path, replacement and I/O failures.
    fn file_is_exist(&self, path: &Path) -> FileResult<bool>;
    /// Verify an original identity still exists; an absent original permits safe creation.
    ///
    /// # Errors
    /// Reports replacement/deletion of an original or any unsafe current entry.
    fn file_verify_identity_by_path(
        &self,
        path: &Path,
        identity: Option<FileIdentity>,
    ) -> FileResult<()>;
    /// Require distinct normalized paths and, when present, distinct regular-file identities.
    ///
    /// # Errors
    /// Reports aliases, unsafe/nonregular entries or observation failures.
    fn file_verify_distinct_by_paths(
        &self,
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<()>;
    /// Pin an existing directory identity and return an owned duplicate, or absence.
    /// The path `.` selects the root; this operation never creates or chmods entries.
    ///
    /// # Errors
    /// Reports unsafe or changed directories, and I/O failures.
    fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<OwnedFd>>;
    /// Observe a held file lock's bounded nonzero PID without creating or writing state.
    /// An absent or currently unheld lock returns `None`.
    ///
    /// # Errors
    /// Reports unsafe/nonregular/aliased locks, changes, malformed owners or I/O failures.
    fn lock_get_owner_by_path(&self, path: &Path) -> FileResult<Option<NonZeroU32>>;
}

/// Private, durable file publication and directory creation under the reader's root.
pub trait FileWriter: FileReader {
    /// Publish complete mode-0600 bytes atomically if absent; preserve an existing winner.
    /// Sync file data before publication and the parent directory before success.
    ///
    /// # Errors
    /// Reports absent parents, unsafe/nonregular destinations, changes or I/O failures.
    /// A failure preserves existing entries and may leave a complete published winner.
    fn file_create_if_absent_by_path(&self, path: &Path, bytes: &[u8]) -> FileResult<()>;
    /// Create missing directory components with mode 0700 and protect the final directory.
    /// Pin the final identity; existing ancestors retain their permissions.
    ///
    /// # Errors
    /// Reports unsafe or changed entries, permission or other I/O failures.
    fn directory_create_by_path(&self, path: &Path) -> FileResult<OwnedFd>;
    /// Verify and sync an existing directory without creating entries or changing modes.
    ///
    /// # Errors
    /// Reports absence, unsafe/replaced directories or durability failures.
    fn directory_sync_by_path(&self, path: &Path) -> FileResult<()>;
}

/// Persistent kernel file locks with exclusive ownership and bounded PID metadata.
/// Acquisition may create private parents and lock files; names remain caller-selected.
/// Owners unlock before closing, and acquisition never replaces an existing inode.
pub trait FileLocker {
    /// Actual RAII capability retaining the held kernel lock until dropped.
    type Lock;
    /// Wait for exclusive ownership, verify identities, then record this process's PID.
    ///
    /// # Errors
    /// Reports unsafe/nonregular/aliased entries, changes, permission or I/O failures.
    fn lock_acquire_by_path(&self, path: &Path) -> FileResult<Self::Lock>;
    /// Try exclusive ownership without waiting; return `None` only for real contention.
    ///
    /// # Errors
    /// Reports unsafe/nonregular/aliased entries, changes, permission or I/O failures.
    fn lock_try_acquire_by_path(&self, path: &Path) -> FileResult<Option<Self::Lock>>;
}

/// Scoped operations on a shared cache database.
///
/// Callbacks take no transaction argument and use this owner for operations.
/// Same-instance nested scopes join; only the outer scope finalizes. A caught
/// nested error, failed write or panic makes the outer scope rollback-only.
/// Reads inside writes see uncommitted data; writes inside reads are rejected
/// and poison that scope. Ordinary operations require an active scope; writes
/// require a write scope. Reads pin cache revision and attached vector identity
/// before invoking the callback. Different owners never join. Failed cleanup
/// that leaves a transaction active disables later scopes on that owner.
/// Observation-only owners cannot write; absent status skips its callback
/// without opening a connection or creating a durable file.
pub trait CacheDatabase: StorageRead + GraphRead {
    /// Verify a use case's entry requirement without opening a transaction.
    /// An incompatible active scope becomes rollback-only, even if the caller
    /// catches the error. Inactive storage satisfies either requirement.
    ///
    /// # Errors
    /// Rejects incompatible active scopes and failed or disabled owners.
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()>;

    /// Execute status observation in a pinned view, skipping unpublished state.
    ///
    /// # Errors
    /// Returns operation errors, malformed metadata or transaction failures.
    fn with_status_read_tx<T, E, F>(&self, operation: F) -> Result<Option<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;
    /// Execute an operation within one pinned cache read view.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;
    /// Return the provisional revision that the active write will publish on commit.
    ///
    /// # Errors
    /// Rejects calls outside a write scope and revisions that cannot be incremented.
    fn get_publication_revision(&self) -> StorageResult<u64>;
    /// Execute and atomically commit a cache write scope.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;
    /// Insert or replace metadata while preserving its row ID.
    ///
    /// # Errors
    /// Rejects unequal knowledge identity/address fields, unsupported numeric values,
    /// or database failures. Any error prevents publication of this write unit.
    fn nodes_upsert_by_id(&self, node: &NodeMetadata) -> StorageResult<i64>;
    /// Insert a declared edge, retaining an absent target without creating a placeholder node.
    /// Targets introduced later in the unit resolve when the unit commits.
    ///
    /// # Errors
    /// Rejects duplicate edges and missing sources; reports database failures.
    fn edges_save_by_source_id_and_target_id_and_type(&self, edge: &Edge) -> StorageResult<()>;
    /// Remove resolved and unresolved declarations owned by a knowledge element.
    ///
    /// # Errors
    /// Reports database failures.
    fn edges_delete_by_source_id(&self, source_id: &KnowledgeId) -> StorageResult<()>;
    /// Delete a node and its FTS entry, retaining incoming declarations as missing targets.
    ///
    /// # Errors
    /// Rejects deletion while outgoing edges remain; reports database failures.
    fn delete_node(&self, id: &EdgeTarget) -> StorageResult<()>;
    /// Replace a knowledge element's contentless FTS entry.
    ///
    /// # Errors
    /// Returns not found for absent knowledge, or a database failure.
    fn replace_text(&self, id: &KnowledgeId, text: &str) -> StorageResult<()>;
    /// Replace a Markdown candidate's state.
    ///
    /// # Errors
    /// Rejects unsupported numbers and inconsistent validity; reports database failures.
    fn files_upsert_by_path(&self, file: &FileRow) -> StorageResult<()>;
    /// Remove a candidate's state.
    ///
    /// # Errors
    /// Reports database failures.
    fn files_delete_by_path(&self, path: &RepoPath) -> StorageResult<()>;
}

/// Scoped operations on a shared vector database.
///
/// Same-instance nested scopes join and only the outer scope finalizes. Caught
/// nested errors, failed writes and panic prevent commit; writes cannot upgrade
/// reads. Operations require an active scope on this owner and writes require a
/// write scope. Identity mismatch skips the callback and returns `ConfigChanged`,
/// preserving compatible outer work. Cache owners and different vector owners
/// never join. Failed cleanup that leaves an active transaction disables reuse.
/// Attempt reservations must commit before calling the provider; provider HTTP
/// must run outside service-owned write scopes.
pub trait VectorDatabase {
    /// Verify a use case's entry requirement without opening a transaction.
    /// An incompatible active scope becomes rollback-only, even if the caller
    /// catches the error. Inactive storage satisfies either requirement.
    ///
    /// # Errors
    /// Rejects incompatible active scopes and failed or disabled owners.
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()>;

    /// Atomically synchronize vector identity after a prior observation.
    ///
    /// # Errors
    /// Reports an active scope, incompatible state, contention or database failure.
    fn reset_vectors(
        &self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>>;
    /// Get dimensions from the current vector view.
    ///
    /// # Errors
    /// Reports an inactive scope, malformed metadata or database failure.
    fn vector_index_config_get_by_id(&self) -> StorageResult<Option<NonZeroUsize>>;
    /// Get the stored failure for a search hash.
    ///
    /// # Errors
    /// Reports an inactive scope, invalid data or database failure.
    fn embed_failures_get_by_search_hash(
        &self,
        search_hash: &Sha256,
    ) -> StorageResult<Option<EmbeddingFailure>>;
    /// Get the last query error.
    ///
    /// # Errors
    /// Reports an inactive scope, invalid data or database failure.
    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>>;
    /// Execute a vector read for the expected configuration identity.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_read_tx<T, E, F>(
        &self,
        expected: &Sha256,
        operation: F,
    ) -> Result<VectorOutcome<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;
    /// Execute a vector write only for the expected configuration identity.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_write_tx<T, E, F>(
        &self,
        expected: &Sha256,
        operation: F,
    ) -> Result<VectorOutcome<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;
    /// Remove vectors and failures absent from this transaction's cache observation.
    /// Concurrent publication can leave recomputable missing vectors until next drain.
    ///
    /// # Errors
    /// Reports database failures and prevents commit of a failed unit.
    fn prune_unreferenced(&self) -> StorageResult<()>;
    /// Reserve one durable attempt within the caller's total budget.
    /// Commit before calling the provider; an unsent reservation still counts.
    /// Returns `true` when an attempt was reserved and `false` when the budget is exhausted.
    ///
    /// # Errors
    /// Rejects zero or budgets above `i64::MAX` and reports database failures.
    fn embed_failures_upsert_by_search_hash_and_attempts(
        &self,
        search_hash: &Sha256,
        max_attempts: u64,
    ) -> StorageResult<bool>;
    /// Replace an existing reservation's error with the caller's safe message.
    /// Returns false when no reservation exists; never creates an attempt.
    ///
    /// # Errors
    /// Reports database failures.
    fn embed_failures_update_by_search_hash(
        &self,
        search_hash: &Sha256,
        error: &str,
    ) -> StorageResult<bool>;
    /// Replace the singleton query failure with an already-safe message.
    ///
    /// # Errors
    /// Rejects timestamps above `i64::MAX` and reports database failures.
    fn query_error_upsert_by_id(&self, at: u64, error: &str) -> StorageResult<()>;
    /// Clear the singleton error after a successful query embedding.
    ///
    /// # Errors
    /// Reports database failures.
    fn query_error_delete_by_id(&self) -> StorageResult<()>;
    /// Store one text vector, preserving an existing key and vector.
    ///
    /// # Errors
    /// Rejects empty, nonfinite or mismatched vectors and reports database corruption/I/O.
    fn store_vector(
        &self,
        search_hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey>;
}
pub use types::{
    CapturedFile, CheckReport, ConfigState, Configuration, DiscoveryConfig, DrainOutcome,
    ElementAddress, ElementLocation, EmbeddingConfig, EmbeddingWork, FileIdentity, FileRow,
    FileStat, GitIgnoreMode, GraphNeighbor, GraphNodeKey, GraphNodeRef, HeldLock, InspectedElement,
    Inspection, KnowledgeMetadata, LockName, NeighborCursor, NeighborDirection,
    NeighborPageRequest, NodeMetadata, ParsedElement, ParsedFile, ProvenanceIssue,
    ProvenanceObservation, ProvenanceRequest, ProviderKind, QueryEmbedding, QueryEmbeddingRequest,
    RecheckFilter, RefreshOutcome, RetainedTree, SearchFilters, SearchFlag, SearchLink,
    SearchRequest, SnapshotEntry, SnapshotFile, SnapshotState, SourceAvailability, SourceMetadata,
    SourceObservation, SourceRevisionComparison, SourceRevisionRequest, StatusReport, StoredNode,
    StoredVectorKey, TextMatch, TransactionRequirement, Traversal, TraversalDirection,
    TraversalLink, TraversalMode, TraversalRequest, VectorMatch, VectorOutcome,
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
pub(crate) use utils::map_document_file_error;

/// Maximum number of already available bindings submitted in one observation batch.
const PROVENANCE_BATCH_SIZE: usize = 256;

/// Current graph diagnostics derived from the injected shared cache.
///
/// Implementations inspect the entire knowledge graph before filtering by exact
/// owner file paths. An empty selector includes all files; repeated selectors do
/// not duplicate findings. Results are sorted by path, reason, element ID and target,
/// with absent optional fields sorting as empty strings. No refresh, write, network
/// call or separate database connection is permitted. A call joins a compatible
/// active cache scope or owns a fresh pinned read scope.
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
    fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>>;
}

/// Current source observations constrained by membership in the injected cache view.
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
    /// filesystem access fails; such failures never become `SourceNotFound`. Calls
    /// join a compatible active cache scope or own a fresh pinned read scope.
    fn get_source(&self, uri: &SourceUri) -> DocumentResult<Option<SourceObservation>>;
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
    fn nodes_get_by_id(&self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>>;
    /// Read at most the requested number of eligible neighbors strictly after a cursor.
    ///
    /// The handle must originate in this view; the positive limit may not exceed 32.
    ///
    /// # Errors
    /// Rejects a limit above the internal quantum; reports database or decoding failures.
    fn edges_get_by_row_id_and_type(
        &self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>>;
}

/// Incremental graph traversal through one shared cache read scope.
///
/// Implementations validate all starts, preserve MC-6 BFS ordering, bound neighbor
/// buffers and selected data, and never read unrelated complete node metadata.
/// Starts are excluded from selected elements; edges preserve declaration orientation.
pub trait GraphTraversal {
    /// Discover bounded graph data, joining an active scope or owning a fresh read.
    ///
    /// # Errors
    /// Rejects empty or absent starts and propagates safe storage/integrity failures.
    /// No partial graph is returned on failure.
    fn traverse(&self, request: &TraversalRequest) -> CommandResult<Graph>;
}

/// Shared-reference observations within an active cache read or write scope.
///
/// Every implementation answers all methods from one consistent view, so results
/// never mix revisions, and never modifies stored data.
pub trait StorageRead {
    /// Count distinct non-source knowledge search hashes in the active cache scope.
    ///
    /// # Errors
    /// Reports database failures or invalid persisted counts.
    fn nodes_get_by_kind(&self) -> StorageResult<u64>;
    /// Validate current native vector rows and count distinct covered knowledge hashes.
    /// Sources and orphan vector keys are excluded. Requires an active cache scope.
    ///
    /// # Errors
    /// Reports missing native rows, invalid counts and database failures.
    fn get_vector_coverage(&self) -> StorageResult<u64>;
    /// Read current uncovered failures at or above the positive attempt threshold.
    /// Every current uncovered row is decoded and validated before threshold filtering;
    /// only matching rows are retained, once each in ascending search-hash order.
    /// Sources, orphan failures and hashes with vector keys are excluded.
    ///
    /// # Errors
    /// Reports malformed persisted hashes/attempts or database failures.
    fn embed_failures_get_by_kind_and_search_hash_and_attempts(
        &self,
        max_attempts: u64,
    ) -> StorageResult<Vec<EmbeddingFailure>>;
    /// Read the first eligible knowledge ID strictly after the optional cursor.
    ///
    /// # Errors
    /// Rejects zero or unrepresentable attempt budgets; reports I/O or invalid data.
    fn nodes_get_by_kind_and_search_hash_and_attempts_and_id(
        &self,
        expected: &Sha256,
        max_attempts: u64,
        id: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;
    /// Observe the native vector dimension under the pinned configuration identity.
    ///
    /// # Errors
    /// Reports malformed native metadata or database failures.
    fn vector_index_config_get_by_id(
        &self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>>;
    /// Read typed metadata by normalized identity.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn nodes_get_by_id(&self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>>;
    /// List distinct non-null author-declared source roots without reading node metadata.
    ///
    /// # Errors
    /// Reports database failures or malformed persisted tree identities.
    fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>>;
    /// List all published node metadata in ascending normalized identity order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn nodes_get(&self) -> StorageResult<Vec<StoredNode>>;
    /// List resolved and missing-target declarations in textual source, target and type order.
    /// An optional source restricts both representations; absent targets have no node metadata.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted edges.
    fn edges_get_by_source_id(&self, source_id: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>>;
    /// Read a candidate file's change-detection state.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>>;
    /// List all candidate file states in ascending exact path order.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn files_get(&self) -> StorageResult<Vec<FileRow>>;
    /// Execute an already-prepared FTS expression without a candidate limit.
    ///
    /// # Errors
    /// Returns an invalid request for malformed expressions, or an internal error for I/O.
    fn text_index_get_by_expression(&self, expression: &str) -> StorageResult<Vec<TextMatch>>;
    /// Read a text-addressed vector under the expected pinned identity.
    ///
    /// # Errors
    /// Reports malformed stored vectors or database failures.
    fn vector_keys_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>>;
    /// Find native cosine matches joined to current node search hashes.
    ///
    /// # Errors
    /// Rejects invalid query vectors or reports database/integrity failures.
    fn vector_index_get_by_vector(
        &self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>>;
    /// Read durable attempts and the latest failure under the pinned identity.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn embed_failures_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>>;
    /// Read the last query embedding failure, independent of model resets.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>>;
    /// Return the revision in the active cache scope.
    ///
    /// # Errors
    /// Rejects calls outside a scope, malformed revisions and database failures.
    fn project_get_by_id(&self) -> StorageResult<u64>;
    /// Return the vector configuration identity in the active cache scope.
    ///
    /// # Errors
    /// Rejects calls outside a scope, malformed identities and database failures.
    fn meta_get_by_id(&self) -> StorageResult<Option<Sha256>>;
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
    fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;
    /// Refresh incrementally and audit all resulting source closures under index.lock.
    /// Verification precedes SQL publication even when the file delta is empty.
    ///
    /// # Errors
    /// Reports capture, integrity, retention and storage failures without partial publication.
    fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;
    /// Reparse every candidate under the index lock and publish once atomically.
    /// Audits resulting source closures before SQL. Invalid files retain their last valid projection. Vectors are not awaited.
    ///
    /// # Errors
    /// Reports capture, provenance or storage failures without a partial publication.
    fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;
}
/// Raw Git index and policy-selected untracked worktree path observations.
///
/// Return nonempty NUL-terminated paths after the supplied root-relative Gitignore
/// patterns apply to tracked and untracked paths, without document-role filtering.
/// Include deleted tracked paths and expand sparse index membership; duplicate stages
/// are permitted. Read only Git index, ignore and sparse-tree metadata; never
/// modify the index or repository.
#[cfg_attr(test, mockall::automock)]
pub trait GitWorktreePaths {
    /// Enumerate raw root-relative paths with standard exclusions selected by mode.
    /// Additional patterns apply to the complete union, including tracked paths.
    ///
    /// # Errors
    /// Reports index, ignore, sparse-object, worktree traversal or repository-policy failures.
    fn worktree_paths_get(&self, ignore_mode: GitIgnoreMode, patterns: &[u8])
    -> GitResult<Vec<u8>>;
}

/// Pure shared Markdown projection and search-text codec.
///
/// Every implementation is deterministic and performs no I/O: equal paths and bytes give equal results.
pub trait MarkdownCodec {
    /// Parse a whole file atomically; unmarked Markdown produces `None`.
    ///
    /// # Errors
    /// Returns safe local diagnostics for invalid UTF-8, metadata or graph markup.
    fn file_parse(
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
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError>;
}
/// Synchronous immutable snapshot operations. Creation alone does not retain.
pub trait GitSnapshots {
    /// Create a tree from exactly these files, preserving captured bytes.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::InvalidRequest`] for invalid, duplicate or
    /// colliding paths, and [`SnapshotErrorKind::InternalError`] when repository
    /// policy, Git I/O or object integrity checks fail.
    fn tree_create(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError>;
    /// Write exact captured bytes without filters or normalization; does not retain.
    ///
    /// # Errors
    /// Reports repository policy, Git I/O or object integrity failures.
    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError>;
    /// Assemble exactly these paths after validating all blobs; does not retain.
    ///
    /// # Errors
    /// Invalid paths, collisions, missing/wrong-type blobs or foreign formats are
    /// invalid requests; policy, I/O and corruption failures are internal errors.
    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError>;
    /// Read a complete blob from an exact tree, without live-file fallback.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root or path,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format,
    /// non-tree root or non-blob path, and [`SnapshotErrorKind::InternalError`]
    /// for repository policy, Git I/O or integrity failures.
    fn blob_get_by_tree_oid_and_path(
        &self,
        tree: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError>;
    /// Validate the exact root and retain its direct ref without auditing descendants.
    /// A retained ref protects healthy reachable objects from normal GC, not corruption.
    ///
    /// # Errors
    /// Missing roots are not found; wrong types/formats are invalid requests.
    /// Root corruption, policy, Git I/O and ref conflicts are internal errors.
    fn tree_retain_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError>;
    /// Validate the complete local closure and retain it through Git GC.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format or
    /// non-tree root, and [`SnapshotErrorKind::InternalError`] for an incomplete
    /// or corrupt closure, repository policy, Git I/O or ref publication failure.
    fn tree_pin_by_oid(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError>;
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
    fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;
    /// Compute one attempt and validate every vector before returning it.
    ///
    /// # Errors
    /// Reports invalid config/input, credentials, transport or malformed output.
    fn vectors_compute_by_texts(
        &self,
        config: &EmbeddingConfig,
        texts: &[&str],
        dimensions: Option<NonZeroUsize>,
    ) -> EmbeddingResult<Vec<Vec<f32>>>;
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
/// Detached launch of a configured executable, argument vector and working directory.
///
/// Implementations preserve literal executable/arguments and working directory, inherit the
/// environment, redirect standard I/O to null and create a separate session. They return
/// without waiting for the launched process to finish.
#[cfg_attr(test, mockall::automock)]
pub trait ProcessLauncher {
    /// Start the configured process without interpreting arguments or waiting for completion.
    ///
    /// # Errors
    /// Reports process setup or launch failures while preserving the private I/O cause.
    fn process_spawn(&self) -> ProcessResult<()>;
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
