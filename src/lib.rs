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
    CommandResponse, ResponseWriter, get_index_mode_by_args, request_parse, response_write,
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

const VECTOR_WRITE_WAIT: Duration = Duration::from_secs(5);
const STORAGE_PATH_WAIT: Duration = Duration::from_secs(5);
const STORAGE_PATH_RETRY: Duration = Duration::from_millis(1);

const FOREGROUND_COMMANDS: [&str; 4] = ["search", "traverse", "inspect", "check"];

/// Parse and run one CLI request, emitting one complete JSON envelope.
/// The exact forms `--help`, `<command> --help` and `--version` are ordinary
/// requests that print plain text without reading `input` or touching any service.
/// Returns 0 only for successfully written success output, otherwise 1.
/// Check, search, traverse and inspect refresh through their application services.
/// Index provides incremental, verified and full publication.
/// Successful foreground operations may launch the existing detached embedding drain.
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

/// Run the executable entry path with scoped foreground SIGINT handling.
///
/// Registers one listener for the exact `search`, `traverse`, `inspect`, `check`
/// and supported `index` argv shapes before stdin or services are used. Other commands
/// keep their default signal behavior. Terminal stdin and a pipe that sends no
/// bytes within 250 ms are treated as empty requests; after the first byte,
/// non-terminal input is read to EOF. A handled SIGINT writes one cancellation
/// envelope through the shared response owner and exits 1; normal output retains
/// its own exit code.
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

/// Construct diagnostics using the supplied shared cache without performing I/O.
/// Calls join an active compatible scope or own a fresh cache read.
pub fn wire_graph_diagnostics<C: service::CacheDatabase>(
    storage: Rc<C>,
) -> impl GraphDiagnostics + use<C> {
    GraphDiagnosticsService::new(storage)
}

/// Wire a status reader for the duration of a callback borrowing its collaborators.
fn with_status_service<T, F>(directory: &Path, operation: F) -> CommandResult<T>
where
    F: FnOnce(&mut dyn StatusReader) -> CommandResult<T>,
{
    let git = open_repository_with_cleanup(directory, Arc::new(GitWriteCleanup::new()))?;
    let root = git.worktree_root_get().to_path_buf();
    let files = LocalFileSystem::new(git.worktree_root_get())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Status, cause))?;
    let service_directory = files
        .directory_get_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Status, cause))?;
    let storage = open_status_cache(&root.join(SERVICE_DIRECTORY), service_directory.as_ref())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Status, cause))?;
    let provenance = wire_source_provenance_with_repository(git)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Status, cause))?;
    let provider = build_embedding_provider()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Status, cause))?;
    let storage = Rc::new(storage);
    let graph = GraphDiagnosticsService::new(Rc::clone(&storage));
    let mut service = StatusService {
        storage,
        files: &files,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    };
    operation(&mut service)
}

/// Wire a configuration reader for the duration of a callback borrowing its collaborators.
fn with_configuration_service<T, F>(directory: &Path, operation: F) -> CommandResult<T>
where
    F: FnOnce(&dyn ConfigReader) -> CommandResult<T>,
{
    let git = open_repository_with_cleanup(directory, Arc::new(GitWriteCleanup::new()))?;
    let files = LocalFileSystem::new(git.worktree_root_get())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Configuration, cause))?;
    let provider = build_embedding_provider()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Configuration, cause))?;
    let service = ConfigurationService {
        files: &files,
        provider: &provider,
    };
    operation(&service)
}

/// Discover a repository while retaining the foreground cancellation cleanup owner.
fn open_repository_with_cleanup(
    directory: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<GitRepository> {
    GitRepository::open_with_cleanup(directory, cleanup, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Repository, cause).into())
}

/// Wire repository initialization with noncreating Git discovery and owned adapters.
///
/// # Errors
/// Returns `invalid_request` outside a non-bare Git work tree, and a safe internal
/// error for repository, filesystem or provider setup failures.
pub fn wire_initializer(start: &Path) -> ServiceResult<impl Initializer + use<>> {
    let git =
        GitRepository::open(start, &[SERVICE_DIRECTORY]).map_err(map_snapshot_2_service_error)?;
    let root = git.worktree_root_get().to_path_buf();
    let files = LocalFileSystem::new(&root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Initialization, cause))?;
    let provider = build_embedding_provider()
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Initialization, cause))?;
    Ok(InitService::new(
        Box::new(files),
        Box::new(initialize_storage as StorageInitializer),
        Box::new(provider),
        root.join(SERVICE_DIRECTORY),
    ))
}

