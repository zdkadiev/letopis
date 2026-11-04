use super::{
    BasisRetainer, CacheWrite, CorpusLister, DocumentError, DocumentErrorKind, DocumentResult,
    FileRow, FileStat, FileSystem, MarkdownCodec, RefreshOutcome, Refresher, SourceProvenance,
    Storage,
};
use crate::domain::{RepoPath, Sha256, TreeOid};
use basis::BasisPreparation;
use projection::{EffectiveGraph, FileProjection, PreparedProjection, prepare_removal, publish};
use std::fmt;

use std::collections::{BTreeMap, BTreeSet};

const RACY_WINDOW_NS: i64 = 1_000_000_000;

/// Refreshes and retains the effective document projection from independently owned collaborators.
pub(crate) struct DocumentRefreshService<G, F, M, P> {
    /// Lists the current corpus candidates.
    pub(crate) git: G,
    /// Captures documents and owns the index exclusion lock.
    pub(crate) filesystem: F,
    /// Parses canonical document metadata and source bindings.
    pub(crate) markdown: M,
    /// Reads the current projection and publishes cache changes atomically.
    pub(crate) storage: Box<dyn Storage>,
    /// Observes source bindings and retains author-declared roots.
    pub(crate) provenance: P,
}

impl<G, F, M, P> fmt::Debug for DocumentRefreshService<G, F, M, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentRefreshService")
            .finish_non_exhaustive()
    }
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
/// Borrows publication collaborators independently of the storage transaction.
struct DocumentPublication<'a, F, M, P> {
    filesystem: &'a F,
    markdown: &'a M,
    provenance: &'a P,
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
impl<G: CorpusLister, F: FileSystem, M: MarkdownCodec, P: SourceProvenance + BasisRetainer>
    Refresher for DocumentRefreshService<G, F, M, P>
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
        let preliminary = self.get_file_state()?;
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
impl<G: CorpusLister, F: FileSystem, M: MarkdownCodec, P: SourceProvenance + BasisRetainer>
    DocumentRefreshService<G, F, M, P>
{
    fn publish_current(
        &mut self,
        requested: PublicationMode,
        verification: SnapshotVerification,
    ) -> DocumentResult<RefreshOutcome> {
        let state = self.get_file_state()?;
        let mode = if state.revision == 0 {
            PublicationMode::Full
        } else {
            requested
        };
        let delta = self.detect_changes(&state, mode)?;
        #[cfg(test)]
        reach_checkpoint("redetected");
        if delta.is_empty() && mode == PublicationMode::Incremental {
            DocumentPublication {
                filesystem: &self.filesystem,
                markdown: &self.markdown,
                provenance: &self.provenance,
            }
            .validate_snapshots(verification, &state.source_snapshots)?;
            return Ok(build_outcome(state.revision, false));
        }
        self.publish_changes(&state, delta, mode, verification)
    }

    fn get_file_state(&mut self) -> DocumentResult<FileState> {
        let mut read = self.storage.begin_read_tx()?;
        Ok(FileState {
            revision: read.get_revision(),
            source_snapshots: read.get_source_snapshots()?,
            files: read
                .get_files()?
                .into_iter()
                .map(|row| (row.path.clone(), row))
                .collect(),
        })
    }

    fn detect_changes(&self, state: &FileState, mode: PublicationMode) -> DocumentResult<Delta> {
        let candidates: BTreeSet<_> = self.git.get_candidates()?.into_iter().collect();
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
            if let Some(observation) = self.detect_file(&path, stat, mode.get_previous(previous))? {
                delta.updated.insert(path, observation);
            }
        }
        Ok(delta)
    }

    fn detect_file(
        &self,
        path: &RepoPath,
        stat: FileStat,
        previous: Option<&FileRow>,
    ) -> DocumentResult<Option<Observation>> {
        let same_stat = previous.is_some_and(|row| matches_stat(row, stat));
        if same_stat
            && previous.is_some_and(|row| !is_racy(row, self.filesystem.get_time_ns().ok()))
        {
            return Ok(None);
        }
        let captured = self.filesystem.get_file(path)?.ok_or_else(fail_changed)?;
        if captured.stat != stat {
            return Err(fail_changed());
        }
        let hash = Sha256::digest(&captured.bytes);
        if same_stat && previous.is_some_and(|row| row.hash == hash) {
            return Ok(None);
        }
        Ok(Some(Observation { stat, hash }))
    }

    fn publish_changes(
        &mut self,
        state: &FileState,
        delta: Delta,
        mode: PublicationMode,
        verification: SnapshotVerification,
    ) -> DocumentResult<RefreshOutcome> {
        let mut graph = self.prepare_publication(state, &delta, mode, verification)?;
        let publication = DocumentPublication {
            filesystem: &self.filesystem,
            markdown: &self.markdown,
            provenance: &self.provenance,
        };
        let mut unit = self.storage.begin_cache_write_tx()?;
        #[cfg(test)]
        reach_checkpoint("begun");
        let mut outcome = build_outcome(state.revision, true);
        let excluded: BTreeSet<_> = delta
            .removed
            .iter()
            .filter(|(_, check_missing)| !**check_missing)
            .map(|(path, _)| path.clone())
            .collect();
        for (path, check_missing) in delta.removed {
            if check_missing && self.filesystem.stat_file(&path)?.is_some() {
                return Err(fail_changed());
            }
            let prepared = prepare_removal(state.revision, path, &graph);
            graph.apply(&prepared);
            outcome = apply_projection(unit.as_mut(), prepared, outcome)?;
        }
        for (path, observation) in delta.updated {
            let prepared = publication.prepare_file(&FileProjection {
                base_revision: state.revision,
                path: &path,
                observed: &observation,
                previous: mode.get_previous(state.files.get(&path)),
                graph: &graph,
            })?;
            graph.apply(&prepared);
            outcome = apply_projection(unit.as_mut(), prepared, outcome)?;
        }
        publication.reconcile_sources(&graph, unit.as_mut())?;
        publication.publish_edges(&graph, unit.as_mut())?;
        if !excluded.is_empty()
            && self
                .git
                .get_candidates()?
                .iter()
                .any(|path| excluded.contains(path))
        {
            return Err(fail_changed());
        }
        #[cfg(test)]
        reach_checkpoint("before_commit");
        #[cfg(test)]
        tests::process::fail_late(unit.as_mut())?;
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
            let mut read = self.storage.begin_read_tx()?;
            if read.get_revision() != state.revision {
                return Err(fail_changed());
            }
            EffectiveGraph::read(read.as_mut())?
        };
        if mode == PublicationMode::Full {
            graph.mark_all_sources();
        }
        let publication = DocumentPublication {
            filesystem: &self.filesystem,
            markdown: &self.markdown,
            provenance: &self.provenance,
        };
        let basis = publication.prepare_basis(BasisPreparation {
            graph: &graph,
            updated: &delta.updated,
            removed: &delta.removed,
            previous: &state.files,
            mode,
        })?;
        publication.validate_basis(&basis, verification)?;
        Ok(graph)
    }
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

fn apply_projection(
    unit: &mut dyn CacheWrite,
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

mod basis {
    use super::{
        DocumentPublication, Observation, PublicationMode, SnapshotVerification, fail_changed,
        projection::EffectiveGraph,
    };
    use crate::{
        domain::{EdgeTarget, RepoPath, Sha256, SourceUri, TreeOid},
        service::{
            BasisRetainer, DocumentResult, FileRow, FileSystem, MarkdownCodec,
            PROVENANCE_BATCH_SIZE, ParsedFile, ProvenanceRequest, SourceProvenance,
        },
    };
    use std::collections::{BTreeMap, BTreeSet};

    /// Metadata captured before the SQL write, never document or search text.
    #[derive(Debug)]
    pub(super) struct BasisPreflight {
        snapshots: BTreeSet<TreeOid>,
        bases: Vec<PreparedBasis>,
    }
    #[derive(Debug)]
    struct PreparedBasis {
        source_snapshot: Option<TreeOid>,
        sources: BTreeSet<SourceUri>,
    }
    /// Borrowed inputs for metadata-only preflight against one pinned cache revision.
    #[derive(Clone, Copy)]
    pub(super) struct BasisPreparation<'a> {
        pub(super) graph: &'a EffectiveGraph,
        pub(super) updated: &'a BTreeMap<RepoPath, Observation>,
        pub(super) removed: &'a BTreeMap<RepoPath, bool>,
        pub(super) previous: &'a BTreeMap<RepoPath, FileRow>,
        pub(super) mode: PublicationMode,
    }
    impl<F: FileSystem, M: MarkdownCodec, P: SourceProvenance + BasisRetainer>
        DocumentPublication<'_, F, M, P>
    {
        /// Prepare only changed-content metadata, releasing each parsed file before the next.
        ///
        /// # Errors
        /// Reports capture failures and observed stat/hash changes; invalid Markdown keeps its old basis.
        pub(super) fn prepare_basis(
            &self,
            request: BasisPreparation<'_>,
        ) -> DocumentResult<BasisPreflight> {
            let BasisPreparation {
                graph,
                updated,
                removed,
                previous,
                mode,
            } = request;
            let mut preflight = BasisPreflight {
                snapshots: BTreeSet::new(),
                bases: Vec::new(),
            };
            let mut replaced: BTreeSet<_> = removed.keys().cloned().collect();
            for (path, observed) in updated {
                // Hash-equal files reuse their last valid projection already represented by graph.
                if mode
                    .get_previous(previous.get(path))
                    .is_some_and(|previous| previous.hash == observed.hash)
                {
                    continue;
                }
                let captured = self.filesystem.get_file(path)?.ok_or_else(fail_changed)?;
                if captured.stat != observed.stat
                    || Sha256::digest(&captured.bytes) != observed.hash
                {
                    return Err(fail_changed());
                }
                let Ok(parsed) = self.markdown.parse_file(path, &captured.bytes) else {
                    continue;
                };
                replaced.insert(path.clone());
                let Some(parsed) = parsed else {
                    continue;
                };
                preflight.add_file(parsed);
            }
            preflight
                .snapshots
                .extend(graph.get_source_snapshots(&replaced));
            Ok(preflight)
        }

        /// Observe new valid source bindings and retain all roots before SQL publication.
        /// The caller holds index.lock; diagnostic findings do not reject valid Markdown.
        ///
        /// # Errors
        /// Reports unsafe source reads, Git corruption, and retention failures before SQL begins.
        pub(super) fn validate_basis(
            &self,
            preflight: &BasisPreflight,
            verification: SnapshotVerification,
        ) -> DocumentResult<()> {
            for bases in preflight.bases.chunks(PROVENANCE_BATCH_SIZE) {
                let requests: Vec<_> = bases
                    .iter()
                    .map(|basis| ProvenanceRequest {
                        source_snapshot: basis.source_snapshot.as_ref(),
                        sources: &basis.sources,
                    })
                    .collect();
                self.provenance.observe_sources_batch(&requests)?;
            }
            self.validate_snapshots(verification, &preflight.snapshots)
        }

        /// Retains declared roots or verifies their closure according to the publication mode.
        ///
        /// # Errors
        /// Reports missing or corrupt snapshot objects and retention failures.
        pub(super) fn validate_snapshots(
            &self,
            verification: SnapshotVerification,
            snapshots: &BTreeSet<TreeOid>,
        ) -> DocumentResult<()> {
            match verification {
                SnapshotVerification::Root => self.provenance.retain_snapshots(snapshots),
                SnapshotVerification::Closure => self.provenance.verify_snapshots(snapshots),
            }
        }
    }
    impl BasisPreflight {
        fn add_file(&mut self, parsed: ParsedFile) {
            for element in parsed.elements {
                let id = element.element.id;
                let source_snapshot = element.element.metadata.source_snapshot;
                self.snapshots.extend(source_snapshot.iter().cloned());
                let sources = parsed
                    .edges
                    .iter()
                    .filter(|edge| edge.get_source() == &id)
                    .filter_map(|edge| match edge.get_target() {
                        EdgeTarget::Source(uri) => Some(uri.clone()),
                        EdgeTarget::Knowledge(_) => None,
                    })
                    .collect();
                self.bases.push(PreparedBasis {
                    source_snapshot,
                    sources,
                });
            }
        }
    }
}

