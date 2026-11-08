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
    CommandResponse, ResponseWriter, is_cancellation_handler_required, request_parse,
    response_write,
};
use crate::commands::run_request;
use crate::embedding::HttpProviders;
use crate::filesystem::LocalFileSystem;
use crate::git::{GitRepository, GitWriteCleanup};
use crate::markdown::MarkdownParser;
use crate::process::{DetachedProcess, ProcessInput, SigintListener, SystemClock};
use crate::service::{
    BackgroundRunner, BackgroundRunnerService, CheckService, Checker, CommandError,
    CommandErrorKind, CommandResult, ConfigReader, ConfigurationService, DocumentError,
    DocumentErrorKind, DocumentRefreshService, DocumentResult, Embedder, EmbeddingError,
    EmbeddingErrorKind, EmbeddingExecutor, EmbeddingProvider, EmbeddingResult, EmbeddingService,
    FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult, FileWriter,
    GitSnapshots, GraphDiagnostics, GraphDiagnosticsService, GraphService, InitService,
    Initializer, InspectionService, Inspector, ProvenanceService, Refresher, SearchService,
    Searcher, ServiceError, ServiceErrorKind, ServiceResult, SnapshotError, SnapshotErrorKind,
    SnapshotVerifier, SourceProvenance, SourceReader, SourceService, StatusReader, StatusService,
    StorageError, StorageErrorKind, StorageResult, TraversalService, Traverser,
};
use crate::storage::{CacheStorage, VectorStorage};
use crate::utils::SERVICE_DIRECTORY;
use std::{
    env,
    ffi::OsString,
    io::{self, Read, Write},
    os::fd::{AsFd, OwnedFd},
    path::Path,
    rc::Rc,
    sync::Arc,
    thread,
    time::{Duration, Instant},
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

#[cfg(test)]
thread_local! {
    static BEFORE_DATABASE_OPERATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
#[path = "../tests/support/git.rs"]
mod process_test_support;

#[cfg(test)]
#[path = "lib_test.rs"]
mod tests;

const VECTOR_WRITE_WAIT: Duration = Duration::from_secs(5);

const STORAGE_PATH_WAIT: Duration = Duration::from_secs(5);

const STORAGE_PATH_RETRY: Duration = Duration::from_millis(1);

// Initialization must validate existing configuration before creating database state.
// The function pointer defers composition until InitService supplies its pinned directory.
type StorageInitializer = fn(&Path, &OwnedFd) -> StorageResult<()>;

impl service::InitStorage for StorageInitializer {
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()> {
        self(directory, pinned)
    }
}

/// Runs one CLI request and writes one JSON envelope or plain help text.
/// The exact forms `--help`, `<command> --help` and `--version` are ordinary
/// requests that print plain text without reading `input` or touching any service.
/// Returns 0 only when success output is written in full, otherwise 1.
/// Check, search, traverse and inspect refresh through their application services.
/// Index provides incremental, verified and full publication.
/// Successful foreground operations may start the detached embedding drain.
///
/// # Arguments
///
/// * `args` — process arguments excluding the executable name.
/// * `input` — reader supplying the command request bytes.
/// * `output` — writer that receives the single response envelope.
///
/// # Returns
/// The process exit code of the executed request.
pub fn run_cli(
    args: impl IntoIterator<Item = OsString>,
    input: impl Read,
    output: impl Write,
) -> u8 {
    let args = args.into_iter().collect::<Vec<_>>();
    response_write(
        output,
        execute_request(&args, input, Arc::new(GitWriteCleanup::new())),
    )
}

/// Runs the executable entry path with `SIGINT` handling for foreground commands.
///
/// Registers one listener for the exact `search`, `traverse`, `inspect`, `check`
/// and supported `index` argument forms before stdin or services are used. Other
/// commands keep their default signal behavior. Terminal stdin and a pipe that
/// sends no bytes within 250 ms are treated as empty requests; after the first
/// byte, non-terminal input is read to EOF. A handled `SIGINT` writes one
/// cancellation envelope through the shared response writer and exits 1; normal
/// output keeps its own exit code.
pub fn run_process_cli() -> u8 {
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let mut output = io::stdout();
    let input = ProcessInput::new(io::stdin());
    run_process_cli_with(
        &args,
        input,
        &mut output,
        SigintListener::signal_listener_install,
    )
}

/// Builds diagnostics over the supplied shared cache without performing I/O.
/// Each call joins an active compatible scope or owns a fresh cache read.
///
/// # Arguments
///
/// * `cache` — shared cache database observed through its read scopes.
///
/// # Returns
/// Graph diagnostics over the supplied cache.
pub fn wire_graph_diagnostics<C: service::CacheDatabase>(
    cache: Rc<C>,
) -> impl GraphDiagnostics + use<C> {
    GraphDiagnosticsService::new(cache)
}

/// Runs a callback with a status reader whose collaborators are wired for the call.
///
/// # Arguments
///
/// * `directory` — path where the command was invoked; discovery walks to its work tree.
/// * `operation` — callback that receives the status reader while its
///   collaborators live.
fn with_status_service<T, F>(directory: &Path, operation: F) -> CommandResult<T>
where
    F: FnOnce(&mut dyn StatusReader) -> CommandResult<T>,
{
    let git = open_repository_with_cleanup(directory, Arc::new(GitWriteCleanup::new()))?;
    let root = git.worktree_root_get().to_path_buf();
    let filesystem =
        LocalFileSystem::new(git.worktree_root_get()).map_err(map_error_to_service_status_error)?;
    let service_directory = filesystem
        .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_service_status_error)?;
    let cache = open_status_cache(&root.join(SERVICE_DIRECTORY), service_directory.as_ref())
        .map_err(map_error_to_service_status_error)?;
    let provenance =
        wire_source_provenance_with_repository(git).map_err(map_error_to_service_status_error)?;
    let provider = build_embedding_provider().map_err(map_error_to_service_status_error)?;
    let cache = Rc::new(cache);
    let graph = GraphDiagnosticsService::new(Rc::clone(&cache));
    let mut service = StatusService {
        cache,
        file_reader: &filesystem,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    };
    operation(&mut service)
}

/// Runs a callback with a configuration reader whose collaborators are wired for the call.
///
/// # Arguments
///
/// * `directory` — path where the command was invoked; discovery walks to its work tree.
/// * `operation` — callback invoked with the configuration reader while its
///   collaborators live.
fn with_configuration_service<T, F>(directory: &Path, operation: F) -> CommandResult<T>
where
    F: FnOnce(&dyn ConfigReader) -> CommandResult<T>,
{
    let git = open_repository_with_cleanup(directory, Arc::new(GitWriteCleanup::new()))?;
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(map_error_to_service_configuration_error)?;
    let provider = build_embedding_provider().map_err(map_error_to_service_configuration_error)?;
    let service = ConfigurationService {
        file_reader: &filesystem,
        provider: &provider,
    };
    operation(&service)
}

/// Opens a repository and keeps the given foreground cancellation cleanup owner.
///
/// # Arguments
///
/// * `directory` — path where the command was invoked; discovery walks to its work tree.
/// * `cleanup` — owner of write resources, shared with foreground cancellation
///   handling.
fn open_repository_with_cleanup(
    directory: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<GitRepository> {
    GitRepository::open_with_cleanup(directory, cleanup, &[SERVICE_DIRECTORY])
        .map_err(map_snapshot_error_to_repository_command_error)
}

/// Builds a repository initializer that uses an existing Git work tree and its
/// own filesystem and provider.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired initializer.
///
/// # Errors
/// Returns `invalid_request` outside a non-bare Git work tree and an internal
/// error for repository, filesystem or provider setup failures.
pub fn wire_initializer(start_path: &Path) -> ServiceResult<impl Initializer + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_snapshot_error_to_service_error)?;
    let root = git.worktree_root_get().to_path_buf();
    let filesystem =
        LocalFileSystem::new(&root).map_err(map_error_to_service_initialization_error)?;
    let provider = build_embedding_provider().map_err(map_error_to_service_initialization_error)?;
    Ok(InitService::new(
        Box::new(filesystem),
        Box::new(initialize_storage as StorageInitializer),
        Box::new(provider),
        root.join(SERVICE_DIRECTORY),
    ))
}