/// Wire bounded traversal with owned graph, source, provenance and readiness collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_traverser(start: &Path) -> ServiceResult<impl Traverser + use<>> {
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    wire_traverser_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_traverser_with_repository(git: GitRepository) -> ServiceResult<impl Traverser + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let storage = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let (vectors, cache) = storage;
    let storage = Rc::new(cache);
    let vectors = Rc::new(vectors);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get())
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?,
        Rc::clone(&storage),
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause))?;
    let embeddings = wire_embedder(git.worktree_root_get(), Rc::clone(&storage), vectors)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    Ok(TraversalService {
        storage: Rc::clone(&storage),
        filesystem,
        refresher: Box::new(refresher),
        embeddings: Box::new(embeddings),
        markdown: Box::new(MarkdownParser::new()),
        sources: Box::new(
            wire_source_reader(git.worktree_root_get(), Rc::clone(&storage)).map_err(|cause| {
                ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause)
            })?,
        ),
        provenance: Box::new(
            wire_source_provenance_with_repository(git).map_err(|cause| {
                ServiceError::new_with_cause(ServiceErrorKind::Traversal, cause)
            })?,
        ),
        graph: Box::new(GraphService::new(storage)),
    })
}

/// Wire live source observations beneath the caller's already-selected Git root.
///
/// Pins the root directory without discovering a repository, opening storage,
/// creating service state, refreshing, pinning Git trees or launching a drain.
/// Each observation joins the injected cache's active scope or owns a fresh read.
///
/// # Errors
/// Reports an inaccessible, symlink or non-directory root.
pub fn wire_source_reader<C: service::CacheDatabase>(
    root: &Path,
    storage: Rc<C>,
) -> DocumentResult<impl SourceReader + use<C>> {
    Ok(SourceService::new(
        storage,
        LocalFileSystem::new(root).map_err(service::map_document_file_error)?,
    ))
}

/// Wire read-only source provenance beneath the repository containing `root`.
/// Does not open storage, create service state, retain refs or fetch sources.
///
/// # Errors
/// Reports unsupported repositories or unsafe filesystem roots.
pub fn wire_source_provenance(root: &Path) -> DocumentResult<impl SourceProvenance + use<>> {
    let git = GitRepository::open(root, &[SERVICE_DIRECTORY]).map_err(|cause| {
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
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    wire_checker_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_checker_with_repository(git: GitRepository) -> ServiceResult<impl Checker + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let storage = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    let storage = Rc::new(storage.1);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get())
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?,
        Rc::clone(&storage),
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?;
    Ok(CheckService::new(
        refresher,
        Rc::clone(&storage),
        Box::new(GraphDiagnosticsService::new(storage)),
        wire_source_provenance_with_repository(git)
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Check, cause))?,
    ))
}

/// Wire hybrid search with independently owned read, embedding and provenance collaborators.
///
/// # Errors
/// Reports unsupported repositories, unsafe paths or storage/provider setup failures.
pub fn wire_searcher(start: &Path) -> ServiceResult<impl Searcher + use<>> {
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    wire_searcher_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_searcher_with_repository(git: GitRepository) -> ServiceResult<impl Searcher + use<>> {
    let filesystem = LocalFileSystem::new(git.worktree_root_get())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let storage = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let (vectors, cache) = storage;
    let storage = Rc::new(cache);
    let vectors = Rc::new(vectors);
    let refresher = wire_document_refresher_with_storage(
        git.clone(),
        LocalFileSystem::new(git.worktree_root_get())
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?,
        Rc::clone(&storage),
    )
    .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Search, cause))?;
    let embeddings = wire_embedder(git.worktree_root_get(), Rc::clone(&storage), vectors)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
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
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause))?;
    wire_inspector_with_repository(git)
}

/// Compose collaborators from the command's shared repository and cleanup owner.
fn wire_inspector_with_repository(git: GitRepository) -> ServiceResult<impl Inspector + use<>> {
    let refresher = wire_document_refresher_with_repository(git.clone())
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Inspection, cause))?;
    let files = LocalFileSystem::new(git.worktree_root_get())
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
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY]).map_err(|cause| {
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
    let filesystem =
        LocalFileSystem::new(git.worktree_root_get()).map_err(service::map_document_file_error)?;
    let directory = filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(service::map_document_file_error)?;
    let storage = crate::open_confined_databases(
        &git.worktree_root_get().join(SERVICE_DIRECTORY),
        &directory,
    )
    .map_err(|cause| {
        DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            "cannot open document storage",
            cause,
        )
    })?;
    wire_document_refresher_with_storage(git, filesystem, Rc::new(storage.1))
}

