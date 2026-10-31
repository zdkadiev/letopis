//! Read-only knowledge graph foundations for Letopis.

// Production code returns errors and documents exceptions with `#[expect]`.
// Tests and integration-test helpers are exempt: `cfg(test)` is off for the lib build only.
#![cfg_attr(
    not(test),
    warn(
        clippy::allow_attributes,
        clippy::allow_attributes_without_reason,
        clippy::expect_used,
        clippy::unwrap_used
    )
)]

use crate::cli::{CommandResponse, ResponseWriter, parse_request, write_response};
use crate::commands::run_request;
use crate::embedding::{HttpProviders, Ollama, Openai};
use crate::filesystem::LocalFileSystem;
use crate::git::{GitRepository, GitWriteCleanup};
use crate::markdown::MarkdownParser;
use crate::process::{DetachedProcess, ProcessInput, SigintListener};
use crate::service::{
    BackgroundRunner, BackgroundService, CheckService, Checker, CommandError, CommandErrorKind,
    CommandResult, DocumentError, DocumentErrorKind, DocumentRefreshService, DocumentResult,
    EmbedLocker, Embedder, EmbeddingError, EmbeddingErrorKind, EmbeddingExecutor, EmbeddingFiles,
    EmbeddingProvider, EmbeddingResult, EmbeddingService, FileSystem, GitSnapshots,
    GraphDiagnostics, GraphDiagnosticsService, GraphService, InitService, Initializer,
    InspectionService, Inspector, ProvenanceService, Refresher, SearchService, Searcher,
    ServiceError, ServiceErrorKind, ServiceResult, SnapshotError, SnapshotErrorKind,
    SnapshotVerifier, SourceProvenance, SourceReader, SourceService, StatusFiles, StatusStorage,
    Storage, StorageResult, TraversalService, Traverser,
};
use crate::storage::{SqliteBootstrap, SqliteStatusStorage, SqliteStorage};
use crate::utils::SERVICE_DIRECTORY;
use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    os::fd::AsFd,
    path::Path,
    sync::Arc,
};

mod cli;
mod commands;
pub mod domain;
mod embedding;
mod filesystem;
mod git;
pub mod markdown;
mod process;
mod storage;
mod utils;

pub mod service;

const FOREGROUND_COMMANDS: [&str; 5] = ["search", "traverse", "inspect", "check", "reindex"];

/// Parse and run one CLI request, emitting one complete JSON envelope.
/// The exact forms `--help`, `<command> --help` and `--version` are ordinary
/// requests that print plain text without reading `input` or touching any service.
/// Returns 0 only for successfully written success output, otherwise 1.
/// Check, search, traverse and inspect refresh through their application services.
/// Reindex forces a complete atomic publication through the same document service.
/// Successful foreground operations may launch the existing detached embedding drain.
pub fn run_cli(
    args: impl IntoIterator<Item = OsString>,
    input: impl Read,
    output: impl Write,
) -> u8 {
    let args = args.into_iter().collect::<Vec<_>>();
    write_response(
        output,
        execute_request(&args, input, Arc::new(GitWriteCleanup::new())),
    )
}

/// Run the executable entry path with scoped foreground SIGINT handling.
///
/// Registers one listener for the exact `search`, `traverse`, `inspect`, `check`
/// and `reindex` argv shapes before stdin or services are used. Other commands
/// keep their default signal behavior. Terminal stdin and a pipe that sends no
/// bytes within 250 ms are treated as empty requests; after the first byte,
/// non-terminal input is read to EOF. A handled SIGINT writes one cancellation
/// envelope through the shared response owner and exits 1; normal output retains
/// its own exit code.
pub fn run_process_cli() -> u8 {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let mut output = io::stdout();
    let input = ProcessInput::new(io::stdin());
    run_process_cli_with(&args, input, &mut output, SigintListener::install)
}

/// Construct current diagnostics without opening storage or performing I/O.
/// The caller supplies the consistent read view to each operation.
pub fn wire_graph_diagnostics() -> impl GraphDiagnostics {
    GraphDiagnosticsService::new()
}

