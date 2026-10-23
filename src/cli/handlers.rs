use super::input::Request;
use crate::service::{
    BackgroundRunner, CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem,
    CommandResult, Inspection, Inspector, SearchLink, Searcher,
};

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
    Check(CheckReport),
    Inspect(Inspection),
    Search(Vec<SearchLink>),
    Drained,
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}
/// Dispatches commands through optional application services, then triggers background work.
pub(crate) struct Handlers<'a> {
    checks: Option<&'a mut dyn Checker>,
    inspections: Option<&'a mut dyn Inspector>,
    searches: Option<&'a mut dyn Searcher>,
    background: Option<&'a mut dyn BackgroundRunner>,
}
impl<'a> Handlers<'a> {
    /// Borrow the services available for this command without performing I/O.
    pub(crate) fn new(
        checks: Option<&'a mut dyn Checker>,
        inspections: Option<&'a mut dyn Inspector>,
        searches: Option<&'a mut dyn Searcher>,
        background: Option<&'a mut dyn BackgroundRunner>,
    ) -> Self {
        Self {
            checks,
            inspections,
            searches,
            background,
        }
    }
    /// Run one service operation and start background work only after foreground success.
    /// Service and background failures propagate without exposing partial results.
    pub(crate) fn execute(&mut self, request: Request) -> CommandResult<CommandResponse> {
        let result = match request {
            Request::Check(paths) => ResponseResult::Check(
                self.checks
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .check(&paths)?,
            ),
            Request::Inspect(ids) => ResponseResult::Inspect(
                self.inspections
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .inspect(&ids)?,
            ),
            Request::Search(request) => ResponseResult::Search(
                self.searches
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .search(&request)?,
            ),
            Request::Drain => {
                self.background
                    .as_deref_mut()
                    .ok_or_else(fail_unavailable)?
                    .drain()?;
                return Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                });
            }
        };
        let warning = match self.background.as_deref_mut() {
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
