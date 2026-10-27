use crate::service::{
    BackgroundRunner, CheckReport, CommandProblem, CommandResult, EmbeddingConfig, Inspection,
    SearchLink, StatusReport, Traversal,
};

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