/// Wire repository initialization with noncreating Git discovery and owned adapters.
///
/// # Errors
/// Returns `invalid_request` outside a non-bare Git work tree, and a safe internal
/// error for repository, filesystem or provider setup failures.
pub fn wire_initializer(start: &Path) -> ServiceResult<impl Initializer + use<>> {
    let git = GitRepository::open(start).map_err(map_snapshot_2_service_error)?;
    let root = git.get_root().to_path_buf();
    let files = LocalFileSystem::open(&root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Initialization, cause))?;
    let provider = build_embedding_provider()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Initialization, cause))?;
    Ok(InitService::new(
        Box::new(files),
        Box::new(SqliteBootstrap),
        Box::new(provider),
        root.join(SERVICE_DIRECTORY),
    ))
}

/// Wire bounded traversal with owned graph, source, provenance and readiness collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_traverser(start: &Path) -> ServiceResult<impl Traverser + use<>> {
    let git = GitRepository::open(start)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    wire_traverser_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_traverser_with_repository(git: GitRepository) -> ServiceResult<impl Traverser + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let filesystem = LocalFileSystem::open(git.get_root())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let directory = filesystem
        .open_service_directory()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    Ok(TraversalService {
        storage,
        filesystem,
        refresher: Box::new(refresher),
        embeddings: Box::new(wire_embedding_executor_at_root(git.get_root())?),
        markdown: Box::new(MarkdownParser::new()),
        sources: Box::new(
            wire_source_reader(git.get_root()).map_err(|cause| {
                ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause)
            })?,
        ),
        provenance: Box::new(
            wire_source_provenance_with_repository(git).map_err(|cause| {
                ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause)
            })?,
        ),
        graph: Box::new(GraphService::new()),
    })
}

/// Wire live source observations beneath the caller's already-selected Git root.
///
/// Pins the root directory without discovering a repository, opening storage,
/// creating service state, refreshing, pinning Git trees or launching a drain.
/// Each read receives the caller's existing pinned storage view.
///
/// # Errors
/// Reports an inaccessible, symlink or non-directory root.
pub fn wire_source_reader(root: &Path) -> DocumentResult<impl SourceReader + use<>> {
    Ok(SourceService::new(LocalFileSystem::open(root)?))
}

/// Wire read-only source provenance beneath the repository containing `root`.
/// Does not open storage, create service state, retain refs or fetch sources.
///
/// # Errors
/// Reports unsupported repositories or unsafe filesystem roots.
pub fn wire_source_provenance(root: &Path) -> DocumentResult<impl SourceProvenance + use<>> {
    let git = GitRepository::open(root).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open provenance repository",
            cause,
        )
    })?;
    wire_source_provenance_with_repository(git)
}

/// Wire corpus validation with one pinned reader and independent live provenance observations.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths or storage setup failures.
pub fn wire_checker(start: &Path) -> ServiceResult<impl Checker + use<>> {
    let git = GitRepository::open(start)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    wire_checker_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_checker_with_repository(git: GitRepository) -> ServiceResult<impl Checker + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let filesystem = LocalFileSystem::open(git.get_root())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let directory = filesystem
        .open_service_directory()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    Ok(CheckService::new(
        refresher,
        storage,
        GraphDiagnosticsService::new(),
        wire_source_provenance_with_repository(git)
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?,
    ))
}

/// Wire hybrid search with independently owned read, embedding and provenance collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_searcher(start: &Path) -> ServiceResult<impl Searcher + use<>> {
    let git = GitRepository::open(start)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    wire_searcher_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_searcher_with_repository(git: GitRepository) -> ServiceResult<impl Searcher + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let filesystem = LocalFileSystem::open(git.get_root())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let directory = filesystem
        .open_service_directory()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let embeddings = wire_embedding_executor_at_root(git.get_root())?;
    Ok(SearchService {
        refresher,
        storage,
        embeddings,
        filesystem,
        markdown: MarkdownParser::new(),
        provenance: wire_source_provenance_with_repository(git)
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?,
    })
}

/// Wire canonical current knowledge/source inspection for the repository containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths or setup failures.
pub fn wire_inspector(start: &Path) -> ServiceResult<impl Inspector + use<>> {
    let git = GitRepository::open(start)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause))?;
    wire_inspector_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_inspector_with_repository(git: GitRepository) -> ServiceResult<impl Inspector + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause))?;
    let files = LocalFileSystem::open(git.get_root())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause))?;
    Ok(InspectionService::new(
        refresher,
        git,
        files,
        MarkdownParser::new(),
    ))
}

