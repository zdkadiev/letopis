use super::output::response_encode;
use crate::service::{
    BackgroundRunner, CheckReport, CommandProblem, CommandResult, Configuration, Inspection,
    RefreshOutcome, SearchLink, StatusReport, Traversal,
};
use std::{io::Write, sync::Mutex};

#[cfg(test)]
#[path = "response_test.rs"]
mod tests;

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
    Initialized,
    Status(StatusReport),
    Config(Option<Configuration>),
    Index(RefreshOutcome),
    Check(CheckReport),
    Inspect(Inspection),
    Search(Vec<SearchLink>),
    Traverse(Traversal),
    Drained,
    /// Plain-text help or version output; the only result not rendered as JSON.
    Help(String),
}

/// One typed command result with the nonfatal warnings that accompany it.
#[derive(Debug)]
pub(crate) struct CommandResponse {
    /// Result the command produced.
    pub result: ResponseResult,
    /// Nonfatal problems reported alongside the result.
    pub warnings: Vec<CommandProblem>,
}

/// Owns the single X-1 response of one executable invocation.
///
/// The completion state records the winning exit code. A caller that loses the
/// race receives that code and never attempts a second output, including after
/// an earlier write failure.
#[derive(Debug, Default)]
pub(crate) struct ResponseWriter {
    completion_mutex: Mutex<Option<u8>>,
}

impl CommandResponse {
    /// Creates a response with no warnings.
    ///
    /// # Arguments
    ///
    /// * `result` — the typed result stored in the new response; its warnings
    ///   start empty.
    pub(crate) fn new(result: ResponseResult) -> Self {
        Self {
            result,
            warnings: Vec::new(),
        }
    }
}

impl ResponseWriter {
    /// Writes one complete response if output is still unclaimed.
    ///
    /// Encoding finishes before the lock is taken. The lock covers both writing
    /// and flushing. Returns the winning response exit code, or 1 for a poisoned
    /// lock, encoding failure or failed output. An encoding failure still claims
    /// the response, so a later caller never writes a second one.
    ///
    /// # Arguments
    ///
    /// * `output` — the writer that receives the envelope only if this caller
    ///   wins.
    /// * `response` — the typed command result to encode and write at most once.
    pub(crate) fn response_write(
        &self,
        output: &mut dyn Write,
        response: CommandResult<CommandResponse>,
    ) -> u8 {
        let encoded = response_encode(response);
        let Ok(mut completion) = self.completion_mutex.lock() else {
            return 1;
        };
        if let Some(exit) = *completion {
            return exit;
        }
        let exit = match encoded {
            Ok((bytes, exit))
                if output
                    .write_all(&bytes)
                    .and_then(|()| output.flush())
                    .is_ok() =>
            {
                exit
            }
            _ => 1,
        };
        *completion = Some(exit);
        exit
    }
}

/// Starts background work after a successful foreground command.
/// A start warning is added to the response; a start failure returns an error.
///
/// # Arguments
///
/// * `result` — the successful foreground result to return.
/// * `background` — the runner started once here; its warning is added to the
///   response and its error is returned.
pub(crate) fn finish_foreground(
    result: ResponseResult,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let warning = background.start_if_needed()?;
    Ok(CommandResponse {
        result,
        warnings: warning.into_iter().collect(),
    })
}
