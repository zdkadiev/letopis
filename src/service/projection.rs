#[cfg(test)]
use super::document_refresher::reach_checkpoint;
use super::{
    CacheWrite, CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileRow,
    FileSystem, KnowledgeMetadata, MarkdownCodec, NodeMetadata, ParsedFile, RefreshOutcome,
    SourceMetadata, StorageRead,
    document_refresher::{Observation, fail_changed},
};
use crate::domain::{
    Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid, derive_element_kind,
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
pub(super) struct FileProjection<'a, F, M> {
    pub(super) filesystem: &'a F,
    pub(super) markdown: &'a M,
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
}
impl EffectiveGraph {
    pub(super) fn read<R: StorageRead>(read: &mut R) -> DocumentResult<Self> {
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
        })
    }

    /// Reobserve every declared source during a full rebuild, including invalid owners.
    pub(super) fn mark_all_sources(&mut self) {
        self.touched_sources.extend(
            self.edges
                .iter()
                .filter_map(|edge| match edge.get_target() {
                    EdgeTarget::Source(uri) => Some(uri.clone()),
                    EdgeTarget::Knowledge(_) => None,
                }),
        );
    }

    /// Return every declared root in the last effective knowledge projection.
    pub(super) fn get_source_snapshots(&self, excluded: &BTreeSet<RepoPath>) -> BTreeSet<TreeOid> {
        let paths: BTreeSet<_> = excluded.iter().map(RepoPath::as_str).collect();
        self.knowledge
            .values()
            .filter(|node| !paths.contains(node.id.as_str().split('#').next().unwrap_or_default()))
            .filter_map(|node| node.source_snapshot.clone())
            .collect()
    }

    fn get_file_ids(&self, path: &RepoPath) -> Vec<KnowledgeId> {
        self.knowledge
            .keys()
            .filter(|id| id.as_str().split('#').next() == Some(path.as_str()))
            .cloned()
            .collect()
    }

    pub(super) fn apply(&mut self, prepared: &PreparedProjection) {
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

    pub(super) fn reconcile_sources<F: FileSystem, W: CacheWrite>(
        &self,
        filesystem: &F,
        unit: &mut W,
    ) -> DocumentResult<()> {
        let references = self.collect_source_references()?;
        let mut paths: BTreeMap<RepoPath, Vec<(SourceUri, u64)>> = BTreeMap::new();
        for uri in &self.touched_sources {
            if !references.contains_key(uri)
                && !self.knowledge.keys().any(|id| id.as_str() == uri.as_str())
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
            publish_source_group(filesystem, unit, &path, sources)?;
        }
        Ok(())
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

pub(super) fn prepare_file<F: FileSystem, M: MarkdownCodec>(
    request: &FileProjection<'_, F, M>,
) -> DocumentResult<PreparedProjection> {
    let FileProjection {
        filesystem,
        markdown,
        base_revision,
        path,
        observed,
        previous,
        graph,
    } = *request;
    let captured = filesystem.get_file(path)?.ok_or_else(fail_changed)?;
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
        match markdown.parse_file(path, &captured.bytes) {
            Ok(parsed) => prepared = prepare_valid(prepared, parsed, graph.get_file_ids(path)),
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

pub(super) fn publish<W: CacheWrite>(
    unit: &mut W,
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
    for edge in &prepared.edges {
        unit.insert_edge(edge)?;
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

fn publish_source_group<F: FileSystem, W: CacheWrite>(
    filesystem: &F,
    unit: &mut W,
    path: &RepoPath,
    sources: Vec<(SourceUri, u64)>,
) -> DocumentResult<()> {
    // One capture serves every affected address of this path and then is released.
    let captured = filesystem.get_file(path)?;
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
