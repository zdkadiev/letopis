use super::input::Request;
use crate::service::{
    BackgroundRunner, CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem,
    CommandResult, DocumentError, DocumentErrorKind, Inspection, Inspector, Refresher, SearchLink,
    Searcher, StatusReader, StatusReport, Traversal, Traverser,
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
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}
/// Borrowed services available for one command; missing services fail safely.
#[derive(Default)]
pub(crate) struct HandlerServices<'a> {
    /// Full corpus publication capability.
    pub(crate) refresher: Option<&'a mut dyn Refresher>,
    /// Observational published diagnostics service.
    pub(crate) status: Option<&'a mut dyn StatusReader>,
    /// Corpus validation service.
    pub(crate) checks: Option<&'a mut dyn Checker>,
    /// Current metadata inspection service.
    pub(crate) inspections: Option<&'a mut dyn Inspector>,
    /// Ranked search service.
    pub(crate) searches: Option<&'a mut dyn Searcher>,
    /// Incremental graph traversal service.
    pub(crate) traversals: Option<&'a mut dyn Traverser>,
    /// Detached work lifecycle service.
    pub(crate) background: Option<&'a mut dyn BackgroundRunner>,
}
/// Dispatches commands through application services, then triggers background work.
pub(crate) struct Handlers<'a> {
    services: HandlerServices<'a>,
}
impl<'a> Handlers<'a> {
    /// Borrow available services without performing I/O.
    pub(crate) fn new(services: HandlerServices<'a>) -> Self {
        Self { services }
    }
    /// Run one service operation and start background work only after foreground success.
    /// Service and background failures propagate without exposing partial results.
    pub(crate) fn execute(&mut self, request: Request) -> CommandResult<CommandResponse> {
        match request {
            Request::Status => {
                let report = self
                    .services
                    .status
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .read_status()?;
                Ok(CommandResponse {
                    result: ResponseResult::Status(report),
                    warnings: Vec::new(),
                })
            }
            Request::Drain => {
                self.services
                    .background
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .drain()?;
                Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                })
            }
            request => {
                let result = self.execute_foreground(request)?;
                self.finish_foreground(result)
            }
        }
    }
    fn execute_foreground(&mut self, request: Request) -> CommandResult<ResponseResult> {
        match request {
            Request::Reindex => {
                let outcome = self
                    .services
                    .refresher
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .reindex()
                    .map_err(map_reindex_error)?;
                Ok(ResponseResult::Reindex(outcome.revision))
            }
            Request::Check(paths) => self
                .services
                .checks
                .as_deref_mut()
                .ok_or_else(fail_unavailable)?
                .check(&paths)
                .map(ResponseResult::Check),
            Request::Inspect(ids) => self
                .services
                .inspections
                .as_deref_mut()
                .ok_or_else(fail_unavailable)?
                .inspect(&ids)
                .map(ResponseResult::Inspect),
            Request::Search(request) => self
                .services
                .searches
                .as_deref_mut()
                .ok_or_else(fail_unavailable)?
                .search(&request)
                .map(ResponseResult::Search),
            Request::Traverse(request) => self
                .services
                .traversals
                .as_deref_mut()
                .ok_or_else(fail_unavailable)?
                .traverse(&request)
                .map(ResponseResult::Traverse),
            Request::Status | Request::Drain => Err(fail_unavailable()),
        }
    }
    fn finish_foreground(&mut self, result: ResponseResult) -> CommandResult<CommandResponse> {
        let warning = match self.services.background.as_deref_mut() {
            Some(background) => background.start_if_needed()?,
            None => None,
        };
        Ok(CommandResponse {
            result,
            warnings: warning.into_iter().collect(),
        })
    }
}
fn fail_unavailable() -> CommandError {
    CommandError::new(
        CommandErrorKind::InternalError,
        "operation unavailable in this build",
    )
}

fn map_reindex_error(cause: DocumentError) -> CommandError {
    let kind = match cause.get_kind() {
        DocumentErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        DocumentErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot rebuild corpus index", cause)
}