/// Wire document refresh operations for the working copy containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths, or storage/I/O failures.
pub fn wire_document_refresher(start: &Path) -> DocumentResult<impl Refresher + use<>> {
    let git = GitRepository::open(start).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open document repository",
            cause,
        )
    })?;
    wire_document_refresher_with_repository(git)
}

/// Compose document refresh and provenance from the command's shared repository.
fn wire_document_refresher_with_repository(
    git: GitRepository,
) -> DocumentResult<impl Refresher + use<>> {
    let filesystem = LocalFileSystem::open(git.get_root())?;
    let directory = filesystem.open_service_directory()?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot open document storage",
                cause,
            )
        })?;
    let provenance = ProvenanceService::new(git.clone(), LocalFileSystem::open(git.get_root())?);
    Ok(DocumentRefreshService {
        git,
        filesystem,
        markdown: MarkdownParser::new(),
        storage,
        provenance,
    })
}

/// Wire provenance without rediscovering the command's repository.
fn wire_source_provenance_with_repository(
    git: GitRepository,
) -> DocumentResult<impl SourceProvenance + use<>> {
    let filesystem = LocalFileSystem::open(git.get_root())?;
    Ok(ProvenanceService::new(git, filesystem))
}

/// Wire the existing embedding executor with confined automatic repository storage.
/// Callers that serve freshly indexed data refresh through `Refresher` before
/// checking work. Direct drains consume the already published durable projection.
///
/// # Errors
/// Reports unsupported repositories, unsafe service/database paths or setup failures.
pub fn wire_embedding_executor(
    start: &Path,
) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>> {
    let git = GitRepository::open(start)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    wire_embedding_executor_at_root(git.get_root())
}

/// Wire embedding collaborators beneath an already discovered repository root.
fn wire_embedding_executor_at_root(
    root: &Path,
) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>> {
    let files = LocalFileSystem::open(root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    let directory = files
        .open_service_directory()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    let storage = SqliteStorage::open_confined(&root.join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    Ok(EmbeddingService::new(
        storage,
        files,
        MarkdownParser::new(),
        Arc::new(
            build_embedding_provider().map_err(|cause| {
                ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause)
            })?,
        ),
    ))
}

/// Wire background orchestration with an optional embedding executor.
///
/// # Errors
/// Reports an inaccessible or unsafe root directory.
pub fn wire_background_runner<'a>(
    root: &Path,
    executable: &Path,
    tasks: Option<&'a mut dyn EmbeddingExecutor>,
) -> ServiceResult<impl BackgroundRunner + use<'a>> {
    let files = LocalFileSystem::open(root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Background, cause))?;
    Ok(BackgroundService::new(
        tasks,
        files,
        DetachedProcess::new(executable.to_path_buf(), root.to_path_buf()),
    ))
}

/// Wire synchronous embeddings with explicit caller-owned storage.
///
/// # Errors
/// Reports confined filesystem or provider construction failures.
pub fn wire_embedder<S: Storage>(
    root: &Path,
    storage: S,
) -> EmbeddingResult<impl Embedder + EmbeddingExecutor + use<S>> {
    Ok(EmbeddingService::new(
        storage,
        open_embedding_files(root)?,
        MarkdownParser::new(),
        Arc::new(build_embedding_provider()?),
    ))
}

/// Open confined embedding file reads and persistent drain exclusion.
/// The checkout directory is pinned and `.letopis` is protected with mode 0700.
///
/// # Errors
/// Reports unsafe service paths, observed replacements or filesystem failures.
pub fn open_embedding_files(root: &Path) -> EmbeddingResult<impl EmbeddingFiles + use<>> {
    let files = LocalFileSystem::open(root).map_err(map_embedding_files)?;
    files
        .open_service_directory()
        .map_err(map_embedding_files)?;
    Ok(files)
}

/// Construct the synchronous provider pair, capturing compatible credentials once.
/// Does not perform inference or require a key for native Ollama.
///
/// # Errors
/// Reserved for failures constructing the provider boundary.
pub fn build_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + Send + Sync + use<>> {
    let key = env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(Ollama::new(), Openai::new(key)))
}

