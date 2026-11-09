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

/// Maximum number of already available bindings submitted in one observation batch.
const PROVENANCE_BATCH_SIZE: usize = 256;

/// Synchronous wall-clock observations and bounded waits.
/// Implementations report failed or unrepresentable time observations as I/O errors.
pub trait Clock: std::fmt::Debug {
    /// Returns the current time in nanoseconds since the Unix epoch.
    ///
    /// # Errors
    /// Reports clock or signed nanosecond representation failures.
    fn clock_get_time_ns(&self) -> std::io::Result<i64>;

    /// Returns the current time in seconds since the Unix epoch.
    ///
    /// # Errors
    /// Reports clock failures.
    fn clock_get_time_secs(&self) -> std::io::Result<u64>;

    /// Waits synchronously for the requested duration.
    ///
    /// # Arguments
    ///
    /// * `delay` — duration to wait before returning.
    ///
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
    /// Returns whether the final path component is a symlink.
    ///
    /// Missing entries return false; directories and other nonsymlinks return false.
    /// The leaf identity and ancestry are verified before returning, including for absent
    /// entries.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path whose final component is checked without following it.
    ///
    /// # Errors
    /// Reports unsafe parents, replacements, invalid relative paths or I/O failures.
    fn symlink_is_exist(&self, path: &Path) -> FileResult<bool>;

    /// Reads the exact bytes of one regular file.
    ///
    /// Identity and content are verified around the I/O. Leaf stability and ancestry are
    /// verified before nonregular exclusions are classified; replacing an observed regular
    /// leaf is always an integrity failure.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the regular file to read.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changes, clock or I/O failures.
    fn file_get_by_path(&self, path: &Path) -> FileResult<Option<CapturedFile>>;

    /// Reads the bytes of one regular file.
    ///
    /// The identity, content and ancestry checks match file capture. The clock is not sampled
    /// and index metadata need not be representable.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the regular file whose bytes are read.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changes or I/O failures.
    fn file_get_bytes_by_path(&self, path: &Path) -> FileResult<Option<Vec<u8>>>;

    /// Reads a regular file's metadata without reading its body.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the regular file whose metadata is read.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, changing observations or I/O failures.
    fn file_get_stat_by_path(&self, path: &Path) -> FileResult<Option<FileStat>>;

    /// Reads one regular file's identity without reading its bytes.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the regular file whose identity is read.
    ///
    /// # Errors
    /// Reports nonregular leaves, unsafe paths, identity changes or I/O failures.
    fn file_get_identity_by_path(&self, path: &Path) -> FileResult<Option<FileIdentity>>;

    /// Returns whether a regular file exists at the path.
    ///
    /// Only absence returns false.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path whose regular-file presence is checked.
    ///
    /// # Errors
    /// Reports nonregular, unsafe-path, replacement and I/O failures.
    fn file_is_exist(&self, path: &Path) -> FileResult<bool>;

    /// Verifies that an original identity still exists.
    ///
    /// An absent original permits safe creation.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path whose current entry is verified.
    /// * `identity` — Original identity to compare, or `None` when the original was absent.
    ///
    /// # Errors
    /// Reports replacement/deletion of an original or any unsafe current entry.
    fn file_verify_identity_by_path(
        &self,
        path: &Path,
        identity: Option<FileIdentity>,
    ) -> FileResult<()>;

    /// Verifies that two paths and their file identities are distinct.
    ///
    /// File identities are compared when both entries exist.
    ///
    /// # Arguments
    ///
    /// * `first_path` — First root-relative path to compare.
    /// * `second_path` — Second root-relative path that must differ from the first.
    ///
    /// # Errors
    /// Reports aliases, unsafe/nonregular entries or observation failures.
    fn file_verify_distinct_by_paths(
        &self,
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<()>;

    /// Returns an owned handle to an existing directory.
    ///
    /// The path `.` selects the root. No entry is created and no mode is changed.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the directory; `.` selects the root.
    ///
    /// # Errors
    /// Reports unsafe or changed directories, and I/O failures.
    fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<OwnedFd>>;

    /// Returns the PID recorded in a held file lock.
    ///
    /// An absent or currently unheld lock returns `None`. No state is created or written.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the lock file to read.
    ///
    /// # Errors
    /// Reports unsafe/nonregular/aliased locks, changes, malformed owners or I/O failures.
    fn lock_get_owner_by_path(&self, path: &Path) -> FileResult<Option<NonZeroU32>>;
}

/// Private, durable file publication and directory creation under the reader's root.
pub trait FileWriter: FileReader {
    /// Creates a mode-0600 file with the given bytes if absent.
    ///
    /// An existing winner is preserved. File data is synced before publication and the parent
    /// directory before success.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative destination path for the new file.
    /// * `bytes` — Complete file contents to write.
    ///
    /// # Errors
    /// Reports absent parents, unsafe/nonregular destinations, changes or I/O failures.
    /// A failure preserves existing entries and may leave a complete published winner.
    fn file_create_if_absent_by_path(&self, path: &Path, bytes: &[u8]) -> FileResult<()>;

