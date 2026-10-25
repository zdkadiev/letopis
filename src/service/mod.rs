//! Application contracts.
use crate::domain::{RepoPath, SourceUri, TreeOid};
use std::collections::BTreeSet;
mod background;
mod check;
mod command;
mod command_traits;
mod document_refresher;
mod document_traits;
mod embedding;
mod embedding_config;
mod embedding_error;
mod embedding_traits;
mod error;
mod graph_diagnostics;
mod inspection;
mod model;
mod projection;
mod provenance;
mod provenance_types;
mod retained_tree;
mod search;
mod search_filters;
mod snapshot_error;
mod snapshot_traits;
mod source;
mod storage;

pub(crate) use background::BackgroundService;
pub(crate) use check::CheckService;
/// Cache refresh implementation and its caller-owned collaborator bundle.
pub(crate) use document_refresher::{DocumentRefresher, RefreshDependencies};
pub(crate) use embedding::EmbeddingService;
pub(crate) use graph_diagnostics::CurrentGraphDiagnostics;
pub(crate) use inspection::InspectionService;
pub(crate) use provenance::ProvenanceService;
/// Source comparison requests and metadata-only outcomes.
pub use provenance_types::{
    ProvenanceIssue, ProvenanceObservation, ProvenanceRequest, SourceRevisionComparison,
    SourceRevisionRequest,
};
pub(crate) use search::{SearchDependencies, SearchService};
/// Concrete source observation assembly, kept inside the crate.
pub(crate) use source::SourceService;

pub use command::{
    CheckReport, CommandError, CommandErrorKind, CommandProblem, CommandResult, ElementAddress,
    InspectedElement, Inspection, SearchFlag, SearchLink, SearchRequest,
};
pub use command_traits::{
    BackgroundRunner, Checker, DrainLauncher, EmbedLocker, EmbeddingExecutor, Inspector, Searcher,
};
pub use document_traits::{CorpusLister, FileSystem, MarkdownCodec, Refresher};
pub use embedding_config::{EmbeddingConfig, ProviderKind};
pub(crate) use embedding_config::{MAX_DIMENSIONS, MIN_DIMENSIONS};
pub use embedding_error::{EmbeddingError, EmbeddingErrorKind, EmbeddingResult};
pub use embedding_traits::{Embedder, EmbeddingFiles, EmbeddingProvider};
pub use error::{
    DocumentError, DocumentErrorKind, DocumentResult, StorageError, StorageErrorKind, StorageResult,
};
pub use model::{
    CapturedFile, ConfigState, Diagnostic, DiagnosticKind, DrainOutcome, EmbeddingFailure,
    EmbeddingWork, FileRow, FileStat, KnowledgeMetadata, NodeMetadata, ParsedElement, ParsedFile,
    QueryEmbedding, QueryEmbeddingRequest, QueryError, RefreshOutcome, SourceMetadata, StoredNode,
    StoredVectorKey, TextMatch, VectorMatch, VectorOutcome,
};
pub use retained_tree::RetainedTree;
/// Typed, normalized search restrictions and current-flag selection.
pub use search_filters::{RecheckFilter, SearchFilters};
pub use snapshot_error::{SnapshotError, SnapshotErrorKind};
pub use snapshot_traits::{
    GitSnapshots, SnapshotEntry, SnapshotFile, SnapshotState, SnapshotVerifier,
};
pub use storage::{CacheWrite, Storage, StorageRead, VectorWrite};

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
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
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
    fn read_source<R: StorageRead>(
        &self,
        view: &mut R,
        uri: &SourceUri,
    ) -> DocumentResult<Option<SourceObservation>>;
}

/// Metadata and current availability of one referenced source; contains no bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceObservation {
    /// Derived identity/creation time and freshly observed hash and line range.
    pub metadata: SourceMetadata,
    /// Availability of the addressed bytes during this explicit observation.
    pub availability: SourceAvailability,
}

/// Current byte availability, separate from source membership in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAvailability {
    /// The complete addressed local bytes were captured successfully.
    Available,
    /// Referenced local bytes are absent, including an incomplete line range.
    SourceNotFound,
    /// The source is remote and no fetch was performed; availability is unknown.
    Unfetched,
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
/// Index publication's retention capability; the caller must hold index.lock.
///
/// Every implementation verifies each unique root once per call and pins every
/// available root, including roots without external sources. A pin failure after
/// successful verification is an error, even if the root disappeared meanwhile.
pub trait BasisRetainer {
    /// Verify unique declared roots and pin every available complete tree closure.
    /// Missing/invalid roots are validation findings, not publication failures.
    ///
    /// # Errors
    /// Returns safe errors for corruption, Git I/O or failed ref publication.
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()>;
}