mod projection {
    #[cfg(test)]
    use super::reach_checkpoint;
    use super::{DocumentPublication, Observation, fail_changed};
    use crate::domain::{
        Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid, derive_element_kind,
    };
    use crate::service::{
        CacheWrite, CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileRow,
        FileSystem, KnowledgeMetadata, MarkdownCodec, NodeMetadata, ParsedFile, RefreshOutcome,
        SourceMetadata, StorageRead,
    };
    use std::collections::{BTreeMap, BTreeSet};

    // Text is owned only by one prepared file. Metadata supplies its id, uri and search_hash.
    struct PreparedText {
        metadata: KnowledgeMetadata,
        search_text: String,
    }
    pub(super) struct PreparedProjection {
        pub base_revision: u64,
        cleared: Vec<KnowledgeId>,
        removed: Vec<KnowledgeId>,
        edges: BTreeSet<Edge>,
        search: Vec<PreparedText>,
        file: FileChange,
    }
    enum FileChange {
        Upsert(FileRow),
        Remove(RepoPath),
    }
    impl PreparedProjection {
        /// Returns `outcome` with this projection's file change counted.
        pub(super) fn add_counts(&self, mut outcome: RefreshOutcome) -> RefreshOutcome {
            match &self.file {
                FileChange::Upsert(row) => {
                    outcome.files_updated += 1;
                    outcome.invalid_files_updated += usize::from(!row.valid);
                }
                FileChange::Remove(_) => outcome.files_removed += 1,
            }
            outcome
        }
    }
    /// Inputs for projecting one observed file against the previously published graph.
    pub(super) struct FileProjection<'a> {
        pub(super) base_revision: u64,
        pub(super) path: &'a RepoPath,
        pub(super) observed: &'a Observation,
        pub(super) previous: Option<&'a FileRow>,
        pub(super) graph: &'a EffectiveGraph,
    }
    pub(super) struct EffectiveGraph {
        knowledge: BTreeMap<KnowledgeId, KnowledgeMetadata>,
        edges: BTreeSet<Edge>,
        touched_sources: BTreeSet<SourceUri>,
        changed_owners: BTreeSet<KnowledgeId>,
    }
    impl<F: FileSystem, M: MarkdownCodec, P> DocumentPublication<'_, F, M, P> {
        /// Captures and parses one changed file against its effective previous projection.
        ///
        /// # Errors
        /// Reports capture failures or changes since detection; invalid Markdown retains old nodes.
        pub(super) fn prepare_file(
            &self,
            request: &FileProjection<'_>,
        ) -> DocumentResult<PreparedProjection> {
            let FileProjection {
                base_revision,
                path,
                observed,
                previous,
                graph,
            } = *request;
            let captured = self.filesystem.get_file(path)?.ok_or_else(fail_changed)?;
            if captured.stat != observed.stat || Sha256::digest(&captured.bytes) != observed.hash {
                return Err(fail_changed());
            }
            let mut file = build_file_row(path, &captured, observed.hash);
            let mut prepared = PreparedProjection {
                base_revision,
                cleared: Vec::new(),
                removed: Vec::new(),
                edges: BTreeSet::new(),
                search: Vec::new(),
                file: FileChange::Upsert(file.clone()),
            };
            if let Some(previous) = previous.filter(|row| row.hash == observed.hash) {
                file.valid = previous.valid;
                file.error.clone_from(&previous.error);
            } else {
                match self.markdown.parse_file(path, &captured.bytes) {
                    Ok(parsed) => {
                        prepared = prepare_valid(prepared, parsed, graph.get_file_ids(path));
                    }
                    Err(errors) => {
                        file.valid = false;
                        file.error = Some(
                            errors
                                .first()
                                .map_or("invalid Markdown", |error| error.message.as_str())
                                .to_owned(),
                        );
                    }
                }
            }
            prepared.file = FileChange::Upsert(file);
            Ok(prepared)
        }

        /// Publishes each affected source using one current capture per local path.
        ///
        /// # Errors
        /// Reports conflicting source addresses, unsafe captures, and transaction write failures.
        pub(super) fn reconcile_sources(
            &self,
            graph: &EffectiveGraph,
            unit: &mut dyn CacheWrite,
        ) -> DocumentResult<()> {
            let references = graph.collect_source_references()?;
            let mut paths: BTreeMap<RepoPath, Vec<(SourceUri, u64)>> = BTreeMap::new();
            for uri in &graph.touched_sources {
                if !references.contains_key(uri)
                    && !graph.knowledge.keys().any(|id| id.as_str() == uri.as_str())
                {
                    unit.delete_node(&EdgeTarget::Source(uri.clone()))?;
                }
                let Some(&created_at) = references.get(uri) else {
                    continue;
                };
                let Some(local) = uri.get_local() else {
                    unit.upsert_node(&NodeMetadata::Source(SourceMetadata {
                        uri: uri.clone(),
                        hash: None,
                        created_at,
                        lines: None,
                    }))?;
                    continue;
                };
                paths
                    .entry(local.get_path().clone())
                    .or_default()
                    .push((uri.clone(), created_at));
            }
            for (path, sources) in paths {
                self.publish_source_group(unit, &path, sources)?;
            }
            Ok(())
        }

        /// Inserts changed owners' final declarations after all nodes and sources exist.
        /// Unchanged owners keep their declarations for storage's commit-time reconciliation.
        ///
        /// # Errors
        /// Reports declaration failures, leaving the caller's transaction unpublished.
        pub(super) fn publish_edges(
            &self,
            graph: &EffectiveGraph,
            unit: &mut dyn CacheWrite,
        ) -> DocumentResult<()> {
            for edge in &graph.edges {
                if graph.changed_owners.contains(edge.get_source()) {
                    unit.insert_edge(edge)?;
                }
            }
            Ok(())
        }

        fn publish_source_group(
            &self,
            unit: &mut dyn CacheWrite,
            path: &RepoPath,
            sources: Vec<(SourceUri, u64)>,
        ) -> DocumentResult<()> {
            // One capture serves every affected address of this path and then is released.
            let captured = self.filesystem.get_file(path)?;
            for (uri, created_at) in sources {
                let local = uri.get_local().ok_or_else(fail_changed)?;
                let lines = local.get_lines();
                let hash = captured
                    .as_ref()
                    .and_then(|file| local.select_bytes(&file.bytes))
                    .map(Sha256::digest);
                unit.upsert_node(&NodeMetadata::Source(SourceMetadata {
                    uri,
                    hash,
                    created_at,
                    lines,
                }))?;
            }
            Ok(())
        }
    }
    impl EffectiveGraph {
        pub(super) fn read(read: &mut dyn StorageRead) -> DocumentResult<Self> {
            let knowledge = read
                .get_nodes()?
                .into_iter()
                .filter_map(|row| match row.metadata {
                    NodeMetadata::Knowledge(node) => Some((node.id.clone(), node)),
                    NodeMetadata::Source(_) => None,
                })
                .collect();
            Ok(Self {
                knowledge,
                edges: read.get_edges_by_source_id(None)?.into_iter().collect(),
                touched_sources: BTreeSet::new(),
                changed_owners: BTreeSet::new(),
            })
        }

        /// Reobserve every declared source during a full rebuild, including invalid owners.
        pub(super) fn mark_all_sources(&mut self) {
            self.touched_sources.extend(self.edges.iter().filter_map(
                |edge| match edge.get_target() {
                    EdgeTarget::Source(uri) => Some(uri.clone()),
                    EdgeTarget::Knowledge(_) => None,
                },
            ));
        }

        /// Return every declared root in the last effective knowledge projection.
        pub(super) fn get_source_snapshots(
            &self,
            excluded: &BTreeSet<RepoPath>,
        ) -> BTreeSet<TreeOid> {
            let paths: BTreeSet<_> = excluded.iter().map(RepoPath::as_str).collect();
            self.knowledge
                .values()
                .filter(|node| {
                    !paths.contains(node.id.as_str().split('#').next().unwrap_or_default())
                })
                .filter_map(|node| node.source_snapshot.clone())
                .collect()
        }

        pub(super) fn apply(&mut self, prepared: &PreparedProjection) {
            self.changed_owners.extend(prepared.cleared.iter().cloned());
            self.edges.retain(|edge| {
                if prepared.cleared.contains(edge.get_source()) {
                    if let EdgeTarget::Source(uri) = edge.get_target() {
                        self.touched_sources.insert(uri.clone());
                    }
                    false
                } else {
                    true
                }
            });
            for id in &prepared.removed {
                self.knowledge.remove(id);
            }
            for text in &prepared.search {
                self.changed_owners.insert(text.metadata.id.clone());
                self.knowledge
                    .insert(text.metadata.id.clone(), text.metadata.clone());
            }
            for edge in &prepared.edges {
                if let EdgeTarget::Source(uri) = edge.get_target() {
                    self.touched_sources.insert(uri.clone());
                }
                self.edges.insert(edge.clone());
            }
        }

        fn get_file_ids(&self, path: &RepoPath) -> Vec<KnowledgeId> {
            self.knowledge
                .keys()
                .filter(|id| id.as_str().split('#').next() == Some(path.as_str()))
                .cloned()
                .collect()
        }

        fn collect_source_references(&self) -> DocumentResult<BTreeMap<SourceUri, u64>> {
            let mut references = BTreeMap::new();
            for edge in &self.edges {
                let EdgeTarget::Source(uri) = edge.get_target() else {
                    continue;
                };
                if self.knowledge.keys().any(|id| id.as_str() == uri.as_str()) {
                    return Err(DocumentError::new(
                        DocumentErrorKind::InvalidRequest,
                        "source address conflicts with a knowledge element",
                    ));
                }
                let knowledge = self
                    .knowledge
                    .get(edge.get_source())
                    .ok_or_else(fail_changed)?;
                references
                    .entry(uri.clone())
                    .and_modify(|created: &mut u64| {
                        *created = (*created).min(knowledge.created_at);
                    })
                    .or_insert(knowledge.created_at);
            }
            Ok(references)
        }
    }

    pub(super) fn prepare_removal(
        base_revision: u64,
        path: RepoPath,
        graph: &EffectiveGraph,
    ) -> PreparedProjection {
        let ids = graph.get_file_ids(&path);
        PreparedProjection {
            base_revision,
            cleared: ids.clone(),
            removed: ids,
            edges: BTreeSet::new(),
            search: Vec::new(),
            file: FileChange::Remove(path),
        }
    }

    pub(super) fn publish(
        unit: &mut dyn CacheWrite,
        prepared: PreparedProjection,
    ) -> DocumentResult<()> {
        for id in &prepared.cleared {
            unit.delete_outgoing(id)?;
        }
        for id in prepared.removed {
            unit.delete_node(&EdgeTarget::Knowledge(id))?;
        }
        #[cfg(test)]
        reach_checkpoint("cleared");
        for text in &prepared.search {
            unit.upsert_node(&NodeMetadata::Knowledge(text.metadata.clone()))?;
        }
        #[cfg(test)]
        reach_checkpoint("projections");
        for text in prepared.search {
            unit.replace_text(&text.metadata.id, &text.search_text)?;
        }
        #[cfg(test)]
        reach_checkpoint("fts");
        match prepared.file {
            FileChange::Upsert(row) => unit.upsert_file(&row)?,
            FileChange::Remove(path) => unit.delete_file(&path)?,
        }
        #[cfg(test)]
        reach_checkpoint("fingerprints");
        Ok(())
    }

    fn build_file_row(path: &RepoPath, captured: &CapturedFile, hash: Sha256) -> FileRow {
        FileRow {
            path: path.clone(),
            size: captured.stat.size,
            mtime_ns: captured.stat.mtime_ns,
            inode: captured.stat.inode,
            hash,
            checked_at_ns: captured.checked_at_ns,
            valid: true,
            error: None,
        }
    }

    fn prepare_valid(
        mut prepared: PreparedProjection,
        parsed: Option<ParsedFile>,
        old_ids: Vec<KnowledgeId>,
    ) -> PreparedProjection {
        prepared.cleared = old_ids;
        if let Some(parsed) = parsed {
            prepared.edges = parsed.edges;
            for parsed in parsed.elements {
                let element = parsed.element;
                prepared.search.push(PreparedText {
                    metadata: KnowledgeMetadata {
                        kind: derive_element_kind(&element.id, &prepared.edges),
                        uri: element.id.clone(),
                        id: element.id,
                        hash: element.hash,
                        created_at: element.metadata.created_at,
                        source_snapshot: element.metadata.source_snapshot,
                        lines: element.lines,
                        search_hash: parsed.search_hash,
                    },
                    search_text: parsed.search_text,
                });
            }
        }
        prepared.removed = prepared
            .cleared
            .iter()
            .filter(|id| !prepared.search.iter().any(|text| &text.metadata.id == *id))
            .cloned()
            .collect();
        prepared
    }
}

