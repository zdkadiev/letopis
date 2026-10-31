use super::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, DocumentError, GraphDiagnostics,
    KnowledgeMetadata, NodeMetadata, PROVENANCE_BATCH_SIZE, ProvenanceIssue, ProvenanceRequest,
    SourceProvenance, StorageError, StorageRead,
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

pub(super) fn collect_check_problems<R: StorageRead, G: GraphDiagnostics>(
    view: &mut R,
    paths: &[RepoPath],
    graph: &G,
    provenance: &dyn SourceProvenance,
) -> CommandResult<Vec<CommandProblem>> {
    let mut problems = graph.find_problems(view, paths)?;
    problems.extend(find_source_problems(view, paths, provenance)?);
    Ok(sort_problems(problems))
}

pub(super) fn sort_problems(mut problems: Vec<CommandProblem>) -> Vec<CommandProblem> {
    problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
    problems.dedup_by(|left, right| get_sort_key(left) == get_sort_key(right));
    problems
}

fn find_source_problems<R: StorageRead>(
    view: &mut R,
    paths: &[RepoPath],
    provenance: &dyn SourceProvenance,
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
        let observations = provenance
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

fn get_source_bindings<R: StorageRead>(
    view: &mut R,
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