/// Builds a traverser with its own graph, source, provenance and embedding
/// collaborators.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired traverser.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe paths or storage and
/// provider setup failures.
pub fn wire_traverser(start_path: &Path) -> ServiceResult<impl Traverser + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_service_traversal_error)?;
    wire_traverser_with_repository(git)
}

/// Builds a traverser from the command's opened repository.
///
/// # Arguments
///
/// * `git` — opened repository shared by the traversal collaborators.
fn wire_traverser_with_repository(git: GitRepository) -> ServiceResult<impl Traverser + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(map_error_to_service_traversal_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_service_traversal_error)?;
    let (vector_database, cache) = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(map_error_to_service_traversal_error)?;
    let cache = Rc::new(cache);
    let vector_database = Rc::new(vector_database);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get())
            .map_err(map_error_to_service_traversal_error)?,
        Rc::clone(&cache),
    )
    .map_err(map_error_to_service_traversal_error)?;
    let embeddings = wire_embedder(git.worktree_root_get(), Rc::clone(&cache), vector_database)
        .map_err(map_error_to_service_embedding_error)?;
    Ok(TraversalService {
        cache: Rc::clone(&cache),
        filesystem,
        refresher: Box::new(refresher),
        embeddings: Box::new(embeddings),
        markdown: Box::new(MarkdownParser::new()),
        sources: Box::new(
            wire_source_reader(git.worktree_root_get(), Rc::clone(&cache))
                .map_err(map_error_to_service_traversal_error)?,
        ),
        provenance: Box::new(
            wire_source_provenance_with_repository(git)
                .map_err(map_error_to_service_traversal_error)?,
        ),
        graph: Box::new(GraphService::new(cache)),
    })
}

