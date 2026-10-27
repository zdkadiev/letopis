use super::output::encode_response;
use crate::service::{
    BackgroundRunner, CheckReport, CommandProblem, CommandResult, EmbeddingConfig, Inspection,
    SearchLink, StatusReport, Traversal,
};
use std::{io::Write, sync::Mutex};

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
    Status(StatusReport),
    Config(Option<EmbeddingConfig>),
    Reindex(u64),
    Check(CheckReport),
    Inspect(Inspection),
    Search(Vec<SearchLink>),
    Traverse(Traversal),
    Drained,
    /// Plain-text help or version output; the only result not rendered as JSON.
    Help(String),
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}
impl CommandResponse {
    /// A response without warnings.
    pub(crate) fn new(result: ResponseResult) -> Self {
        Self {
            result,
            warnings: Vec::new(),
        }
    }
}

/// Start background work after a successful foreground command.
/// A launch warning is attached to the response; a readiness failure replaces it.
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

/// Owns the single X-1 response of one executable invocation.
///
/// The completion state records the winning exit code. A caller that loses the
/// race receives that code and never attempts a second output, including after
/// an earlier write failure.
#[derive(Debug, Default)]
pub(crate) struct ResponseWriter {
    completion: Mutex<Option<u8>>,
}

impl ResponseWriter {
    /// Serialize one complete response and write it if output is still unclaimed.
    ///
    /// Encoding finishes before the output guard is taken. The guard covers both
    /// writing and flushing. Returns the winning response exit code, or 1 for a
    /// poisoned state, encoding failure or failed output.
    pub(crate) fn write_response(
        &self,
        output: &mut dyn Write,
        response: CommandResult<CommandResponse>,
    ) -> u8 {
        let Ok((bytes, exit)) = encode_response(response) else {
            return 1;
        };
        let Ok(mut completion) = self.completion.lock() else {
            return 1;
        };
        if let Some(exit) = *completion {
            return exit;
        }
        let exit = if output
            .write_all(&bytes)
            .and_then(|()| output.flush())
            .is_ok()
        {
            exit
        } else {
            1
        };
        *completion = Some(exit);
        exit
    }
}

#[cfg(test)]
mod tests;