    /// Creates missing directory components with mode 0700.
    ///
    /// The final directory identity is pinned; existing ancestors keep their permissions.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative directory path whose missing components are created.
    ///
    /// # Errors
    /// Reports unsafe or changed entries, permission or other I/O failures.
    fn directory_create_by_path(&self, path: &Path) -> FileResult<OwnedFd>;

    /// Syncs an existing directory.
    ///
    /// No entry is created and no mode changes.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the existing directory to sync.
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

    /// Acquires the lock for this process.
    ///
    /// Waits for exclusive ownership and verifies identities first. Records this process's PID
    /// on success.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the lock file to acquire.
    ///
    /// # Errors
    /// Reports unsafe/nonregular/aliased entries, changes, permission or I/O failures.
    fn lock_acquire_by_path(&self, path: &Path) -> FileResult<Self::Lock>;

    /// Tries to acquire the lock without waiting.
    ///
    /// Returns `None` only for real contention.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the lock file to try to acquire.
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
    /// Verifies a use case's entry requirement without opening a transaction.
    ///
    /// An incompatible active scope becomes rollback-only, even if the caller catches the
    /// error. Inactive storage satisfies either requirement.
    ///
    /// # Arguments
    ///
    /// * `requirement` — Scope requirement the use case expects to satisfy.
    ///
    /// # Errors
    /// Rejects incompatible active scopes and failed or disabled owners.
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()>;

    /// Runs an operation in a pinned status read view.
    ///
    /// Returns `None` when no publication is stored. Unpublished state is skipped.
    ///
    /// # Arguments
    ///
    /// * `operation` — Operation to run in the pinned status view; it receives no transaction
    ///   argument.
    ///
    /// # Errors
    /// Returns operation errors, malformed metadata or transaction failures.
    fn with_status_read_tx<T, E, F>(&self, operation: F) -> Result<Option<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;

    /// Runs an operation in one pinned cache read view.
    ///
    /// The operation runs after the cache revision and vector identity are pinned.
    ///
    /// # Arguments
    ///
    /// * `operation` — Operation to run in the pinned read view; it receives no transaction
    ///   argument.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;

    /// Returns the revision that the active write will publish on commit.
    ///
    /// # Errors
    /// Rejects calls outside a write scope and revisions that cannot be incremented.
    fn project_get_revision(&self) -> StorageResult<u64>;

    /// Runs an operation in an atomically committed cache write scope.
    ///
    /// Returns the operation value after the commit succeeds.
    ///
    /// # Arguments
    ///
    /// * `operation` — Operation to run in the write scope; it receives no transaction argument.
    ///
    /// # Errors
    /// Returns the operation error or a storage transaction failure.
    fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>;

    /// Inserts or replaces metadata while keeping its row ID.
    ///
    /// Returns the row ID of the stored metadata.
    ///
    /// # Arguments
    ///
    /// * `node` — Knowledge or source metadata to insert or replace.
    ///
    /// # Errors
    /// Rejects unequal knowledge identity/address fields, unsupported numeric values,
    /// or database failures. Any error prevents publication of this write unit.
    fn nodes_upsert_by_id(&self, node: &NodeMetadata) -> StorageResult<i64>;

    /// Saves a declared edge without creating a node for an absent target.
    ///
    /// Targets added later in the unit resolve when the unit commits.
    ///
    /// # Arguments
    ///
    /// * `edge` — Declared edge to store, preserving its source, target and type.
    ///
    /// # Errors
    /// Rejects duplicate edges and missing sources; reports database failures.
    fn edges_save_by_source_id_and_target_id_and_type(&self, edge: &Edge) -> StorageResult<()>;

    /// Deletes the outgoing edges of one knowledge element.
    ///
    /// # Arguments
    ///
    /// * `source_id` — Identity of the knowledge element whose outgoing edges are removed.
    ///
    /// # Errors
    /// Reports database failures.
    fn edges_delete_by_source_id(&self, source_id: &KnowledgeId) -> StorageResult<()>;

    /// Deletes a node with its search entry.
    ///
    /// Incoming declarations stay and become missing targets.
    ///
    /// # Arguments
    ///
    /// * `id` — Address of the node to delete.
    ///
    /// # Errors
    /// Rejects deletion while outgoing edges remain; reports database failures.
    fn delete_node(&self, id: &EdgeTarget) -> StorageResult<()>;

    /// Replaces the search text of one knowledge element.
    ///
    /// # Arguments
    ///
    /// * `id` — Identity of the knowledge element whose search text is replaced.
    /// * `text` — Search text to store for the element.
    ///
    /// # Errors
    /// Returns not found for absent knowledge, or a database failure.
    fn replace_text(&self, id: &KnowledgeId, text: &str) -> StorageResult<()>;

    /// Inserts or replaces the state of one Markdown candidate.
    ///
    /// # Arguments
    ///
    /// * `file` — Candidate file state to insert or replace.
    ///
    /// # Errors
    /// Rejects unsupported numbers and inconsistent validity; reports database failures.
    fn files_upsert_by_path(&self, file: &FileRow) -> StorageResult<()>;

