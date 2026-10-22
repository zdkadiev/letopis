use super::input::Request;
use crate::service::{
    Background, CheckReport, Checks, CommandError, CommandErrorKind, CommandProblem, CommandResult,
    Inspection, Inspections,
};

#[derive(Debug)]
pub(crate) enum ResponseResult {
    Check(CheckReport),
    Inspect(Inspection),
    Drained,
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}
pub(crate) struct Handlers<'a> {
    checks: Option<&'a mut dyn Checks>,
    inspections: Option<&'a mut dyn Inspections>,
    background: Option<&'a mut dyn Background>,
}
impl<'a> Handlers<'a> {
    pub(crate) fn new(
        checks: Option<&'a mut dyn Checks>,
        inspections: Option<&'a mut dyn Inspections>,
        background: Option<&'a mut dyn Background>,
    ) -> Self {
        Self {
            checks,
            inspections,
            background,
        }
    }
    pub(crate) fn execute(&mut self, request: Request) -> CommandResult<CommandResponse> {
        let result = match request {
            Request::Check(paths) => ResponseResult::Check(
                self.checks
                    .as_deref_mut()
                    .ok_or_else(unavailable)?
                    .check(&paths)?,
            ),
            Request::Inspect(ids) => ResponseResult::Inspect(
                self.inspections
                    .as_deref_mut()
                    .ok_or_else(unavailable)?
                    .inspect(&ids)?,
            ),
            Request::Drain => {
                self.background
                    .as_deref_mut()
                    .ok_or_else(unavailable)?
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
fn unavailable() -> CommandError {
    CommandError::new(
        CommandErrorKind::InternalError,
        "operation unavailable in this build",
    )
}