#[cfg(test)]
pub(super) use tests::process::reach_checkpoint;

#[cfg(test)]
mod tests {
    use super::DocumentRefreshService;
    use crate::{
        domain::{RepoPath, TreeOid},
        markdown::MarkdownParser,
        service::{
            BasisRetainer, CapturedFile, Diagnostic, DocumentError, DocumentErrorKind,
            DocumentResult, FileStat, FileSystem, MarkdownCodec, MockCorpusLister, ParsedFile,
            ProvenanceObservation, ProvenanceRequest, Refresher, SnapshotError, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest,
        },
        storage::SqliteStorage,
    };
    use rusqlite::Connection;
    use std::{
        cell::{Cell, RefCell},
        collections::BTreeSet,
        path::Path,
    };
    use tempfile::TempDir;

    const INITIAL: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nAlphaToken\n";
    const EDITED: &[u8] = b"<!-- letopis: {\"created_at\":1} -->\nBravoToken\n";
    const NOW: i64 = 5_000_000_000;
    type Candidates = MockCorpusLister;
    struct TestFiles {
        bytes: RefCell<Vec<u8>>,
        stat: Cell<FileStat>,
        now: Cell<i64>,
        reads: Cell<usize>,
        locks: Cell<usize>,
        replace_on_read: Cell<Option<usize>>,
        fail_on_read: Cell<Option<usize>>,
        missing: Cell<bool>,
    }
    impl TestFiles {
        fn new() -> Self {
            Self {
                bytes: RefCell::new(INITIAL.to_vec()),
                stat: Cell::new(FileStat {
                    size: u64::try_from(INITIAL.len()).unwrap(),
                    mtime_ns: 1,
                    inode: 2,
                    device: 3,
                }),
                now: Cell::new(NOW),
                reads: Cell::new(0),
                locks: Cell::new(0),
                replace_on_read: Cell::new(None),
                fail_on_read: Cell::new(None),
                missing: Cell::new(false),
            }
        }
        fn reset_counts(&self) {
            self.reads.set(0);
            self.locks.set(0);
        }
    }
    impl FileSystem for TestFiles {
        type IndexLock = ();
        fn stat_file(&self, _: &RepoPath) -> DocumentResult<Option<FileStat>> {
            Ok((!self.missing.get()).then(|| self.stat.get()))
        }
        fn get_file(&self, _: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
            let reads = self.reads.get() + 1;
            self.reads.set(reads);
            if self.fail_on_read.get() == Some(reads) {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    "cannot read document file",
                ));
            }
            if self.replace_on_read.get() == Some(reads) {
                self.bytes.replace(EDITED.to_vec());
            }
            Ok((!self.missing.get()).then(|| CapturedFile {
                bytes: self.bytes.borrow().clone(),
                stat: self.stat.get(),
                checked_at_ns: self.now.get(),
            }))
        }
        fn get_time_ns(&self) -> DocumentResult<i64> {
            Ok(self.now.get())
        }
        fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
            self.locks.set(self.locks.get() + 1);
            Ok(())
        }
    }
    struct CountingMarkdown(Cell<usize>);
    impl MarkdownCodec for CountingMarkdown {
        fn parse_file(
            &self,
            path: &RepoPath,
            bytes: &[u8],
        ) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
            self.0.set(self.0.get() + 1);
            MarkdownParser::new().parse_file(path, bytes)
        }
    }
    struct TestProvenance;
    impl SourceProvenance for TestProvenance {
        fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            panic!("this scenario does not validate requested revisions")
        }
        fn observe_sources(
            &self,
            _: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            Ok(ProvenanceObservation {
                flags: BTreeSet::new(),
                issues: BTreeSet::new(),
            })
        }
        fn compare_revision(
            &self,
            _: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            panic!("refresh does not compare requested revisions")
        }
    }
    impl BasisRetainer for TestProvenance {
        fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
            assert!(snapshots.is_empty());
            Ok(())
        }
        fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
            assert!(snapshots.is_empty());
            Ok(())
        }
    }
    type TestService =
        DocumentRefreshService<Candidates, TestFiles, CountingMarkdown, TestProvenance>;
    #[test]
    fn skip_body_reads_for_unchanged_non_racy_marked_unmarked_and_invalid_files() {
        // Given
        for bytes in [
            INITIAL,
            &b"ordinary markdown\n"[..],
            &b"<!-- letopis: invalid -->"[..],
        ] {
            let (_dir, mut service) = build_service();
            service.filesystem.bytes.replace(bytes.to_vec());
            service.filesystem.stat.update(|mut stat| {
                stat.size = u64::try_from(bytes.len()).unwrap();
                stat
            });
            service.refresh().unwrap();
            service.filesystem.reset_counts();
            let parses = service.markdown.0.get();
            // When
            let result = service.refresh().unwrap();
            // Then
            assert!(!result.published);
            assert_eq!(result.revision, 1);
            assert_eq!(
                (
                    service.filesystem.reads.get(),
                    service.filesystem.locks.get()
                ),
                (0, 0)
            );
            assert_eq!(service.markdown.0.get(), parses);
        }
    }

    #[test]
    fn detect_same_tick_equal_stat_edit_and_do_not_publish_unchanged_racy_bytes() {
        // Given: same-tick bytes with unchanged file statistics.
        let (_dir, mut service) = build_service();
        service.filesystem.stat.update(|mut stat| {
            stat.mtime_ns = NOW;
            stat
        });
        service.refresh().unwrap();
        let before = service
            .storage
            .begin_read_tx()
            .unwrap()
            .get_files()
            .unwrap();
        service.filesystem.reset_counts();
        // When: refresh checks the captured bytes.
        let unchanged = service.refresh().unwrap();
        // Then: only changed stable bytes advance the publication.
        assert!(!unchanged.published);
        assert_eq!(
            (
                service.filesystem.reads.get(),
                service.filesystem.locks.get()
            ),
            (1, 0)
        );
        assert_eq!(
            service
                .storage
                .begin_read_tx()
                .unwrap()
                .get_files()
                .unwrap(),
            before
        );
        // Given: exact same size, inode and mtime; only bytes differ.
        service.filesystem.bytes.replace(EDITED.to_vec());
        // When: refresh checks the captured bytes.
        let changed = service.refresh().unwrap();
        // Then: only changed stable bytes advance the publication.
        assert_eq!(changed.revision, 2);
        assert_eq!(get_matches(&mut service, "BravoToken"), ["note.md"]);
        assert!(get_matches(&mut service, "AlphaToken").is_empty());
    }

    #[test]
    fn treat_racy_boundary_clock_regression_and_checked_arithmetic_conservatively() {
        // Given
        let cases = [
            (NOW - 1_000_000_000, NOW, NOW),
            (1, NOW, 2),
            (i64::MAX - 1, i64::MAX, i64::MAX),
            (0, i64::MIN, i64::MIN),
        ];
        for (mtime, checked, now) in cases {
            let (_dir, mut service) = build_service();
            service.filesystem.stat.update(|mut stat| {
                stat.mtime_ns = mtime;
                stat
            });
            service.filesystem.now.set(checked);
            service.refresh().unwrap();
            service.filesystem.reset_counts();
            service.filesystem.now.set(now);
            // When
            let result = service.refresh().unwrap();
            // Then
            assert!(!result.published);
            assert_eq!(
                service.filesystem.reads.get(),
                1,
                "case {mtime}/{checked}/{now}"
            );
            assert_eq!(service.filesystem.locks.get(), 0);
        }
    }

    #[test]
    fn stat_only_change_preserves_projection_and_invalid_state_without_parsing() {
        // Given: valid and invalid publications with stat-only changes.
        for bytes in [INITIAL, &b"<!-- letopis: invalid -->"[..]] {
            let (dir, mut service) = build_service();
            service.filesystem.bytes.replace(bytes.to_vec());
            service.filesystem.stat.update(|mut stat| {
                stat.size = u64::try_from(bytes.len()).unwrap();
                stat
            });
            service.refresh().unwrap();
            let before_nodes = service
                .storage
                .begin_read_tx()
                .unwrap()
                .get_nodes()
                .unwrap();
            let before_files = service
                .storage
                .begin_read_tx()
                .unwrap()
                .get_files()
                .unwrap();
            let before_fts = get_fts_blocks(dir.path());
            let parses = service.markdown.0.get();
            service.filesystem.stat.update(|mut stat| {
                stat.inode += 1;
                stat
            });
            service.filesystem.now.set(NOW + 1);
            // When: refresh processes the changed file statistics.
            let outcome = service.refresh().unwrap();
            // Then: file state updates without reparsing or replacing the projection.
            assert!(outcome.published);
            assert_eq!(outcome.files_updated, 1);
            assert_eq!(service.markdown.0.get(), parses);
            assert_eq!(get_fts_blocks(dir.path()), before_fts);
            assert_eq!(
                service
                    .storage
                    .begin_read_tx()
                    .unwrap()
                    .get_nodes()
                    .unwrap(),
                before_nodes
            );
            let after_files = service
                .storage
                .begin_read_tx()
                .unwrap()
                .get_files()
                .unwrap();
            assert_eq!(after_files[0].inode, before_files[0].inode + 1);
            assert_eq!(after_files[0].valid, before_files[0].valid);
            assert_eq!(after_files[0].error, before_files[0].error);
            assert_eq!(after_files[0].hash, before_files[0].hash);
            assert_eq!(after_files[0].checked_at_ns, NOW + 1);
        }
    }

    #[test]
    fn changed_capture_between_locked_detection_and_preparation_aborts() {
        // Given: bytes that change between detection and preparation.
        let (_dir, mut service) = build_service();
        service.filesystem.replace_on_read.set(Some(3));
        // When: refresh captures the candidate.
        let result = service.refresh();
        // Then: inconsistent captures abort publication.
        assert_eq!(service.filesystem.reads.get(), 3);
        assert!(result.is_err());
        let mut read = service.storage.begin_read_tx().unwrap();
        assert_eq!(read.get_revision(), 0);
        assert!(read.get_files().unwrap().is_empty());
        assert!(read.get_nodes().unwrap().is_empty());
    }

    #[test]
    fn reject_candidate_reappearing_before_publication() {
        // Given: an indexed document disappears from enumeration and then reappears.
        let (_dir, mut service) = build_service();
        service.refresh().unwrap();
        let calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let observed = std::sync::Arc::clone(&calls);
        let mut candidates = Candidates::new();
        candidates.expect_get_candidates().returning(move || {
            let count = observed.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            if count < 3 {
                Ok(Vec::new())
            } else {
                Ok(vec![RepoPath::parse("note.md").unwrap()])
            }
        });
        service.git = candidates;
        // When: refresh tries to publish the removal.
        let result = service.refresh();
        // Then: the race aborts publication and preserves the old projection.
        assert!(result.is_err());
        let mut read = service.storage.begin_read_tx().unwrap();
        assert_eq!(read.get_revision(), 1);
        assert_eq!(read.get_files().unwrap().len(), 1);
        assert_eq!(
            read.get_text_matches_by_expression("AlphaToken")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn fatal_read_is_not_an_invalid_markdown_publication() {
        // Given: a published projection and a filesystem failure on the next changed read.
        let (_dir, mut service) = build_service();
        service.refresh().unwrap();
        let before = service
            .storage
            .begin_read_tx()
            .unwrap()
            .get_files()
            .unwrap();
        service.filesystem.reset_counts();
        service.filesystem.stat.update(|mut stat| {
            stat.inode += 1;
            stat
        });
        service.filesystem.fail_on_read.set(Some(3));
        // When / Then: refresh fails rather than publishing an invalid Markdown result.
        assert!(service.refresh().is_err());
        let mut read = service.storage.begin_read_tx().unwrap();
        // Then: revision, file state and searchable text retain the prior publication.
        assert_eq!(read.get_revision(), 1);
        assert_eq!(read.get_files().unwrap(), before);
        assert_eq!(
            read.get_text_matches_by_expression("AlphaToken")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn never_published_missing_candidate_is_not_a_delta_after_empty_initial_build() {
        // Given
        let (_dir, mut service) = build_service();
        service.filesystem.missing.set(true);
        assert_eq!(service.refresh().unwrap().revision, 1);
        service.filesystem.reset_counts();
        // When
        let result = service.refresh().unwrap();
        // Then
        assert!(!result.published);
        assert_eq!(
            (
                service.filesystem.reads.get(),
                service.filesystem.locks.get()
            ),
            (0, 0)
        );
        // Given
        service.filesystem.missing.set(false);
        // When / Then
        assert_eq!(service.refresh().unwrap().files_updated, 1);
    }

    #[test]
    fn discard_preliminary_capture_and_publish_the_locked_observation() {
        // Given
        let (_dir, mut service) = build_service();
        service.refresh().unwrap();
        service.filesystem.reset_counts();
        service.filesystem.stat.update(|mut stat| {
            stat.inode += 1;
            stat
        });
        service.filesystem.replace_on_read.set(Some(2));
        // When
        let outcome = service.refresh().unwrap();
        // Then
        assert_eq!(outcome.revision, 2);
        assert_eq!(get_matches(&mut service, "BravoToken"), ["note.md"]);
        assert!(get_matches(&mut service, "AlphaToken").is_empty());
    }

    // A same-text DELETE/INSERT changes FTS segment bytes even if search results agree.
    fn get_fts_blocks(root: &Path) -> Vec<(i64, Vec<u8>)> {
        let connection = Connection::open(root.join("cache.db")).unwrap();
        connection
            .prepare("select id, block from text_index_data order by id")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }

    fn build_service() -> (TempDir, TestService) {
        let dir = tempfile::tempdir().unwrap();
        let storage =
            SqliteStorage::open(&dir.path().join("cache.db"), &dir.path().join("vectors.db"))
                .unwrap();
        (
            dir,
            DocumentRefreshService {
                git: build_candidates(),
                filesystem: TestFiles::new(),
                markdown: CountingMarkdown(Cell::new(0)),
                storage: Box::new(storage),
                provenance: TestProvenance,
            },
        )
    }

    fn get_matches(service: &mut TestService, text: &str) -> Vec<String> {
        service
            .storage
            .begin_read_tx()
            .unwrap()
            .get_text_matches_by_expression(text)
            .unwrap()
            .into_iter()
            .map(|row| row.id.as_str().to_owned())
            .collect()
    }

    mod basis {
        use super::{Candidates, CountingMarkdown, TestFiles, TestProvenance, build_service};
        use crate::service::{GraphDiagnostics, ReadTransaction, VectorWrite};
        use crate::{
            domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
            filesystem::LocalFileSystem,
            service::{
                BasisRetainer, CacheWrite, CapturedFile, DocumentError, DocumentErrorKind,
                DocumentRefreshService, DocumentResult, FileRow, FileStat, FileSystem,
                NodeMetadata, ProvenanceObservation, ProvenanceRequest, Refresher, SnapshotError,
                SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, Storage,
                StorageResult, VectorOutcome,
            },
            storage::SqliteStorage,
            wire_graph_diagnostics,
        };
        use std::{
            cell::{Cell, RefCell},
            collections::BTreeSet,
            fs,
            path::Path,
            rc::Rc,
        };
        use tempfile::TempDir;

        type Events = Rc<RefCell<Vec<&'static str>>>;
        type Service =
            DocumentRefreshService<Candidates, BoundaryFiles, CountingMarkdown, BoundaryProvenance>;
        type OrderingService =
            DocumentRefreshService<Candidates, LocalFileSystem, CountingMarkdown, TestProvenance>;
        struct BoundaryFiles {
            inner: TestFiles,
            events: Events,
        }
        struct BoundaryProvenance {
            batches: RefCell<Vec<usize>>,
            events: Events,
            failure: Option<&'static str>,
        }
        struct BoundaryStorage {
            inner: Box<dyn Storage>,
            events: Events,
            operations: Option<Events>,
        }
        struct BoundaryWrite<'a> {
            inner: Box<dyn CacheWrite + 'a>,
            events: Events,
            operations: Option<Events>,
        }

        #[test]
        fn publishes_all_nodes_and_sources_before_declared_edges() {
            // Given: a forward anchor reference, two source kinds, and a truly absent target.
            let (_dir, mut service, operations) = build_ordering_service();
            // When: one reindex publishes the final node set and declarations together.
            assert_eq!(service.reindex().unwrap().revision, 1);
            // Then: all four real endpoints precede every edge, and absence remains diagnostic.
            assert_eq!(
                *operations.borrow(),
                [
                    "node", "node", "node", "node", "edge", "edge", "edge", "edge"
                ]
            );
            let mut read = service.storage.begin_read_tx().unwrap();
            let problems = wire_graph_diagnostics()
                .find_problems(read.as_mut(), &[])
                .unwrap();
            assert_eq!(problems.len(), 1);
            assert_eq!(problems[0].details["target"], "missing.md");
        }

        #[test]
        fn target_only_refresh_does_not_reparse_or_reinsert_unchanged_owner() {
            // Given: the owner stays unchanged while its target is removed and restored.
            let (dir, mut service, operations) = build_ordering_service();
            service.reindex().unwrap();
            fs::remove_file(dir.path().join("z.md")).unwrap();
            service.refresh().unwrap();
            operations.borrow_mut().clear();
            let parses = service.markdown.0.get();
            write_ordering_target(dir.path());
            // When: a target-only refresh resolves the surviving declaration at commit.
            assert_eq!(service.refresh().unwrap().files_updated, 1);
            // Then: only the target is reparsed and upserted; unchanged declarations are not reinserted.
            assert_eq!(service.markdown.0.get() - parses, 2);
            assert_eq!(*operations.borrow(), ["node"]);
        }

        #[test]
        fn preflight_batches_only_prepared_bindings_in_bounded_chunks() {
            // Given: one document with 257 independently marked knowledge sections.
            let (_dir, mut service, events) = build_boundary_service(None);
            let bytes: String = (0..257)
                .map(|n| {
                    format!(
                        "## Item {{#item{n}}}\n<!-- letopis: {{\"created_at\":1}} -->\nKnowledge\n"
                    )
                })
                .collect();
            service.filesystem.inner.stat.update(|mut stat| {
                stat.size = bytes.len() as u64;
                stat
            });
            service.filesystem.inner.bytes.replace(bytes.into_bytes());
            // When: preflight validates the prepared projection before publishing it.
            let outcome = service.refresh().unwrap();
            // Then: each binding is observed in bounded batches, before retention and SQL write.
            assert!(outcome.published);
            assert_eq!(*service.provenance.batches.borrow(), [256, 1]);
            assert_eq!(
                events
                    .borrow()
                    .iter()
                    .filter(|event| **event == "observe")
                    .count(),
                257
            );
            let events = events.borrow();
            let retain = events.iter().position(|event| *event == "retain").unwrap();
            let begin = events.iter().position(|event| *event == "begin").unwrap();
            let last_observe = events
                .iter()
                .rposition(|event| *event == "observe")
                .unwrap();
            assert!(last_observe < retain);
            assert!(retain < begin);
        }

        #[test]
        fn verified_unchanged_refresh_audits_without_sql_write() {
            // Given: an already published document and no file delta.
            let (_dir, mut service, events) = build_boundary_service(None);
            service.refresh().unwrap();
            events.borrow_mut().clear();
            let parses = service.markdown.0.get();
            // When: full-corpus check requests a verified incremental refresh.
            let outcome = service.refresh_verified().unwrap();
            // Then: audit runs under lock, with no SQL write or body parse.
            assert_eq!(*events.borrow(), ["lock", "verify"]);
            assert!(!outcome.published);
            assert_eq!(service.markdown.0.get(), parses);
        }

        #[test]
        fn verified_refresh_failure_preserves_previous_publication() {
            // Given: a new projection whose source audit fails.
            let (_dir, mut service, events) = build_boundary_service(Some("verify"));
            // When: verification fails before any SQL write begins.
            assert!(service.refresh_verified().is_err());
            // Then: no partial graph is published.
            assert_eq!(
                *events.borrow(),
                ["lock", "read", "read", "observe", "verify"]
            );
            assert_eq!(service.storage.begin_read_tx().unwrap().get_revision(), 0);
        }

        #[test]
        fn retain_under_index_lock_before_sql_publication() {
            // Given
            let (_dir, mut service, events) = build_boundary_service(None);
            // When
            let outcome = service.refresh().unwrap();
            // Then
            assert!(outcome.published);
            assert_eq!(outcome.revision, 1);
            assert_eq!(
                *events.borrow(),
                [
                    "lock", "read", "read", "observe", "retain", "begin", "read", "commit"
                ]
            );
        }

        #[test]
        fn preflight_read_failure_never_opens_sql_write() {
            // Given
            let (_dir, mut service, events) = build_boundary_service(None);
            service.filesystem.inner.fail_on_read.set(Some(2));
            // When / Then
            assert!(service.refresh().is_err());
            // Then
            assert_eq!(*events.borrow(), ["lock", "read", "read"]);
        }

        #[test]
        fn observation_failure_never_opens_sql_write() {
            // Given
            let (_dir, mut service, events) = build_boundary_service(Some("observe"));
            // When / Then
            assert!(service.refresh().is_err());
            // Then
            assert_eq!(*events.borrow(), ["lock", "read", "read", "observe"]);
        }

        #[test]
        fn pin_failure_never_opens_sql_write() {
            // Given
            let (_dir, mut service, events) = build_boundary_service(Some("retain"));
            // When / Then
            assert!(service.refresh().is_err());
            // Then
            assert_eq!(
                *events.borrow(),
                ["lock", "read", "read", "observe", "retain"]
            );
        }

        #[test]
        fn changed_capture_after_preflight_rolls_back_without_commit() {
            // Given: a capture that changes after preflight.
            let (_dir, mut service, events) = build_boundary_service(None);
            service.filesystem.inner.replace_on_read.set(Some(3));
            // When / Then: refresh rejects the inconsistent capture; no cache commit occurs.
            assert!(service.refresh().is_err());
            // Then: no cache commit occurs.
            assert_eq!(
                *events.borrow(),
                ["lock", "read", "read", "observe", "retain", "begin", "read"]
            );
            assert_eq!(service.storage.begin_read_tx().unwrap().get_revision(), 0);
        }

        #[test]
        fn unchanged_basis_retention_skips_sql_write_and_body_parse() {
            // Given
            let (_dir, mut service, events) = build_boundary_service(None);
            let tree = TreeOid::parse(&"1".repeat(40)).unwrap();
            let note = format!(
                "<!-- letopis: {{\"created_at\":1,\"source_snapshot\":\"{}\"}} -->\nKnowledge\n",
                tree.as_str()
            );
            service
                .filesystem
                .inner
                .bytes
                .replace(note.as_bytes().to_vec());
            service.filesystem.inner.stat.update(|mut stat| {
                stat.size = note.len() as u64;
                stat
            });
            service.refresh().unwrap();
            events.borrow_mut().clear();
            let parses = service.markdown.0.get();
            // When
            let outcome = service.refresh().unwrap();
            // Then
            assert!(!outcome.published);
            assert_eq!(outcome.revision, 1);
            assert_eq!(service.markdown.0.get(), parses);
            assert_eq!(*events.borrow(), ["lock", "retain"]);
        }

        #[test]
        fn initial_build_and_full_reindex_lock_before_capturing_and_validate_basis() {
            // Given.
            let (_dir, mut service, events) = build_boundary_service(None);
            // When: the first build follows the full path, not preliminary file capture.
            service.refresh().unwrap();
            // Then.
            assert_eq!(events.borrow().first(), Some(&"lock"));
            // Given
            events.borrow_mut().clear();
            let parses = service.markdown.0.get();
            // When
            service.reindex().unwrap();
            // Then
            assert_eq!(
                *events.borrow(),
                [
                    "lock", "read", "read", "observe", "verify", "begin", "read", "commit"
                ]
            );
            assert!(service.markdown.0.get() > parses);
        }

        fn build_ordering_service() -> (TempDir, OrderingService, Events) {
            let dir = TempDir::new().unwrap();
            fs::write(dir.path().join("a.md"), concat!(
                "<!-- letopis: {\"created_at\":1} -->\n",
                "[anchor](z.md#target \"letopis:type=supports;path=z.md#target\") ",
                "[local](code.rs \"letopis:type=external;path=code.rs\") ",
                "[remote](https://Host/x?q=1#part \"letopis:type=external;path=https://Host/x?q=1#part\") ",
                "[absent](missing.md \"letopis:type=related_to;path=missing.md\")\n",
            )).unwrap();
            write_ordering_target(dir.path());
            fs::write(dir.path().join("code.rs"), "source bytes\n").unwrap();
            let mut candidates = Candidates::new();
            candidates.expect_get_candidates().returning(|| {
                Ok(vec![
                    RepoPath::parse("a.md").unwrap(),
                    RepoPath::parse("z.md").unwrap(),
                ])
            });
            let operations = Rc::new(RefCell::new(Vec::new()));
            let service = DocumentRefreshService {
                git: candidates,
                filesystem: LocalFileSystem::open(dir.path()).unwrap(),
                markdown: CountingMarkdown(Cell::new(0)),
                storage: Box::new(BoundaryStorage {
                    inner: Box::new(
                        SqliteStorage::open(
                            &dir.path().join("cache.db"),
                            &dir.path().join("vectors.db"),
                        )
                        .unwrap(),
                    ),
                    events: Rc::new(RefCell::new(Vec::new())),
                    operations: Some(Rc::clone(&operations)),
                }),
                provenance: TestProvenance,
            };
            (dir, service, operations)
        }

        fn write_ordering_target(root: &Path) {
            fs::write(
                root.join("z.md"),
                "# Target {#target}\n<!-- letopis: {\"created_at\":2} -->\nTarget\n",
            )
            .unwrap();
        }

        fn build_boundary_service(failure: Option<&'static str>) -> (TempDir, Service, Events) {
            let (dir, service) = build_service();
            let events = Rc::new(RefCell::new(Vec::new()));
            let service = DocumentRefreshService {
                git: service.git,
                filesystem: BoundaryFiles {
                    inner: service.filesystem,
                    events: Rc::clone(&events),
                },
                markdown: service.markdown,
                storage: Box::new(BoundaryStorage {
                    inner: service.storage,
                    events: Rc::clone(&events),
                    operations: None,
                }),
                provenance: BoundaryProvenance {
                    batches: RefCell::new(Vec::new()),
                    events: Rc::clone(&events),
                    failure,
                },
            };
            (dir, service, events)
        }
        impl FileSystem for BoundaryFiles {
            type IndexLock = ();
            fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
                self.inner.stat_file(path)
            }
            fn get_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
                self.events.borrow_mut().push("read");
                self.inner.get_file(path)
            }
            fn get_time_ns(&self) -> DocumentResult<i64> {
                self.inner.get_time_ns()
            }
            fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
                self.events.borrow_mut().push("lock");
                self.inner.lock_index()
            }
        }
        impl SourceProvenance for BoundaryProvenance {
            fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
                panic!("this scenario does not validate requested revisions")
            }
            fn observe_sources(
                &self,
                _: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                self.events.borrow_mut().push("observe");
                if self.failure == Some("observe") {
                    return Err(build_failure());
                }
                Ok(ProvenanceObservation {
                    flags: BTreeSet::new(),
                    issues: BTreeSet::new(),
                })
            }
            fn observe_sources_batch(
                &self,
                requests: &[ProvenanceRequest<'_>],
            ) -> DocumentResult<Vec<ProvenanceObservation>> {
                self.batches.borrow_mut().push(requests.len());
                requests
                    .iter()
                    .map(|request| self.observe_sources(*request))
                    .collect()
            }
            fn compare_revision(
                &self,
                _: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                panic!("refresh must not compare requested revisions")
            }
        }
        impl BasisRetainer for BoundaryProvenance {
            fn verify_snapshots(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
                self.events.borrow_mut().push("verify");
                if self.failure == Some("verify") {
                    return Err(build_failure());
                }
                Ok(())
            }
            fn retain_snapshots(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
                self.events.borrow_mut().push("retain");
                if self.failure == Some("retain") {
                    return Err(build_failure());
                }
                Ok(())
            }
        }
        impl Storage for BoundaryStorage {
            fn begin_vector_write_tx(
                &mut self,
                expected: &Sha256,
            ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>> {
                self.inner.begin_vector_write_tx(expected)
            }
            fn reset_vectors(
                &mut self,
                observed: Option<&Sha256>,
                identity: &Sha256,
            ) -> StorageResult<VectorOutcome<bool>> {
                self.inner.reset_vectors(observed, identity)
            }
            fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>> {
                self.events.borrow_mut().push("begin");
                Ok(Box::new(BoundaryWrite {
                    inner: self.inner.begin_cache_write_tx()?,
                    events: Rc::clone(&self.events),
                    operations: self.operations.clone(),
                }))
            }
            fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>> {
                self.inner.begin_read_tx()
            }
        }
        impl<'a> CacheWrite for BoundaryWrite<'a> {
            fn upsert_node(&mut self, node: &NodeMetadata) -> StorageResult<i64> {
                if let Some(operations) = &self.operations {
                    operations.borrow_mut().push("node");
                }
                self.inner.upsert_node(node)
            }
            fn insert_edge(&mut self, edge: &Edge) -> StorageResult<()> {
                if let Some(operations) = &self.operations {
                    operations.borrow_mut().push("edge");
                }
                self.inner.insert_edge(edge)
            }
            fn delete_outgoing(&mut self, id: &KnowledgeId) -> StorageResult<()> {
                self.inner.delete_outgoing(id)
            }
            fn delete_node(&mut self, id: &EdgeTarget) -> StorageResult<()> {
                self.inner.delete_node(id)
            }
            fn replace_text(&mut self, id: &KnowledgeId, text: &str) -> StorageResult<()> {
                self.inner.replace_text(id, text)
            }
            fn upsert_file(&mut self, file: &FileRow) -> StorageResult<()> {
                self.inner.upsert_file(file)
            }
            fn delete_file(&mut self, path: &RepoPath) -> StorageResult<()> {
                self.inner.delete_file(path)
            }
            fn commit(self: Box<Self>) -> StorageResult<u64> {
                self.events.borrow_mut().push("commit");
                self.inner.commit()
            }
        }
        fn build_failure() -> DocumentError {
            DocumentError::new(
                DocumentErrorKind::InternalError,
                "injected provenance failure",
            )
        }
    }
    pub(super) mod process {
        //! Production-process schedules. Only this private test driver reads control variables.
        use super::super::DocumentRefreshService;
        use crate::{
            domain::{Edge, RepoPath, Sha256},
            filesystem::LocalFileSystem,
            git::GitRepository,
            markdown::MarkdownParser,
            service::{
                CacheWrite, EmbeddingFailure, FileRow, FileSystem, GraphDiagnosticsService,
                NodeMetadata, ProvenanceService, QueryError, Refresher, StatusReader, StatusReport,
                StatusService, Storage, StorageError, StorageResult, StoredNode, VectorOutcome,
            },
            storage::{SqliteStatusStorage, SqliteStorage},
            wire_document_refresher,
        };
        use rusqlite::Connection;
        use std::{
            cell::RefCell,
            fs::{self, File},
            io::{BufRead, BufReader, Read, Write},
            os::unix::{
                fs::MetadataExt,
                net::{UnixListener, UnixStream},
                process::ExitStatusExt,
            },
            path::{Path, PathBuf},
            process::{Child, Command, ExitStatus, Stdio},
            sync::mpsc,
            thread,
            time::{Duration, Instant},
        };
        use tempfile::TempDir;

        const TIME: i64 = 42;
        const WAIT: Duration = Duration::from_secs(20);
        const CHILD: &str = "service::document_refresher::tests::process::run_child";
        const SOURCE: &[u8] = b"external source\r\nexact bytes\n";
        const OLD: &str = "<!-- letopis: {\"created_at\":1} -->\nAlphaToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
        const NEW: &str = "<!-- letopis: {\"created_at\":3} -->\nBravoToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
        const LATEST: &str = "<!-- letopis: {\"created_at\":3} -->\nCharlieToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
        const DIAGNOSTIC_CANARY: &str = "TK28_DIAGNOSTIC_CANARY";
        const INVALID: &str = "<!-- letopis: invalid TK28_DIAGNOSTIC_CANARY -->\n";
        const RETAINED: &str = "<!-- letopis: {\"created_at\":2} -->\nRetainedToken [source](source.txt \"letopis:type=external;path=source.txt\")\n";
        const CUTS: [&str; 10] = [
            "detected",
            "locked",
            "redetected",
            "begun",
            "cleared",
            "projections",
            "fts",
            "fingerprints",
            "before_commit",
            "after_commit",
        ];
        type Service = DocumentRefreshService<
            GitRepository,
            LocalFileSystem,
            MarkdownParser,
            ProvenanceService<LocalFileSystem>,
        >;
        type Tokens = Vec<(String, i64, String, i64)>;
        thread_local! {
            static CONTROL: RefCell<Option<UnixStream>> = const { RefCell::new(None) };
            static FAIL_LATE: RefCell<bool> = const { RefCell::new(false) };
        }

        pub(in crate::service) fn reach_checkpoint(name: &str) {
            CONTROL.with_borrow_mut(|control| {
                if let Some(stream) = control {
                    writeln!(stream, "{name}").unwrap();
                    eprintln!("checkpoint {name}");
                    let mut response = [0];
                    stream.read_exact(&mut response).unwrap();
                    assert_eq!(response, [b'c']);
                }
            });
        }

        pub(in crate::service) fn fail_late(unit: &mut dyn CacheWrite) -> StorageResult<()> {
            if FAIL_LATE.with_borrow(|enabled| *enabled) {
                return unit.upsert_file(&FileRow {
                    path: RepoPath::parse("invalid-size.md").unwrap(),
                    size: u64::MAX,
                    mtime_ns: 0,
                    inode: 0,
                    hash: Sha256::digest(b"overflow"),
                    checked_at_ns: TIME,
                    valid: true,
                    error: None,
                });
            }
            Ok(())
        }

        #[test]
        #[ignore = "subprocess entry; invoked only by the production schedule tests"]
        fn run_child() {
            // Given
            let socket = std::env::var_os("TK10_SOCKET").expect("private child control");
            let root = PathBuf::from(std::env::var_os("TK10_ROOT").unwrap());
            let mode = std::env::var("TK10_MODE").unwrap();
            let stream = UnixStream::connect(socket).unwrap();
            stream.set_read_timeout(Some(WAIT)).unwrap();
            stream.set_write_timeout(Some(WAIT)).unwrap();
            CONTROL.with_borrow_mut(|control| *control = Some(stream));
            reach_checkpoint("connected");
            // When
            if let Some(mode) = mode.strip_prefix("normal-") {
                run_normal_service(&root, mode);
                reach_checkpoint("finished");
                return;
            }
            let mut service = build_service(&root);
            if mode == "hold" {
                let _guard = service.filesystem.lock_index().unwrap();
                reach_checkpoint("holding");
            } else {
                FAIL_LATE.with_borrow_mut(|enabled| *enabled = mode.ends_with("fail"));
                let result = if mode.starts_with("reindex") {
                    service.reindex()
                } else {
                    service.refresh()
                };
                eprintln!("refresh result: {result:?}");
                if mode.ends_with("fail") {
                    let error = result.unwrap_err();
                    // Then
                    assert!(
                        std::error::Error::source(&error)
                            .unwrap()
                            .downcast_ref::<StorageError>()
                            .is_some()
                    );
                    assert!(!format!("{error:?}").contains(DIAGNOSTIC_CANARY));
                } else {
                    let result = result.unwrap();
                    assert_eq!(result.published, mode != "noop");
                }
            }
            drop(service);
            reach_checkpoint("finished");
        }

        #[test]
        fn normal_open_waits_at_index_lock_during_cache_write_then_redetects() {
            // Given: the owner holds a real cache transaction, not just index.lock.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let expected = fixture.build_expected(NEW, "BravoToken");
            let mut owner = Worker::spawn(fixture.root.path(), "publish");
            owner.reach("begun");
            // When: normal composition must open before the owner releases its SQLite writer.
            let mut waiter = Worker::spawn(fixture.root.path(), "normal-noop");
            waiter.reach("opened");
            waiter.resume();
            waiter.reach("contended");
            owner.resume();
            owner.finish();
            waiter.resume();
            waiter.finish();
            // Then: the waiter redetects the committed publication and does not republish.
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(NEW);
        }

        #[test]
        fn normal_open_redetects_after_active_cache_writer_is_killed() {
            // Given a real transaction with an old publication still visible.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let mut owner = Worker::spawn(fixture.root.path(), "publish");
            owner.reach("begun");
            let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
            waiter.reach("contended");
            // When: change the file while waiting, then release the writer through SIGKILL.
            fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
            let expected = fixture.build_expected(LATEST, "CharlieToken");
            owner.kill();
            waiter.resume();
            waiter.finish();
            // Then: only the newest complete publication becomes visible.
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(LATEST);
        }

        #[test]
        fn normal_open_and_noop_finish_during_vector_write_and_held_index_lock() {
            // Given: independent live vector and kernel-lock owners.
            let fixture = Fixture::new();
            let mut owner = Worker::spawn(fixture.root.path(), "hold");
            owner.reach("holding");
            let mut storage = build_service(fixture.root.path()).storage;
            let identity = Sha256::digest(b"model identity");
            let VectorOutcome::Current(mut unit) =
                storage.begin_vector_write_tx(&identity).unwrap()
            else {
                panic!("identity changed");
            };
            unit.set_query_error(99, "uncommitted").unwrap();
            // When: normal composition and a no-op must finish with both owners still active.
            Worker::spawn(fixture.root.path(), "normal-noop").finish();
            // Then: neither the cache nor the committed vector state was changed.
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            assert_eq!(get_vectors(fixture.root.path()), fixture.vector_state);
            drop(unit);
            owner.kill();
        }

        struct Worker {
            child: Child,
            stream: BufReader<UnixStream>,
            _socket: TempDir,
            receipt: File,
        }
        impl Worker {
            fn spawn(root: &Path, mode: &str) -> Self {
                let socket = tempfile::Builder::new()
                    .prefix("tk10-")
                    .tempdir_in("/tmp")
                    .unwrap();
                let listener = UnixListener::bind(socket.path().join("control")).unwrap();
                let evidence = std::env::var_os("LETOPIS_TEST_EVIDENCE")
                    .map_or_else(|| PathBuf::from(".artifacts/tk10/process"), PathBuf::from);
                fs::create_dir_all(&evidence).unwrap();
                let name = socket.path().file_name().unwrap().to_str().unwrap();
                let log = File::create(evidence.join(format!("{name}.log"))).unwrap();
                let child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", CHILD, "--ignored", "--nocapture"])
                    .env("TK10_SOCKET", socket.path().join("control"))
                    .env("TK10_ROOT", root)
                    .env("TK10_MODE", mode)
                    .stdout(Stdio::from(log.try_clone().unwrap()))
                    .stderr(log)
                    .spawn()
                    .unwrap();
                let (sender, receiver) = mpsc::channel();
                let accept = thread::spawn(move || sender.send(listener.accept()).ok());
                let accepted = receiver.recv_timeout(WAIT);
                if accepted.is_err() {
                    let mut child = child;
                    let _ = child.kill();
                    let _ = child.wait();
                    // Unblock the accept thread before unwinding on a failed startup.
                    let _ = UnixStream::connect(socket.path().join("control"));
                    let _ = accept.join();
                    panic!("child did not connect: {accepted:?}");
                }
                let (stream, _) = accepted.unwrap().unwrap();
                accept.join().unwrap();
                stream.set_read_timeout(Some(WAIT)).unwrap();
                stream.set_write_timeout(Some(WAIT)).unwrap();
                let receipt = File::create(evidence.join(format!("{name}.receipt"))).unwrap();
                let mut worker = Self {
                    child,
                    stream: BufReader::new(stream),
                    _socket: socket,
                    receipt,
                };
                writeln!(worker.receipt, "mode={mode} pid={}", worker.child.id()).unwrap();
                assert_eq!(worker.get_event(), "connected");
                worker.resume();
                worker
            }
            fn get_event(&mut self) -> String {
                let mut line = String::new();
                assert!(
                    self.stream.read_line(&mut line).unwrap() > 0,
                    "child exited before acknowledgement"
                );
                writeln!(self.receipt, "{}", line.trim()).unwrap();
                line.trim().to_owned()
            }
            fn resume(&mut self) {
                self.stream.get_mut().write_all(b"c").unwrap();
            }
            fn reach(&mut self, target: &str) {
                loop {
                    let event = self.get_event();
                    if event == target {
                        return;
                    }
                    assert_ne!(event, "finished", "checkpoint {target} was never reached");
                    self.resume();
                }
            }
            fn finish(mut self) {
                self.reach("finished");
                self.resume();
                let status = self.wait_exit();
                writeln!(self.receipt, "exit={status}").unwrap();
                assert!(status.success(), "{status}");
            }
            fn wait_exit(&mut self) -> ExitStatus {
                let deadline = Instant::now() + WAIT;
                loop {
                    if let Some(status) = self.child.try_wait().unwrap() {
                        return status;
                    }
                    assert!(
                        Instant::now() < deadline,
                        "child did not exit within 20 seconds"
                    );
                    // Only an exit watchdog; schedules advance exclusively by acknowledgements.
                    thread::sleep(Duration::from_millis(1));
                }
            }
            fn kill(mut self) {
                self.child.kill().unwrap();
                let status = self.wait_exit();
                writeln!(self.receipt, "signal={:?}", status.signal()).unwrap();
                assert_eq!(status.signal(), Some(9));
            }
        }
        impl Drop for Worker {
            fn drop(&mut self) {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }

        #[derive(Clone, Debug, PartialEq, Eq)]
        struct State {
            revision: u64,
            nodes: Vec<StoredNode>,
            edges: Vec<Edge>,
            files: Vec<FileRow>,
            tokens: Tokens,
        }
        #[derive(Debug, PartialEq)]
        struct VectorState {
            identity: Option<Sha256>,
            vector: VectorOutcome<Option<Vec<f32>>>,
            failure: VectorOutcome<Option<EmbeddingFailure>>,
            query_error: Option<QueryError>,
        }
        struct Fixture {
            root: TempDir,
            old: State,
            vectors: Vec<u8>,
            vector_state: VectorState,
            git_objects: Vec<u8>,
        }
        impl Fixture {
            fn new() -> Self {
                let root = tempfile::tempdir().unwrap();
                assert!(
                    Command::new("git")
                        .args(["init", "-q"])
                        .arg(root.path())
                        .status()
                        .unwrap()
                        .success()
                );
                for (name, bytes) in [
                    ("a.md", OLD),
                    (
                        "b.md",
                        "<!-- letopis: {\"created_at\":1} -->\nDeletedToken\n",
                    ),
                    ("i.md", RETAINED),
                    (
                        "z.md",
                        "<!-- letopis: {\"created_at\":1} -->\nStableToken\n",
                    ),
                ] {
                    fs::write(root.path().join(name), bytes).unwrap();
                }
                fs::write(root.path().join("source.txt"), SOURCE).unwrap();
                let git_objects = get_git_objects(root.path());
                let mut service = build_service(root.path());
                assert_eq!(service.refresh().unwrap().revision, 1);
                assert_eq!(get_git_objects(root.path()), git_objects);
                seed_vectors(service.storage.as_mut());
                drop(service);
                let old = get_state(root.path());
                let vectors = fs::read(root.path().join(".letopis/vectors.db")).unwrap();
                let vector_state = get_vectors(root.path());
                Self {
                    root,
                    old,
                    vectors,
                    vector_state,
                    git_objects,
                }
            }
            fn edit(&self, bytes: &str) {
                fs::write(self.root.path().join("a.md"), bytes).unwrap();
                fs::write(self.root.path().join("i.md"), INVALID).unwrap();
                fs::remove_file(self.root.path().join("b.md")).unwrap();
            }
            fn invalidate_retained_file(&self) {
                fs::write(self.root.path().join("i.md"), INVALID).unwrap();
            }
            fn restore_retained_file(&self) {
                fs::write(self.root.path().join("i.md"), RETAINED).unwrap();
            }
            fn assert_preserved(&self, bytes: &str) {
                assert_eq!(
                    fs::read(self.root.path().join("a.md")).unwrap(),
                    bytes.as_bytes()
                );
                assert_eq!(
                    fs::read(self.root.path().join("i.md")).unwrap(),
                    INVALID.as_bytes()
                );
                assert_eq!(
                    fs::read(self.root.path().join("source.txt")).unwrap(),
                    SOURCE
                );
                assert_eq!(
                    fs::read(self.root.path().join("z.md")).unwrap(),
                    b"<!-- letopis: {\"created_at\":1} -->\nStableToken\n"
                );
                assert!(!self.root.path().join("b.md").exists());
                assert_eq!(get_git_objects(self.root.path()), self.git_objects);
                assert_eq!(get_vectors(self.root.path()), self.vector_state);
                assert_eq!(
                    fs::read(self.root.path().join(".letopis/vectors.db")).unwrap(),
                    self.vectors
                );
            }
            fn build_expected(&self, bytes: &str, token: &str) -> State {
                let mut expected = self.old.clone();
                expected.revision = 2;
                expected.nodes.retain(
                    |node| !matches!(&node.metadata, NodeMetadata::Knowledge(k) if k.id.as_str() == "b.md"),
                );
                for node in &mut expected.nodes {
                    match &mut node.metadata {
                        NodeMetadata::Knowledge(k) if k.id.as_str() == "a.md" => {
                            k.hash = Sha256::digest(bytes.as_bytes());
                            k.search_hash = Sha256::digest(format!("{token} source").as_bytes());
                            k.created_at = 3;
                        }
                        NodeMetadata::Source(s) => s.created_at = 2,
                        _ => {}
                    }
                }
                expected.files.retain(|file| file.path.as_str() != "b.md");
                for file in &mut expected.files {
                    let contents = match file.path.as_str() {
                        "a.md" => bytes,
                        "i.md" => INVALID,
                        _ => continue,
                    };
                    // Independent oracle: observe the file with std, not the adapter under test.
                    let metadata = fs::metadata(self.root.path().join(file.path.as_str())).unwrap();
                    file.size = metadata.len();
                    file.mtime_ns = metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec();
                    file.inode = metadata.ino();
                    file.hash = Sha256::digest(contents.as_bytes());
                    file.checked_at_ns = TIME;
                    if file.path.as_str() == "i.md" {
                        file.valid = false;
                        file.error = Some("invalid letopis metadata".to_owned());
                    }
                }
                expected
                    .tokens
                    .retain(|(term, _, _, _)| term != "deletedtoken");
                for (term, _, _, _) in &mut expected.tokens {
                    if term == "alphatoken" {
                        *term = token.to_lowercase();
                    }
                }
                expected.tokens.sort();
                expected
            }
        }
        #[test]
        fn sigkill_keeps_exact_old_or_complete_new_publication() {
            for cut in CUTS {
                // Given: independently captured old state and literal expected new state.
                let fixture = Fixture::new();
                fixture.edit(NEW);
                let expected = fixture.build_expected(NEW, "BravoToken");
                let mut worker = Worker::spawn(fixture.root.path(), "publish");
                worker.reach(cut);
                if matches!(cut, "cleared" | "projections" | "fts" | "fingerprints") {
                    // The first file is a deletion. Cut the second file after real replacements.
                    worker.resume();
                    worker.reach(cut);
                }
                // When: genuine process death at an acknowledged operation boundary.
                worker.kill();
                // Then: reopen, including SQLite recovery, before retrying refresh.
                let actual = get_state(fixture.root.path());
                assert_eq!(
                    actual,
                    if cut == "after_commit" {
                        &expected
                    } else {
                        &fixture.old
                    }
                    .clone(),
                    "cut={cut}"
                );
                let filesystem = LocalFileSystem::open(fixture.root.path()).unwrap();
                let guard = filesystem.lock_index().unwrap();
                drop(guard);
                fixture.assert_preserved(NEW);
                Worker::spawn(
                    fixture.root.path(),
                    if cut == "after_commit" {
                        "noop"
                    } else {
                        "publish"
                    },
                )
                .finish();
                assert_eq!(
                    get_state(fixture.root.path()),
                    expected,
                    "restart cut={cut}"
                );
                fixture.assert_preserved(NEW);
            }
        }

        #[test]
        fn duplicate_publishers_commit_once_after_real_contention() {
            // Given
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let expected = fixture.build_expected(NEW, "BravoToken");
            let mut owner = Worker::spawn(fixture.root.path(), "publish");
            owner.reach("locked");
            let mut waiter = Worker::spawn(fixture.root.path(), "noop");
            waiter.reach("contended");
            // When
            owner.resume();
            owner.finish();
            waiter.resume();
            waiter.finish();
            // Then
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(NEW);
        }

        #[test]
        fn waiting_publisher_redetects_newer_edit_after_owner_is_killed() {
            // Given
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let mut owner = Worker::spawn(fixture.root.path(), "hold");
            owner.reach("holding");
            let mut waiter = Worker::spawn(fixture.root.path(), "publish");
            waiter.reach("contended");
            fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
            let expected = fixture.build_expected(LATEST, "CharlieToken");
            let lock_before =
                fs::metadata(fixture.root.path().join(".letopis/index.lock")).unwrap();
            // When
            owner.kill();
            waiter.resume();
            waiter.finish();
            // Then
            assert_eq!(
                fs::metadata(fixture.root.path().join(".letopis/index.lock"))
                    .unwrap()
                    .ino(),
                lock_before.ino()
            );
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(LATEST);
        }

        #[test]
        fn noop_finishes_while_another_process_holds_index_lock() {
            // Given
            let fixture = Fixture::new();
            let mut owner = Worker::spawn(fixture.root.path(), "hold");
            owner.reach("holding");
            // When
            Worker::spawn(fixture.root.path(), "noop").finish();
            // Then
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            owner.kill();
        }

        #[test]
        fn pinned_wal_reader_keeps_old_snapshot_and_new_reader_sees_complete_commit() {
            // Given: a WAL reader pinned before another publication.
            let fixture = Fixture::new();
            fixture.edit(NEW);
            let expected = fixture.build_expected(NEW, "BravoToken");
            let mut storage = build_service(fixture.root.path()).storage;
            let mut pinned = storage.begin_read_tx().unwrap();
            let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
            fts.execute_batch("begin;").unwrap();
            assert_eq!(get_tokens(&fts), fixture.old.tokens);
            // When: a separate worker publishes changed contents.
            Worker::spawn(fixture.root.path(), "publish").finish();
            // Then: old and new readers see complete distinct revisions.
            assert_eq!(pinned.get_revision(), fixture.old.revision);
            assert_eq!(pinned.get_nodes().unwrap(), fixture.old.nodes);
            assert_eq!(
                pinned.get_edges_by_source_id(None).unwrap(),
                fixture.old.edges
            );
            assert_eq!(pinned.get_files().unwrap(), fixture.old.files);
            assert_eq!(get_tokens(&fts), fixture.old.tokens);
            assert_eq!(get_state(fixture.root.path()), expected);
            fixture.assert_preserved(NEW);
        }

        #[test]
        fn late_storage_error_rolls_back_every_cache_mutation() {
            // Given
            let fixture = Fixture::new();
            fixture.edit(NEW);
            // When
            Worker::spawn(fixture.root.path(), "fail").finish();
            // Then
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            fixture.assert_preserved(NEW);
            // When
            Worker::spawn(fixture.root.path(), "publish").finish();
            // Then
            assert_eq!(
                get_state(fixture.root.path()),
                fixture.build_expected(NEW, "BravoToken")
            );
        }

        #[test]
        fn uncommitted_file_diagnostic_is_absent_from_status() {
            // Given: a valid publication, then one authored file becomes invalid.
            let fixture = Fixture::new();
            fixture.invalidate_retained_file();

            // When: the cache commit fails after the invalid file projection is staged.
            Worker::spawn(fixture.root.path(), "fail").finish();

            // Then: the old publication remains and a fresh status derives no file problem.
            assert_eq!(get_state(fixture.root.path()), fixture.old);
            let status = get_status(fixture.root.path());
            assert_eq!(status.revision, Some(1));
            assert!(get_invalid_file_problems(&status).is_empty());
            assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
        }

        #[test]
        fn failed_publication_retry_persists_the_file_diagnostic() {
            // Given: a failed publication with one invalid authored file.
            let fixture = Fixture::new();
            fixture.invalidate_retained_file();
            Worker::spawn(fixture.root.path(), "fail").finish();

            // When: the injected fault is gone and publication retries.
            Worker::spawn(fixture.root.path(), "publish").finish();

            // Then: the real safe diagnostic is persisted and survives a fresh status.
            let status = get_status(fixture.root.path());
            assert_eq!(status.revision, Some(2));
            assert_eq!(
                get_invalid_file_problems(&status),
                vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
            );
            assert!(!format!("{status:?}").contains(DIAGNOSTIC_CANARY));
        }

        #[test]
        fn repaired_file_problem_disappears_after_publication() {
            // Given: a publication that stored the invalid-file diagnostic.
            let fixture = Fixture::new();
            fixture.invalidate_retained_file();
            Worker::spawn(fixture.root.path(), "publish").finish();
            assert_eq!(
                get_invalid_file_problems(&get_status(fixture.root.path())),
                vec![("i.md".to_owned(), "invalid_metadata".to_owned())]
            );

            // When: the authored file is repaired and republished.
            fixture.restore_retained_file();
            Worker::spawn(fixture.root.path(), "publish").finish();

            // Then: the current problem disappears.
            let status = get_status(fixture.root.path());
            assert_eq!(status.revision, Some(3));
            assert!(get_invalid_file_problems(&status).is_empty());
        }

        fn run_normal_service(root: &Path, mode: &str) {
            LocalFileSystem::configure_next_process_test(
                TIME,
                Box::new(|| reach_checkpoint("contended")),
            );
            let mut service = wire_document_refresher(root).unwrap();
            reach_checkpoint("opened");
            let outcome = service.refresh().unwrap();
            assert_eq!(outcome.published, mode != "noop");
        }

        fn get_state(root: &Path) -> State {
            let mut storage = build_service(root).storage;
            let mut read = storage.begin_read_tx().unwrap();
            let connection = Connection::open(root.join(".letopis/cache.db")).unwrap();
            State {
                revision: read.get_revision(),
                nodes: read.get_nodes().unwrap(),
                edges: read.get_edges_by_source_id(None).unwrap(),
                files: read.get_files().unwrap(),
                tokens: get_tokens(&connection),
            }
        }

        fn get_tokens(connection: &Connection) -> Tokens {
            connection.execute_batch("create virtual table if not exists temp.tokens using fts5vocab(main, text_index, instance);").unwrap();
            connection
                .prepare("select term, doc, col, offset from temp.tokens order by term, doc, col, offset")
                .unwrap()
                .query_map([], |row| {
                    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        }

        fn get_vectors(root: &Path) -> VectorState {
            let mut storage = build_service(root).storage;
            let mut read = storage.begin_read_tx().unwrap();
            let identity = Sha256::digest(b"model identity");
            VectorState {
                identity: read.get_config_identity(),
                vector: read
                    .get_vector_by_search_hash(&identity, &Sha256::digest(b"retained vector"))
                    .unwrap(),
                failure: read
                    .get_failure_by_search_hash(&identity, &Sha256::digest(b"failed vector"))
                    .unwrap(),
                query_error: read.get_query_error().unwrap(),
            }
        }

        fn get_status(root: &Path) -> StatusReport {
            let git = GitRepository::open(root).unwrap();
            let files = LocalFileSystem::open(git.get_root()).unwrap();
            let pinned = files.open_existing_service_directory().unwrap().unwrap();
            let storage =
                SqliteStatusStorage::open_confined(&git.get_root().join(".letopis"), &pinned)
                    .unwrap();
            let provenance = ProvenanceService::new(
                Box::new(GitRepository::open(root).unwrap()),
                LocalFileSystem::open(root).unwrap(),
            );
            let provider = crate::build_embedding_provider().unwrap();
            let graph = GraphDiagnosticsService::new();
            let mut status = StatusService {
                storage: Box::new(storage),
                files: &files,
                graph: &graph,
                provenance: &provenance,
                provider: &provider,
            };
            status.get_status().unwrap()
        }

        fn get_invalid_file_problems(report: &StatusReport) -> Vec<(String, String)> {
            report
                .problems
                .iter()
                .filter_map(|problem| {
                    let path = problem.details.get("path")?.as_str()?.to_owned();
                    let reason = problem.details.get("reason")?.as_str()?.to_owned();
                    (path == "i.md" && reason == "invalid_metadata").then_some((path, reason))
                })
                .collect()
        }

        fn build_service(root: &Path) -> Service {
            let git = GitRepository::open(root).unwrap();
            let mut filesystem = LocalFileSystem::open(git.get_root()).unwrap();
            filesystem.configure_process_test(TIME, Box::new(|| reach_checkpoint("contended")));
            let directory = filesystem.open_service_directory().unwrap();
            let storage =
                SqliteStorage::open_confined(&git.get_root().join(".letopis"), &directory).unwrap();
            let provenance = ProvenanceService::new(
                Box::new(GitRepository::open(root).unwrap()),
                LocalFileSystem::open(root).unwrap(),
            );
            DocumentRefreshService {
                git,
                filesystem,
                markdown: MarkdownParser::new(),
                storage: Box::new(storage),
                provenance,
            }
        }

        fn get_git_objects(root: &Path) -> Vec<u8> {
            let output = Command::new("git")
                .arg("-C")
                .arg(root)
                .args(["count-objects", "-v"])
                .output()
                .unwrap();
            assert!(output.status.success());
            output.stdout
        }

        fn seed_vectors(storage: &mut dyn Storage) {
            let identity = Sha256::digest(b"model identity");
            storage.reset_vectors(None, &identity).unwrap();
            let VectorOutcome::Current(mut unit) =
                storage.begin_vector_write_tx(&identity).unwrap()
            else {
                panic!("identity changed");
            };
            unit.store_vector(&Sha256::digest(b"retained vector"), &[1.0, 0.0], 2)
                .unwrap();
            let failure = Sha256::digest(b"failed vector");
            assert!(unit.reserve_attempt(&failure, 3).unwrap());
            unit.record_failure(&failure, "safe failure").unwrap();
            unit.set_query_error(7, "safe query error").unwrap();
            unit.commit().unwrap();
        }

        mod reindex {
            use super::{Fixture, LATEST, NEW, Worker, build_service, get_state, get_tokens};
            use crate::service::Refresher;
            use rusqlite::Connection;
            use std::{fs, os::unix::process::ExitStatusExt};

            #[test]
            fn reindex_sigint_preserves_exact_old_or_complete_new_state() {
                for cut in ["locked", "begun", "fts", "before_commit", "after_commit"] {
                    // Given: the actual child pauses at an acknowledged operation boundary.
                    let fixture = Fixture::new();
                    fixture.edit(NEW);
                    let expected = fixture.build_expected(NEW, "BravoToken");
                    let mut worker = Worker::spawn(fixture.root.path(), "reindex");
                    worker.reach(cut);
                    // When.
                    let pid =
                        rustix::process::Pid::from_raw(i32::try_from(worker.child.id()).unwrap())
                            .unwrap();
                    rustix::process::kill_process(pid, rustix::process::Signal::INT).unwrap();
                    let status = worker.wait_exit();
                    // Then: signal death is distinct from TK-29 handled JSON cancellation.
                    assert_eq!(status.signal(), Some(2));
                    assert_eq!(
                        get_state(fixture.root.path()),
                        if cut == "after_commit" {
                            expected
                        } else {
                            fixture.old.clone()
                        }
                    );
                    fixture.assert_preserved(NEW);
                    // The process's kernel lease has been released; a fresh call can publish.
                    let mut service = build_service(fixture.root.path());
                    assert!(service.reindex().unwrap().published);
                }
            }

            #[test]
            fn reindex_keeps_pinned_wal_view_and_publishes_one_complete_new_view() {
                // Given: pinned graph and FTS views with a worker paused before commit.
                let fixture = Fixture::new();
                fixture.edit(NEW);
                let expected = fixture.build_expected(NEW, "BravoToken");
                let mut storage = build_service(fixture.root.path()).storage;
                let mut pinned = storage.begin_read_tx().unwrap();
                let fts = Connection::open(fixture.root.path().join(".letopis/cache.db")).unwrap();
                fts.execute_batch("begin").unwrap();
                assert_eq!(get_tokens(&fts), fixture.old.tokens);
                let mut worker = Worker::spawn(fixture.root.path(), "reindex");
                worker.reach("before_commit");
                assert_eq!(get_state(fixture.root.path()), fixture.old);
                // When: the reindex worker is allowed to commit.
                worker.resume();
                worker.finish();
                // Then: old readers keep the old state and a fresh read sees the complete new state.
                assert_eq!(pinned.get_revision(), 1);
                assert_eq!(pinned.get_nodes().unwrap(), fixture.old.nodes);
                assert_eq!(
                    pinned.get_edges_by_source_id(None).unwrap(),
                    fixture.old.edges
                );
                assert_eq!(pinned.get_files().unwrap(), fixture.old.files);
                assert_eq!(get_tokens(&fts), fixture.old.tokens);
                assert_eq!(get_state(fixture.root.path()), expected);
            }

            #[test]
            fn publisher_waits_for_reindex_and_redetects_latest_file_bytes() {
                // Given: reindex finishes capturing the old bytes before the next edit.
                let fixture = Fixture::new();
                fixture.edit(NEW);
                let mut owner = Worker::spawn(fixture.root.path(), "reindex");
                owner.reach("before_commit");
                let mut waiter = Worker::spawn(fixture.root.path(), "normal-publish");
                waiter.reach("contended");
                fs::write(fixture.root.path().join("a.md"), LATEST).unwrap();
                let mut expected = fixture.build_expected(LATEST, "CharlieToken");
                expected.revision = 3;
                // When.
                owner.resume();
                owner.finish();
                waiter.resume();
                waiter.finish();
                // Then.
                assert_eq!(get_state(fixture.root.path()), expected);
                fixture.assert_preserved(LATEST);
            }

            #[test]
            fn reindex_late_sql_error_rolls_back_all_state_and_releases_lock() {
                // Given.
                let fixture = Fixture::new();
                fixture.edit(NEW);
                // When.
                Worker::spawn(fixture.root.path(), "reindex-fail").finish();
                // Then.
                assert_eq!(get_state(fixture.root.path()), fixture.old);
                fixture.assert_preserved(NEW);
                Worker::spawn(fixture.root.path(), "reindex").finish();
                assert_eq!(
                    get_state(fixture.root.path()),
                    fixture.build_expected(NEW, "BravoToken")
                );
            }
        }
    }

    mod reading {
        use super::{CountingMarkdown, TestFiles, TestProvenance, build_candidates, build_service};
        use crate::service::{ReadTransaction, VectorWrite};
        use crate::{
            domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
            service::{
                BasisRetainer, CacheWrite, DocumentRefreshService, DocumentResult, ElementAddress,
                EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary, EmbeddingWork,
                FileRow, GraphNeighbor, GraphNodeRef, GraphRead, NeighborPageRequest, NodeMetadata,
                ProvenanceObservation, ProvenanceRequest, QueryError, Refresher, SnapshotError,
                SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, Storage,
                StorageRead, StorageResult, StoredNode, TextMatch, VectorMatch, VectorOutcome,
            },
        };
        use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};

        struct ReadingStorage {
            inner: Box<dyn Storage>,
            reads: Rc<Cell<usize>>,
        }
        struct ReadingView<'a> {
            inner: Box<dyn ReadTransaction + 'a>,
            reads: Rc<Cell<usize>>,
        }
        struct RetainingProvenance;

        #[test]
        fn warm_refresh_reads_basis_roots_without_loading_nodes() {
            // Given: a warm projection with declared basis roots.
            let (_root, mut initial) = build_service();
            initial.refresh().unwrap();
            let mut node = initial
                .storage
                .begin_read_tx()
                .unwrap()
                .get_nodes()
                .unwrap()
                .remove(0)
                .metadata;
            let NodeMetadata::Knowledge(ref mut knowledge) = node else {
                panic!()
            };
            knowledge.source_snapshot = Some(TreeOid::parse(&"a".repeat(40)).unwrap());
            let mut write = initial.storage.begin_cache_write_tx().unwrap();
            write.upsert_node(&node).unwrap();
            write.commit().unwrap();
            let reads = Rc::new(Cell::new(0));
            let mut service = DocumentRefreshService {
                git: build_candidates(),
                filesystem: TestFiles::new(),
                markdown: CountingMarkdown(Cell::new(0)),
                storage: Box::new(ReadingStorage {
                    inner: initial.storage,
                    reads: Rc::clone(&reads),
                }),
                provenance: RetainingProvenance,
            };
            // When: refresh checks the unchanged corpus.
            let result = service.refresh().unwrap();
            // Then: basis roots are observed without loading node metadata.
            assert!(!result.published);
            assert_eq!(result.revision, 2);
            assert_eq!(reads.get(), 2);
            assert_eq!(service.filesystem.locks.get(), 1);
            assert_eq!(service.filesystem.reads.get(), 0);
            assert_eq!(service.markdown.0.get(), 0);
        }

        impl Storage for ReadingStorage {
            fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>> {
                Ok(Box::new(ReadingView {
                    inner: self.inner.begin_read_tx()?,
                    reads: Rc::clone(&self.reads),
                }))
            }
            fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>> {
                self.inner.begin_cache_write_tx()
            }
            fn reset_vectors(
                &mut self,
                observed: Option<&Sha256>,
                identity: &Sha256,
            ) -> StorageResult<VectorOutcome<bool>> {
                self.inner.reset_vectors(observed, identity)
            }
            fn begin_vector_write_tx(
                &mut self,
                expected: &Sha256,
            ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>> {
                self.inner.begin_vector_write_tx(expected)
            }
        }
        impl<'a> GraphRead for ReadingView<'a> {
            fn get_node_ref_by_id(
                &mut self,
                id: &ElementAddress,
            ) -> StorageResult<Option<GraphNodeRef>> {
                self.inner.get_node_ref_by_id(id)
            }
            fn get_neighbor_page(
                &mut self,
                request: &NeighborPageRequest<'_>,
            ) -> StorageResult<Vec<GraphNeighbor>> {
                self.inner.get_neighbor_page(request)
            }
        }
        impl<'a> StorageRead for ReadingView<'a> {
            fn get_embedding_status(
                &mut self,
                policy: &EmbeddingStatusPolicy,
            ) -> StorageResult<EmbeddingStatusSummary> {
                self.inner.get_embedding_status(policy)
            }

            fn get_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
                panic!("warm refresh loaded node metadata")
            }
            fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
                self.reads.set(self.reads.get() + 1);
                self.inner.get_source_snapshots()
            }
            fn get_next_embedding_work_by_id_and_max_attempts(
                &mut self,
                expected: &Sha256,
                max_attempts: u64,
                after: Option<&KnowledgeId>,
            ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
                self.inner.get_next_embedding_work_by_id_and_max_attempts(
                    expected,
                    max_attempts,
                    after,
                )
            }
            fn get_vector_dimensions(
                &mut self,
                expected: &Sha256,
            ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
                self.inner.get_vector_dimensions(expected)
            }
            fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
                self.inner.get_node_by_id(id)
            }
            fn get_edges_by_source_id(
                &mut self,
                source: Option<&KnowledgeId>,
            ) -> StorageResult<Vec<Edge>> {
                self.inner.get_edges_by_source_id(source)
            }
            fn get_file_by_path(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
                self.inner.get_file_by_path(path)
            }
            fn get_files(&mut self) -> StorageResult<Vec<FileRow>> {
                self.inner.get_files()
            }
            fn get_text_matches_by_expression(
                &mut self,
                expression: &str,
            ) -> StorageResult<Vec<TextMatch>> {
                self.inner.get_text_matches_by_expression(expression)
            }
            fn get_vector_by_search_hash(
                &mut self,
                expected: &Sha256,
                search_hash: &Sha256,
            ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
                self.inner.get_vector_by_search_hash(expected, search_hash)
            }
            fn get_nearest_vectors(
                &mut self,
                expected: &Sha256,
                query: &[f32],
                dimensions: usize,
            ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
                self.inner.get_nearest_vectors(expected, query, dimensions)
            }
            fn get_failure_by_search_hash(
                &mut self,
                expected: &Sha256,
                search_hash: &Sha256,
            ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
                self.inner.get_failure_by_search_hash(expected, search_hash)
            }
            fn get_query_error(&mut self) -> StorageResult<Option<QueryError>> {
                self.inner.get_query_error()
            }
            fn get_revision(&self) -> u64 {
                self.inner.get_revision()
            }
            fn get_config_identity(&self) -> Option<Sha256> {
                self.inner.get_config_identity()
            }
        }
        impl SourceProvenance for RetainingProvenance {
            fn validate_revisions(
                &self,
                revisions: &BTreeSet<TreeOid>,
            ) -> Result<(), SnapshotError> {
                TestProvenance.validate_revisions(revisions)
            }
            fn observe_sources(
                &self,
                request: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                TestProvenance.observe_sources(request)
            }
            fn compare_revision(
                &self,
                request: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                TestProvenance.compare_revision(request)
            }
        }
        impl BasisRetainer for RetainingProvenance {
            fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
                assert_eq!(
                    snapshots,
                    &BTreeSet::from([
                        TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
                    ])
                );
                Ok(())
            }
            fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
                assert_eq!(
                    snapshots,
                    &BTreeSet::from([
                        TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()
                    ])
                );
                Ok(())
            }
        }
    }

    mod reindex {
        use super::{INITIAL, build_service, get_matches};
        use crate::service::Refresher;
        use rusqlite::Connection;

        #[test]
        fn reindex_rebuilds_unchanged_fts_and_advances_once() {
            // Given: FTS is deliberately missing while file fingerprints are unchanged.
            let (dir, mut service) = build_service();
            service.refresh().unwrap();
            let db = Connection::open(dir.path().join("cache.db")).unwrap();
            db.execute("delete from text_index", []).unwrap();
            assert!(!service.refresh().unwrap().published);
            assert!(get_matches(&mut service, "AlphaToken").is_empty());
            // When.
            let rebuilt = service.reindex().unwrap();
            // Then: this must not be an incremental no-op.
            assert_eq!(
                (rebuilt.revision, rebuilt.published, rebuilt.files_updated),
                (2, true, 1)
            );
            assert_eq!(get_matches(&mut service, "AlphaToken"), ["note.md"]);
            assert_eq!(service.reindex().unwrap().revision, 3);
        }

        #[test]
        fn reindex_reparses_identical_valid_unmarked_and_invalid_bytes() {
            for bytes in [
                INITIAL,
                &b"ordinary markdown\n"[..],
                &b"<!-- letopis: invalid -->"[..],
            ] {
                // Given.
                let (_dir, mut service) = build_service();
                service.filesystem.bytes.replace(bytes.to_vec());
                service.filesystem.stat.update(|mut stat| {
                    stat.size = bytes.len() as u64;
                    stat
                });
                service.refresh().unwrap();
                let parses = service.markdown.0.get();
                service.filesystem.reset_counts();
                // When.
                let outcome = service.reindex().unwrap();
                // Then.
                assert_eq!(outcome.revision, 2);
                assert!(service.markdown.0.get() > parses);
                assert_eq!(service.filesystem.locks.get(), 1);
                assert_eq!(outcome.files_updated, 1);
            }
        }

        #[test]
        fn reindex_capture_failure_preserves_complete_publication() {
            // Given: fail the publication capture after detection and preflight.
            let (_dir, mut service) = build_service();
            service.refresh().unwrap();
            let before = service
                .storage
                .begin_read_tx()
                .unwrap()
                .get_files()
                .unwrap();
            service.filesystem.reset_counts();
            service.filesystem.fail_on_read.set(Some(3));
            // When: reindex fails during publication capture.
            assert!(service.reindex().is_err());
            // Then: the prior revision, file state and FTS entry remain intact.
            let mut read = service.storage.begin_read_tx().unwrap();
            assert_eq!(read.get_revision(), 1);
            assert_eq!(read.get_files().unwrap(), before);
            assert_eq!(
                read.get_text_matches_by_expression("AlphaToken").unwrap()[0]
                    .id
                    .as_str(),
                "note.md"
            );
        }
    }

    fn build_candidates() -> Candidates {
        let mut candidates = Candidates::new();
        candidates
            .expect_get_candidates()
            .returning(|| Ok(vec![RepoPath::parse("note.md").unwrap()]));
        candidates
    }
}