fn wire_document_refresher_with_storage(
    git: GitRepository,
    filesystem: LocalFileSystem,
    storage: Rc<storage::CacheStorage>,
) -> DocumentResult<impl Refresher + use<>> {
    filesystem
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(service::map_document_file_error)?;
    let provenance = ProvenanceService::new(
        Box::new(git.clone()),
        LocalFileSystem::new(git.worktree_root_get()).map_err(service::map_document_file_error)?,
    );
    Ok(DocumentRefreshService {
        git,
        filesystem,
        clock: Box::new(SystemClock::new()),
        markdown: MarkdownParser::new(),
        storage,
        provenance,
    })
}

/// Wire provenance without rediscovering the command's repository.
fn wire_source_provenance_with_repository(
    git: GitRepository,
) -> DocumentResult<impl SourceProvenance + use<>> {
    let filesystem =
        LocalFileSystem::new(git.worktree_root_get()).map_err(service::map_document_file_error)?;
    Ok(ProvenanceService::new(Box::new(git), filesystem))
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
    let git = GitRepository::open(start, &[SERVICE_DIRECTORY])
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    wire_embedding_executor_at_root(git.worktree_root_get())
}

/// Wire embedding collaborators beneath an already discovered repository root.
fn wire_embedding_executor_at_root(
    root: &Path,
) -> ServiceResult<impl EmbeddingExecutor + Embedder + use<>> {
    let files = LocalFileSystem::new(root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    let directory = files
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    let storage = crate::open_confined_databases(&root.join(SERVICE_DIRECTORY), &directory)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?;
    let (vectors, cache) = storage;
    Ok(EmbeddingService::new(
        Rc::new(cache),
        Rc::new(vectors),
        files,
        Box::new(SystemClock::new()),
        MarkdownParser::new(),
        build_embedding_provider()
            .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Embedding, cause))?,
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
    let files = LocalFileSystem::new(root)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Background, cause))?;
    Ok(BackgroundRunnerService::new(
        tasks,
        files,
        DetachedProcess::new(
            executable.to_path_buf(),
            vec!["embed".into(), "--drain".into()],
            root.to_path_buf(),
        ),
    ))
}

/// Wire synchronous embeddings with explicit caller-owned storage.
///
/// # Errors
/// Reports confined filesystem or provider construction failures.
pub fn wire_embedder<C: service::CacheDatabase, V: service::VectorDatabase>(
    root: &Path,
    cache: Rc<C>,
    vectors: Rc<V>,
) -> EmbeddingResult<impl Embedder + EmbeddingExecutor + use<C, V>> {
    let files = LocalFileSystem::new(root).map_err(map_embedding_files)?;
    files
        .directory_create_by_path(Path::new(SERVICE_DIRECTORY))
        .map_err(map_embedding_files)?;
    Ok(EmbeddingService::new(
        cache,
        vectors,
        files,
        Box::new(SystemClock::new()),
        MarkdownParser::new(),
        build_embedding_provider()?,
    ))
}

/// Construct the synchronous provider pair, capturing compatible credentials once.
/// Does not perform inference or require a key for native Ollama.
///
/// # Errors
/// Reserved for failures constructing the provider boundary.
pub fn build_embedding_provider() -> EmbeddingResult<impl EmbeddingProvider + use<>> {
    let key = env::var("OPENAI_API_KEY").ok();
    Ok(HttpProviders::new(key))
}

/// Open immutable Git snapshot operations and read-only root verification at `start`.
/// Requires a UTF-8 checkout root and a SHA-1 or SHA-256
/// working repository without an object alternates path (even empty/inactive).
/// Alternates are checked again before each operation; use a self-contained clone.
/// Tree creation rejects `.letopis` as the first path component before writing any
/// object; addressed historical reads and retention do not apply that creation policy.
///
/// # Errors
/// Returns a snapshot error when the working repository is unsupported,
/// the checkout root cannot be decoded, an alternates path exists, or Git I/O fails.
pub fn open_git_snapshots(
    start: &Path,
) -> Result<impl GitSnapshots + SnapshotVerifier, SnapshotError> {
    GitRepository::open(start, &[SERVICE_DIRECTORY])
}

