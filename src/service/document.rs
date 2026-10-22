use super::{
    CacheWrite, CorpusFiles, DocumentError, DocumentErrorKind, DocumentResult, Documents, FileRow,
    FileStat, FileSystem, Markdown, RefreshOutcome, Storage, StorageRead,
    projection::{EffectiveGraph, PreparedProjection, prepare_file, prepare_removal, publish},
};
use crate::domain::{RepoPath, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const RACY_WINDOW_NS: i64 = 1_000_000_000;

pub(crate) struct DocumentService<G, F, M, S> {
    git: G,
    filesystem: F,
    markdown: M,
    storage: S,
}
struct FileState {
    revision: u64,
    files: BTreeMap<RepoPath, FileRow>,
}
pub(super) struct Observation {
    pub stat: FileStat,
    pub hash: Sha256,
}
struct Delta {
    updated: BTreeMap<RepoPath, Observation>,
    // True means Git still selected this path, but it was consistently missing.
    removed: BTreeMap<RepoPath, bool>,
}
impl Delta {
    fn is_empty(&self) -> bool {
        self.updated.is_empty() && self.removed.is_empty()
    }
}
impl<G, F, M, S> DocumentService<G, F, M, S> {
    pub(crate) fn new(git: G, filesystem: F, markdown: M, storage: S) -> Self {
        Self {
            git,
            filesystem,
            markdown,
            storage,
        }
    }
}
impl<G: CorpusFiles, F: FileSystem, M: Markdown, S: Storage> DocumentService<G, F, M, S> {
    fn read_file_state(&mut self) -> DocumentResult<FileState> {
        let mut read = self.storage.begin_read()?;
        Ok(FileState {
            revision: read.get_revision(),
            files: read
                .list_files()?
                .into_iter()
                .map(|row| (row.path.clone(), row))
                .collect(),
        })
    }

    fn detect_changes(&self, state: &FileState) -> DocumentResult<Delta> {
        let candidates: BTreeSet<_> = self.git.list_candidates()?.into_iter().collect();
        let mut delta = Delta {
            updated: BTreeMap::new(),
            removed: BTreeMap::new(),
        };
        for path in state
            .files
            .keys()
            .filter(|path| !candidates.contains(*path))
        {
            delta.removed.insert(path.clone(), false);
        }
        for path in candidates {
            let previous = state.files.get(&path);
            let Some(stat) = self.filesystem.stat_file(&path)? else {
                if previous.is_some() {
                    delta.removed.insert(path, true);
                }
                continue;
            };
            if let Some(observation) = detect_file(&self.filesystem, &path, stat, previous)? {
                delta.updated.insert(path, observation);
            }
        }
        Ok(delta)
    }

    fn publish_changes(
        &mut self,
        state: &FileState,
        delta: Delta,
    ) -> DocumentResult<RefreshOutcome> {
        // Keep only metadata after releasing this pinned read, before BEGIN IMMEDIATE.
        let mut graph = {
            let mut read = self.storage.begin_read()?;
            if read.get_revision() != state.revision {
                return Err(fail_changed());
            }
            EffectiveGraph::read(&mut read)?
        };
        let mut unit = self.storage.begin_cache_write()?;
        let mut outcome = build_outcome(state.revision, true);
        for (path, check_missing) in delta.removed {
            if check_missing && self.filesystem.stat_file(&path)?.is_some() {
                return Err(fail_changed());
            }
            let prepared = prepare_removal(state.revision, path, &graph);
            apply_projection(&mut unit, &mut graph, prepared, &mut outcome)?;
        }
        for (path, observation) in delta.updated {
            let prepared = prepare_file(
                &self.filesystem,
                &self.markdown,
                state.revision,
                &path,
                &observation,
                state.files.get(&path),
                &graph,
            )?;
            apply_projection(&mut unit, &mut graph, prepared, &mut outcome)?;
        }
        graph.reconcile_sources(&self.filesystem, &mut unit)?;
        outcome.revision = unit.commit()?;
        Ok(outcome)
    }
}
impl<G: CorpusFiles, F: FileSystem, M: Markdown, S: Storage> Documents
    for DocumentService<G, F, M, S>
{
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        let preliminary = self.read_file_state()?;
        if self.detect_changes(&preliminary)?.is_empty() && preliminary.revision != 0 {
            return Ok(build_outcome(preliminary.revision, false));
        }
        drop(preliminary);
        let _lock = self.filesystem.lock_index()?;
        let state = self.read_file_state()?;
        let delta = self.detect_changes(&state)?;
        if delta.is_empty() && state.revision != 0 {
            return Ok(build_outcome(state.revision, false));
        }
        self.publish_changes(&state, delta)
    }
}

fn detect_file<F: FileSystem>(
    filesystem: &F,
    path: &RepoPath,
    stat: FileStat,
    previous: Option<&FileRow>,
) -> DocumentResult<Option<Observation>> {
    let same_stat = previous.is_some_and(|row| matches_stat(row, stat));
    if same_stat && previous.is_some_and(|row| !is_racy(row, filesystem.get_time_ns().ok())) {
        return Ok(None);
    }
    let captured = filesystem.read_file(path)?.ok_or_else(fail_changed)?;
    if captured.stat != stat {
        return Err(fail_changed());
    }
    let hash = Sha256::digest(&captured.bytes);
    if same_stat && previous.is_some_and(|row| row.hash == hash) {
        return Ok(None);
    }
    Ok(Some(Observation { stat, hash }))
}

fn matches_stat(previous: &FileRow, stat: FileStat) -> bool {
    previous.size == stat.size && previous.mtime_ns == stat.mtime_ns && previous.inode == stat.inode
}

fn is_racy(previous: &FileRow, now: Option<i64>) -> bool {
    let Some(now) = now else {
        return true;
    };
    if now < previous.checked_at_ns {
        return true;
    }
    previous
        .checked_at_ns
        .checked_sub(RACY_WINDOW_NS)
        .is_none_or(|boundary| previous.mtime_ns >= boundary)
}

fn apply_projection<W: CacheWrite>(
    unit: &mut W,
    graph: &mut EffectiveGraph,
    prepared: PreparedProjection,
    outcome: &mut RefreshOutcome,
) -> DocumentResult<()> {
    if prepared.base_revision != outcome.revision {
        return Err(fail_changed());
    }
    prepared.add_counts(outcome);
    graph.apply(&prepared);
    publish(unit, prepared)
}

fn build_outcome(revision: u64, published: bool) -> RefreshOutcome {
    RefreshOutcome {
        revision,
        published,
        files_updated: 0,
        files_removed: 0,
        invalid_files_updated: 0,
    }
}

pub(super) fn fail_changed() -> DocumentError {
    DocumentError::new(
        DocumentErrorKind::InternalError,
        "documents changed during refresh",
    )
}

#[cfg(test)]
mod tests;
