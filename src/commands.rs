//! Command wiring: builds the services each command needs and runs it.

use crate::cli::{
    CommandResponse, IndexMode, IndexRequest, Request, ResponseResult, finish_foreground,
};
use crate::domain::RepoPath;
use crate::git::GitWriteCleanup;
use crate::service::{
    BackgroundRunner, Checker, CommandError, CommandErrorKind, CommandResult, DocumentError,
    DocumentErrorKind, ElementAddress, Initializer, Inspector, Refresher, SearchRequest, Searcher,
    ServiceError, ServiceErrorKind, TraversalRequest, Traverser,
};
use crate::{
    open_repository_with_cleanup, wire_background_runner, wire_checker_with_repository,
    wire_document_refresher_with_repository, wire_embedding_executor,
    wire_embedding_executor_at_root, wire_initializer, wire_inspector_with_repository,
    wire_searcher_with_repository, wire_traverser_with_repository, with_configuration_service,
    with_status_service,
};
use std::{
    env,
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) fn run_request(
    request: Request,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    // A failed lookup only fails the commands that need the environment, so help works anywhere.
    let environment = locate_environment();
    match request {
        Request::Help(text) => Ok(CommandResponse::new(ResponseResult::Help(text))),
        Request::Init => environment.and_then(|(directory, _)| run_init(&directory)),
        Request::Status => environment.and_then(|(directory, _)| run_status(&directory)),
        Request::Config => environment.and_then(|(directory, _)| run_config(&directory)),
        Request::Index(request) => environment.and_then(|(directory, executable)| {
            run_index(&request, &directory, &executable, cleanup)
        }),
        Request::Check(paths) => environment.and_then(|(directory, executable)| {
            run_check(&paths, &directory, &executable, cleanup)
        }),
        Request::Drain => {
            environment.and_then(|(directory, executable)| run_drain(&directory, &executable))
        }
        Request::Search(request) => environment.and_then(|(directory, executable)| {
            run_search(&request, &directory, &executable, cleanup)
        }),
        Request::Traverse(request) => environment.and_then(|(directory, executable)| {
            run_traverse(&request, &directory, &executable, cleanup)
        }),
        Request::Inspect(ids) => environment.and_then(|(directory, executable)| {
            run_inspect(&ids, &directory, &executable, cleanup)
        }),
    }
}

fn run_init(directory: &Path) -> CommandResult<CommandResponse> {
    wire_initializer(directory)?.initialize()?;
    Ok(CommandResponse::new(ResponseResult::Initialized))
}