/// Open existing published storage beneath an already pinned service directory.
/// Does not create database files, bootstrap schemas or synchronize vector identity.
///
/// # Errors
/// Reports unsafe paths, incompatible schemas or database observation failures.
pub fn open_status_storage<F: AsFd>(
    directory: &Path,
    pinned: &F,
) -> StorageResult<impl service::CacheDatabase + use<F>> {
    crate::open_observed_cache(directory, pinned)
}

/// Open the disk cache and independent vector database.
///
/// # Errors
/// Returns an invalid request for aliased or invalid paths, or an internal error
/// for unsupported capabilities, incompatible schemas, contention or I/O errors.
pub fn open_storage(
    cache: &Path,
    vectors: &Path,
) -> StorageResult<(
    impl service::VectorDatabase + use<>,
    impl service::CacheDatabase + use<>,
)> {
    crate::open_sqlite_databases(cache, vectors)
}

/// Open cache read and publication scopes with the independent vector read view.
///
/// # Errors
/// Reports unsafe or aliased paths, unsupported schemas, native capabilities or I/O.
pub fn open_cache_storage(
    cache: &Path,
    vectors: &Path,
) -> StorageResult<impl service::CacheDatabase + use<>> {
    open_sqlite_databases(cache, vectors).map(|storage| storage.1)
}

/// Open confined regular-file observations and persistent index/embedding locking.
///
/// # Errors
/// Reports an inaccessible or non-directory root, including a symlink root.
pub fn open_filesystem(root: &Path) -> FileResult<impl FileWriter + FileLocker + use<>> {
    LocalFileSystem::new(root)
}

// Initialization must validate existing configuration before creating database state.
// The function pointer defers composition until InitService supplies its pinned directory.
type StorageInitializer = fn(&Path, &OwnedFd) -> StorageResult<()>;
impl service::InitStorage for StorageInitializer {
    fn initialize(&self, directory: &Path, pinned: &OwnedFd) -> StorageResult<()> {
        self(directory, pinned)
    }
}

fn initialize_storage(directory: &Path, pinned: &OwnedFd) -> StorageResult<()> {
    open_confined_databases(directory, pinned).map(drop)
}

fn open_sqlite_databases(
    cache: &Path,
    vectors: &Path,
) -> StorageResult<(VectorStorage, CacheStorage)> {
    let [cache, vectors] = resolve_database_paths(cache, vectors)?;
    let mut cache_writer = storage::open_database(&cache)?;
    storage::bootstrap_cache(&mut cache_writer)?;
    let mut vector_writer = storage::open_database(&vectors)?;
    storage::bootstrap_vectors(&mut vector_writer)?;
    storage::attach_read_only_vectors(&cache_writer, &vectors)?;
    vector_writer
        .busy_timeout(VECTOR_WRITE_WAIT)
        .map_err(storage::map_sqlite)?;
    storage::attach_cache(&vector_writer, &cache)?;
    let read = storage::open_database(&cache)?;
    storage::attach_vectors(&read, &vectors)?;
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
        .map_err(storage::map_sqlite)?;
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
        if let Some(vectors) = get_observed_database(directory, pinned, "vectors.db")? {
            storage::validate_vector_database(&vectors)?;
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
    with_verified_database_paths(directory, pinned, |cache, vectors| {
        let identity = match name {
            "cache.db" => cache,
            "vectors.db" => vectors,
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
    with_verified_database_paths(directory, pinned, |_, vectors| {
        if vectors.is_none() {
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
    let files = LocalFileSystem::new(directory).map_err(|cause| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "cannot validate document storage path",
            cause,
        )
    })?;
    let (cache, vectors) = wait_for_stable_storage_paths(|| {
        verify_storage_directory(&files, pinned)?;
        let cache = files
            .file_get_identity_by_path(Path::new("cache.db"))
            .map_err(map_storage_path_error)?;
        let vectors = files
            .file_get_identity_by_path(Path::new("vectors.db"))
            .map_err(map_storage_path_error)?;
        verify_storage_files(&files, cache, vectors)?;
        Ok((cache, vectors))
    })?;
    #[cfg(test)]
    if let Some(hook) = BEFORE_DATABASE_OPERATION.take() {
        hook();
    }
    let result = operation(cache, vectors);
    wait_for_stable_storage_paths(|| {
        verify_storage_directory(&files, pinned)?;
        verify_storage_files(&files, cache, vectors)
    })?;
    result
}

/// Repeat path verification while another process is creating a database leaf.
///
/// A presence flip between two leaf observations is reported as a transient
/// `Changed` error. A replacement that stays different still exhausts the budget.
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

fn verify_storage_directory(files: &dyn FileReader, pinned: &impl AsFd) -> StorageResult<()> {
    let current = files
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
    let current = rustix::fs::fstat(&current).map_err(|cause| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "cannot validate document storage path",
            cause,
        )
    })?;
    let pinned = rustix::fs::fstat(pinned).map_err(|cause| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "cannot validate document storage path",
            cause,
        )
    })?;
    if !utils::has_same_identity(&current, &pinned) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "document storage directory changed",
        ));
    }
    Ok(())
}