/// Builds a source reader for the caller's already selected Git root.
///
/// Pins the root directory without discovering a repository, opening storage,
/// creating service state, refreshing, pinning Git trees or starting a drain.
/// Each observation joins the cache's active scope or owns a fresh read.
///
/// # Arguments
///
/// * `root` — already selected Git worktree root observed by the source reader.
/// * `cache` — shared cache database used by each source observation.
///
/// # Returns
/// The wired source reader.
///
/// # Errors
/// Returns an error when the root is inaccessible, a symlink or not a directory.
pub fn wire_source_reader<C: service::CacheDatabase>(
    root: &Path,
    cache: Rc<C>,
) -> DocumentResult<impl SourceReader + use<C>> {
    Ok(SourceService::new(
        cache,
        LocalFileSystem::new(root).map_err(map_error_to_document_path_changed_error)?,
    ))
}

/// Builds read-only source provenance for the repository containing `root`.
/// Does not open storage, create service state, keep refs or fetch sources.
///
/// # Arguments
///
/// * `root` — path inside the repository whose provenance is observed.
///
/// # Returns
/// The wired source provenance reader.
///
/// # Errors
/// Returns an error for unsupported repositories or unsafe filesystem roots.
pub fn wire_source_provenance(root: &Path) -> DocumentResult<impl SourceProvenance + use<>> {
    let git = GitRepository::open(root, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_provenance_repository_error)?;
    wire_source_provenance_with_repository(git)
}

/// Builds a corpus checker with a pinned reader and live provenance observations.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired checker.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe service paths or storage
/// setup failures.
pub fn wire_checker(start_path: &Path) -> ServiceResult<impl Checker + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_service_check_error)?;
    wire_checker_with_repository(git)
}

/// Builds a corpus checker from the command's opened repository.
///
/// # Arguments
///
/// * `git` — opened repository shared by the check collaborators.
fn wire_checker_with_repository(git: GitRepository) -> ServiceResult<impl Checker + use<>> {
    let filesystem =
        LocalFileSystem::new(git.worktree_root_get()).map_err(map_error_to_service_check_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_service_check_error)?;
    let (_, cache) = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(map_error_to_service_check_error)?;
    let cache = Rc::new(cache);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get()).map_err(map_error_to_service_check_error)?,
        Rc::clone(&cache),
    )
    .map_err(map_error_to_service_check_error)?;
    Ok(CheckService::new(
        refresher,
        Rc::clone(&cache),
        Box::new(GraphDiagnosticsService::new(cache)),
        wire_source_provenance_with_repository(git).map_err(map_error_to_service_check_error)?,
    ))
}

/// Builds a searcher with its own read, embedding and provenance collaborators.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired searcher.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe paths or storage and
/// provider setup failures.
pub fn wire_searcher(start_path: &Path) -> ServiceResult<impl Searcher + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_service_search_error)?;
    wire_searcher_with_repository(git)
}

/// Builds a searcher from the command's opened repository.
///
/// # Arguments
///
/// * `git` — opened repository shared by the search collaborators.
fn wire_searcher_with_repository(git: GitRepository) -> ServiceResult<impl Searcher + use<>> {
    let filesystem =
        LocalFileSystem::new(git.worktree_root_get()).map_err(map_error_to_service_search_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_service_search_error)?;
    let (vector_database, cache) = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(map_error_to_service_search_error)?;
    let cache = Rc::new(cache);
    let vector_database = Rc::new(vector_database);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get()).map_err(map_error_to_service_search_error)?,
        Rc::clone(&cache),
    )
    .map_err(map_error_to_service_search_error)?;
    let embeddings = wire_embedder(git.worktree_root_get(), Rc::clone(&cache), vector_database)
        .map_err(map_error_to_service_embedding_error)?;
    Ok(SearchService {
        refresher,
        cache,
        embeddings,
        filesystem,
        markdown: MarkdownParser::new(),
        provenance: wire_source_provenance_with_repository(git)
            .map_err(map_error_to_service_search_error)?,
    })
}

