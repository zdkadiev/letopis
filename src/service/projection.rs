use super::{
    CacheWrite, CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileRow,
    FileSystem, KnowledgeMetadata, Markdown, NodeMetadata, ParsedFile, RefreshOutcome,
    SourceMetadata, StorageRead,
    document::{Observation, fail_changed},
};
use crate::domain::{
    Edge, EdgeTarget, KnowledgeId, LineRange, RepoPath, Sha256, SourceUri, derive_element_kind,
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
    pub(super) fn add_counts(&self, outcome: &mut RefreshOutcome) {
        match &self.file {
            FileChange::Upsert(row) => {
                outcome.files_updated += 1;
                outcome.invalid_files_updated += usize::from(!row.valid);
            }
            FileChange::Remove(_) => outcome.files_removed += 1,
        }
    }
}
pub(super) struct EffectiveGraph {
    knowledge: BTreeMap<KnowledgeId, KnowledgeMetadata>,
    edges: BTreeSet<Edge>,
    touched_sources: BTreeSet<SourceUri>,
}
impl EffectiveGraph {
    pub(super) fn read<R: StorageRead>(read: &mut R) -> DocumentResult<Self> {
        let knowledge = read
            .list_nodes()?
            .into_iter()
            .filter_map(|row| match row.metadata {
                NodeMetadata::Knowledge(node) => Some((node.id.clone(), node)),
                NodeMetadata::Source(_) => None,
            })
            .collect();
        Ok(Self {
            knowledge,
            edges: read.list_edges(None)?.into_iter().collect(),
            touched_sources: BTreeSet::new(),
        })
    }

    fn list_file_ids(&self, path: &RepoPath) -> Vec<KnowledgeId> {
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
            if let Some(&created_at) = references.get(uri) {
                paths
                    .entry(parse_source_path(uri)?)
                    .or_default()
                    .push((uri.clone(), created_at));
            } else if !self.knowledge.keys().any(|id| id.as_str() == uri.as_str()) {
                unit.delete_node(&EdgeTarget::Source(uri.clone()))?;
            }
        }
        for (path, sources) in paths {
            // One capture serves every affected address of this path and then is released.
            let captured = filesystem.read_file(&path)?;
            for (uri, created_at) in sources {
                let lines = parse_source_lines(&uri)?;
                let hash = captured
                    .as_ref()
                    .and_then(|file| select_source_bytes(&file.bytes, lines))
                    .map(Sha256::digest);
                unit.upsert_node(&NodeMetadata::Source(SourceMetadata {
                    uri,
                    hash,
                    created_at,
                    lines,
                }))?;
            }
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
    let ids = graph.list_file_ids(&path);
    PreparedProjection {
        base_revision,
        cleared: ids.clone(),
        removed: ids,
        edges: BTreeSet::new(),
        search: Vec::new(),
        file: FileChange::Remove(path),
    }
}

pub(super) fn prepare_file<F: FileSystem, M: Markdown>(
    filesystem: &F,
    markdown: &M,
    base_revision: u64,
    path: &RepoPath,
    observed: &Observation,
    previous: Option<&FileRow>,
    graph: &EffectiveGraph,
) -> DocumentResult<PreparedProjection> {
    let captured = filesystem.read_file(path)?.ok_or_else(fail_changed)?;
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
            Ok(parsed) => prepare_valid(&mut prepared, parsed, graph.list_file_ids(path)),
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
    prepared: &mut PreparedProjection,
    parsed: Option<ParsedFile>,
    old_ids: Vec<KnowledgeId>,
) {
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
    super::document::checkpoint("cleared");
    for text in &prepared.search {
        unit.upsert_node(&NodeMetadata::Knowledge(text.metadata.clone()))?;
    }
    for edge in &prepared.edges {
        unit.insert_edge(edge)?;
    }
    #[cfg(test)]
    super::document::checkpoint("projections");
    for text in prepared.search {
        unit.replace_text(&text.metadata.id, &text.search_text)?;
    }
    #[cfg(test)]
    super::document::checkpoint("fts");
    match prepared.file {
        FileChange::Upsert(row) => unit.upsert_file(&row)?,
        FileChange::Remove(path) => unit.delete_file(&path)?,
    }
    #[cfg(test)]
    super::document::checkpoint("fingerprints");
    Ok(())
}

fn parse_source_path(uri: &SourceUri) -> DocumentResult<RepoPath> {
    let path = uri.as_str().split('#').next().ok_or_else(fail_changed)?;
    RepoPath::parse(path).map_err(|_| fail_changed())
}

fn parse_source_lines(uri: &SourceUri) -> DocumentResult<Option<LineRange>> {
    let Some((_, fragment)) = uri.as_str().split_once("#L") else {
        return Ok(None);
    };
    let (start, end) = fragment.split_once("-L").ok_or_else(fail_changed)?;
    let start = start.parse().map_err(|_| fail_changed())?;
    let end = end.parse().map_err(|_| fail_changed())?;
    LineRange::new(start, end)
        .map(Some)
        .map_err(|_| fail_changed())
}

fn select_source_bytes(bytes: &[u8], lines: Option<LineRange>) -> Option<&[u8]> {
    let Some(lines) = lines else {
        return Some(bytes);
    };
    let mut offset = 0;
    let mut start = None;
    for (index, line) in bytes.split_inclusive(|byte| *byte == b'\n').enumerate() {
        let number = u64::try_from(index).ok()?.checked_add(1)?;
        if number == lines.get_start() {
            start = Some(offset);
        }
        offset += line.len();
        if number == lines.get_end() {
            return Some(&bytes[start?..offset]);
        }
    }
    None
}
