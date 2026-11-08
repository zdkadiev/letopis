use super::utils::{
    CandidateObservation, CandidateSelection, DOCUMENTS_CHANGED_MESSAGE, DiscoveryObservation,
    is_selected_path, map_document_transaction_error_to_document_error,
    map_file_result_to_document_result,
};
use super::{
    CacheDatabase, CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileError,
    FileErrorKind, FileReader, FileRow, FileStat, GitWorktreeListing, KnowledgeMetadata,
    MarkdownCodec, NodeMetadata, PROVENANCE_BATCH_SIZE, ParsedFile, ProvenanceRequest,
    RefreshOutcome, Refresher, SnapshotRefsUpdater, SourceMetadata, SourceProvenance, StorageRead,
};
use super::{Clock, FileLocker};
use crate::domain::{
    Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid, derive_element_kind,
};
use crate::utils::INDEX_LOCK_PATH;
use std::path::Path;
use std::{fmt, rc::Rc};

use std::collections::{BTreeMap, BTreeSet};

const RACY_WINDOW_NS: i64 = 1_000_000_000;

/// Refreshes and retains the effective document projection from independently owned collaborators.
pub(crate) struct DocumentRefreshService<G, F, M, P, C> {
    /// Lists the current corpus candidates.
    pub(crate) git: G,
    /// Captures documents and owns the index exclusion lock.
    pub(crate) filesystem: F,
    pub(crate) clock: Box<dyn Clock>,
    /// Parses canonical document metadata and source bindings.
    pub(crate) markdown: M,
    /// Reads the current projection and publishes cache changes atomically.
    pub(crate) cache: Rc<C>,
    /// Observes source bindings and retains author-declared roots.
    pub(crate) provenance: P,
}

/// Metadata captured before the SQL write, never document or search text.
#[derive(Debug)]
struct BasisPreflight {
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
struct BasisPreparation<'a> {
    graph: &'a EffectiveGraph,
    updated: &'a BTreeMap<RepoPath, Observation>,
    removed: &'a BTreeMap<RepoPath, bool>,
    previous: &'a BTreeMap<RepoPath, FileRow>,
    mode: PublicationMode,
}

/// Text is owned only by one prepared file; metadata supplies its id, uri and search hash.
struct PreparedText {
    metadata: KnowledgeMetadata,
    search_text: String,
}

struct PreparedProjection {
    base_revision: u64,
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

/// Inputs for projecting one observed file against the previously published graph.
struct FileProjection<'a> {
    base_revision: u64,
    path: &'a RepoPath,
    observed: &'a Observation,
    previous: Option<&'a FileRow>,
    graph: &'a EffectiveGraph,
}

struct EffectiveGraph {
    knowledge: BTreeMap<KnowledgeId, KnowledgeMetadata>,
    edges: BTreeSet<Edge>,
    touched_sources: BTreeSet<SourceUri>,
    changed_owners: BTreeSet<KnowledgeId>,
}

#[derive(Clone, Copy)]
enum PublicationState {
    Unchanged,
    Published,
}

impl<G, F, M, P, C> fmt::Debug for DocumentRefreshService<G, F, M, P, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("DocumentRefreshService")
            .finish_non_exhaustive()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PublicationMode {
    Incremental,
    Full,
}

impl PublicationMode {
    fn get_previous_row(self, previous: Option<&FileRow>) -> Option<&FileRow> {
        previous.filter(|_| self == Self::Incremental)
    }
}

#[derive(Clone, Copy)]
enum SnapshotVerification {
    Root,
    Closure,
}

/// Borrows publication collaborators independently of the storage transaction.
struct DocumentPublication<'a, F, M, P, C> {
    cache: &'a C,
    filesystem: &'a F,
    markdown: &'a M,
    provenance: &'a P,
}

struct FileState {
    revision: u64,
    source_snapshots: BTreeSet<TreeOid>,
    files: BTreeMap<RepoPath, FileRow>,
}

