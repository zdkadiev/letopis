use super::{
    CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem, CommandResult,
    DocumentError, GraphDiagnostics, KnowledgeMetadata, NodeMetadata, ProvenanceIssue,
    ProvenanceRequest, Refresher, SourceProvenance, Storage, StorageError, StorageRead,
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

/// Refreshes and validates current knowledge through one pinned cache view.
#[derive(Debug)]
pub(crate) struct CheckService<D, S, G, P> {
    refresher: D,
    storage: S,
    diagnostics: G,
    provenance: P,
}

impl<D, S, G, P> CheckService<D, S, G, P> {
    /// Assemble validation collaborators without refreshing or opening a read view.
    pub(crate) fn new(refresher: D, storage: S, diagnostics: G, provenance: P) -> Self {
        Self {
            refresher,
            storage,
            diagnostics,
            provenance,
        }
    }
}

impl<D: Refresher, S: Storage, G: GraphDiagnostics, P: SourceProvenance> Checker
    for CheckService<D, S, G, P>
{
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
        self.refresher.refresh().map_err(map_refresh_error)?;
        let mut view = self.storage.begin_read().map_err(map_storage_error)?;
        let mut errors = self.diagnostics.find_problems(&mut view, paths)?;
        errors.extend(find_source_problems(&mut view, paths, &self.provenance)?);
        errors.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
        errors.dedup_by(|left, right| get_sort_key(left) == get_sort_key(right));
        Ok(CheckReport { errors })
    }
}

fn find_source_problems<R: StorageRead, P: SourceProvenance>(
    view: &mut R,
    paths: &[RepoPath],
    provenance: &P,
) -> CommandResult<Vec<CommandProblem>> {
    let nodes = view.list_nodes().map_err(map_storage_error)?;
    let sources = read_source_bindings(view)?;
    let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
    let empty = BTreeSet::new();
    let mut problems = Vec::new();
    for node in nodes {
        let NodeMetadata::Knowledge(metadata) = node.metadata else {
            continue;
        };
        if !selected.is_empty() && !selected.contains(get_owner_path(&metadata.id)) {
            continue;
        }
        let observation = provenance
            .observe_sources(ProvenanceRequest {
                source_snapshot: metadata.source_snapshot.as_ref(),
                sources: sources.get(&metadata.id).unwrap_or(&empty),
            })
            .map_err(map_provenance_error)?;
        problems.extend(
            observation
                .issues
                .iter()
                .map(|issue| format_source_problem(&metadata, issue)),
        );
    }
    Ok(problems)
}

fn read_source_bindings<R: StorageRead>(
    view: &mut R,
) -> CommandResult<BTreeMap<KnowledgeId, BTreeSet<SourceUri>>> {
    let mut sources: BTreeMap<KnowledgeId, BTreeSet<SourceUri>> = BTreeMap::new();
    for edge in view.list_edges(None).map_err(map_storage_error)? {
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

fn map_refresh_error(cause: DocumentError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot refresh corpus for check",
        cause,
    )
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

#[cfg(test)]
mod tests;
