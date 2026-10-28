use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, Configuration,
    EmbeddingProvider, EmbeddingStatusPolicy, EmbeddingStatusSummary, GraphDiagnostics,
    SourceProvenance, StatusFiles, StatusReader, StatusReport, StatusStorage, StorageError,
    StorageRead,
    check_problems::{collect_check_problems, sort_problems},
    validate_configuration,
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt};

const REASON: &str = "reason";
const INVALID_CONFIG: &str = "invalid_embedding_config";
const QUERY_FAILURE: &str = "query_embedding_failed";
const EXHAUSTED: &str = "embedding_attempts_exhausted";

/// Observes the last publication and live coordination without changing either.
pub(crate) struct StatusService<'a, S, G> {
    pub(crate) storage: S,
    pub(crate) files: &'a dyn StatusFiles,
    pub(crate) graph: &'a G,
    pub(crate) provenance: &'a dyn SourceProvenance,
    pub(crate) provider: &'a dyn EmbeddingProvider,
}

impl<S, G> fmt::Debug for StatusService<'_, S, G> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusService").finish_non_exhaustive()
    }
}

impl<S: StatusStorage, G: GraphDiagnostics> StatusReader for StatusService<'_, S, G> {
    fn read_status(&mut self) -> CommandResult<StatusReport> {
        let (policy, config_problem) = read_config_policy(self.files, self.provider);
        let observation = self
            .storage
            .begin_status_read()
            .map_err(map_storage_error)?;
        let mut report = if let Some(mut view) = observation {
            read_published(&mut view, &policy, self.graph, self.provenance)?
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
        report.locks = self.files.observe_locks()?;
        report.problems = sort_problems(report.problems);
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
    if !matches!(policy, EmbeddingStatusPolicy::Disabled) {
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
    let Ok(configuration) = validate_configuration(&bytes, provider) else {
        return (
            EmbeddingStatusPolicy::Unavailable,
            Some(invalid_config_problem()),
        );
    };
    match configuration {
        Configuration::Enabled(config) => (
            EmbeddingStatusPolicy::Ready {
                identity: config.get_identity(),
                max_attempts: config.get_max_attempts(),
            },
            None,
        ),
        Configuration::Unconfigured { .. } | Configuration::Disabled(_) => {
            (EmbeddingStatusPolicy::Disabled, None)
        }
    }
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
