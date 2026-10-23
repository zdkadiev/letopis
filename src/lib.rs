//! Read-only knowledge graph foundations for Letopis.

use crate::embedding::{HttpProviders, Ollama, Openai};
use crate::filesystem::LocalFileSystem;
use crate::git::GitCli;
use crate::markdown::MarkdownParser;
use crate::process::DetachedProcess;
use crate::service::{
    Background, BackgroundService, CommandError, CommandErrorKind, CommandResult, DocumentError,
    DocumentErrorKind, DocumentResult, DocumentService, Documents, EmbedLocks, EmbeddingError,
    EmbeddingErrorKind, EmbeddingFiles, EmbeddingProvider, EmbeddingResult, EmbeddingService,
    EmbeddingTasks, Embeddings, FileSystem, GitSnapshots, InspectionService, Inspections,
    SearchDependencies, SearchService, Searches, SnapshotError, Storage, StorageResult,
};
use crate::storage::SqliteStorage;
use std::{
    env,
    ffi::OsString,
    io::{Read, Write},
    path::Path,
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
/// Check and inspect remain unavailable; drain uses the real embedding executor.
pub fn run_cli(
    args: impl IntoIterator<Item = OsString>,
    input: &mut impl Read,
    output: &mut impl Write,
) -> u8 {
    let args = args.into_iter().collect::<Vec<_>>();
    let response = cli::parse_request(&args, input).and_then(|request| {
        let root = env::current_dir().map_err(|cause| {
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
        if !matches!(request, cli::Request::Drain) {
            return cli::Handlers::new(None, None, None).execute(request);
        }
        let mut tasks = open_embedding_tasks(&root)?;
        let mut background = open_background(&root, &executable, Some(&mut tasks))?;
        cli::Handlers::new(None, None, Some(&mut background)).execute(request)
    });
    cli::write_response(output, response)
}

/// Open hybrid knowledge search with independently owned read and embedding connections.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn open_searches(start: &Path) -> CommandResult<impl Searches + use<>> {
    let documents = open_documents(start).map_err(map_search_setup)?;
    let git = GitCli::open(start).map_err(map_search_setup)?;
    let filesystem = LocalFileSystem::open(git.get_root()).map_err(map_search_setup)?;
    let directory = filesystem
        .open_service_directory()
        .map_err(map_search_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory)
        .map_err(map_search_setup)?;
    let embeddings = open_embedding_tasks(start)?;
    Ok(SearchService::new(SearchDependencies {
        documents,
        storage,
        embeddings,
        filesystem,
        markdown: MarkdownParser::new(),
    }))
}

/// Open canonical current-knowledge inspection for the repository containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths or setup failures.
pub fn open_inspections(start: &Path) -> CommandResult<impl Inspections + use<>> {
    let documents = open_documents(start).map_err(map_inspection_setup)?;
    let git = GitCli::open(start).map_err(map_inspection_setup)?;
    let files = LocalFileSystem::open(git.get_root()).map_err(map_inspection_setup)?;
    Ok(InspectionService::new(
        documents,
        git,
        files,
        MarkdownParser::new(),
    ))
}

/// Open document refresh operations for the working copy containing `start`.
///
/// # Errors
/// Reports unsupported repositories, unsafe service paths, or storage/I/O failures.
pub fn open_documents(start: &Path) -> DocumentResult<impl Documents + use<>> {
    let git = GitCli::open(start).map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open document repository",
            cause,
        )
    })?;
    let filesystem = LocalFileSystem::open(git.get_root())?;
    let directory = filesystem.open_service_directory()?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory)
        .map_err(|cause| {
            DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot open document storage",
                cause,
            )
        })?;
    Ok(DocumentService::new(
        git,
        filesystem,
        MarkdownParser::new(),
        storage,
    ))
}

/// Open the existing embedding executor with confined automatic repository storage.
/// Callers that serve freshly indexed data refresh through `Documents` before
/// checking work. Direct drains consume the already published durable projection.
///
/// # Errors
/// Reports unsupported repositories, unsafe service/database paths or setup failures.
pub fn open_embedding_tasks(
    start: &Path,
) -> CommandResult<impl EmbeddingTasks + Embeddings + use<>> {
    let git = GitCli::open(start).map_err(map_embedding_setup)?;
    let files = LocalFileSystem::open(git.get_root()).map_err(map_embedding_setup)?;
    let directory = files
        .open_service_directory()
        .map_err(map_embedding_setup)?;
    let storage = SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory)
        .map_err(map_embedding_setup)?;
    Ok(EmbeddingService::new(
        storage,
        files,
        MarkdownParser::new(),
        open_embedding_provider().map_err(map_embedding_setup)?,
    ))
}

/// Open background orchestration with an optional embedding executor.
///
/// # Errors
/// Reports an inaccessible or unsafe root directory.
pub fn open_background<'a>(
    root: &Path,
    executable: &Path,
    tasks: Option<&'a mut dyn EmbeddingTasks>,
) -> CommandResult<impl Background + use<'a>> {
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

/// Assemble synchronous embeddings with explicit caller-owned storage.
///
/// # Errors
/// Reports confined filesystem or provider construction failures.
pub fn open_embeddings<S: Storage>(
    root: &Path,
    storage: S,
) -> EmbeddingResult<impl Embeddings + EmbeddingTasks + use<S>> {
    Ok(EmbeddingService::new(
        storage,
        open_embedding_files(root)?,
        MarkdownParser::new(),
        open_embedding_provider()?,
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
pub fn open_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>> {
    let key = std::env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(Ollama::new(), Openai::new(key)))
}

/// Open immutable Git snapshot operations at the working copy containing `start`.
/// Requires Git 2.29 or newer, a UTF-8 checkout root, and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
///
/// # Errors
/// Returns a snapshot error when Git or the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(start: &Path) -> Result<impl GitSnapshots, SnapshotError> {
    GitCli::open(start)
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
pub fn open_filesystem(root: &Path) -> DocumentResult<impl FileSystem + EmbedLocks + use<>> {
    LocalFileSystem::open(root)
}

fn map_search_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open search service",
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