    /// Deletes the state of one candidate.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the candidate state to remove.
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
    /// Verifies a use case's entry requirement without opening a transaction.
    ///
    /// An incompatible active scope becomes rollback-only, even if the caller catches the
    /// error. Inactive storage satisfies either requirement.
    ///
    /// # Arguments
    ///
    /// * `requirement` — Scope requirement the use case expects to satisfy.
    ///
    /// # Errors
    /// Rejects incompatible active scopes and failed or disabled owners.
    fn verify_transaction_scope(&self, requirement: TransactionRequirement) -> StorageResult<()>;

    /// Resets stored vectors to the desired configuration identity.
    ///
    /// Returns `false` when the identity already matches and `true` after a reset. Returns
    /// `ConfigChanged` when the observed identity no longer matches.
    ///
    /// # Arguments
    ///
    /// * `observed` — Vector configuration identity observed by the caller, or `None` when
    ///   none was observed.
    /// * `desired` — Configuration identity that the stored vectors must match.
    ///
    /// # Errors
    /// Reports an active scope, incompatible state, contention or database failure.
    fn reset_vectors(
        &self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>>;

    /// Returns the vector dimensions in the current view.
    ///
    /// # Errors
    /// Reports an inactive scope, malformed metadata or database failure.
    fn vector_index_config_get_by_id(&self) -> StorageResult<Option<NonZeroUsize>>;

    /// Returns the stored failure for one search hash.
    ///
    /// # Arguments
    ///
    /// * `search_hash` — Search hash whose stored failure is read.
    ///
    /// # Errors
    /// Reports an inactive scope, invalid data or database failure.
    fn embed_failures_get_by_search_hash(
        &self,
        search_hash: &Sha256,
    ) -> StorageResult<Option<EmbeddingFailure>>;

    /// Returns the last query error.
    ///
    /// # Errors
    /// Reports an inactive scope, invalid data or database failure.
    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>>;

    /// Runs an operation in a vector read view for the expected identity.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the operation expects the stored vectors to match.
    /// * `operation` — Operation to run in the pinned vector read view; it receives no
    ///   transaction argument.
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

    /// Runs an operation in a vector write view for the expected identity.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the operation expects the stored vectors to match.
    /// * `operation` — Operation to run in the vector write scope; it receives no transaction
    ///   argument.
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

    /// Deletes vectors and failures that this transaction's cache observation does not
    /// reference.
    ///
    /// Concurrent publication can leave recomputable missing vectors until the next drain.
    ///
    /// # Errors
    /// Reports database failures and prevents commit of a failed unit.
    fn prune_unreferenced(&self) -> StorageResult<()>;

    /// Reserves one durable attempt within the caller's total budget.
    ///
    /// Commit before calling the provider; an unsent reservation still counts. Returns `true`
    /// when an attempt was reserved and `false` when the budget is exhausted.
    ///
    /// # Arguments
    ///
    /// * `search_hash` — Search hash whose durable attempt is reserved.
    /// * `max_attempts` — Total attempt budget, including the attempt being reserved.
    ///
    /// # Errors
    /// Rejects zero or budgets above `i64::MAX` and reports database failures.
    fn embed_failures_upsert_by_search_hash_and_attempts(
        &self,
        search_hash: &Sha256,
        max_attempts: u64,
    ) -> StorageResult<bool>;

    /// Replaces the error of an existing reservation.
    ///
    /// Returns false when no reservation exists and never creates an attempt.
    ///
    /// # Arguments
    ///
    /// * `search_hash` — Search hash whose existing reservation is updated.
    /// * `error` — Safe, already-sanitized message stored with the reservation.
    ///
    /// # Errors
    /// Reports database failures.
    fn embed_failures_update_by_search_hash(
        &self,
        search_hash: &Sha256,
        error: &str,
    ) -> StorageResult<bool>;

    /// Stores the singleton query failure.
    ///
    /// # Arguments
    ///
    /// * `at` — Observation time in seconds since the Unix epoch.
    /// * `error` — Safe, already-sanitized query failure message.
    ///
    /// # Errors
    /// Rejects timestamps above `i64::MAX` and reports database failures.
    fn query_error_upsert_by_id(&self, at: u64, error: &str) -> StorageResult<()>;

    /// Deletes the singleton query error.
    ///
    /// # Errors
    /// Reports database failures.
    fn query_error_delete_by_id(&self) -> StorageResult<()>;

    /// Stores one text vector.
    ///
    /// An existing key and vector are preserved. Returns the stored key and whether this unit
    /// inserted it.
    ///
    /// # Arguments
    ///
    /// * `search_hash` — Text-addressed key under which the vector is stored.
    /// * `vector` — Vector values to store.
    /// * `dimensions` — Required vector length, which must match the active index dimensions.
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
    RecheckFilter, RefreshOutcome, SearchFilters, SearchFlag, SearchLink, SearchRequest,
    SnapshotEntry, SnapshotFile, SnapshotState, SourceAvailability, SourceMetadata,
    SourceObservation, SourceRevisionComparison, SourceRevisionRequest, StatusReport, StoredNode,
    StoredVectorKey, TextMatch, TransactionRequirement, Traversal, TraversalDirection,
    TraversalLink, TraversalMode, TraversalRequest, VectorMatch, VectorOutcome,
};
pub(crate) use types::{
    GENERATED_CONFIG, MAX_DIMENSIONS, MAX_SEARCH_LIMIT, MIN_DIMENSIONS, NEIGHBOR_PAGE_QUANTUM,
};
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
pub(crate) use configuration::{ConfigurationService, map_configuration_error_to_command_error};
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

/// Current graph diagnostics derived from the injected shared cache.
///
/// Implementations inspect the entire knowledge graph before filtering by exact
/// owner file paths. An empty selector includes all files; repeated selectors do
/// not duplicate findings. Results are sorted by path, reason, element ID and target,
/// with absent optional fields sorting as empty strings. No refresh, write, network
/// call or separate database connection is permitted. A call joins a compatible
/// active cache scope or owns a fresh pinned read scope.
pub trait GraphDiagnostics {
    /// Returns safe findings for invalid files, dangling targets and includes cycles.
    ///
    /// Each cycle contributes one finding per owning file with its minimum local
    /// element ID; each dangling pair retains its source element ID and target.
    /// Every finding has `ValidationFailed` classification and static safe text.
    ///
    /// # Arguments
    ///
    /// * `paths` — Exact owner file paths to filter by; empty includes all files.
    ///
    /// # Errors
    /// Reports an internal error with a private storage cause if any read fails;
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
    /// Returns current bytes or typed availability for one source.
    ///
    /// Unknown addresses and knowledge nodes return `None` without filesystem I/O.
    /// A local observation has no cross-file or historical snapshot guarantee.
    /// Missing files and incomplete ranges return `SourceNotFound`; remote
    /// availability remains `Unfetched`. Results never contain source bytes.
    ///
    /// # Arguments
    ///
    /// * `uri` — Normalized source address to observe.
    ///
    /// # Errors
    /// Reports a safe document error with a private cause when storage or confined
    /// filesystem access fails; such failures never become `SourceNotFound`. Calls
    /// join a compatible active cache scope or own a fresh pinned read scope.
    fn get_source_by_uri(&self, uri: &SourceUri) -> DocumentResult<Option<SourceObservation>>;
}

/// Read-only source comparison using caller-owned consistent metadata.
///
/// Implementations never refresh, open storage, retain refs, write or fetch URLs.
/// Callers gather all request fields from the same pinned view or validated projection.
/// Declared roots are checked even without sources. Missing/invalid bases require
/// rechecking; missing local addresses are reported independently. Remote bytes
/// remain unknown and are never classified as changed or missing by comparison.
pub trait SourceProvenance {
    /// Validates every requested root independently of source bindings.
    ///
    /// Checks roots in their ordered set order; an empty set succeeds. This is
    /// read-only and must run before candidate-dependent exits or query inference.
    /// The first invalid root rejects the request without partial results.
    ///
    /// # Arguments
    ///
    /// * `revisions` — Unique declared roots to validate in their ordered set order.
    ///
    /// # Errors
    /// Missing trees are `NotFound`, wrong type/format is `InvalidRequest`, and
    /// repository or object integrity failures remain `InternalError`.
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError>;