/// Builds an inspector for knowledge and source metadata in the repository
/// containing `start_path`.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired inspector.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe service paths or setup
/// failures.
pub fn wire_inspector(start_path: &Path) -> ServiceResult<impl Inspector + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_service_inspection_error)?;
    wire_inspector_with_repository(git)
}

/// Builds an inspector from the command's opened repository.
///
/// # Arguments
///
/// * `git` — opened repository shared by the inspection collaborators.
fn wire_inspector_with_repository(git: GitRepository) -> ServiceResult<impl Inspector + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(map_error_to_service_inspection_error)?;
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(map_error_to_service_inspection_error)?;
    Ok(InspectionService::new(
        refresher,
        git,
        filesystem,
        MarkdownParser::new(),
    ))
}

/// Builds a document refresher for the working copy containing `start_path`.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired document refresher.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe service paths or storage
/// and I/O failures.
pub fn wire_document_refresher(start_path: &Path) -> DocumentResult<impl Refresher + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_document_repository_error)?;
    wire_document_refresher_with_repository(git)
}

/// Builds a document refresher from the command's opened repository.
///
/// # Arguments
///
/// * `git` — opened repository shared by the refresh collaborators.
fn wire_document_refresher_with_repository(
    git: GitRepository,
) -> DocumentResult<impl Refresher + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(map_error_to_document_path_changed_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_document_path_changed_error)?;
    let (_, cache) = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(map_error_to_document_storage_error)?;
    wire_document_refresher_with_storage(git, filesystem, Rc::new(cache))
}

fn wire_document_refresher_with_storage(
    git: GitRepository,
    filesystem: LocalFileSystem,
    cache: Rc<storage::CacheStorage>,
) -> DocumentResult<impl Refresher + use<>> {
    filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_document_path_changed_error)?;
    let provenance = ProvenanceService::new(
        Box::new(git.clone()),
        LocalFileSystem::new(git.worktree_root_get())
            .map_err(map_error_to_document_path_changed_error)?,
    );
    Ok(DocumentRefreshService {
        git,
        filesystem,
        clock: Box::new(SystemClock::new()),
        markdown: MarkdownParser::new(),
        cache,
        provenance,
    })
}

/// Builds provenance from the command's already opened repository.
///
/// # Arguments
///
/// * `git` — already opened repository observed by the provenance service.
fn wire_source_provenance_with_repository(
    git: GitRepository,
) -> DocumentResult<impl SourceProvenance + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(map_error_to_document_path_changed_error)?;
    Ok(ProvenanceService::new(Box::new(git), filesystem))
}

/// Builds an embedding executor with storage confined to the repository containing
/// `start_path`.
/// Callers that serve freshly indexed data refresh through `Refresher` before
/// checking work. Direct drains use the already published stored data.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// # Returns
/// The wired embedding executor.
///
/// # Errors
/// Returns an error for unsupported repositories, unsafe service or database paths,
/// or setup failures.
pub fn wire_embedding_executor(
    start_path: &Path,
) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>> {
    let git = GitRepository::open(start_path, &[SERVICE_DIRECTORY])
        .map_err(map_error_to_service_embedding_error)?;
    wire_embedding_executor_at_root(git.worktree_root_get())
}

/// Builds an embedding executor beneath an already discovered repository root.
///
/// # Arguments
///
/// * `root` — already discovered worktree root whose service storage is opened.
fn wire_embedding_executor_at_root(
    root: &Path,
) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>> {
    let filesystem = LocalFileSystem::new(root).map_err(map_error_to_service_embedding_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_service_embedding_error)?;
    let (vector_database, cache) =
        crate::open_confined_databases(&root.join(SERVICE_DIRECTORY), &directory)
            .map_err(map_error_to_service_embedding_error)?;
    Ok(EmbeddingService::new(
        Rc::new(cache),
        Rc::new(vector_database),
        filesystem,
        Box::new(SystemClock::new()),
        MarkdownParser::new(),
        build_embedding_provider().map_err(map_error_to_service_embedding_error)?,
    ))
}

