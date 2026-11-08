use super::utils::{
    ELEMENT_ID, PATH, REASON, TARGET, get_detail_by_key, get_owner_path_by_id, get_sort_key,
};
use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandProblem, CommandResult, DiagnosticKind,
    FileRow, GraphDiagnostics, NodeMetadata, TransactionError,
};
use crate::domain::{GraphProblem, KnowledgeId, RepoPath, find_graph_problems};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PARSER_KINDS: [DiagnosticKind; 10] = [
    DiagnosticKind::InvalidUtf8,
    DiagnosticKind::InvalidMetadata,
    DiagnosticKind::InvalidAnchor,
    DiagnosticKind::DuplicateAnchor,
    DiagnosticKind::OrphanMetadata,
    DiagnosticKind::NestedElement,
    DiagnosticKind::InvalidLinkTitle,
    DiagnosticKind::InvalidTarget,
    DiagnosticKind::LinkPathMismatch,
    DiagnosticKind::SelfLoop,
];

/// Derives safe current findings from a caller-owned read view.
#[derive(Debug)]
pub(crate) struct GraphDiagnosticsService<C> {
    cache: std::rc::Rc<C>,
}

impl<C> GraphDiagnosticsService<C> {
    pub(crate) fn new(cache: std::rc::Rc<C>) -> Self {
        Self { cache }
    }
}

impl<C: CacheDatabase> GraphDiagnostics for GraphDiagnosticsService<C> {
    fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
        self.cache
            .with_read_tx(|| {
                let nodes = self
                    .cache
                    .nodes_get()
                    .map_err(map_error_to_graph_diagnostics_command_error)?;
                let edges = self
                    .cache
                    .edges_get_by_source_id(None)
                    .map_err(map_error_to_graph_diagnostics_command_error)?;
                let files = self
                    .cache
                    .files_get()
                    .map_err(map_error_to_graph_diagnostics_command_error)?;
                let knowledge: BTreeSet<KnowledgeId> = nodes
                    .into_iter()
                    .filter_map(|node| match node.metadata {
                        NodeMetadata::Knowledge(metadata) => Some(metadata.id),
                        NodeMetadata::Source(_) => None,
                    })
                    .collect();
                let edges = edges.into_iter().collect();
                let mut problems: Vec<CommandProblem> = files
                    .iter()
                    .filter(|file| !file.valid)
                    .map(format_invalid_file)
                    .collect();
                for problem in find_graph_problems(&knowledge, &edges) {
                    problems.extend(format_graph_problem(&problem));
                }
                let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
                problems.retain(|problem| {
                    selected.is_empty() || selected.contains(get_detail_by_key(problem, PATH))
                });
                problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
                Ok(problems)
            })
            .map_err(map_transaction_error_to_graph_diagnostics_command_error)
    }
}

fn format_invalid_file(file: &FileRow) -> CommandProblem {
    let kind = classify_cached_message(file.error.as_deref());
    let (reason, message) = kind.map_or(("invalid_markup", "invalid knowledge file"), |kind| {
        (kind.get_code(), kind.get_message())
    });
    CommandProblem {
        code: CommandErrorKind::ValidationFailed,
        message,
        details: BTreeMap::from([
            (PATH.into(), Value::from(file.path.as_str())),
            (REASON.into(), Value::from(reason)),
        ]),
    }
}

fn classify_cached_message(message: Option<&str>) -> Option<DiagnosticKind> {
    PARSER_KINDS
        .into_iter()
        .find(|kind| Some(kind.get_message()) == message)
}

fn format_graph_problem(problem: &GraphProblem) -> Vec<CommandProblem> {
    match problem {
        GraphProblem::DanglingTarget { source, target } => vec![CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "knowledge target is missing",
            details: BTreeMap::from([
                (PATH.into(), Value::from(get_owner_path_by_id(source))),
                (REASON.into(), Value::from("dangling_target")),
                (ELEMENT_ID.into(), Value::from(source.as_str())),
                (TARGET.into(), Value::from(target.as_str())),
            ]),
        }],
        GraphProblem::IncludesCycle { members } => format_cycle(members),
    }
}

fn format_cycle(members: &[KnowledgeId]) -> Vec<CommandProblem> {
    let mut representatives = BTreeMap::new();
    // Domain components are sorted, so the first member of each file is its minimum.
    for member in members {
        representatives
            .entry(get_owner_path_by_id(member))
            .or_insert(member);
    }
    representatives
        .into_iter()
        .map(|(path, member)| CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "knowledge participates in an includes cycle",
            details: BTreeMap::from([
                (PATH.into(), Value::from(path)),
                (REASON.into(), Value::from("includes_cycle")),
                (ELEMENT_ID.into(), Value::from(member.as_str())),
            ]),
        })
        .collect()
}

/// Converts a failure raised while reading graph diagnostics to a command error.
///
/// # Arguments
///
/// * `cause` — Failure from a diagnostics read.
fn map_error_to_graph_diagnostics_command_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read graph diagnostics",
        cause,
    )
}

/// Converts a transaction failure from the diagnostics read to a command error.
///
/// # Arguments
///
/// * `error` — Transaction failure from the pinned read.
fn map_transaction_error_to_graph_diagnostics_command_error(
    error: TransactionError<CommandError>,
) -> CommandError {
    match error {
        TransactionError::Operation(error) => error,
        TransactionError::Storage(error) => map_error_to_graph_diagnostics_command_error(error),
    }
}

#[cfg(test)]
#[path = "graph_diagnostics_test.rs"]
mod tests;
