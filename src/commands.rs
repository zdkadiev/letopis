//! Command wiring: builds the services each command needs and runs it.

use crate::cli::{
    CommandResponse, Request, execute_check, execute_drain, execute_help, execute_inspect,
    execute_reindex, execute_search, execute_status, execute_traverse,
};
use crate::domain::RepoPath;
use crate::filesystem::LocalFileSystem;
use crate::git::GitCli;
use crate::service::{
    CommandError, CommandErrorKind, CommandResult, DocumentError, ElementAddress,
    GraphDiagnosticsService, ProvenanceService, SearchRequest, StatusService, TraversalRequest,
};
use crate::storage::SqliteStatusStorage;
use crate::utils::SERVICE_DIRECTORY;
use crate::{
    build_embedding_provider, map_embedding_setup, wire_background_runner, wire_checker,
    wire_document_refresher, wire_embedding_executor, wire_inspector, wire_searcher,
    wire_traverser,
};
use std::{
    env,
    path::{Path, PathBuf},
};

pub(crate) fn run_request(request: Request) -> CommandResult<CommandResponse> {
    // A failed lookup only fails the commands that need the environment, so help works anywhere.
    let environment = locate_environment();
    match request {
        Request::Help(text) => Ok(execute_help(text)),
        Request::Status => environment.and_then(|(directory, _)| run_status(&directory)),
        Request::Reindex => {
            environment.and_then(|(directory, executable)| run_reindex(&directory, &executable))
        }
        Request::Check(paths) => environment
            .and_then(|(directory, executable)| run_check(&paths, &directory, &executable)),
        Request::Drain => {
            environment.and_then(|(directory, executable)| run_drain(&directory, &executable))
        }
        Request::Search(request) => environment
            .and_then(|(directory, executable)| run_search(&request, &directory, &executable)),
        Request::Traverse(request) => environment
            .and_then(|(directory, executable)| run_traverse(&request, &directory, &executable)),
        Request::Inspect(ids) => environment
            .and_then(|(directory, executable)| run_inspect(&ids, &directory, &executable)),
    }
}

fn run_reindex(directory: &Path, executable: &Path) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut documents = wire_document_refresher(&root).map_err(map_reindex_setup)?;
    execute_reindex(&mut documents, &mut background)
}

fn run_status(directory: &Path) -> CommandResult<CommandResponse> {
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
    let graph = GraphDiagnosticsService::new();
    let mut status = StatusService {
        storage,
        files: &files,
        graph: &graph,
        provenance: &provenance,
        provider: &provider,
    };
    execute_status(&mut status)
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

fn run_drain(root: &Path, executable: &Path) -> CommandResult<CommandResponse> {
    let mut tasks = wire_embedding_executor(root)?;
    let mut background = wire_background_runner(root, executable, Some(&mut tasks))?;
    execute_drain(&mut background)
}

fn run_check(
    paths: &[RepoPath],
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut checks = wire_checker(&root)?;
    execute_check(&mut checks, paths, &mut background)
}

fn run_search(
    request: &SearchRequest,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut searches = wire_searcher(&root)?;
    execute_search(&mut searches, request, &mut background)
}

fn run_traverse(
    request: &TraversalRequest,
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut traversals = wire_traverser(&root)?;
    execute_traverse(&mut traversals, request, &mut background)
}

fn run_inspect(
    ids: &[ElementAddress],
    directory: &Path,
    executable: &Path,
) -> CommandResult<CommandResponse> {
    let root = open_repository_root(directory)?;
    let mut tasks = wire_embedding_executor(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut inspections = wire_inspector(&root)?;
    execute_inspect(&mut inspections, ids, &mut background)
}

/// Foreground services and background exclusion must share the discovered repository root.
fn open_repository_root(directory: &Path) -> CommandResult<PathBuf> {
    let git = GitCli::open(directory).map_err(map_embedding_setup)?;
    Ok(git.get_root().to_path_buf())
}

fn map_reindex_setup(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open corpus reindex service",
        cause,
    )
}

fn map_status_setup(error: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot open status service",
        error,
    )
}