/// Builds a background runner with an optional embedding executor.
///
/// # Arguments
///
/// * `root` — worktree root whose service directory is validated and used by the drain.
/// * `executable` — current executable used to launch the detached embedding drain.
/// * `tasks` — optional foreground embedding executor reused by the detached drain.
///
/// # Returns
/// The wired background runner.
///
/// # Errors
/// Returns an error for an inaccessible or unsafe root directory.
pub fn wire_background_runner<'a>(
    root: &Path,
    executable: &Path,
    tasks: Option<&'a mut dyn EmbeddingExecutor>,
) -> ServiceResult<impl BackgroundRunner + use<'a>> {
    let filesystem = LocalFileSystem::new(root).map_err(map_error_to_service_background_error)?;
    Ok(BackgroundRunnerService::new(
        tasks,
        filesystem,
        DetachedProcess::new(
            executable.to_path_buf(),
            vec!["embed".into(), "--drain".into()],
            root.to_path_buf(),
        ),
    ))
}

/// Builds a synchronous embedder over the caller's cache and vector databases.
///
/// # Arguments
///
/// * `root` — worktree root under which the service directory is created.
/// * `cache` — caller-owned cache database shared by the embedder.
/// * `vector_database` — caller-owned vector database shared by the embedder.
///
/// # Returns
/// The wired embedder.
///
/// # Errors
/// Returns an error for filesystem or provider setup failures.
pub fn wire_embedder<C: service::CacheDatabase, V: service::VectorDatabase>(
    root: &Path,
    cache: Rc<C>,
    vector_database: Rc<V>,
) -> EmbeddingResult<impl Embedder + EmbeddingExecutor + use<C, V>> {
    let filesystem = LocalFileSystem::new(root).map_err(map_error_to_embedding_internal_error)?;
    filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_error_to_embedding_internal_error)?;
    Ok(EmbeddingService::new(
        cache,
        vector_database,
        filesystem,
        Box::new(SystemClock::new()),
        MarkdownParser::new(),
        build_embedding_provider()?,
    ))
}

/// Builds the embedding provider and reads the `OpenAI` key once.
/// Does not perform inference or require a key for native Ollama.
///
/// # Returns
/// The configured embedding provider.
///
/// # Errors
/// Reserved for provider construction failures.
pub fn build_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>> {
    let key = env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(key))
}

/// Opens Git snapshots and read-only root verification at `start_path`.
///
/// # Arguments
///
/// * `start_path` — path where the command was invoked; discovery walks to its work tree.
///
/// Requires a UTF-8 checkout root and a SHA-1 or SHA-256 repository without an
/// object alternates path (even empty or inactive). Alternates are checked again
/// before each operation; use a self-contained clone. Tree creation rejects
/// `.letopis` as the first path component before writing any object; addressed
/// historical reads and retention do not apply that creation policy.
///
/// # Returns
/// Git snapshots with read-only root verification.
///
/// # Errors
/// Returns a snapshot error when the repository is unsupported, the checkout root
/// cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(
    start_path: &Path,
) -> Result<impl GitSnapshots + SnapshotVerifier, SnapshotError> {
    GitRepository::open(start_path, &[SERVICE_DIRECTORY])
}

/// Opens the existing published databases in an already pinned service directory.
/// Does not create database files, bootstrap schemas or synchronize vector identity.
///
/// # Arguments
///
/// * `directory` — already pinned service directory containing the published databases.
/// * `pinned` — pinned directory descriptor compared against the observed storage paths.
///
/// # Returns
/// Cache storage over the published databases.
///
/// # Errors
/// Returns an error for unsafe paths, incompatible schemas or database read
/// failures.
pub fn open_status_storage<F: AsFd>(
    directory: &Path,
    pinned: &F,
) -> StorageResult<impl service::CacheDatabase + use<F>> {
    crate::open_observed_cache(directory, pinned)
}

/// Opens the cache and vector databases at the given paths.
///
/// # Arguments
///
/// * `cache_path` — path of the cache database file.
/// * `vectors_path` — path of the vector database file.
///
/// # Returns
/// The vector storage and cache database together.
///
/// # Errors
/// Returns an invalid request for invalid or aliased paths, or an internal error
/// for unsupported capabilities, incompatible schemas, contention or I/O errors.
pub fn open_storage(
    cache_path: &Path,
    vectors_path: &Path,
) -> StorageResult<(
    impl service::VectorDatabase + use<>,
    impl service::CacheDatabase + use<>,
)> {
    crate::open_sqlite_databases(cache_path, vectors_path)
}

/// Opens the cache database and attaches the vector database for reads.
///
/// # Arguments
///
/// * `cache_path` — path of the cache database file.
/// * `vectors_path` — path of the vector database file.
///
/// # Returns
/// Cache storage with the vector database attached for reads.
///
/// # Errors
/// Returns an error for unsafe or aliased paths, unsupported schemas, native
/// capabilities or I/O.
pub fn open_cache_storage(
    cache_path: &Path,
    vectors_path: &Path,
) -> StorageResult<impl service::CacheDatabase + use<>> {
    open_sqlite_databases(cache_path, vectors_path).map(|storage| storage.1)
}

