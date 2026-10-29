use super::{
    BasisRetainer, CacheWrite, CorpusLister, DocumentError, DocumentErrorKind, DocumentResult,
    FileRow, FileStat, FileSystem, MarkdownCodec, RefreshOutcome, Refresher, SourceProvenance,
    Storage, StorageRead,
    projection::{
        EffectiveGraph, FileProjection, PreparedProjection, prepare_file, prepare_removal, publish,
    },
};
use crate::domain::{RepoPath, Sha256, TreeOid};
use basis::{BasisPreparation, prepare_basis};

use std::collections::{BTreeMap, BTreeSet};

mod basis;

const RACY_WINDOW_NS: i64 = 1_000_000_000;

/// Refreshes and retains the effective document projection from independently owned collaborators.
#[derive(Debug)]
pub(crate) struct DocumentRefreshService<G, F, M, S, P> {
    /// Lists the current corpus candidates.
    pub(crate) git: G,
    /// Captures documents and owns the index exclusion lock.
    pub(crate) filesystem: F,
    /// Parses canonical document metadata and source bindings.
    pub(crate) markdown: M,
    /// Reads the current projection and publishes cache changes atomically.
    pub(crate) storage: S,
    /// Observes source bindings and retains author-declared roots.
    pub(crate) provenance: P,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PublicationMode {
    Incremental,
    Full,
}
impl PublicationMode {
    pub(super) fn get_previous(self, previous: Option<&FileRow>) -> Option<&FileRow> {
        previous.filter(|_| self == Self::Incremental)
    }
}
#[derive(Clone, Copy)]
pub(super) enum SnapshotVerification {
    Root,
    Closure,
}
impl SnapshotVerification {
    fn validate<P: BasisRetainer>(
        self,
        provenance: &P,
        snapshots: &BTreeSet<TreeOid>,
    ) -> DocumentResult<()> {
        match self {
            Self::Root => provenance.retain_snapshots(snapshots),
            Self::Closure => provenance.verify_snapshots(snapshots),
        }
    }
}
struct FileState {
    revision: u64,
    source_snapshots: BTreeSet<TreeOid>,
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
impl<
    G: CorpusLister,
    F: FileSystem,
    M: MarkdownCodec,
    S: Storage,
    P: SourceProvenance + BasisRetainer,
> Refresher for DocumentRefreshService<G, F, M, S, P>
{
    fn refresh_verified(&mut self) -> DocumentResult<RefreshOutcome> {
        let _lock = self.filesystem.lock_index()?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(PublicationMode::Incremental, SnapshotVerification::Closure)
    }

    fn reindex(&mut self) -> DocumentResult<RefreshOutcome> {
        let _lock = self.filesystem.lock_index()?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(PublicationMode::Full, SnapshotVerification::Closure)
    }

    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        let preliminary = self.read_file_state()?;
        if preliminary.revision != 0
            && self
                .detect_changes(&preliminary, PublicationMode::Incremental)?
                .is_empty()
            && preliminary.source_snapshots.is_empty()
        {
            return Ok(build_outcome(preliminary.revision, false));
        }
        #[cfg(test)]
        reach_checkpoint("detected");
        drop(preliminary);
        let _lock = self.filesystem.lock_index()?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(PublicationMode::Incremental, SnapshotVerification::Root)
    }
}
impl<
    G: CorpusLister,
    F: FileSystem,
    M: MarkdownCodec,
    S: Storage,
    P: SourceProvenance + BasisRetainer,
> DocumentRefreshService<G, F, M, S, P>
{
    fn publish_current(
        &mut self,
        requested: PublicationMode,
        verification: SnapshotVerification,
    ) -> DocumentResult<RefreshOutcome> {
        let state = self.read_file_state()?;
        let mode = if state.revision == 0 {
            PublicationMode::Full
        } else {
            requested
        };
        let delta = self.detect_changes(&state, mode)?;
        #[cfg(test)]
        reach_checkpoint("redetected");
        if delta.is_empty() && mode == PublicationMode::Incremental {
            verification.validate(&self.provenance, &state.source_snapshots)?;
            return Ok(build_outcome(state.revision, false));
        }
        self.publish_changes(&state, delta, mode, verification)
    }

    fn read_file_state(&mut self) -> DocumentResult<FileState> {
        let mut read = self.storage.begin_read()?;
        Ok(FileState {
            revision: read.get_revision(),
            source_snapshots: read.list_source_snapshots()?,
            files: read
                .list_files()?
                .into_iter()
                .map(|row| (row.path.clone(), row))
                .collect(),
        })
    }

    fn detect_changes(&self, state: &FileState, mode: PublicationMode) -> DocumentResult<Delta> {
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
            if let Some(observation) =
                detect_file(&self.filesystem, &path, stat, mode.get_previous(previous))?
            {
                delta.updated.insert(path, observation);
            }
        }
        Ok(delta)
    }

