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

use crate::cli::{
    CommandResponse, HandlerServices, Handlers, Request, parse_request, write_response,
};
use crate::embedding::{HttpProviders, Ollama, Openai};
use crate::filesystem::LocalFileSystem;
use crate::git::GitCli;
use crate::markdown::MarkdownParser;
use crate::process::DetachedProcess;
use crate::service::{
    BackgroundRunner, BackgroundService, CheckService, Checker, CommandError, CommandErrorKind,
    CommandResult, CurrentGraphDiagnostics, DocumentError, DocumentErrorKind, DocumentRefresher,
    DocumentResult, EmbedLocker, Embedder, EmbeddingError, EmbeddingErrorKind, EmbeddingExecutor,
    EmbeddingFiles, EmbeddingProvider, EmbeddingResult, EmbeddingService, FileSystem, GitSnapshots,
    GraphDiagnostics, GraphService, InspectionService, Inspector, ProvenanceService,
    RefreshDependencies, Refresher, SearchDependencies, SearchService, Searcher, SnapshotError,
    SnapshotVerifier, SourceProvenance, SourceReader, SourceService, StatusDependencies,
    StatusFiles, StatusService, StatusStorage, Storage, StorageResult, TraversalDependencies,
    TraversalService, Traverser,
};
use crate::storage::{SqliteStatusStorage, SqliteStorage};
use crate::utils::SERVICE_DIRECTORY;
use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    os::fd::AsFd,
    path::{Path, PathBuf},
};

mod cli;
pub mod domain;
mod embedding;
mod filesystem;
mod git;
pub mod markdown;
mod process;
mod storage;
mod utils;

pub mod service;

/// Parse and run one CLI request, emitting one complete JSON envelope.
/// Returns 0 only for successfully written success output, otherwise 1.
/// Check, search, traverse and inspect refresh through their application services.
/// Successful foreground operations may launch the existing detached embedding drain.
pub fn run_cli(
    args: impl IntoIterator<Item = OsString>,
    input: impl Read,
    output: impl Write,
) -> u8 {
    let args = args.into_iter().collect::<Vec<_>>();
    let response = parse_request(&args, input).and_then(run_request);
    write_response(output, response)
}

/// Construct current diagnostics without opening storage or performing I/O.
/// The caller supplies the consistent read view to each operation.
pub fn wire_graph_diagnostics() -> impl GraphDiagnostics {
    CurrentGraphDiagnostics::new()
}

/// Wire bounded traversal with owned graph, source, provenance and readiness collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_traverser(start: &Path) -> CommandResult<impl Traverser + use<>> {
    let refresher = wire_document_refresher(start).map_err(map_traversal_setup)?;
    let git = GitCli::open(start).map_err(map_traversal_setup)?;
    let filesystem = LocalFileSystem::open(git.get_root()).map_err(map_traversal_setup)?;
    let directory = filesystem
        .open_service_directory()
        .map_err(map_traversal_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(map_traversal_setup)?;
    Ok(TraversalService::new(TraversalDependencies {
        storage,
        filesystem,
        refresher: Box::new(refresher),
        embeddings: Box::new(wire_embedding_executor(start)?),
        markdown: Box::new(MarkdownParser::new()),
        sources: Box::new(wire_source_reader(git.get_root()).map_err(map_traversal_setup)?),
        provenance: Box::new(wire_source_provenance(start).map_err(map_traversal_setup)?),
        graph: Box::new(GraphService::new()),
    }))
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
    let git = GitCli::open(root).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open provenance repository",
            cause,
        )
    })?;
    let filesystem = LocalFileSystem::open(git.get_root())?;
    Ok(ProvenanceService::new(git, filesystem))
}