    /// Returns source flags and current diagnostics without source bytes.
    ///
    /// # Arguments
    ///
    /// * `request` — One element's basis and sources to observe.
    ///
    /// # Errors
    /// Reports safe errors for filesystem, Git policy, I/O or corruption failures.
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation>;

    /// Returns one observation per request in order, each from a fresh live capture.
    ///
    /// Stops at the same first demanded error as sequential observation. Implementations
    /// may reuse bounded immutable historical successes for this call only; no persistent
    /// cache or retention is allowed. The default preserves existing test doubles and
    /// implementations by calling `observe_sources` sequentially.
    ///
    /// # Arguments
    ///
    /// * `requests` — Ordered requests, one per element binding, to observe.
    ///
    /// # Errors
    /// Reports the first filesystem, Git policy, I/O or corruption failure.
    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        requests
            .iter()
            .map(|request| self.observe_sources(*request))
            .collect()
    }

    /// Compares one address between a declared basis and a requested tree.
    ///
    /// Validates the requested root before returning any unavailable/unknown state.
    /// Both trees use the same local path and complete inclusive line range.
    ///
    /// # Arguments
    ///
    /// * `request` — Address, basis and explicit revision to compare.
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
pub trait SnapshotRefsUpdater {
    /// Retains direct refs for each validated root without traversing descendants.
    ///
    /// # Arguments
    ///
    /// * `snapshots` — Unique roots whose direct refs are validated and updated.
    ///
    /// # Errors
    /// Reports root corruption, policy, Git I/O and ref failures.
    fn snapshot_refs_update_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;