/// Opens confined file operations and persistent locks beneath `root`.
///
/// # Arguments
///
/// * `root` — directory that confines all file and lock operations.
///
/// # Returns
/// Confined file operations and persistent locks.
///
/// # Errors
/// Returns an error for an inaccessible or non-directory root, including a symlink
/// root.
pub fn open_filesystem(root: &Path) -> FileResult<impl FileWriter + FileLocker + use<>> {
    LocalFileSystem::new(root)
}

fn initialize_storage(directory: &Path, pinned: &OwnedFd) -> StorageResult<()> {
    open_confined_databases(directory, pinned).map(drop)
}

fn open_sqlite_databases(
    cache_path: &Path,
    vectors_path: &Path,
) -> StorageResult<(VectorStorage, CacheStorage)> {
    let [cache_path, vectors_path] = resolve_database_paths(cache_path, vectors_path)?;
    let mut cache_writer = storage::open_database(&cache_path)?;
    storage::bootstrap_cache(&mut cache_writer)?;
    let mut vector_writer = storage::open_database(&vectors_path)?;
    storage::bootstrap_vectors(&mut vector_writer)?;
    storage::attach_read_only_vectors(&cache_writer, &vectors_path)?;
    vector_writer
        .busy_timeout(VECTOR_WRITE_WAIT)
        .map_err(map_error_to_storage_internal_error)?;
    storage::attach_cache(&vector_writer, &cache_path)?;
    let read = storage::open_database(&cache_path)?;
    storage::attach_vectors(&read, &vectors_path)?;
    Ok((
        VectorStorage::new(vector_writer),
        CacheStorage::new(read, cache_writer),
    ))
}

fn open_confined_databases(
    directory: &Path,
    pinned: &impl AsFd,
) -> StorageResult<(VectorStorage, CacheStorage)> {
    let mut cache_writer = open_confined_database(directory, pinned, "cache.db")?;
    storage::bootstrap_cache(&mut cache_writer)?;
    let mut vector_writer = open_confined_database(directory, pinned, "vectors.db")?;
    storage::bootstrap_vectors(&mut vector_writer)?;
    if !attach_observed_vectors(directory, pinned, &cache_writer)? {
        return Err(service::StorageError::new(
            service::StorageErrorKind::InternalError,
            "vector database disappeared during open",
        ));
    }
    vector_writer
        .busy_timeout(VECTOR_WRITE_WAIT)
        .map_err(map_error_to_storage_internal_error)?;
    attach_confined_cache(directory, pinned, &vector_writer)?;
    let read = open_confined_database(directory, pinned, "cache.db")?;
    attach_confined_vectors(directory, pinned, &read)?;
    Ok((
        VectorStorage::new(vector_writer),
        CacheStorage::new(read, cache_writer),
    ))
}

fn open_status_cache(directory: &Path, pinned: Option<&OwnedFd>) -> StorageResult<CacheStorage> {
    match pinned {
        Some(pinned) => open_observed_cache(directory, pinned),
        None => Ok(CacheStorage::new_observed(None)),
    }
}

fn open_observed_cache(directory: &Path, pinned: &impl AsFd) -> StorageResult<CacheStorage> {
    let Some(read) = get_observed_database(directory, pinned, "cache.db")? else {
        if let Some(vector_connection) = get_observed_database(directory, pinned, "vectors.db")? {
            storage::validate_vector_database(&vector_connection)?;
        }
        return Ok(CacheStorage::new_observed(None));
    };
    storage::validate_cache(&read)?;
    if attach_observed_vectors(directory, pinned, &read)? {
        storage::validate_vectors(&read)?;
    } else {
        storage::attach_empty_vectors(&read)?;
    }
    Ok(CacheStorage::new_observed(Some(read)))
}

fn open_confined_database(
    directory: &Path,
    pinned: &impl AsFd,
    name: &str,
) -> StorageResult<rusqlite::Connection> {
    let connection = with_verified_database_paths(directory, pinned, |_, _| {
        storage::open_confined_database(&directory.join(name))
    })?;
    storage::configure_connection(&connection)?;
    Ok(connection)
}

fn get_observed_database(
    directory: &Path,
    pinned: &impl AsFd,
    name: &str,
) -> StorageResult<Option<rusqlite::Connection>> {
    with_verified_database_paths(directory, pinned, |cache_identity, vectors_identity| {
        let identity = match name {
            "cache.db" => cache_identity,
            "vectors.db" => vectors_identity,
            _ => {
                return Err(StorageError::new(
                    StorageErrorKind::InvalidRequest,
                    "unsupported storage database",
                ));
            }
        };
        if identity.is_none() {
            return Ok(None);
        }
        let connection = storage::open_observed_database(&directory.join(name))?;
        Ok(Some(connection))
    })
}