/// Wire corpus validation with one pinned reader and independent live provenance observations.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths or storage setup failures.
pub fn wire_checker(start: &Path) -> CommandResult<impl Checker + use<>> {
    let refresher = wire_document_refresher(start).map_err(map_check_setup)?;
    let git = GitCli::open(start).map_err(map_check_setup)?;
    let filesystem = LocalFileSystem::open(git.get_root()).map_err(map_check_setup)?;
    let directory = filesystem
        .open_service_directory()
        .map_err(map_check_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(map_check_setup)?;
    Ok(CheckService::new(
        refresher,
        storage,
        CurrentGraphDiagnostics::new(),
        ProvenanceService::new(
            GitCli::open(start).map_err(map_check_setup)?,
            LocalFileSystem::open(git.get_root()).map_err(map_check_setup)?,
        ),
    ))
}

/// Wire hybrid search with independently owned read, embedding and provenance collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_searcher(start: &Path) -> CommandResult<impl Searcher + use<>> {
    let refresher = wire_document_refresher(start).map_err(map_search_setup)?;
    let git = GitCli::open(start).map_err(map_search_setup)?;
    let filesystem = LocalFileSystem::open(git.get_root()).map_err(map_search_setup)?;
    let directory = filesystem
        .open_service_directory()
        .map_err(map_search_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(map_search_setup)?;
    let embeddings = wire_embedding_executor(start)?;
    Ok(SearchService::new(SearchDependencies {
        refresher,
        storage,
        embeddings,
        filesystem,
        markdown: MarkdownParser::new(),
        provenance: ProvenanceService::new(
            GitCli::open(start).map_err(map_search_setup)?,
            LocalFileSystem::open(git.get_root()).map_err(map_search_setup)?,
        ),
    }))
}

/// Wire canonical current-knowledge inspection for the repository containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths or setup failures.
pub fn wire_inspector(start: &Path) -> CommandResult<impl Inspector + use<>> {
    let refresher = wire_document_refresher(start).map_err(map_inspection_setup)?;
    let git = GitCli::open(start).map_err(map_inspection_setup)?;
    let files = LocalFileSystem::open(git.get_root()).map_err(map_inspection_setup)?;
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
    let git = GitCli::open(start).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open document repository",
            cause,
        )
    })?;
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
    let provenance = ProvenanceService::new(
        GitCli::open(start).map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot open provenance repository",
                cause,
            )
        })?,
        LocalFileSystem::open(git.get_root())?,
    );
    Ok(DocumentRefresher::new(RefreshDependencies {
        git,
        filesystem,
        markdown: MarkdownParser::new(),
        storage,
        provenance,
    }))
}

/// Wire the existing embedding executor with confined automatic repository storage.
/// Callers that serve freshly indexed data refresh through `Refresher` before
/// checking work. Direct drains consume the already published durable projection.
///
/// # Errors
/// Reports unsupported repositories, unsafe service/database paths or setup failures.
pub fn wire_embedding_executor(
    start: &Path,
) -> CommandResult<impl EmbeddingExecutor + Embedder + use<>> {
    let git = GitCli::open(start).map_err(map_embedding_setup)?;
    let files = LocalFileSystem::open(git.get_root()).map_err(map_embedding_setup)?;
    let directory = files
        .open_service_directory()
        .map_err(map_embedding_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(SERVICE_DIRECTORY), &directory)
        .map_err(map_embedding_setup)?;
    Ok(EmbeddingService::new(
        storage,
        files,
        MarkdownParser::new(),
        build_embedding_provider().map_err(map_embedding_setup)?,
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
) -> CommandResult<impl BackgroundRunner + use<'a>> {
    let files = LocalFileSystem::open(root).map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "cannot open background root",
            cause,
        )
    })?;
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
        build_embedding_provider()?,
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
pub fn build_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>> {
    let key = env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(Ollama::new(), Openai::new(key)))
}