    /// Retains each available root after auditing its complete local closure.
    ///
    /// Existing refs do not bypass repeated corruption checks.
    ///
    /// # Arguments
    ///
    /// * `snapshots` — Unique roots whose complete local closures are verified and their refs updated.
    ///
    /// # Errors
    /// Reports incomplete/corrupt closures, policy, Git I/O and ref failures.
    fn snapshot_sync_by_oids(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;
}

/// Bounded graph reads from one consistent caller-owned view.
///
/// Handles belong to this view only. Pages sort by (edge type, reached identity),
/// preserve original edge orientation and exclude missing endpoints before the
/// limit. No complete node metadata is decoded for neighbor enumeration.
/// A cursor sequence fixes node, direction, mode and types. Only an empty page
/// ends a stream; a short nonempty page does not guarantee exhaustion.
pub trait GraphRead {
    /// Returns the node for one normalized identity.
    ///
    /// The stored classification resolves the identity.
    ///
    /// # Arguments
    ///
    /// * `uri` — Normalized element identity to resolve.
    ///
    /// # Errors
    /// Reports database failures or invalid persisted identities/kinds.
    fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>>;

    /// Returns one page of eligible neighbors after a cursor.
    ///
    /// The handle must originate in this view; the limit must be positive and at most 32.
    ///
    /// # Arguments
    ///
    /// * `request` — Page request carrying the handle, fixed stream parameters and cursor.
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
pub trait GraphWalk {
    /// Returns the graph reached from the requested starts.
    ///
    /// The traversal joins an active scope or opens a fresh read.
    ///
    /// # Arguments
    ///
    /// * `request` — Starts to validate, traversal selection and output bounds.
    ///
    /// # Errors
    /// Rejects empty or absent starts and reports safe storage or integrity failures.
    /// No partial graph is returned on failure.
    fn traverse(&self, request: &TraversalRequest) -> CommandResult<Graph>;
}

/// Shared-reference observations within an active cache read or write scope.
///
/// Every implementation answers all methods from one consistent view, so results
/// never mix revisions, and never modifies stored data.
pub trait StorageRead {
    /// Returns the count of distinct non-source knowledge search hashes.
    ///
    /// The active cache scope supplies the count.
    ///
    /// # Errors
    /// Reports database failures or invalid persisted counts.
    fn nodes_get_by_kind(&self) -> StorageResult<u64>;

    /// Returns the count of distinct covered knowledge hashes.
    ///
    /// Current native vector rows are validated. Sources and orphan vector keys are excluded.
    /// An active cache scope is required.
    ///
    /// # Errors
    /// Reports missing native rows, invalid counts and database failures.
    fn nodes_get_coverage_by_kind(&self) -> StorageResult<u64>;

    /// Returns current uncovered failures at or above the attempt threshold.
    ///
    /// Every current uncovered row is decoded and validated before threshold filtering;
    /// only matching rows are kept, once each in ascending search-hash order.
    /// Sources, orphan failures and hashes with vector keys are excluded.
    ///
    /// # Arguments
    ///
    /// * `max_attempts` — Positive attempt threshold; only failures at or above it are kept.
    ///
    /// # Errors
    /// Reports malformed persisted hashes or attempts, or database failures.
    fn embed_failures_get_by_kind_and_search_hash_and_attempts(
        &self,
        max_attempts: u64,
    ) -> StorageResult<Vec<EmbeddingFailure>>;

    /// Returns the next eligible embedding work after the optional cursor.
    ///
    /// Returns `ConfigChanged` when the stored identity differs from `expected`.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity whose current vectors make work ineligible.
    /// * `max_attempts` — Positive attempt budget used to select eligible work.
    /// * `uri` — Exclusive lower bound for the ascending search; `None` starts at the beginning.
    ///
    /// # Errors
    /// Rejects zero or unrepresentable attempt budgets; reports I/O or invalid data.
    fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
        &self,
        expected: &Sha256,
        max_attempts: u64,
        uri: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>>;