/// Open immutable Git snapshot operations and read-only root verification at `start`.
/// Requires a UTF-8 checkout root and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
///
/// # Errors
/// Returns a snapshot error when the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(
    start: &Path,
) -> Result<impl GitSnapshots + SnapshotVerifier, SnapshotError> {
    GitRepository::open(start)
}

/// Open existing published storage beneath an already pinned service directory.
/// Does not create database files, bootstrap schemas or synchronize vector identity.
///
/// # Errors
/// Reports unsafe paths, incompatible schemas or database observation failures.
pub fn open_status_storage<F: AsFd>(
    directory: &Path,
    pinned: &F,
) -> StorageResult<impl StatusStorage + use<F>> {
    SqliteStatusStorage::open_confined(directory, pinned)
}

/// Open status observations beneath a pinned checkout without creating service state.
///
/// # Errors
/// Reports an unsafe or inaccessible checkout root.
pub fn open_status_files(root: &Path) -> DocumentResult<impl StatusFiles + use<>> {
    LocalFileSystem::open(root)
}

/// Open the disk cache and independent vector database.
///
/// # Errors
/// Returns an invalid request for aliased or invalid paths, or an internal error
/// for unsupported capabilities, incompatible schemas, contention or I/O errors.
pub fn open_storage(cache: &Path, vectors: &Path) -> StorageResult<impl Storage + use<>> {
    SqliteStorage::open(cache, vectors)
}

/// Open confined regular-file observations and persistent index/embedding locking.
///
/// # Errors
/// Reports an inaccessible or non-directory root, including a symlink root.
pub fn open_filesystem(root: &Path) -> DocumentResult<impl FileSystem + EmbedLocker + use<>> {
    LocalFileSystem::open(root)
}

fn run_process_cli_with<F>(
    args: &[OsString],
    input: impl Read,
    output: &mut dyn Write,
    install: F,
) -> u8
where
    F: FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<SigintListener>,
{
    let writer = Arc::new(ResponseWriter::default());
    let cleanup = Arc::new(GitWriteCleanup::new());
    let Ok(listener) = install_foreground_listener(args, &writer, Arc::clone(&cleanup), install)
    else {
        let error = CommandError::new(
            CommandErrorKind::InternalError,
            "cannot install command cancellation",
        );
        return writer.write_response(output, Err(error));
    };
    let exit = writer.write_response(output, execute_request(args, input, cleanup));
    drop(listener);
    exit
}

fn install_foreground_listener<F>(
    args: &[OsString],
    writer: &Arc<ResponseWriter>,
    cleanup: Arc<GitWriteCleanup>,
    install: F,
) -> io::Result<Option<SigintListener>>
where
    F: FnOnce(Box<dyn FnOnce() + Send>) -> io::Result<SigintListener>,
{
    if !requires_cancellation_handler(args) {
        return Ok(None);
    }
    let writer = Arc::clone(writer);
    let on_interrupt = Box::new(move || {
        let response = Err(CommandError::new(
            CommandErrorKind::Cancelled,
            "command cancelled",
        ));
        let mut output = io::stdout();
        let _ = writer.write_response(&mut output, response);
        let _ = cleanup.cancel_and_cleanup();
    });
    install(on_interrupt).map(Some)
}

fn requires_cancellation_handler(args: &[OsString]) -> bool {
    match args {
        [command] => command
            .to_str()
            .is_some_and(|command| FOREGROUND_COMMANDS.contains(&command)),
        _ => false,
    }
}

fn execute_request(
    args: &[OsString],
    input: impl Read,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    parse_request(args, input).and_then(|request| run_request(request, cleanup))
}

fn map_snapshot_2_service_error(cause: SnapshotError) -> ServiceError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::InvalidRequest => ServiceErrorKind::RepositoryRequired,
        SnapshotErrorKind::NotFound | SnapshotErrorKind::InternalError => {
            ServiceErrorKind::Repository
        }
    };
    ServiceError::new_with_cause(kind, cause)
}

fn map_embedding_files(error: DocumentError) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
}

#[cfg(test)]
#[path = "../tests/support/git.rs"]
mod process_test_support;