/// Open immutable Git snapshot operations and read-only root verification at `start`.
/// Requires Git 2.29 or newer, a UTF-8 checkout root, and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
///
/// # Errors
/// Returns a snapshot error when Git or the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(
    start: &Path,
) -> Result<impl GitSnapshots + SnapshotVerifier, SnapshotError> {
    GitCli::open(start)
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

fn run_request(request: Request) -> CommandResult<CommandResponse> {
    let (directory, executable) = locate_environment()?;
    match request {
        Request::Status => run_status(request, &directory),
        Request::Check(_) => run_check(request, &directory, &executable),
        Request::Drain => run_drain(request, &directory, &executable),
        Request::Search(_) => run_search(request, &directory, &executable),
        Request::Traverse(_) => run_traverse(request, &directory, &executable),
        Request::Inspect(_) => run_inspect(request, &directory, &executable),
    }
}

fn run_status(request: Request, directory: &Path) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let git = GitCli::open(&root).map_err(map_status_setup)?;
    let files = LocalFileSystem::open(git.get_root()).map_err(map_status_setup)?;
    let service_directory = files
        .open_existing_service_directory()
        .map_err(map_status_setup)?;
    let storage = if let Some(pinned) = service_directory.as_ref() {
        SqliteStatusStorage::open_confined(&root.join(SERVICE_DIRECTORY), pinned)
            .map_err(map_status_setup)?
    } else {
        SqliteStatusStorage::default()
    };
    let provenance = ProvenanceService::new(
        GitCli::open(&root).map_err(map_status_setup)?,
        LocalFileSystem::open(&root).map_err(map_status_setup)?,
    );
    let provider = build_embedding_provider().map_err(map_status_setup)?;
    let graph = CurrentGraphDiagnostics::new();
    let mut status = StatusService::new(StatusDependencies {
        storage,
        files: &files,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    });
    Handlers::new(HandlerServices {
        status: Some(&mut status),
        ..HandlerServices::default()
    })
    .execute(request)
}

fn locate_environment() -> CommandResult<(PathBuf, PathBuf)> {
    let directory = env::current_dir().map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "cannot locate command directory",
            cause,
        )
    })?;
    let executable = env::current_exe().map_err(|cause| {
        CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "cannot locate command executable",
            cause,
        )
    })?;
    Ok((directory, executable))
}

fn run_drain(request: Request, root: &Path, executable: &Path) -> CommandResult<CommandResponse> {
    let mut tasks = wire_embedding_executor(root)?;
    let mut background = wire_background_runner(root, executable, Some(&mut tasks))?;
    Handlers::new(HandlerServices {
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request)
}

fn run_check(
    request: Request,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut checks = wire_checker(&root)?;
    Handlers::new(HandlerServices {
        checks: Some(&mut checks),
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request)
}

fn run_search(
    request: Request,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut searches = wire_searcher(&root)?;
    Handlers::new(HandlerServices {
        searches: Some(&mut searches),
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request)
}

fn run_traverse(
    request: Request,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut traversals = wire_traverser(&root)?;
    Handlers::new(HandlerServices {
        traversals: Some(&mut traversals),
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request)
}

fn run_inspect(
    request: Request,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut inspections = wire_inspector(&root)?;
    Handlers::new(HandlerServices {
        inspections: Some(&mut inspections),
        background: Some(&mut background),
        ..HandlerServices::default()
    })
    .execute(request)
}

/// Foreground services and background exclusion must share the discovered repository root.
fn open_repository_root(directory: &Path) -> CommandResult<PathBuf> {
    let git = GitCli::open(directory).map_err(map_embedding_setup)?;
    Ok(git.get_root().to_path_buf())
}

fn map_check_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open check service",
        error,
    )
}

fn map_status_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open status service",
        error,
    )
}

fn map_search_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open search service",
        error,
    )
}

fn map_traversal_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot prepare traversal",
        error,
    )
}

fn map_inspection_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open inspection service",
        error,
    )
}

fn map_embedding_files(error: DocumentError) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
}

fn map_embedding_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open embedding service",
        error,
    )
}

#[cfg(test)]
#[path = "../tests/support/git.rs"]
mod process_test_support;