fn verify_storage_files(
    files: &dyn FileReader,
    cache: Option<FileIdentity>,
    vectors: Option<FileIdentity>,
) -> StorageResult<()> {
    files
        .file_verify_identity_by_path(Path::new("cache.db"), cache)
        .map_err(map_storage_path_error)?;
    files
        .file_verify_identity_by_path(Path::new("vectors.db"), vectors)
        .map_err(map_storage_path_error)?;
    files
        .file_verify_distinct_by_paths(Path::new("cache.db"), Path::new("vectors.db"))
        .map_err(map_storage_path_error)
}

fn map_storage_path_error(cause: FileError) -> StorageError {
    let message = match cause.get_kind() {
        FileErrorKind::Changed => "document storage database changed",
        FileErrorKind::Aliased => "storage databases must be distinct files",
        FileErrorKind::UnsafePath | FileErrorKind::NonRegular => "unsafe document storage path",
        _ => "cannot validate document storage path",
    };
    StorageError::new_with_cause(StorageErrorKind::InvalidRequest, message, cause)
}

fn resolve_database_paths(cache: &Path, vectors: &Path) -> StorageResult<[std::path::PathBuf; 2]> {
    for path in [cache, vectors] {
        if path.to_str().is_none() {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            ));
        }
    }
    let paths =
        LocalFileSystem::file_get_distinct_paths_by_paths(cache, vectors).map_err(|cause| {
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

#[cfg(test)]
thread_local! {
    static BEFORE_DATABASE_OPERATION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) };
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
        let _ = writer.response_write(&mut output, response);
        let _ = cleanup.writes_cancel_and_cleanup();
    });
    install(on_interrupt).map(Some)
}

fn requires_cancellation_handler(args: &[OsString]) -> bool {
    if get_index_mode_by_args(args).is_some() {
        return true;
    }
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
    request_parse(args, input).and_then(|request| run_request(request, cleanup))
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

fn map_embedding_files(error: FileError) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, error)
}

#[cfg(test)]
#[path = "../tests/support/git.rs"]
mod process_test_support;

