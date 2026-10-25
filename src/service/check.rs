use super::{
    CheckReport, Checker, CommandError, CommandErrorKind, CommandProblem, CommandResult,
    DocumentError, GraphDiagnostics, KnowledgeMetadata, NodeMetadata, Refresher, SnapshotError,
    SnapshotState, SnapshotVerifier, Storage, StorageError, StorageRead,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, TreeOid};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PATH: &str = "path";
const REASON: &str = "reason";
const ELEMENT_ID: &str = "element_id";
const TARGET: &str = "target";
const MISSING_SNAPSHOT: &str = "missing_source_snapshot";
const UNAVAILABLE_SNAPSHOT: &str = "source_snapshot_unavailable";

/// Refreshes and validates current knowledge through one pinned cache view.
#[derive(Debug)]
pub(crate) struct CheckService<D, S, G, V> {
    refresher: D,
    storage: S,
    diagnostics: G,
    snapshots: V,
}

struct SnapshotChecks<'a, V> {
    snapshots: &'a V,
    outcomes: BTreeMap<TreeOid, SnapshotState>,
}

impl<D, S, G, V> CheckService<D, S, G, V> {
    /// Assemble validation collaborators without refreshing or opening a read view.
    pub(crate) fn new(refresher: D, storage: S, diagnostics: G, snapshots: V) -> Self {
        Self {
            refresher,
            storage,
            diagnostics,
            snapshots,
        }
    }
}

impl<D: Refresher, S: Storage, G: GraphDiagnostics, V: SnapshotVerifier> Checker
    for CheckService<D, S, G, V>
{
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
        self.refresher.refresh().map_err(map_refresh_error)?;
        let mut view = self.storage.begin_read().map_err(map_storage_error)?;
        let mut errors = self.diagnostics.find_problems(&mut view, paths)?;
        let mut snapshots = SnapshotChecks {
            snapshots: &self.snapshots,
            outcomes: BTreeMap::new(),
        };
        errors.extend(snapshots.find_problems(&mut view, paths)?);
        errors.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
        errors.dedup_by(|left, right| get_sort_key(left) == get_sort_key(right));
        Ok(CheckReport { errors })
    }
}

impl<V: SnapshotVerifier> SnapshotChecks<'_, V> {
    fn find_problems<R: StorageRead>(
        &mut self,
        view: &mut R,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let nodes = view.list_nodes().map_err(map_storage_error)?;
        let external: BTreeSet<KnowledgeId> = view
            .list_edges(None)
            .map_err(map_storage_error)?
            .into_iter()
            .filter(|edge| matches!(edge.get_target(), EdgeTarget::Source(_)))
            .map(|edge| edge.get_source().clone())
            .collect();
        let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
        let mut problems = Vec::new();
        for node in nodes {
            let NodeMetadata::Knowledge(metadata) = node.metadata else {
                continue;
            };
            if !selected.is_empty() && !selected.contains(get_owner_path(&metadata.id)) {
                continue;
            }
            if let Some(problem) = self.find_problem(&metadata, &external)? {
                problems.push(problem);
            }
        }
        Ok(problems)
    }

    fn find_problem(
        &mut self,
        metadata: &KnowledgeMetadata,
        external: &BTreeSet<KnowledgeId>,
    ) -> CommandResult<Option<CommandProblem>> {
        let Some(oid) = &metadata.source_snapshot else {
            return Ok(external
                .contains(&metadata.id)
                .then(|| format_snapshot_problem(metadata, None)));
        };
        let state = match self.outcomes.get(oid) {
            Some(state) => *state,
            None => {
                let state = self
                    .snapshots
                    .verify_snapshot(oid)
                    .map_err(map_snapshot_error)?;
                self.outcomes.insert(oid.clone(), state);
                state
            }
        };
        Ok((state != SnapshotState::Available)
            .then(|| format_snapshot_problem(metadata, Some(oid))))
    }
}

fn format_snapshot_problem(metadata: &KnowledgeMetadata, oid: Option<&TreeOid>) -> CommandProblem {
    let (reason, message) = if oid.is_some() {
        (
            UNAVAILABLE_SNAPSHOT,
            "knowledge source snapshot is unavailable",
        )
    } else {
        (MISSING_SNAPSHOT, "knowledge source snapshot is missing")
    };
    let mut details = BTreeMap::from([
        (PATH.into(), Value::from(get_owner_path(&metadata.id))),
        (REASON.into(), Value::from(reason)),
        (ELEMENT_ID.into(), Value::from(metadata.id.as_str())),
    ]);
    if let Some(oid) = oid {
        details.insert(TARGET.into(), Value::from(oid.as_str()));
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

fn map_snapshot_error(cause: SnapshotError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot verify source snapshot",
        cause,
    )
}

#[cfg(test)]
mod tests;
