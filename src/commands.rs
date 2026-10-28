//! Command wiring: builds the services each command needs and runs it.

use crate::cli::{CommandResponse, Request, ResponseResult, finish_foreground};
use crate::domain::RepoPath;
use crate::filesystem::LocalFileSystem;
use crate::git::GitCli;
use crate::service::{
    BackgroundRunner, Checker, CommandError, CommandErrorKind, CommandResult, DocumentError,
    DocumentErrorKind, ElementAddress, GraphDiagnosticsService, Inspector, ProvenanceService,
    Refresher, SearchRequest, Searcher, StatusReader, StatusService, TraversalRequest, Traverser,
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
        Request::Help(text) => Ok(CommandResponse::new(ResponseResult::Help(text))),
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
    let outcome = documents.reindex().map_err(map_reindex_error)?;
    finish_foreground(ResponseResult::Reindex(outcome.revision), &mut background)
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
    let report = status.read_status()?;
    Ok(CommandResponse::new(ResponseResult::Status(report)))
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
    background.drain()?;
    Ok(CommandResponse::new(ResponseResult::Drained))
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
    let report = checks.check(paths)?;
    finish_foreground(ResponseResult::Check(report), &mut background)
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
    let links = searches.search(request)?;
    finish_foreground(ResponseResult::Search(links), &mut background)
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
    let traversal = traversals.traverse(request)?;
    finish_foreground(ResponseResult::Traverse(traversal), &mut background)
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
    let inspection = inspections.inspect(ids)?;
    finish_foreground(ResponseResult::Inspect(inspection), &mut background)
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

fn map_reindex_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot rebuild corpus index", cause)
}