    /// Returns the vector dimensions under the pinned configuration identity.
    ///
    /// Returns `ConfigChanged` when the stored identity differs from `expected`.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the vector index must match.
    ///
    /// # Errors
    /// Reports malformed native metadata or database failures.
    fn vector_index_config_get_by_id(
        &self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>>;

    /// Returns the stored node metadata for one identity.
    ///
    /// # Arguments
    ///
    /// * `uri` — Address of the node metadata to read.
    ///
    /// # Errors
    /// Returns an internal error for I/O or malformed persisted values.
    fn nodes_get_by_uri(&self, uri: &EdgeTarget) -> StorageResult<Option<StoredNode>>;

    /// Returns the distinct non-null source roots.
    ///
    /// Node metadata is not read.
    ///
    /// # Errors
    /// Reports database failures or malformed persisted tree identities.
    fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>>;

    /// Returns all published node metadata ordered by identity.
    ///
    /// # Errors
    /// Reports an internal error for I/O or malformed persisted values.
    fn nodes_get(&self) -> StorageResult<Vec<StoredNode>>;

    /// Returns declarations ordered by source, target and type.
    ///
    /// An optional source restricts both resolved and missing-target declarations; absent
    /// targets have no node metadata.
    ///
    /// # Arguments
    ///
    /// * `source_id` — Optional declaring knowledge identity; `None` returns declarations for
    ///   all sources.
    ///
    /// # Errors
    /// Reports an internal error for I/O or malformed persisted edges.
    fn edges_get_by_source_id(&self, source_id: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>>;

    /// Returns the stored state of one candidate file.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the candidate state to read.
    ///
    /// # Errors
    /// Reports an internal error for I/O or malformed persisted values.
    fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>>;

    /// Returns all candidate file states ordered by path.
    ///
    /// # Errors
    /// Reports an internal error for I/O or malformed persisted values.
    fn files_get(&self) -> StorageResult<Vec<FileRow>>;
    /// Selects at most `limit` FTS knowledge hits ordered by BM25 then identity.
    ///
    /// Only selected rows are decoded; filtering and fusion operate on this bounded pool.
    ///
    /// # Arguments
    ///
    /// * `expression` — Prepared FTS5 expression to execute.
    /// * `limit` — Maximum number of knowledge hits to select.
    ///
    /// # Errors
    /// Reports an invalid request for malformed expressions, or an internal error for I/O.
    fn text_index_get_by_expression(
        &self,
        expression: &str,
        limit: NonZeroUsize,
    ) -> StorageResult<Vec<TextMatch>>;

    /// Returns the vector stored under one search hash.
    ///
    /// The stored vector must match the expected configuration identity; otherwise returns
    /// `ConfigChanged`.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the stored vector must match.
    /// * `search_hash` — Text-addressed key whose vector is read.
    ///
    /// # Errors
    /// Reports malformed stored vectors or database failures.
    fn vector_keys_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>>;
    /// Selects at most `limit` current knowledge hits ordered by cosine distance then identity.
    ///
    /// Native matching, joins, ordering and the knowledge-row limit execute in `SQLite`.
    /// Unreferenced vectors consume no returned slots; only selected rows are decoded.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the vector index must match.
    /// * `query` — Query vector used for native cosine search.
    /// * `dimensions` — Expected length of `query` and of the indexed vectors.
    /// * `limit` — Maximum number of knowledge hits to select.
    ///
    /// # Errors
    /// Rejects invalid query vectors or reports database/integrity failures.
    fn vector_index_get_by_vector(
        &self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
        limit: NonZeroUsize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>>;

    /// Returns durable attempts and the latest failure for one search hash.
    ///
    /// The stored attempts must match the expected configuration identity; otherwise returns
    /// `ConfigChanged`.
    ///
    /// # Arguments
    ///
    /// * `expected` — Configuration identity the stored attempts must match.
    /// * `search_hash` — Search hash whose durable attempts and latest failure are read.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn embed_failures_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>>;

    /// Returns the last query embedding failure.
    ///
    /// Model resets do not clear it.
    ///
    /// # Errors
    /// Reports malformed persisted values or database failures.
    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>>;

    /// Returns the revision in the active cache scope.
    ///
    /// # Errors
    /// Rejects calls outside a scope, malformed revisions and database failures.
    fn project_get_by_id(&self) -> StorageResult<u64>;

    /// Returns the vector configuration identity in the active cache scope.
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
    /// Publishes changed file projections atomically.
    ///
    /// # Arguments
    ///
    /// * `_paths` — Selected owner paths to refresh; empty refreshes the complete corpus.
    ///
    /// # Errors
    /// Reports unsafe paths, concurrent changes, or Git, filesystem and storage failures.
    fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;

    /// Refreshes incrementally while auditing source closures.
    ///
    /// The index lock is held, and verification precedes SQL publication even when the file
    /// delta is empty.
    ///
    /// # Arguments
    ///
    /// * `_paths` — Selected owner paths to refresh; empty refreshes the complete corpus.
    ///
    /// # Errors
    /// Reports capture, integrity, retention and storage failures without partial publication.
    fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;

    /// Reparses every candidate in one atomic publication.
    ///
    /// The index lock is held, source closures are audited before SQL, invalid files keep their
    /// last valid projection, and vectors are not awaited.
    ///
    /// # Arguments
    ///
    /// * `_paths` — Selected owner paths to reindex; empty reindexes the complete corpus.
    ///
    /// # Errors
    /// Reports capture, provenance or storage failures without a partial publication.
    fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome>;
}

/// Raw Git index and policy-selected untracked worktree path observations.
///
/// Return nonempty NUL-terminated paths after the supplied root-relative ignore
/// bytes apply to tracked and untracked paths, without document-role filtering.
/// Include deleted tracked paths and expand sparse index membership; duplicate stages
/// are permitted. Read only Git index, ignore and sparse-tree metadata; never
/// modify the index or repository.
#[cfg_attr(test, mockall::automock)]
pub trait GitWorktreeListing {
    /// Returns raw root-relative paths selected by ignore mode and ignore bytes.
    ///
    /// Additional ignore bytes apply to the complete union, including tracked paths.
    ///
    /// # Arguments
    ///
    /// * `ignore_mode` — Selects standard Git exclusion sources or their absence.
    /// * `ignore_bytes` — Root-relative Gitignore bytes applied to tracked and untracked paths.
    ///
    /// # Errors
    /// Reports index, ignore, sparse-object, worktree traversal or repository-policy failures.
    fn worktree_paths_get_by_ignore_mode_and_ignore_bytes(
        &self,
        ignore_mode: GitIgnoreMode,
        ignore_bytes: &[u8],
    ) -> GitResult<Vec<u8>>;
}

/// Pure shared Markdown projection and search-text codec.
///
/// Every implementation is deterministic and performs no I/O: equal paths and bytes give equal results.
pub trait MarkdownCodec {
    /// Parses a whole file atomically.
    ///
    /// Unmarked Markdown produces `None`.
    ///
    /// # Arguments
    ///
    /// * `path` — Root-relative path of the whole file being parsed.
    /// * `bytes` — Complete file bytes to parse atomically.
    ///
    /// # Errors
    /// Reports safe local diagnostics for invalid UTF-8, metadata or graph markup.
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
    /// Returns the state of one exact root.
    ///
    /// No Git objects are created, written or retained.
    ///
    /// # Arguments
    ///
    /// * `tree` — Exact root tree object to verify.
    ///
    /// # Errors
    /// Reports [`SnapshotErrorKind::InternalError`] for repository policy, Git I/O
    /// or object integrity failures; these failures must not become a state.
    fn tree_verify_by_oid(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError>;
}

/// Synchronous immutable snapshot operations. Creation alone does not retain.
pub trait GitSnapshots {
    /// Creates a tree from exactly these files.
    ///
    /// The captured bytes are preserved.
    ///
    /// # Arguments
    ///
    /// * `files` — Exact captured files whose bytes become the new tree.
    ///
    /// # Errors
    /// Reports [`SnapshotErrorKind::InvalidRequest`] for invalid, duplicate or
    /// colliding paths, and [`SnapshotErrorKind::InternalError`] when repository
    /// policy, Git I/O or object integrity checks fail.
    fn tree_create(&self, files: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError>;

    /// Writes exact bytes as one blob.
    ///
    /// No filters or normalization are applied, and nothing is retained.
    ///
    /// # Arguments
    ///
    /// * `bytes` — Exact bytes to write as one blob.
    ///
    /// # Errors
    /// Reports repository policy, Git I/O or object integrity failures.
    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError>;

    /// Creates a tree from exactly these validated paths and blobs.
    ///
    /// The tree is not retained.
    ///
    /// # Arguments
    ///
    /// * `entries` — Validated paths and blob identities to assemble.
    ///
    /// # Errors
    /// Invalid paths, collisions, missing/wrong-type blobs or foreign formats are
    /// invalid requests; policy, I/O and corruption failures are internal errors.
    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError>;

    /// Returns a complete blob from an exact tree.
    ///
    /// There is no live-file fallback.
    ///
    /// # Arguments
    ///
    /// * `tree` — Exact tree root containing the blob.
    /// * `path` — Root-relative path of the blob within the tree.
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

    /// Retains the direct ref of one exact root.
    ///
    /// The root is validated, but descendants are not audited. An updated ref protects healthy
    /// reachable objects from normal GC, not from corruption.
    ///
    /// # Arguments
    ///
    /// * `tree` — Exact root whose direct ref is validated and updated.
    ///
    /// # Errors
    /// Missing roots are not found; wrong types/formats are invalid requests.
    /// Root corruption, policy, Git I/O and ref conflicts are internal errors.
    fn ref_update_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError>;

    /// Retains one root through Git GC after verifying its complete local closure.
    ///
    /// # Arguments
    ///
    /// * `tree` — Exact root whose complete local closure is verified and its ref updated.
    ///
    /// # Errors
    /// Returns [`SnapshotErrorKind::NotFound`] for a missing root,
    /// [`SnapshotErrorKind::InvalidRequest`] for an incompatible OID format or
    /// non-tree root, and [`SnapshotErrorKind::InternalError`] for an incomplete
    /// or corrupt closure, repository policy, Git I/O or ref publication failure.
    fn tree_sync_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError>;
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
    /// Validates the endpoint and protocol configuration locally.
    ///
    /// No inference is performed.
    ///
    /// # Arguments
    ///
    /// * `config` — Endpoint and protocol configuration to validate.
    ///
    /// # Errors
    /// Reports invalid configuration when the endpoint or protocol is unsupported.
    fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;

    /// Returns vectors for the given texts in one attempt.
    ///
    /// Every vector is validated before it is returned.
    ///
    /// # Arguments
    ///
    /// * `config` — Validated provider configuration for this attempt.
    /// * `texts` — Texts to embed, in the order vectors are returned.
    /// * `dimensions` — Expected vector length for this attempt, or `None` when no explicit
    ///   expectation applies.
    ///
    /// # Errors
    /// Reports invalid config or input, credentials, transport or malformed output.
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
    /// Returns the current configuration state.
    ///
    /// The configuration is validated before any vector synchronization. Valid disabled or
    /// unconfigured documents return `ConfigState::Disabled` without observing or resetting
    /// vector identity. A superseded enabled observation returns `ConfigState::Changed` without
    /// retrying or resetting.
    ///
    /// # Errors
    /// Reports storage failures; unreadable or invalid configuration is a typed invalid state.
    fn get_configuration(&mut self) -> EmbeddingResult<ConfigState>;

    /// Drains current eligible documents using durable bounded attempts.
    ///
    /// # Errors
    /// Reports storage or environment failures that prevent safe coordination.
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome>;

    /// Returns one transient query vector.
    ///
    /// Only a safe diagnostic is stored.
    ///
    /// # Arguments
    ///
    /// * `text` — Query text sent once to the configured provider.
    ///
    /// # Errors
    /// Reports storage or clock failures; ordinary provider errors are unavailable.
    fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding>;

    /// Returns one transient query vector from captured inputs.
    ///
    /// Configuration is not reread or reset. A changed committed identity and incompatible
    /// captured or current dimensions are rejected. No write unit or embedding lock spans
    /// inference, and document attempts are untouched. Only the captured identity's guarded safe
    /// diagnostic may be persisted or cleared.
    ///
    /// # Arguments
    ///
    /// * `request` — Captured configuration, dimensions and query text for one transient
    ///   embedding.
    ///
    /// # Errors
    /// Reports storage or clock failures; invalid provider policy and provider failures are
    /// unavailable.
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
    /// Returns validation findings for the given paths or the whole corpus.
    ///
    /// # Arguments
    ///
    /// * `paths` — Exact owner paths to validate; empty validates the whole corpus.
    ///
    /// # Errors
    /// Reports refresh or infrastructure failures with private causes; no partial report
    /// is returned on failure. Validation findings are successful reports.
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport>;
}

/// Read the latest published diagnostics without refreshing or mutating state.
pub trait StatusReader {
    /// Returns one report from a pinned database view, the current configuration and busy
    /// locks.
    ///
    /// # Errors
    /// Reports storage, provenance or unsafe lock failures without a partial report.
    fn get_status(&mut self) -> CommandResult<StatusReport>;
}

/// Storage capability required to initialize both service schemas.
pub trait InitStorage {
    /// Initializes the cache and vector schemas under one pinned directory.
    ///
    /// # Arguments
    ///
    /// * `directory` — Repository directory beneath which the schemas are opened.
    /// * `pinned` — Already pinned directory descriptor that confines the operation.
    ///
    /// # Errors
    /// Reports unsafe paths, incompatible schemas, contention or database I/O failures.
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()>;
}

/// One repository initialization operation.
pub trait Initializer {
    /// Initializes the service directory, storage and configuration.
    ///
    /// Existing valid state is not replaced.
    ///
    /// # Errors
    /// Reports unsafe repository, config or storage state, invalid retained settings or
    /// an environment failure before success is observable.
    fn initialize(&mut self) -> CommandResult<()>;
}

/// Read the current validated embedding configuration without changing state.
///
/// Implementations observe only the non-creating configuration source and perform local
/// provider validation. They never expose raw bytes, identity, dimensions-derived data or
/// credentials, and never start readiness, vector resets, storage bootstrap or inference.
pub trait ConfigReader {
    /// Returns the current configuration or its stable absence.
    ///
    /// On success returns the complete nested configuration or stable absence.
    ///
    /// # Errors
    /// Reports `EmbeddingFailed` for a legacy flat document and every malformed or
    /// provider-rejected value; reports `InternalError` with
    /// `cannot read embedding configuration` for unreadable or unsafe files. Causes stay
    /// private and no partial configuration is returned.
    fn get_config(&self) -> CommandResult<Option<Configuration>>;
}

/// Consistent metadata-only hybrid knowledge search.
pub trait Searcher {
    /// Returns search links from one refreshed pinned revision.
    ///
    /// Live knowledge and source freshness are observed. Source bindings and metadata share the
    /// ranking view; live bytes are observed separately. Flags are deduplicated X-1 values and
    /// links contain no source text or excerpts.
    ///
    /// # Arguments
    ///
    /// * `request` — Query, result limit and normalized filters.
    ///
    /// # Errors
    /// Reports refresh, storage, integrity or non-configuration filesystem failures.
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
    /// Returns metadata for the requested elements from one snapshot.
    ///
    /// # Arguments
    ///
    /// * `ids` — Normalized element addresses to inspect in one snapshot.
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
    /// Returns whether the configuration is ready and eligible durable work exists.
    ///
    /// # Errors
    /// Reports failures inspecting durable work.
    fn is_work_available(&mut self) -> CommandResult<bool>;

    /// Drains eligible work under the executor-owned lock.
    ///
    /// Missing or invalid configuration, a changed identity and an occupied lock finish cleanly.
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
    /// Starts the configured process without waiting for completion.
    ///
    /// Arguments are not interpreted.
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
    /// Starts eligible work when durable work is available.
    ///
    /// Returns a safe nonfatal warning if the launch fails.
    ///
    /// # Errors
    /// Reports failures inspecting durable readiness.
    fn start_if_needed(&mut self) -> CommandResult<Option<CommandProblem>>;

    /// Delegates once to the embedding executor.
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
    /// Returns selected elements and links from one pinned publication revision.
    ///
    /// # Arguments
    ///
    /// * `request` — Starts, traversal selection and output bounds.
    ///
    /// # Errors
    /// Reports safe refresh, storage, start-membership or observation errors without
    /// partial output. The pinned view is released on every return path.
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal>;
}

#[cfg(test)]
#[path = "cache_test_support.rs"]
pub(crate) mod cache_test_support;
