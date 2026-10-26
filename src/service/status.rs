use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, EmbeddingConfig,
    EmbeddingProvider, EmbeddingStatusPolicy, EmbeddingStatusSummary, GraphDiagnostics,
    SourceProvenance, StatusFiles, StatusReader, StatusReport, StatusStorage, StorageError,
    StorageRead,
    check_problems::{collect_check_problems, sort_problems},
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

const REASON: &str = "reason";
const INVALID_CONFIG: &str = "invalid_embedding_config";
const QUERY_FAILURE: &str = "query_embedding_failed";
const EXHAUSTED: &str = "embedding_attempts_exhausted";

/// Caller-owned storage with borrowed, read-only status collaborators.
pub(crate) struct StatusDependencies<'a, S, G> {
    pub(crate) storage: S,
    pub(crate) files: &'a dyn StatusFiles,
    pub(crate) graph: &'a G,
    pub(crate) provenance: &'a dyn SourceProvenance,
    pub(crate) provider: &'a dyn EmbeddingProvider,
}

impl<S, G> fmt::Debug for StatusDependencies<'_, S, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusDependencies").finish_non_exhaustive()
    }
}

/// Observes the last publication and live coordination without changing either.
pub(crate) struct StatusService<'a, S, G> {
    deps: StatusDependencies<'a, S, G>,
}

impl<S, G> fmt::Debug for StatusService<'_, S, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusService").finish_non_exhaustive()
    }
}

impl<'a, S, G> StatusService<'a, S, G> {
    /// Assemble status collaborators without opening a view or reading configuration.
    pub(crate) fn new(deps: StatusDependencies<'a, S, G>) -> Self {
        Self { deps }
    }
}

impl<S: StatusStorage, G: GraphDiagnostics> StatusReader for StatusService<'_, S, G> {
    fn read_status(&mut self) -> CommandResult<StatusReport> {
        let (policy, config_problem) = read_config_policy(self.deps.files, self.deps.provider);
        let observation = self
            .deps
            .storage
            .begin_status_read()
            .map_err(map_storage_error)?;
        let mut report = if let Some(mut view) = observation {
            read_published(&mut view, &policy, self.deps.graph, self.deps.provenance)?
        } else {
            StatusReport {
                revision: None,
                embedding_backlog: 0,
                embedding_failed: 0,
                locks: Vec::new(),
                problems: Vec::new(),
            }
        };
        if let Some(problem) = config_problem {
            report.problems.push(problem);
        }
        report.locks = self.deps.files.observe_locks()?;
        sort_problems(&mut report.problems);
        Ok(report)
    }
}

fn read_published<R: StorageRead, G: GraphDiagnostics>(
    view: &mut R,
    policy: &EmbeddingStatusPolicy,
    graph: &G,
    provenance: &dyn SourceProvenance,
) -> CommandResult<StatusReport> {
    let revision = view.get_revision();
    let mut problems = collect_check_problems(view, &[], graph, provenance)?;
    let summary = view
        .read_embedding_status(policy)
        .map_err(map_storage_error)?;
    problems.extend(collect_exhausted(&summary));
    if view
        .read_query_error()
        .map_err(map_storage_error)?
        .is_some()
    {
        problems.push(build_problem(
            CommandErrorKind::EmbeddingFailed,
            "query embedding failed",
            QUERY_FAILURE,
            None,
        ));
    }
    Ok(StatusReport {
        revision: Some(revision),
        embedding_backlog: summary.backlog,
        embedding_failed: summary.failed,
        locks: Vec::new(),
        problems,
    })
}

fn read_config_policy(
    files: &dyn StatusFiles,
    provider: &dyn EmbeddingProvider,
) -> (EmbeddingStatusPolicy, Option<CommandProblem>) {
    let Ok(bytes) = files.read_config() else {
        return (
            EmbeddingStatusPolicy::Unavailable,
            Some(invalid_config_problem()),
        );
    };
    let Some(bytes) = bytes else {
        return (EmbeddingStatusPolicy::Unavailable, None);
    };
    let Ok(config) = EmbeddingConfig::parse(&bytes) else {
        return (
            EmbeddingStatusPolicy::Unavailable,
            Some(invalid_config_problem()),
        );
    };
    if provider.validate_config(&config).is_err() {
        return (
            EmbeddingStatusPolicy::Unavailable,
            Some(invalid_config_problem()),
        );
    }
    (
        EmbeddingStatusPolicy::Ready {
            identity: config.get_identity(),
            max_attempts: config.get_max_attempts(),
        },
        None,
    )
}

fn collect_exhausted(summary: &EmbeddingStatusSummary) -> Vec<CommandProblem> {
    summary
        .exhausted
        .iter()
        .map(|failure| {
            build_problem(
                CommandErrorKind::EmbeddingFailed,
                "embedding attempts exhausted",
                EXHAUSTED,
                Some(&failure.search_hash.to_hex()),
            )
        })
        .collect()
}

fn invalid_config_problem() -> CommandProblem {
    build_problem(
        CommandErrorKind::ValidationFailed,
        "invalid embedding configuration",
        INVALID_CONFIG,
        None,
    )
}

fn build_problem(
    code: CommandErrorKind,
    message: &'static str,
    reason: &'static str,
    target: Option<&str>,
) -> CommandProblem {
    let mut details = BTreeMap::from([(REASON.into(), Value::from(reason))]);
    if let Some(target) = target {
        details.insert("target".into(), Value::from(target));
    }
    CommandProblem {
        code,
        message,
        details,
    }
}

fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read published status",
        cause,
    )
}

#[cfg(test)]
mod tests;