struct Observation {
    stat: FileStat,
    hash: Sha256,
}

struct Delta {
    discovery: DiscoveryObservation,
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
    G: GitWorktreeListing,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: SourceProvenance + SnapshotRefsUpdater,
    C: CacheDatabase,
> Refresher for DocumentRefreshService<G, F, M, P, C>
{
    fn refresh(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        let preliminary = self.get_file_state()?;
        if preliminary.revision != 0 {
            let delta = self.detect_changes(&preliminary, PublicationMode::Incremental, paths)?;
            if delta.is_empty() && preliminary.source_snapshots.is_empty() {
                #[cfg(test)]
                reach_checkpoint("noop_policy");
                self.verify_discovery_unchanged(&delta.discovery)?;
                return Ok(build_outcome(
                    preliminary.revision,
                    PublicationState::Unchanged,
                ));
            }
        }
        #[cfg(test)]
        reach_checkpoint("detected");
        drop(preliminary);
        let _lock = self
            .filesystem
            .lock_acquire_by_path(Path::new(INDEX_LOCK_PATH))
            .map_err(map_file_error_to_document_error)?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(
            PublicationMode::Incremental,
            SnapshotVerification::Root,
            paths,
        )
    }

    fn refresh_verified(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        let _lock = self
            .filesystem
            .lock_acquire_by_path(Path::new(INDEX_LOCK_PATH))
            .map_err(map_file_error_to_document_error)?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(
            PublicationMode::Incremental,
            SnapshotVerification::Closure,
            paths,
        )
    }

    fn reindex(&mut self, paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        let _lock = self
            .filesystem
            .lock_acquire_by_path(Path::new(INDEX_LOCK_PATH))
            .map_err(map_file_error_to_document_error)?;
        #[cfg(test)]
        reach_checkpoint("locked");
        self.publish_current(PublicationMode::Full, SnapshotVerification::Closure, paths)
    }
}

impl<
    G: GitWorktreeListing,
    F: FileReader + FileLocker,
    M: MarkdownCodec,
    P: SourceProvenance + SnapshotRefsUpdater,
    C: CacheDatabase,
> DocumentRefreshService<G, F, M, P, C>
{
    fn publish_current(
        &mut self,
        requested: PublicationMode,
        verification: SnapshotVerification,
        paths: &[RepoPath],
    ) -> DocumentResult<RefreshOutcome> {
        let state = self.get_file_state()?;
        let mode = if state.revision == 0 {
            PublicationMode::Full
        } else {
            requested
        };
        let delta = self.detect_changes(&state, mode, paths)?;
        #[cfg(test)]
        reach_checkpoint("redetected");
        if delta.is_empty() && mode == PublicationMode::Incremental {
            DocumentPublication {
                cache: self.cache.as_ref(),
                filesystem: &self.filesystem,
                markdown: &self.markdown,
                provenance: &self.provenance,
            }
            .snapshot_sync_by_oids(verification, &state.source_snapshots)?;
            #[cfg(test)]
            reach_checkpoint("locked_noop_policy");
            self.verify_discovery_unchanged(&delta.discovery)?;
            return Ok(build_outcome(state.revision, PublicationState::Unchanged));
        }
        self.publish_changes(&state, delta, mode, verification, paths)
    }

    fn get_file_state(&self) -> DocumentResult<FileState> {
        self.cache
            .with_read_tx(|| {
                Ok(FileState {
                    revision: self.cache.project_get_by_id()?,
                    source_snapshots: self.cache.nodes_get_by_source_snapshot()?,
                    files: self
                        .cache
                        .files_get()?
                        .into_iter()
                        .map(|row| (row.path.clone(), row))
                        .collect(),
                })
            })
            .map_err(map_document_transaction_error_to_document_error)
    }

    fn detect_changes(
        &self,
        state: &FileState,
        mode: PublicationMode,
        paths: &[RepoPath],
    ) -> DocumentResult<Delta> {
        let observation = self.get_candidate_observation_by_paths(paths)?;
        let candidates: BTreeSet<_> = observation.paths.into_iter().collect();
        let mut delta = Delta {
            discovery: observation.discovery,
            updated: BTreeMap::new(),
            removed: BTreeMap::new(),
        };
        for path in state.files.keys().filter(|path| {
            is_selected_path(path.as_str().as_bytes(), paths) && !candidates.contains(*path)
        }) {
            delta.removed.insert(path.clone(), false);
        }
        for path in candidates {
            let previous = state.files.get(&path);
            let Some(stat) = map_file_result_to_document_result(
                self.filesystem
                    .file_get_stat_by_path(Path::new(path.as_str())),
            )?
            else {
                if previous.is_some() {
                    delta.removed.insert(path, true);
                }
                continue;
            };
            if let Some(observation) =
                self.detect_file(&path, stat, mode.get_previous_row(previous))?
            {
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
        let same_stat = previous.is_some_and(|row| is_stat_match(row, stat));
        if same_stat
            && previous.is_some_and(|row| !is_racy(row, self.clock.clock_get_time_ns().ok()))
        {
            return Ok(None);
        }
        let captured = map_file_result_to_document_result(
            self.filesystem.file_get_by_path(Path::new(path.as_str())),
        )?
        .ok_or_else(build_documents_changed_error)?;
        if captured.stat != stat {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
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
        paths: &[RepoPath],
    ) -> DocumentResult<RefreshOutcome> {
        let mut graph = self.prepare_publication(state, &delta, mode, verification, paths)?;
        let publication = DocumentPublication {
            cache: self.cache.as_ref(),
            filesystem: &self.filesystem,
            markdown: &self.markdown,
            provenance: &self.provenance,
        };
        let outcome = self
            .cache
            .with_write_tx(|| -> DocumentResult<RefreshOutcome> {
                #[cfg(test)]
                reach_checkpoint("begun");
                let mut outcome = build_outcome(state.revision, PublicationState::Published);
                let excluded: BTreeSet<_> = delta
                    .removed
                    .iter()
                    .filter(|(_, is_missing)| !**is_missing)
                    .map(|(path, _)| path.clone())
                    .collect();
                for (path, is_missing) in delta.removed {
                    if is_missing {
                        self.verify_removed_candidate(&path)?;
                    }
                    let prepared = prepare_removal(state.revision, path, &graph);
                    graph.apply(&prepared);
                    outcome = publication.publish_projection(prepared, outcome)?;
                }
                for (path, observation) in delta.updated {
                    let prepared = publication.prepare_file(&FileProjection {
                        base_revision: state.revision,
                        path: &path,
                        observed: &observation,
                        previous: mode.get_previous_row(state.files.get(&path)),
                        graph: &graph,
                    })?;
                    graph.apply(&prepared);
                    outcome = publication.publish_projection(prepared, outcome)?;
                }
                publication.reconcile_sources(&graph)?;
                publication.publish_edges(&graph)?;
                self.verify_excluded_candidates(&excluded, paths)?;
                #[cfg(test)]
                reach_checkpoint("before_commit");
                self.verify_discovery_unchanged(&delta.discovery)?;
                #[cfg(test)]
                tests::process::fail_late(self.cache.as_ref())?;
                outcome.revision = self.cache.project_get_revision()?;
                Ok(outcome)
            })
            .map_err(map_document_transaction_error_to_document_error)?;
        #[cfg(test)]
        reach_checkpoint("after_commit");
        Ok(outcome)
    }

    fn verify_removed_candidate(&self, path: &RepoPath) -> DocumentResult<()> {
        let stat = map_file_result_to_document_result(
            self.filesystem
                .file_get_stat_by_path(Path::new(path.as_str())),
        )?;
        if stat.is_some() {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
        }
        Ok(())
    }

    fn prepare_publication(
        &mut self,
        state: &FileState,
        delta: &Delta,
        mode: PublicationMode,
        verification: SnapshotVerification,
        paths: &[RepoPath],
    ) -> DocumentResult<EffectiveGraph> {
        // Keep only metadata after releasing this pinned read, before BEGIN IMMEDIATE.
        let mut graph = self
            .cache
            .with_read_tx(|| -> DocumentResult<EffectiveGraph> {
                if self.cache.project_get_by_id()? != state.revision {
                    return Err(DocumentError::new(
                        DocumentErrorKind::InternalError,
                        DOCUMENTS_CHANGED_MESSAGE,
                    ));
                }
                EffectiveGraph::from_storage(self.cache.as_ref())
            })
            .map_err(map_document_transaction_error_to_document_error)?;
        if mode == PublicationMode::Full {
            graph.mark_sources_by_paths(paths);
        }
        let publication = DocumentPublication {
            cache: self.cache.as_ref(),
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
        publication.verify_basis(&basis, verification)?;
        Ok(graph)
    }

    /// Rejects candidates that reappear after their removal was prepared.
    ///
    /// # Arguments
    ///
    /// * `excluded` — Paths whose removal was prepared and that must stay absent.
    /// * `paths` — Refresh selection used to observe the candidates again.
    fn verify_excluded_candidates(
        &self,
        excluded: &BTreeSet<RepoPath>,
        paths: &[RepoPath],
    ) -> DocumentResult<()> {
        if !excluded.is_empty()
            && self
                .get_candidate_observation_by_paths(paths)?
                .paths
                .iter()
                .any(|path| excluded.contains(path))
        {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
        }
        Ok(())
    }
}

impl<G: GitWorktreeListing, F: FileReader, M, P, C> DocumentRefreshService<G, F, M, P, C> {
    fn get_candidate_observation_by_paths(
        &self,
        paths: &[RepoPath],
    ) -> DocumentResult<CandidateObservation> {
        CandidateSelection {
            git: &self.git,
            filesystem: &self.filesystem,
        }
        .get_candidate_observation_by_paths(paths)
    }

    /// Rejects a discovery policy edit made after the refresh observed its inputs.
    ///
    /// # Arguments
    ///
    /// * `observation` — Policy bytes captured when the refresh began.
    ///
    /// # Errors
    /// Reports unreadable policy files or an observation that no longer matches.
    fn verify_discovery_unchanged(&self, observation: &DiscoveryObservation) -> DocumentResult<()> {
        CandidateSelection {
            git: &self.git,
            filesystem: &self.filesystem,
        }
        .verify_discovery_unchanged(observation)
    }
}

impl<F: FileReader + FileLocker, M: MarkdownCodec, P: SourceProvenance + SnapshotRefsUpdater, C>
    DocumentPublication<'_, F, M, P, C>
{
    /// Prepares metadata for changed files only.
    ///
    /// Each parsed file is released before the next is read.
    ///
    /// # Arguments
    ///
    /// * `request` — Borrowed graph, observed updates and removals, previous rows and
    ///   publication mode.
    ///
    /// # Errors
    /// Reports capture failures and observed stat or hash changes; invalid Markdown keeps its
    /// previous metadata.
    fn prepare_basis(&self, request: BasisPreparation<'_>) -> DocumentResult<BasisPreflight> {
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
                .get_previous_row(previous.get(path))
                .is_some_and(|previous| previous.hash == observed.hash)
            {
                continue;
            }
            let captured = map_file_result_to_document_result(
                self.filesystem.file_get_by_path(Path::new(path.as_str())),
            )?
            .ok_or_else(build_documents_changed_error)?;
            if captured.stat != observed.stat || Sha256::digest(&captured.bytes) != observed.hash {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    DOCUMENTS_CHANGED_MESSAGE,
                ));
            }
            let Ok(parsed) = self.markdown.file_parse(path, &captured.bytes) else {
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
            .extend(graph.get_source_snapshots_by_excluded_paths(&replaced));
        Ok(preflight)
    }

    /// Observes source bindings before SQL publication.
    ///
    /// The caller holds index.lock. Diagnostic findings do not reject valid Markdown.
    ///
    /// # Arguments
    ///
    /// * `preflight` — Changed-file metadata prepared before the SQL write.
    /// * `verification` — Whether declared roots are retained or their closure is verified.
    ///
    /// # Errors
    /// Reports unsafe source reads, Git corruption and snapshot failures before SQL begins.
    fn verify_basis(
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
        self.snapshot_sync_by_oids(verification, &preflight.snapshots)
    }

    /// Retains or verifies snapshot roots according to the publication mode.
    ///
    /// # Arguments
    ///
    /// * `verification` — Selects retention of declared roots or closure verification.
    /// * `snapshots` — Source snapshot roots affected by this publication.
    ///
    /// # Errors
    /// Reports missing or corrupt snapshot objects and failures to retain them.
    fn snapshot_sync_by_oids(
        &self,
        verification: SnapshotVerification,
        snapshots: &BTreeSet<TreeOid>,
    ) -> DocumentResult<()> {
        match verification {
            SnapshotVerification::Root => self.provenance.snapshot_refs_update_by_oids(snapshots),
            SnapshotVerification::Closure => self.provenance.snapshot_sync_by_oids(snapshots),
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

impl<F: FileReader + FileLocker, M: MarkdownCodec, P, C: CacheDatabase>
    DocumentPublication<'_, F, M, P, C>
{
    /// Prepares one changed file against its previous projection.
    ///
    /// # Arguments
    ///
    /// * `request` — Inputs for one changed file: base revision, path, observation, previous
    ///   row and graph.
    ///
    /// # Errors
    /// Reports capture failures or changes since detection; invalid Markdown keeps its previous
    /// nodes.
    fn prepare_file(&self, request: &FileProjection<'_>) -> DocumentResult<PreparedProjection> {
        let FileProjection {
            base_revision,
            path,
            observed,
            previous,
            graph,
        } = *request;
        let captured = map_file_result_to_document_result(
            self.filesystem.file_get_by_path(Path::new(path.as_str())),
        )?
        .ok_or_else(build_documents_changed_error)?;
        if captured.stat != observed.stat || Sha256::digest(&captured.bytes) != observed.hash {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
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
            match self.markdown.file_parse(path, &captured.bytes) {
                Ok(parsed) => {
                    prepared = prepare_valid(prepared, parsed, graph.get_file_ids_by_path(path));
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

    /// Publishes each touched source.
    ///
    /// Sources that are no longer referenced are deleted. One current capture serves local paths
    /// with several sources.
    ///
    /// # Arguments
    ///
    /// * `graph` — Effective graph whose touched sources are published.
    ///
    /// # Errors
    /// Reports conflicting source addresses, unsafe captures and storage write failures.
    fn reconcile_sources(&self, graph: &EffectiveGraph) -> DocumentResult<()> {
        let references = graph.collect_source_references()?;
        let mut paths: BTreeMap<RepoPath, Vec<(SourceUri, u64)>> = BTreeMap::new();
        for uri in &graph.touched_sources {
            if !references.contains_key(uri)
                && !graph.knowledge.keys().any(|id| id.as_str() == uri.as_str())
            {
                self.cache.delete_node(&EdgeTarget::Source(uri.clone()))?;
            }
            let Some(&created_at) = references.get(uri) else {
                continue;
            };
            let Some(local) = uri.get_local() else {
                self.cache
                    .nodes_upsert_by_id(&NodeMetadata::Source(SourceMetadata {
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
            self.publish_source_group(&path, sources)?;
        }
        Ok(())
    }

    fn publish_source_group(
        &self,
        path: &RepoPath,
        sources: Vec<(SourceUri, u64)>,
    ) -> DocumentResult<()> {
        // One capture serves every affected address of this path and then is released.
        let captured = map_file_result_to_document_result(
            self.filesystem.file_get_by_path(Path::new(path.as_str())),
        )?;
        for (uri, created_at) in sources {
            let local = uri.get_local().ok_or_else(build_documents_changed_error)?;
            let lines = local.get_lines();
            let hash = captured
                .as_ref()
                .and_then(|file| local.select_bytes(&file.bytes))
                .map(Sha256::digest);
            self.cache
                .nodes_upsert_by_id(&NodeMetadata::Source(SourceMetadata {
                    uri,
                    hash,
                    created_at,
                    lines,
                }))?;
        }
        Ok(())
    }
}

impl<F, M, P, C: CacheDatabase> DocumentPublication<'_, F, M, P, C> {
    /// Writes final edges for changed owners after all nodes and sources exist.
    ///
    /// Unchanged owners keep their edges for storage to reconcile at commit.
    ///
    /// # Arguments
    ///
    /// * `graph` — Effective graph with the final edges and the owners changed by this
    ///   publication.
    ///
    /// # Errors
    /// Reports write failures, leaving the caller's transaction unpublished.
    fn publish_edges(&self, graph: &EffectiveGraph) -> DocumentResult<()> {
        for edge in &graph.edges {
            if graph.changed_owners.contains(edge.get_source()) {
                self.cache
                    .edges_save_by_source_id_and_target_id_and_type(edge)?;
            }
        }
        Ok(())
    }

    /// Writes a prepared projection to the active cache write scope.
    ///
    /// # Arguments
    ///
    /// * `prepared` — Prepared projection of one file.
    /// * `outcome` — Refresh outcome that this projection extends.
    ///
    /// # Errors
    /// Rejects a changed base revision and reports storage write failures.
    fn publish_projection(
        &self,
        prepared: PreparedProjection,
        outcome: RefreshOutcome,
    ) -> DocumentResult<RefreshOutcome> {
        if prepared.base_revision != outcome.revision {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                DOCUMENTS_CHANGED_MESSAGE,
            ));
        }
        let outcome = prepared.add_counts(outcome);
        for id in &prepared.cleared {
            self.cache.edges_delete_by_source_id(id)?;
        }
        for id in prepared.removed {
            self.cache.delete_node(&EdgeTarget::Knowledge(id))?;
        }
        #[cfg(test)]
        reach_checkpoint("cleared");
        for text in &prepared.search {
            self.cache
                .nodes_upsert_by_id(&NodeMetadata::Knowledge(text.metadata.clone()))?;
        }
        #[cfg(test)]
        reach_checkpoint("projections");
        for text in prepared.search {
            self.cache
                .replace_text(&text.metadata.id, &text.search_text)?;
        }
        #[cfg(test)]
        reach_checkpoint("fts");
        match prepared.file {
            FileChange::Upsert(row) => self.cache.files_upsert_by_path(&row)?,
            FileChange::Remove(path) => self.cache.files_delete_by_path(&path)?,
        }
        #[cfg(test)]
        reach_checkpoint("fingerprints");
        Ok(outcome)
    }
}

impl PreparedProjection {
    /// Returns `outcome` with this file change counted.
    ///
    /// # Arguments
    ///
    /// * `outcome` — Refresh outcome extended with this file change count.
    fn add_counts(&self, mut outcome: RefreshOutcome) -> RefreshOutcome {
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

impl EffectiveGraph {
    fn from_storage(cache: &dyn StorageRead) -> DocumentResult<Self> {
        let knowledge = cache
            .nodes_get()?
            .into_iter()
            .filter_map(|row| match row.metadata {
                NodeMetadata::Knowledge(node) => Some((node.id.clone(), node)),
                NodeMetadata::Source(_) => None,
            })
            .collect();
        Ok(Self {
            knowledge,
            edges: cache.edges_get_by_source_id(None)?.into_iter().collect(),
            touched_sources: BTreeSet::new(),
            changed_owners: BTreeSet::new(),
        })
    }

    /// Marks declared sources for observation during a full rebuild.
    ///
    /// Sources declared by invalid files are included.
    ///
    /// # Arguments
    ///
    /// * `paths` — Refresh selection whose declared sources are marked.
    fn mark_sources_by_paths(&mut self, paths: &[RepoPath]) {
        self.touched_sources
            .extend(self.edges.iter().filter_map(|edge| {
                match edge.get_target() {
                    EdgeTarget::Source(uri)
                        if is_selected_path(
                            edge.get_source()
                                .as_str()
                                .split('#')
                                .next()
                                .unwrap_or_default()
                                .as_bytes(),
                            paths,
                        ) =>
                    {
                        Some(uri.clone())
                    }
                    EdgeTarget::Source(_) | EdgeTarget::Knowledge(_) => None,
                }
            }));
    }

    /// Returns every declared snapshot root in the current knowledge graph.
    ///
    /// # Arguments
    ///
    /// * `excluded` — Paths whose knowledge must not contribute snapshot roots.
    fn get_source_snapshots_by_excluded_paths(
        &self,
        excluded: &BTreeSet<RepoPath>,
    ) -> BTreeSet<TreeOid> {
        let paths: BTreeSet<_> = excluded.iter().map(RepoPath::as_str).collect();
        self.knowledge
            .values()
            .filter(|node| !paths.contains(node.id.as_str().split('#').next().unwrap_or_default()))
            .filter_map(|node| node.source_snapshot.clone())
            .collect()
    }

    fn apply(&mut self, prepared: &PreparedProjection) {
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

    fn get_file_ids_by_path(&self, path: &RepoPath) -> Vec<KnowledgeId> {
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
                .ok_or_else(build_documents_changed_error)?;
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

fn is_stat_match(previous: &FileRow, stat: FileStat) -> bool {
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

fn build_outcome(revision: u64, publication: PublicationState) -> RefreshOutcome {
    RefreshOutcome {
        revision,
        is_published: matches!(publication, PublicationState::Published),
        files_updated: 0,
        files_removed: 0,
        invalid_files_updated: 0,
    }
}

fn prepare_removal(
    base_revision: u64,
    path: RepoPath,
    graph: &EffectiveGraph,
) -> PreparedProjection {
    let ids = graph.get_file_ids_by_path(&path);
    PreparedProjection {
        base_revision,
        cleared: ids.clone(),
        removed: ids,
        edges: BTreeSet::new(),
        search: Vec::new(),
        file: FileChange::Remove(path),
    }
}

fn build_file_row(path: &RepoPath, captured: &CapturedFile, content_hash: Sha256) -> FileRow {
    FileRow {
        path: path.clone(),
        size: captured.stat.size,
        mtime_ns: captured.stat.mtime_ns,
        inode: captured.stat.inode,
        hash: content_hash,
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

#[cfg(test)]
pub(super) use tests::process::reach_checkpoint;

fn map_file_error_to_document_error(cause: FileError) -> DocumentError {
    let (kind, message) = match cause.get_kind() {
        FileErrorKind::Aliased => (
            DocumentErrorKind::InvalidRequest,
            "private coordination lock required",
        ),
        FileErrorKind::NonRegular => (
            DocumentErrorKind::InvalidRequest,
            "regular document file required",
        ),
        _ => (
            DocumentErrorKind::InternalError,
            "cannot acquire coordination lock",
        ),
    };
    DocumentError::new_with_cause(kind, message, cause)
}

/// Builds the error reported when observed documents changed during refresh.
fn build_documents_changed_error() -> DocumentError {
    DocumentError::new(DocumentErrorKind::InternalError, DOCUMENTS_CHANGED_MESSAGE)
}

#[cfg(test)]
#[path = "document_refresher_test.rs"]
mod tests;
