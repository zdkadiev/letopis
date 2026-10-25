//! Application contracts.
use crate::domain::RepoPath;
mod background;
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
mod retained_tree;
mod search;
mod snapshot_error;
mod snapshot_traits;
mod storage;

pub(crate) use background::BackgroundService;
pub(crate) use document_refresher::DocumentRefresher;
pub(crate) use embedding::EmbeddingService;
pub(crate) use graph_diagnostics::CurrentGraphDiagnostics;
pub(crate) use inspection::InspectionService;
pub(crate) use search::{SearchDependencies, SearchService};

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