#[cfg(test)]
mod database_tests {
    use crate::service::StorageResult;
    use crate::storage::open_database;
    use rusqlite::Connection;
    use rustix::fd::{AsFd, BorrowedFd};
    use std::{cell::RefCell, path::Path};
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
    };
    use tempfile::TempDir;

    #[test]
    fn reject_cache_attachment_symlink_installed_after_validation() {
        // Given: pin the service directory and open the vector connection.
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("vectors.db").unwrap();
        let link = path.join("cache.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        // When / Then: the read-only ATTACH inherits NOFOLLOW, including a URI.
        assert!(directory.attach_cache(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
        assert!(!outside.path().join("outside.db-wal").exists());
    }

    #[test]
    fn reject_database_leaf_symlink_installed_after_directory_validation() {
        for name in ["cache.db", "vectors.db"] {
            // Given
            let root = TempDir::new().unwrap();
            let path = fs::canonicalize(root.path()).unwrap();
            let outside = TempDir::new().unwrap();
            let target = outside.path().join("outside.db");
            fs::write(&target, []).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = DatabaseFixture::new(&path, &pinned);
            let link = path.join(name);
            let destination = target.clone();
            directory.before_open.replace(Some(Box::new(move || {
                symlink(destination, link).unwrap();
            })));
            // When / Then
            assert!(directory.open_database(name).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"");
        }
    }

    #[test]
    fn reject_vector_attachment_symlink_installed_after_validation() {
        // Given
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("cache.db").unwrap();
        let link = path.join("vectors.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        // When / Then
        assert!(directory.attach_vectors(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
    }

    #[test]
    fn reject_replaced_service_directory_before_initialization() {
        for use_symlink in [false, true] {
            // Given
            let root = TempDir::new().unwrap();
            let root_path = fs::canonicalize(root.path()).unwrap();
            let path = root_path.join(".letopis");
            fs::create_dir(&path).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = DatabaseFixture::new(&path, &pinned);
            fs::rename(&path, root_path.join("original")).unwrap();
            let outside = TempDir::new().unwrap();
            if use_symlink {
                symlink(outside.path(), &path).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            // When / Then
            assert!(directory.open_database("cache.db").is_err());
            assert!(!path.join("cache.db").exists());
            assert!(!outside.path().join("cache.db").exists());
        }
    }

    #[test]
    fn rejects_regular_cache_replacement_during_attachment() {
        // Given: an existing cache and vector connection under a pinned directory.
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let pinned = File::open(&path).unwrap();
        drop(open_database(&path.join("cache.db")).unwrap());
        drop(open_database(&path.join("replacement.db")).unwrap());
        let directory = DatabaseFixture::new(&path, &pinned);
        let connection = directory.open_database("vectors.db").unwrap();
        let replaced = path.clone();
        directory.before_open.replace(Some(Box::new(move || {
            fs::rename(replaced.join("cache.db"), replaced.join("original.db")).unwrap();
            fs::rename(replaced.join("replacement.db"), replaced.join("cache.db")).unwrap();
        })));
        // When: the cache leaf changes during SQLite attachment.
        let result = directory.attach_cache(&connection);
        // Then: a regular replacement is rejected even though directory identity matches.
        assert!(result.is_err());
    }

    struct DatabaseFixture<'a> {
        path: &'a Path,
        pinned: BorrowedFd<'a>,
        before_open: RefCell<Option<Box<dyn FnOnce()>>>,
    }
    impl<'a> DatabaseFixture<'a> {
        fn new(path: &'a Path, pinned: &'a impl AsFd) -> Self {
            Self {
                path,
                pinned: pinned.as_fd(),
                before_open: RefCell::new(None),
            }
        }

        fn open_database(&self, name: &str) -> StorageResult<Connection> {
            super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::open_confined_database(self.path, &self.pinned, name)
        }

        fn attach_vectors(&self, connection: &Connection) -> StorageResult<()> {
            super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::attach_confined_vectors(self.path, &self.pinned, connection)
        }

        fn attach_cache(&self, connection: &Connection) -> StorageResult<()> {
            super::BEFORE_DATABASE_OPERATION.set(self.before_open.take());
            super::attach_confined_cache(self.path, &self.pinned, connection)
        }
    }

    mod storage_contracts {
        use crate::service::TransactionError;
        use crate::storage::get_current;

        use crate::storage::TestDatabases;
        use crate::{
            domain::Sha256,
            service::{CacheDatabase, StorageError, StorageRead, VectorDatabase},
        };
        use std::fs::File;
        use tempfile::TempDir;

        #[test]
        fn nested_read_works_on_both_production_open_paths() {
            // Given: each production opener constructs the shared cache storage.
            for confined in [false, true] {
                let root = TempDir::new().unwrap();
                let path = root.path().canonicalize().unwrap();
                let pinned = File::open(&path).unwrap();
                let storage = if confined {
                    crate::storage::open_confined_test_storage(&path, &pinned).unwrap()
                } else {
                    crate::storage::open_test_storage(
                        &path.join("cache.db"),
                        &path.join("vectors.db"),
                    )
                    .unwrap()
                };
                // When: a service joins a read scope inside the cache write scope.
                let result = storage
                    .cache
                    .with_write_tx(|| -> Result<u64, StorageError> {
                        storage
                            .cache
                            .with_read_tx(|| storage.cache.project_get_by_id())
                            .map_err(|error| match error {
                                TransactionError::Storage(error)
                                | TransactionError::Operation(error) => error,
                            })
                    });
                // Then: the nested read uses the real writer and sees its starting revision.
                assert_eq!(result.unwrap(), 0);
                assert!(
                    storage
                        .cache
                        .get_test_writer()
                        .execute("delete from vec.meta", [])
                        .is_err()
                );
            }
        }

        #[test]
        fn vector_writer_wait_is_bounded_without_changing_cache_or_read_policy() {
            for confined in [false, true] {
                // Given: exercise both supported open paths against independent databases.
                let root = TempDir::new().unwrap();
                let path = root.path().canonicalize().unwrap();
                let pinned = File::open(&path).unwrap();
                let storage = if confined {
                    crate::storage::open_confined_test_storage(&path, &pinned).unwrap()
                } else {
                    crate::storage::open_test_storage(
                        &path.join("cache.db"),
                        &path.join("vectors.db"),
                    )
                    .unwrap()
                };
                // When / Then: only vectors writers have the five-second contention budget.
                for (connection, expected) in [
                    (storage.vectors.get_test_connection(), 5000),
                    (storage.cache.get_test_writer(), 0),
                    (storage.cache.get_test_reader(), 0),
                ] {
                    let timeout: i64 = connection
                        .pragma_query_value(None, "busy_timeout", |r| r.get(0))
                        .unwrap();
                    assert_eq!(timeout, expected);
                }
            }
        }

        #[test]
        fn confined_vector_pruning_uses_read_only_cache_and_closes_cleanly() {
            // Given: only the confined production opener is under test.
            let root = TempDir::new().unwrap();
            let path = root.path().canonicalize().unwrap();
            let pinned = File::open(&path).unwrap();
            let storage = crate::storage::open_confined_test_storage(&path, &pinned).unwrap();
            let identity = Sha256::digest(b"confined");
            seed_vector(&storage, &identity);
            // When: pruning requires the read-only cache attachment used for hash-reference checks.
            storage
                .vectors
                .with_write_tx(&identity, || {
                    let unit = &storage.vectors;
                    unit.prune_unreferenced().unwrap();

                    Ok::<_, StorageError>(())
                })
                .map(get_current)
                .unwrap();
            // Then: both native deletion and lifetime/permission policy.
            assert_eq!(
                get_current(
                    storage
                        .cache
                        .with_read_tx(|| storage
                            .cache
                            .vector_keys_get_by_search_hash(&identity, &identity))
                        .unwrap()
                ),
                None
            );
            assert!(
                storage
                    .vectors
                    .get_test_connection()
                    .execute_batch("delete from cache.nodes")
                    .is_err()
            );
            drop(storage);
            assert!(!path.join("cache.db-wal").exists());
            assert!(!path.join("cache.db-shm").exists());
        }

        fn seed_vector(storage: &TestDatabases, identity: &Sha256) {
            storage.vectors.reset_vectors(None, identity).unwrap();
            storage
                .vectors
                .with_write_tx(identity, || {
                    storage.vectors.store_vector(identity, &[1.0, 0.0, 0.0], 3)
                })
                .map(get_current)
                .unwrap();
        }
    }
}

#[cfg(test)]
mod database_path_tests {
    use super::{
        StorageResult, resolve_database_paths, wait_for_stable_storage_paths,
        with_verified_database_paths,
    };
    use crate::service::{FileError, FileErrorKind, StorageError, StorageErrorKind};
    use std::fs::{self, File};
    use std::path::Path;
    use tempfile::TempDir;

    #[test]
    fn retries_transient_changed_database_observations() {
        // Given: a path verification that reports a concurrent creation as changed once.
        let attempts = std::cell::Cell::new(0);
        // When: storage verification observes the same leaves again.
        let result = wait_for_stable_storage_paths(|| {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err(StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "document storage database changed",
                    FileError::new(FileErrorKind::Changed),
                ))
            } else {
                Ok(())
            }
        });
        // Then: the transient change is retried and the stable observation succeeds.
        result.unwrap();
        assert_eq!(attempts.get(), 2);
    }

    #[test]
    fn rejects_replacement_of_an_observed_database() {
        // Given: a retained directory and a regular database leaf.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        fs::write(path.join("cache.db"), b"original").unwrap();
        // When: a database operation replaces the original leaf and fails.
        let error = with_verified_database_paths(&path, &pinned, |_, _| {
            fs::rename(path.join("cache.db"), path.join("saved.db")).unwrap();
            fs::write(path.join("cache.db"), b"replacement").unwrap();
            Err::<(), _>(super::StorageError::new(
                StorageErrorKind::InternalError,
                "sqlite failed",
            ))
        })
        .unwrap_err();
        // Then: integrity verification takes precedence over the database failure.
        assert_eq!(error.get_message(), "document storage database changed");
        assert_eq!(fs::read(path.join("saved.db")).unwrap(), b"original");
    }

    #[test]
    fn rejects_directory_replacement_before_fresh_file_observation() {
        // Given: the original directory capability survives path replacement.
        let root = TempDir::new().unwrap();
        let path = root.path().join("state");
        fs::create_dir(&path).unwrap();
        let pinned = File::open(&path).unwrap();
        fs::rename(&path, root.path().join("saved")).unwrap();
        fs::create_dir(&path).unwrap();
        // When: composition starts a new observation under the replaced name.
        let error = with_verified_database_paths(&path, &pinned, |_, _| -> StorageResult<()> {
            panic!("unsafe operation entered")
        })
        .unwrap_err();
        // Then: the retained original capability rejects the fresh directory.
        assert_eq!(error.get_message(), "document storage directory changed");
        assert_eq!(fs::read_dir(&path).unwrap().count(), 0);
    }

    #[test]
    fn rejects_existing_database_alias_before_entering_operation() {
        // Given: both database names already refer to one regular file.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        fs::write(path.join("cache.db"), b"untouched").unwrap();
        fs::hard_link(path.join("cache.db"), path.join("vectors.db")).unwrap();
        // When: composition validates the pair before giving it to SQLite.
        let error = with_verified_database_paths(&path, &pinned, |_, _| -> StorageResult<()> {
            panic!("aliased operation entered")
        })
        .unwrap_err();
        // Then: alias detection prevents the operation and preserves the original bytes.
        assert_eq!(
            error.get_message(),
            "storage databases must be distinct files"
        );
        assert_eq!(fs::read(path.join("cache.db")).unwrap(), b"untouched");
    }

    #[test]
    fn permits_creation_but_rejects_hard_link_aliases() {
        // Given: both files are initially absent beneath the pinned directory.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        // When: the operation creates independent regular files.
        with_verified_database_paths(&path, &pinned, |_, _| {
            fs::write(path.join("cache.db"), []).unwrap();
            fs::write(path.join("vectors.db"), []).unwrap();
            Ok(())
        })
        .unwrap();
        fs::remove_file(path.join("vectors.db")).unwrap();
        // When: an operation fills the missing leaf with an alias to the cache.
        let error = with_verified_database_paths(&path, &pinned, |_, _| {
            fs::hard_link(path.join("cache.db"), path.join("vectors.db")).unwrap();
            Ok(())
        })
        .unwrap_err();
        // Then: post-operation distinctness rejects the alias.
        assert_eq!(
            error.get_message(),
            "storage databases must be distinct files"
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn rejects_non_utf8_targets_resolved_from_utf8_symlinks() {
        use std::ffi::OsString;
        use std::os::unix::{ffi::OsStringExt, fs::symlink};
        // Given: a UTF-8 alias resolves to a native path SQLite cannot represent.
        let root = TempDir::new().unwrap();
        let target = root.path().join(OsString::from_vec(vec![0xff]));
        fs::write(&target, b"untouched").unwrap();
        let alias = root.path().join("alias");
        symlink(&target, &alias).unwrap();
        // When: composition resolves external storage paths.
        let error = resolve_database_paths(&alias, &root.path().join("other")).unwrap_err();
        // Then: the original safe classification and context are preserved.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(error.get_message(), "invalid storage database path");
        assert_eq!(fs::read(&target).unwrap(), b"untouched");
        assert!(!root.path().join("other").exists());
    }

    #[test]
    fn rejects_database_paths_with_nul_bytes() {
        // Given / When: resolve a path containing an embedded NUL byte.
        let error =
            resolve_database_paths(Path::new("bad\0database"), Path::new("other")).unwrap_err();
        // Then: SQLite must never receive a path that is not a filesystem name.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }

    #[test]
    fn rejects_an_existing_directory_as_a_database_path() {
        // Given: a directory exists where a database file is expected.
        let root = TempDir::new().unwrap();
        // When: resolve the directory as a database path.
        let error = resolve_database_paths(root.path(), &root.path().join("other")).unwrap_err();
        // Then: reject the invalid request without changing the filesystem.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }

    #[test]
    fn preserves_non_absence_errors_when_observing_database_identity() {
        // Given: a regular parent causes ENOTDIR rather than a missing leaf.
        let root = TempDir::new().unwrap();
        let parent = root.path().join("ordinary-file");
        fs::write(&parent, b"preserved").unwrap();
        // When: resolve database leaves below the regular file.
        let error =
            resolve_database_paths(&parent.join("cache.db"), &root.path().join("vectors.db"))
                .unwrap_err();
        // Then: reject the invalid request without changing the filesystem.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(fs::read(&parent).unwrap(), b"preserved");
        assert!(!root.path().join("vectors.db").exists());
    }
}
