use super::input::Request;
use crate::service::{
    BackgroundRunner, CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem,
    CommandResult, Inspection, Inspector, SearchLink, Searcher, Traversal, Traverser,
};

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
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
        let result = match request {
            Request::Check(paths) => ResponseResult::Check(
                self.services
                    .checks
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .check(&paths)?,
            ),
            Request::Inspect(ids) => ResponseResult::Inspect(
                self.services
                    .inspections
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .inspect(&ids)?,
            ),
            Request::Search(request) => ResponseResult::Search(
                self.services
                    .searches
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .search(&request)?,
            ),
            Request::Traverse(request) => ResponseResult::Traverse(
                self.services
                    .traversals
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .traverse(&request)?,
            ),
            Request::Drain => {
                self.services
                    .background
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .drain()?;
                return Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                });
            }
        };
        self.finish_foreground(result)
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