fn attach_observed_vectors(
    directory: &Path,
    pinned: &impl AsFd,
    connection: &rusqlite::Connection,
) -> StorageResult<bool> {
    with_verified_database_paths(directory, pinned, |_, vectors_identity| {
        if vectors_identity.is_none() {
            return Ok(false);
        }
        storage::attach_read_only_vectors(connection, &directory.join("vectors.db"))?;
        Ok(true)
    })
}

fn attach_confined_vectors(
    directory: &Path,
    pinned: &impl AsFd,
    connection: &rusqlite::Connection,
) -> StorageResult<()> {
    with_verified_database_paths(directory, pinned, |_, _| {
        storage::attach_unconfigured_vectors(connection, &directory.join("vectors.db"))
    })?;
    storage::configure_vectors(connection)
}

fn attach_confined_cache(
    directory: &Path,
    pinned: &impl AsFd,
    connection: &rusqlite::Connection,
) -> StorageResult<()> {
    with_verified_database_paths(directory, pinned, |_, _| {
        storage::attach_cache(connection, &directory.join("cache.db"))
    })
}

fn with_verified_database_paths<T, F>(
    directory: &Path,
    pinned: &impl AsFd,
    operation: F,
) -> StorageResult<T>
where
    F: FnOnce(Option<FileIdentity>, Option<FileIdentity>) -> StorageResult<T>,
{
    let filesystem = LocalFileSystem::new(directory)
        .map_err(map_error_to_document_storage_path_storage_error)?;
    let (cache_identity, vectors_identity) = wait_for_stable_storage_paths(|| {
        verify_storage_directory(&filesystem, pinned)?;
        let cache_identity = filesystem
            .file_get_identity_by_path(Path::new("cache.db"))
            .map_err(map_file_error_to_storage_error)?;
        let vectors_identity = filesystem
            .file_get_identity_by_path(Path::new("vectors.db"))
            .map_err(map_file_error_to_storage_error)?;
        verify_storage_files(&filesystem, cache_identity, vectors_identity)?;
        Ok((cache_identity, vectors_identity))
    })?;
    #[cfg(test)]
    if let Some(hook) = BEFORE_DATABASE_OPERATION.take() {
        hook();
    }
    let result = operation(cache_identity, vectors_identity);
    wait_for_stable_storage_paths(|| {
        verify_storage_directory(&filesystem, pinned)?;
        verify_storage_files(&filesystem, cache_identity, vectors_identity)
    })?;
    result
}

/// Repeats a path check while another process is creating a database file.
///
/// A presence flip between two file checks is reported as a transient `Changed`
/// error. A replacement that stays different still exhausts the time limit.
///
/// # Arguments
///
/// * `operation` — check retried while it reports a transient `Changed` error.
fn wait_for_stable_storage_paths<T>(
    mut operation: impl FnMut() -> StorageResult<T>,
) -> StorageResult<T> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_storage_path_change(&error) && started.elapsed() < STORAGE_PATH_WAIT =>
            {
                thread::sleep(STORAGE_PATH_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_storage_path_change(error: &StorageError) -> bool {
    std::error::Error::source(error)
        .and_then(|cause| cause.downcast_ref::<FileError>())
        .is_some_and(|cause| cause.get_kind() == FileErrorKind::Changed)
}

fn verify_storage_directory(file_reader: &dyn FileReader, pinned: &impl AsFd) -> StorageResult<()> {
    let current = file_reader
        .directory_get_by_path(Path::new("."))
        .map_err(|cause| {
            let message = if cause.get_kind() == FileErrorKind::Changed {
                "document storage directory changed"
            } else {
                "cannot validate document storage path"
            };
            StorageError::new_with_cause(StorageErrorKind::InvalidRequest, message, cause)
        })?
        .ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "document storage directory changed",
            )
        })?;
    let current =
        rustix::fs::fstat(&current).map_err(map_error_to_document_storage_path_storage_error)?;
    let pinned =
        rustix::fs::fstat(pinned).map_err(map_error_to_document_storage_path_storage_error)?;
    if !is_same_identity(&current, &pinned) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "document storage directory changed",
        ));
    }
    Ok(())
}

