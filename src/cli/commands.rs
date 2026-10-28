use crate::domain::RepoPath;
use crate::service::{
    BackgroundRunner, CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem,
    CommandResult, DocumentError, DocumentErrorKind, ElementAddress, Inspection, Inspector,
    Refresher, SearchLink, SearchRequest, Searcher, StatusReader, StatusReport, Traversal,
    TraversalRequest, Traverser,
};

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
    Status(StatusReport),
    Reindex(u64),
    Check(CheckReport),
    Inspect(Inspection),
    Search(Vec<SearchLink>),
    Traverse(Traversal),
    Drained,
    Help(String),
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}

/// Return help or version text; needs no repository, services or input.
pub(crate) fn execute_help(text: String) -> CommandResponse {
    respond(ResponseResult::Help(text), None)
}

/// Read the published status; observational, so no background work starts.
pub(crate) fn execute_status(status: &mut dyn StatusReader) -> CommandResult<CommandResponse> {
    let report = status.read_status()?;
    Ok(respond(ResponseResult::Status(report), None))
}

/// Run the detached embedding drain itself, without scheduling another one.
pub(crate) fn execute_drain(
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    background.drain()?;
    Ok(respond(ResponseResult::Drained, None))
}

/// Rebuild the corpus index, then start background work after success.
/// Service and background failures propagate without exposing partial results.
pub(crate) fn execute_reindex(
    refresher: &mut dyn Refresher,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let outcome = refresher.reindex().map_err(map_reindex_error)?;
    finish_foreground(ResponseResult::Reindex(outcome.revision), background)
}

/// Validate the corpus, then start background work after success.
pub(crate) fn execute_check(
    checks: &mut dyn Checker,
    paths: &[RepoPath],
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let report = checks.check(paths)?;
    finish_foreground(ResponseResult::Check(report), background)
}

/// Inspect current metadata, then start background work after success.
pub(crate) fn execute_inspect(
    inspections: &mut dyn Inspector,
    ids: &[ElementAddress],
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let inspection = inspections.inspect(ids)?;
    finish_foreground(ResponseResult::Inspect(inspection), background)
}

/// Search the corpus, then start background work after success.
pub(crate) fn execute_search(
    searches: &mut dyn Searcher,
    request: &SearchRequest,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let links = searches.search(request)?;
    finish_foreground(ResponseResult::Search(links), background)
}

/// Traverse the graph, then start background work after success.
pub(crate) fn execute_traverse(
    traversals: &mut dyn Traverser,
    request: &TraversalRequest,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let traversal = traversals.traverse(request)?;
    finish_foreground(ResponseResult::Traverse(traversal), background)
}

fn finish_foreground(
    result: ResponseResult,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let warning = background.start_if_needed()?;
    Ok(respond(result, warning))
}

fn respond(result: ResponseResult, warning: Option<CommandProblem>) -> CommandResponse {
    CommandResponse {
        result,
        warnings: warning.into_iter().collect(),
    }
}

fn map_reindex_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot rebuild corpus index", cause)
}
