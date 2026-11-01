use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, DocumentError, GraphDiagnostics,
    KnowledgeMetadata, NodeMetadata, PROVENANCE_BATCH_SIZE, ParsedFile, ProvenanceIssue,
    ProvenanceRequest, SourceProvenance, StorageError, StorageRead,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PATH: &str = "path";
const REASON: &str = "reason";
const ELEMENT_ID: &str = "element_id";
const TARGET: &str = "target";
const MISSING_SNAPSHOT: &str = "missing_source_snapshot";
const UNAVAILABLE_SNAPSHOT: &str = "source_snapshot_unavailable";
const SOURCE_NOT_FOUND: &str = "source_not_found";

/// Borrows graph and provenance observers for one pinned check/status operation.
pub(super) struct CheckObservations<'a> {
    /// Graph-only diagnostics over the same supplied read view.
    pub(super) graph: &'a dyn GraphDiagnostics,
    /// Source observations with bounded batching for the selected owners.
    pub(super) provenance: &'a dyn SourceProvenance,
}

impl std::fmt::Debug for CheckObservations<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CheckObservations")
            .finish_non_exhaustive()
    }
}

impl CheckObservations<'_> {
    /// Collects sorted graph and source-provenance findings from one supplied read view.
    ///
    /// # Errors
    /// Returns graph diagnostics, storage or provenance failures without publishing changes.
    pub(super) fn collect_check_problems(
        &self,
        view: &mut dyn StorageRead,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let mut problems = self.graph.find_problems(view, paths)?;
        problems.extend(self.find_source_problems(view, paths)?);
        Ok(sort_problems(problems))
    }

    fn find_source_problems(
        &self,
        view: &mut dyn StorageRead,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let nodes = view.get_nodes().map_err(map_storage_error)?;
        let sources = get_source_bindings(view)?;
        let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
        let empty = BTreeSet::new();
        let mut problems = Vec::new();
        let mut selected_nodes = nodes.iter().filter_map(|node| match &node.metadata {
            NodeMetadata::Knowledge(metadata)
                if selected.is_empty() || selected.contains(get_owner_path(&metadata.id)) =>
            {
                Some(metadata)
            }
            _ => None,
        });
        loop {
            let batch: Vec<_> = selected_nodes
                .by_ref()
                .take(PROVENANCE_BATCH_SIZE)
                .collect();
            if batch.is_empty() {
                break;
            }
            let requests: Vec<_> = batch
                .iter()
                .map(|metadata| ProvenanceRequest {
                    source_snapshot: metadata.source_snapshot.as_ref(),
                    sources: sources.get(&metadata.id).unwrap_or(&empty),
                })
                .collect();
            let observations = self
                .provenance
                .observe_sources_batch(&requests)
                .map_err(map_provenance_error)?;
            for (metadata, observation) in batch.into_iter().zip(observations) {
                problems.extend(
                    observation
                        .issues
                        .iter()
                        .map(|issue| format_source_problem(metadata, issue)),
                );
            }
        }
        Ok(problems)
    }
}

/// Compares parsed knowledge data with the selected pinned metadata.
/// Returns false when no parsed knowledge file is available or its element differs.
pub(super) fn is_current_knowledge(node: &KnowledgeMetadata, parsed: Option<&ParsedFile>) -> bool {
    parsed.is_some_and(|parsed| {
        parsed.elements.iter().any(|current| {
            current.element.id == node.id
                && current.element.hash == node.hash
                && current.element.lines == node.lines
        })
    })
}

/// Sorts findings by path, reason, element ID and target, removing duplicates.
pub(super) fn sort_problems(mut problems: Vec<CommandProblem>) -> Vec<CommandProblem> {
    problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
    problems.dedup_by(|left, right| get_sort_key(left) == get_sort_key(right));
    problems
}

fn get_source_bindings(
    view: &mut dyn StorageRead,
) -> CommandResult<BTreeMap<KnowledgeId, BTreeSet<SourceUri>>> {
    let mut sources: BTreeMap<KnowledgeId, BTreeSet<SourceUri>> = BTreeMap::new();
    for edge in view
        .get_edges_by_source_id(None)
        .map_err(map_storage_error)?
    {
        if let EdgeTarget::Source(source) = edge.get_target() {
            sources
                .entry(edge.get_source().clone())
                .or_default()
                .insert(source.clone());
        }
    }
    Ok(sources)
}

fn format_source_problem(metadata: &KnowledgeMetadata, issue: &ProvenanceIssue) -> CommandProblem {
    let (reason, message, target) = match issue {
        ProvenanceIssue::MissingSnapshot => (
            MISSING_SNAPSHOT,
            "knowledge source snapshot is missing",
            None,
        ),
        ProvenanceIssue::SnapshotUnavailable { snapshot } => (
            UNAVAILABLE_SNAPSHOT,
            "knowledge source snapshot is unavailable",
            Some(snapshot.as_str()),
        ),
        ProvenanceIssue::SourceNotFound { source } => (
            SOURCE_NOT_FOUND,
            "knowledge source is not found",
            Some(source.as_str()),
        ),
    };
    let mut details = BTreeMap::from([
        (PATH.into(), Value::from(get_owner_path(&metadata.id))),
        (REASON.into(), Value::from(reason)),
        (ELEMENT_ID.into(), Value::from(metadata.id.as_str())),
    ]);
    if let Some(target) = target {
        details.insert(TARGET.into(), Value::from(target));
    }
    CommandProblem {
        code: CommandErrorKind::ValidationFailed,
        message,
        details,
    }
}

fn get_owner_path(id: &KnowledgeId) -> &str {
    id.as_str()
        .split_once('#')
        .map_or(id.as_str(), |(path, _)| path)
}

fn get_sort_key(problem: &CommandProblem) -> [&str; 4] {
    [PATH, REASON, ELEMENT_ID, TARGET].map(|key| {
        problem
            .details
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
    })
}

fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read corpus for check",
        cause,
    )
}
fn map_provenance_error(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot observe knowledge sources",
        cause,
    )
}