    fn publish_changes(
        &mut self,
        state: &FileState,
        delta: Delta,
        mode: PublicationMode,
        verification: SnapshotVerification,
    ) -> DocumentResult<RefreshOutcome> {
        let mut graph = self.prepare_publication(state, &delta, mode, verification)?;
        let mut unit = self.storage.begin_cache_write()?;
        #[cfg(test)]
        reach_checkpoint("begun");
        let mut outcome = build_outcome(state.revision, true);
        for (path, check_missing) in delta.removed {
            if check_missing && self.filesystem.stat_file(&path)?.is_some() {
                return Err(fail_changed());
            }
            let prepared = prepare_removal(state.revision, path, &graph);
            graph.apply(&prepared);
            outcome = apply_projection(&mut unit, prepared, outcome)?;
        }
        for (path, observation) in delta.updated {
            let prepared = prepare_file(&FileProjection {
                filesystem: &self.filesystem,
                markdown: &self.markdown,
                base_revision: state.revision,
                path: &path,
                observed: &observation,
                previous: mode.get_previous(state.files.get(&path)),
                graph: &graph,
            })?;
            graph.apply(&prepared);
            outcome = apply_projection(&mut unit, prepared, outcome)?;
        }
        graph.reconcile_sources(&self.filesystem, &mut unit)?;
        #[cfg(test)]
        reach_checkpoint("before_commit");
        #[cfg(test)]
        tests::process::fail_late(&mut unit)?;
        outcome.revision = unit.commit()?;
        #[cfg(test)]
        reach_checkpoint("after_commit");
        Ok(outcome)
    }

    fn prepare_publication(
        &mut self,
        state: &FileState,
        delta: &Delta,
        mode: PublicationMode,
        verification: SnapshotVerification,
    ) -> DocumentResult<EffectiveGraph> {
        // Keep only metadata after releasing this pinned read, before BEGIN IMMEDIATE.
        let mut graph = {
            let mut read = self.storage.begin_read()?;
            if read.get_revision() != state.revision {
                return Err(fail_changed());
            }
            EffectiveGraph::read(&mut read)?
        };
        if mode == PublicationMode::Full {
            graph.mark_all_sources();
        }
        let basis = prepare_basis(
            &self.filesystem,
            &self.markdown,
            BasisPreparation {
                graph: &graph,
                updated: &delta.updated,
                removed: &delta.removed,
                previous: &state.files,
                mode,
            },
        )?;
        basis.validate_basis(&self.provenance, verification)?;
        Ok(graph)
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
    prepared: PreparedProjection,
    outcome: RefreshOutcome,
) -> DocumentResult<RefreshOutcome> {
    if prepared.base_revision != outcome.revision {
        return Err(fail_changed());
    }
    let outcome = prepared.add_counts(outcome);
    publish(unit, prepared)?;
    Ok(outcome)
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

#[cfg(test)]
pub(super) use tests::process::reach_checkpoint;