/// Returns whether two file observations have the same device and inode.
///
/// # Arguments
///
/// * `left` — first file status observation.
/// * `right` — second file status observation.
fn is_same_identity(left: &rustix::fs::Stat, right: &rustix::fs::Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn verify_storage_files(
    file_reader: &dyn FileReader,
    cache_identity: Option<FileIdentity>,
    vectors_identity: Option<FileIdentity>,
) -> StorageResult<()> {
    file_reader
        .file_verify_identity_by_path(Path::new("cache.db"), cache_identity)
        .map_err(map_file_error_to_storage_error)?;
    file_reader
        .file_verify_identity_by_path(Path::new("vectors.db"), vectors_identity)
        .map_err(map_file_error_to_storage_error)?;
    file_reader
        .file_verify_distinct_by_paths(Path::new("cache.db"), Path::new("vectors.db"))
        .map_err(map_file_error_to_storage_error)
}

fn map_file_error_to_storage_error(cause: FileError) -> StorageError {
    let message = match cause.get_kind() {
        FileErrorKind::Changed => "document storage database changed",
        FileErrorKind::Aliased => "storage databases must be distinct files",
        FileErrorKind::UnsafePath | FileErrorKind::NonRegular => "unsafe document storage path",
        _ => "cannot validate document storage path",
    };
    StorageError::new_with_cause(StorageErrorKind::InvalidRequest, message, cause)
}

fn resolve_database_paths(
    cache_path: &Path,
    vectors_path: &Path,
) -> StorageResult<[std::path::PathBuf; 2]> {
    for path in [cache_path, vectors_path] {
        if path.to_str().is_none() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            ));
        }
    }
    let paths = LocalFileSystem::file_get_distinct_paths_by_paths(cache_path, vectors_path)
        .map_err(|cause| {
            let message = match cause.get_kind() {
                FileErrorKind::Aliased => "storage databases must be distinct files",
                FileErrorKind::UnsafePath | FileErrorKind::NonRegular => {
                    "invalid storage database path"
                }
                _ => "storage parent or file is unavailable",
            };
            StorageError::new_with_cause(StorageErrorKind::InvalidRequest, message, cause)
        })?;
    if paths.iter().any(|path| path.to_str().is_none()) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage database path",
        ));
    }
    Ok(paths)
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
        return writer.response_write(output, Err(error));
    };
    let exit = writer.response_write(output, execute_request(args, input, cleanup));
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
    if !is_cancellation_handler_required(args) {
        return Ok(None);
    }
    let writer = Arc::clone(writer);
    let on_interrupt = Box::new(move || {
        let response = Err(CommandError::new(
            CommandErrorKind::Cancelled,
            "command cancelled",
        ));
        let mut output = io::stdout();
        let _ = writer.response_write(&mut output, response);
        let _ = cleanup.shutdown();
    });
    install(on_interrupt).map(Some)
}

fn execute_request(
    args: &[OsString],
    input: impl Read,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    request_parse(args, input).and_then(|request| run_request(request, cleanup))
}

fn map_snapshot_error_to_service_error(cause: SnapshotError) -> ServiceError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::InvalidRequest => ServiceErrorKind::RepositoryRequired,
        SnapshotErrorKind::NotFound | SnapshotErrorKind::InternalError => {
            ServiceErrorKind::Repository
        }
    };
    ServiceError::new_with_cause(kind, cause)
}

fn map_error_to_service_status_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Status, cause)
}

fn map_error_to_service_configuration_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Configuration, cause)
}

fn map_error_to_service_initialization_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Initialization, cause)
}

fn map_error_to_service_traversal_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause)
}

fn map_error_to_service_embedding_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause)
}

fn map_error_to_service_check_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Check, cause)
}

fn map_error_to_service_search_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Search, cause)
}

fn map_error_to_service_inspection_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause)
}

fn map_error_to_service_background_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> ServiceError {
    ServiceError::new_with_cause(ServiceErrorKind::Background, cause)
}

fn map_snapshot_error_to_repository_command_error(cause: SnapshotError) -> CommandError {
    ServiceError::new_with_cause(ServiceErrorKind::Repository, cause).into()
}

fn map_error_to_document_path_changed_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "document path changed during observation",
        cause,
    )
}

fn map_error_to_provenance_repository_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "cannot open provenance repository",
        cause,
    )
}

fn map_error_to_document_repository_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "cannot open document repository",
        cause,
    )
}

fn map_error_to_document_storage_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "cannot open document storage",
        cause,
    )
}

fn map_error_to_embedding_internal_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
}

fn map_error_to_storage_internal_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        storage::STORAGE_FAILURE,
        cause,
    )
}

fn map_error_to_document_storage_path_storage_error<
    E: std::error::Error + Send + Sync + 'static,
>(
    cause: E,
) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InvalidRequest,
        "cannot validate document storage path",
        cause,
    )
}
