use super::utils::{ELEMENT_ID, PATH, REASON, TARGET, get_detail, get_owner_path, get_sort_key};
use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandProblem, CommandResult, DiagnosticKind,
    FileRow, GraphDiagnostics, NodeMetadata,
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
                let nodes = self.cache.nodes_get().map_err(|error| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot read graph diagnostics",
                        error,
                    )
                })?;
                let edges = self.cache.edges_get_by_source_id(None).map_err(|error| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot read graph diagnostics",
                        error,
                    )
                })?;
                let files = self.cache.files_get().map_err(|error| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot read graph diagnostics",
                        error,
                    )
                })?;
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
                    selected.is_empty() || selected.contains(get_detail(problem, PATH))
                });
                problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
                Ok(problems)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot read graph diagnostics",
                    error,
                ),
            })
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
                (PATH.into(), Value::from(get_owner_path(source))),
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
            .entry(get_owner_path(member))
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

#[cfg(test)]
#[path = "graph_diagnostics_test.rs"]
mod tests;