fn run_index(
    request: &IndexRequest,
    directory: &Path,
    executable: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    let git = open_repository_with_cleanup(directory, cleanup)?;
    let root = git.worktree_root_get().to_path_buf();
    let mut tasks = wire_embedding_executor_at_root(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut documents = wire_document_refresher_with_repository(git)
        .map_err(|cause| ServiceError::new_with_cause(ServiceErrorKind::Reindex, cause))?;
    let outcome = match request.mode {
        IndexMode::Incremental => documents.refresh(&request.paths),
        IndexMode::Verified => documents.refresh_verified(&request.paths),
        IndexMode::Full => documents.reindex(&request.paths),
    }
    .map_err(map_reindex_error)?;
    finish_foreground(ResponseResult::Index(outcome), &mut background)
}

fn run_status(directory: &Path) -> CommandResult<CommandResponse> {
    let report = with_status_service(directory, |status| status.get_status())?;
    Ok(CommandResponse::new(ResponseResult::Status(report)))
}

/// Observe the current configuration through non-creating files without background work.
fn run_config(directory: &Path) -> CommandResult<CommandResponse> {
    let config = with_configuration_service(directory, |configuration| configuration.get_config())?;
    Ok(CommandResponse::new(ResponseResult::Config(config)))
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
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    let git = open_repository_with_cleanup(directory, cleanup)?;
    let root = git.worktree_root_get().to_path_buf();
    let mut tasks = wire_embedding_executor_at_root(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut checks = wire_checker_with_repository(git)?;
    let report = checks.check(paths)?;
    finish_foreground(ResponseResult::Check(report), &mut background)
}

fn run_search(
    request: &SearchRequest,
    directory: &Path,
    executable: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    let git = open_repository_with_cleanup(directory, cleanup)?;
    let root = git.worktree_root_get().to_path_buf();
    let mut tasks = wire_embedding_executor_at_root(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut searches = wire_searcher_with_repository(git)?;
    let links = searches.search(request)?;
    finish_foreground(ResponseResult::Search(links), &mut background)
}

fn run_traverse(
    request: &TraversalRequest,
    directory: &Path,
    executable: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    let git = open_repository_with_cleanup(directory, cleanup)?;
    let root = git.worktree_root_get().to_path_buf();
    let mut tasks = wire_embedding_executor_at_root(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut traversals = wire_traverser_with_repository(git)?;
    let traversal = traversals.traverse(request)?;
    finish_foreground(ResponseResult::Traverse(traversal), &mut background)
}

fn run_inspect(
    ids: &[ElementAddress],
    directory: &Path,
    executable: &Path,
    cleanup: Arc<GitWriteCleanup>,
) -> CommandResult<CommandResponse> {
    let git = open_repository_with_cleanup(directory, cleanup)?;
    let root = git.worktree_root_get().to_path_buf();
    let mut tasks = wire_embedding_executor_at_root(&root)?;
    let mut background = wire_background_runner(&root, executable, Some(&mut tasks))?;
    let mut inspections = wire_inspector_with_repository(git)?;
    let inspection = inspections.inspect(ids)?;
    finish_foreground(ResponseResult::Inspect(inspection), &mut background)
}

fn map_reindex_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot rebuild corpus index", cause)
}

impl From<ServiceError> for CommandError {
    fn from(error: ServiceError) -> Self {
        map_service_error(error)
    }
}

fn map_service_error(error: ServiceError) -> CommandError {
    let (kind, message) = match error.get_kind() {
        ServiceErrorKind::RepositoryRequired => {
            (CommandErrorKind::InvalidRequest, "Git repository required")
        }
        ServiceErrorKind::Repository => {
            (CommandErrorKind::InternalError, "cannot locate repository")
        }
        ServiceErrorKind::Initialization => (
            CommandErrorKind::InternalError,
            "cannot initialize repository",
        ),
        ServiceErrorKind::Check => (CommandErrorKind::InternalError, "cannot open check service"),
        ServiceErrorKind::Search => (
            CommandErrorKind::InternalError,
            "cannot open search service",
        ),
        ServiceErrorKind::Traversal => {
            (CommandErrorKind::InternalError, "cannot prepare traversal")
        }
        ServiceErrorKind::Inspection => (
            CommandErrorKind::InternalError,
            "cannot open inspection service",
        ),
        ServiceErrorKind::Status => (
            CommandErrorKind::InternalError,
            "cannot open status service",
        ),
        ServiceErrorKind::Configuration => (
            CommandErrorKind::InternalError,
            "cannot read embedding configuration",
        ),
        ServiceErrorKind::Embedding => (
            CommandErrorKind::InternalError,
            "cannot open embedding service",
        ),
        ServiceErrorKind::Background => (
            CommandErrorKind::InternalError,
            "cannot open background root",
        ),
        ServiceErrorKind::Reindex => (
            CommandErrorKind::InternalError,
            "cannot open corpus reindex service",
        ),
    };
    CommandError::new_with_cause(kind, message, error)
}

#[cfg(test)]
mod tests {
    use super::map_service_error;
    use crate::service::{CommandErrorKind, ServiceError, ServiceErrorKind};
    use std::{error::Error, io};

    #[test]
    fn maps_each_service_kind_and_keeps_causes_private() {
        // Given: every service setup kind with one private cause sentinel.
        // When / Then: public classification/message is stable and the cause stays private.
        for (service_kind, command_kind, message) in SERVICE_MAPPINGS {
            let error = ServiceError::new_with_cause(
                service_kind,
                io::Error::other("PRIVATE_SERVICE_CAUSE"),
            );
            let mapped = map_service_error(error);
            assert_eq!(mapped.get_kind(), command_kind, "{service_kind:?}");
            assert_eq!(mapped.get_message(), message, "{service_kind:?}");
            assert!(!format!("{mapped:?}").contains("PRIVATE_SERVICE_CAUSE"));
            let source = mapped.source().unwrap();
            let service = source.downcast_ref::<ServiceError>().unwrap();
            assert!(
                service
                    .source()
                    .unwrap()
                    .to_string()
                    .contains("PRIVATE_SERVICE_CAUSE")
            );
        }
    }

    const SERVICE_MAPPINGS: [(ServiceErrorKind, CommandErrorKind, &str); 12] = [
        (
            ServiceErrorKind::RepositoryRequired,
            CommandErrorKind::InvalidRequest,
            "Git repository required",
        ),
        (
            ServiceErrorKind::Repository,
            CommandErrorKind::InternalError,
            "cannot locate repository",
        ),
        (
            ServiceErrorKind::Initialization,
            CommandErrorKind::InternalError,
            "cannot initialize repository",
        ),
        (
            ServiceErrorKind::Check,
            CommandErrorKind::InternalError,
            "cannot open check service",
        ),
        (
            ServiceErrorKind::Search,
            CommandErrorKind::InternalError,
            "cannot open search service",
        ),
        (
            ServiceErrorKind::Traversal,
            CommandErrorKind::InternalError,
            "cannot prepare traversal",
        ),
        (
            ServiceErrorKind::Inspection,
            CommandErrorKind::InternalError,
            "cannot open inspection service",
        ),
        (
            ServiceErrorKind::Status,
            CommandErrorKind::InternalError,
            "cannot open status service",
        ),
        (
            ServiceErrorKind::Configuration,
            CommandErrorKind::InternalError,
            "cannot read embedding configuration",
        ),
        (
            ServiceErrorKind::Embedding,
            CommandErrorKind::InternalError,
            "cannot open embedding service",
        ),
        (
            ServiceErrorKind::Background,
            CommandErrorKind::InternalError,
            "cannot open background root",
        ),
        (
            ServiceErrorKind::Reindex,
            CommandErrorKind::InternalError,
            "cannot open corpus reindex service",
        ),
    ];
}

#[cfg(test)]
#[path = "commands/construction_tests.rs"]
mod construction_tests;
