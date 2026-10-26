use super::knowledge_link::is_current_knowledge;
use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigState, Embedder, EmbeddingConfig,
    FileSystem, KnowledgeMetadata, MarkdownCodec, NodeMetadata, ProvenanceRequest, QueryEmbedding,
    QueryEmbeddingRequest, Refresher, SearchFilters, SearchFlag, SearchLink, SearchRequest,
    Searcher, SnapshotError, SnapshotErrorKind, SourceProvenance, SourceRevisionComparison,
    SourceRevisionRequest, Storage, StorageRead, TextMatch, VectorMatch, VectorOutcome,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    num::NonZeroUsize,
};

mod ranking;
use ranking::{rank_candidates, validate_metrics};

/// Borrow disjoint read-only boundaries while storage owns the pinned read view.
struct SearchEligibility<'a, F, M, P> {
    filesystem: &'a F,
    markdown: &'a M,
    provenance: &'a P,
}

/// Independently owned boundaries; inference cannot upgrade the search read into a write.
#[derive(Debug)]
pub(crate) struct SearchDependencies<D, S, E, F, M, P> {
    /// Canonical document publication before pinning.
    pub(crate) refresher: D,
    /// Exclusive scoped search read connection.
    pub(crate) storage: S,
    /// Query executor owning independent configuration and diagnostic connections.
    pub(crate) embeddings: E,
    /// Confined live knowledge observations.
    pub(crate) filesystem: F,
    /// Shared whole-file knowledge codec.
    pub(crate) markdown: M,
    /// Read-only observations and comparisons of candidate source bindings.
    pub(crate) provenance: P,
}
/// Refreshes and ranks complete candidate channels from one scoped read.
#[derive(Debug)]
pub(crate) struct SearchService<D, S, E, F, M, P> {
    refresher: D,
    storage: S,
    embeddings: E,
    filesystem: F,
    markdown: M,
    provenance: P,
}
impl<D, S, E, F, M, P> SearchService<D, S, E, F, M, P> {
    /// Assemble search from owned boundaries without performing I/O.
    pub(crate) fn new(dependencies: SearchDependencies<D, S, E, F, M, P>) -> Self {
        Self {
            refresher: dependencies.refresher,
            storage: dependencies.storage,
            embeddings: dependencies.embeddings,
            filesystem: dependencies.filesystem,
            markdown: dependencies.markdown,
            provenance: dependencies.provenance,
        }
    }
}
impl<D: Refresher, S: Storage, E: Embedder, F: FileSystem, M: MarkdownCodec, P: SourceProvenance>
    Searcher for SearchService<D, S, E, F, M, P>
{
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>> {
        self.refresher.refresh().map_err(map_search_error)?;
        self.provenance
            .validate_revisions(&request.filters.source_revisions)
            .map_err(map_revision_error)?;
        let config = self
            .embeddings
            .read_configuration()
            .map_err(map_search_error)?;
        let mut read = self.storage.begin_read().map_err(map_search_error)?;
        let captured = capture_vector_config(&mut read, &config)?;
        let Some(expression) = build_fts_expression(&request.query) else {
            return Ok(Vec::new());
        };
        let vectors =
            find_vector_candidates(&mut read, &mut self.embeddings, captured, &request.query)?;
        let fts = read.match_text(&expression).map_err(map_search_error)?;
        validate_metrics(&fts, &vectors)?;
        let metadata = read_candidate_metadata(&mut read, &fts, &vectors)?;
        let eligibility = SearchEligibility {
            filesystem: &self.filesystem,
            markdown: &self.markdown,
            provenance: &self.provenance,
        };
        let mut links = eligibility.read_eligible_links(&mut read, &metadata, &request.filters)?;
        let eligible = links.keys().cloned().collect();
        let ranked = rank_candidates(&fts, &vectors, &eligible, request.limit)?;
        ranked
            .into_iter()
            .map(|id| links.remove(&id).ok_or_else(fail_invalid_search_data))
            .collect()
    }
}

impl<F: FileSystem, M: MarkdownCodec, P: SourceProvenance> SearchEligibility<'_, F, M, P> {
    fn read_eligible_links<R: StorageRead>(
        &self,
        read: &mut R,
        metadata: &BTreeMap<KnowledgeId, KnowledgeMetadata>,
        filters: &SearchFilters,
    ) -> CommandResult<BTreeMap<KnowledgeId, SearchLink>> {
        let mut links = BTreeMap::new();
        for (id, node) in metadata {
            let sources = read_source_bindings(read, id)?;
            if !self.matches_sources(node, &sources, filters)? {
                continue;
            }
            let mut link = build_link(node, read.get_revision(), self.filesystem, self.markdown)?;
            let observed = self
                .provenance
                .observe_sources(ProvenanceRequest {
                    source_snapshot: node.source_snapshot.as_ref(),
                    sources: &sources,
                })
                .map_err(map_search_error)?;
            link.flags.extend(observed.flags);
            if filters.matches_flags(&link.flags) {
                links.insert(id.clone(), link);
            }
        }
        Ok(links)
    }

    fn matches_sources(
        &self,
        node: &KnowledgeMetadata,
        sources: &BTreeSet<SourceUri>,
        filters: &SearchFilters,
    ) -> CommandResult<bool> {
        if filters.sources.is_empty() && filters.source_revisions.is_empty() {
            return Ok(true);
        }
        for source in sources {
            if !filters.sources.is_empty() && !filters.sources.contains(source) {
                continue;
            }
            if filters.source_revisions.is_empty() {
                return Ok(true);
            }
            if self.matches_revision(node, source, filters)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn matches_revision(
        &self,
        node: &KnowledgeMetadata,
        source: &SourceUri,
        filters: &SearchFilters,
    ) -> CommandResult<bool> {
        for revision in &filters.source_revisions {
            let comparison = self
                .provenance
                .compare_revision(SourceRevisionRequest {
                    source_snapshot: node.source_snapshot.as_ref(),
                    source,
                    revision,
                })
                .map_err(map_revision_error)?;
            if comparison == SourceRevisionComparison::Equal {
                return Ok(true);
            }
        }
        Ok(false)
    }
}

fn read_source_bindings<R: StorageRead>(
    read: &mut R,
    id: &KnowledgeId,
) -> CommandResult<BTreeSet<SourceUri>> {
    Ok(read
        .list_edges(Some(id))
        .map_err(map_search_error)?
        .into_iter()
        .filter_map(|edge| match edge.get_target() {
            EdgeTarget::Source(source) => Some(source.clone()),
            EdgeTarget::Knowledge(_) => None,
        })
        .collect())
}

fn capture_vector_config<'a, R: StorageRead>(
    read: &mut R,
    state: &'a ConfigState,
) -> CommandResult<Option<(&'a EmbeddingConfig, NonZeroUsize)>> {
    let ConfigState::Ready(config) = state else {
        return Ok(None);
    };
    if read.get_config_identity() != Some(config.get_identity()) {
        return Ok(None);
    }
    let VectorOutcome::Current(Some(dimensions)) = read
        .read_vector_dimensions(&config.get_identity())
        .map_err(map_search_error)?
    else {
        return Ok(None);
    };
    if config
        .get_dimensions()
        .is_some_and(|size| size != dimensions)
    {
        return Ok(None);
    }
    Ok(Some((config, dimensions)))
}

/// Quote deterministic Unicode alphanumeric segments; punctuation never becomes FTS grammar.
fn build_fts_expression(query: &str) -> Option<String> {
    let tokens: Vec<_> = query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect();
    if tokens.is_empty() {
        return None;
    }
    Some(tokens.join(" AND "))
}

fn find_vector_candidates<R: StorageRead, E: Embedder>(
    read: &mut R,
    embeddings: &mut E,
    captured: Option<(&EmbeddingConfig, NonZeroUsize)>,
    query: &str,
) -> CommandResult<Vec<VectorMatch>> {
    let Some((config, dimensions)) = captured else {
        return Ok(Vec::new());
    };
    let result = embeddings
        .embed_query_with_config(QueryEmbeddingRequest {
            text: query,
            config,
            dimensions: Some(dimensions),
        })
        .map_err(map_search_error)?;
    let QueryEmbedding::Available { identity, vector } = result else {
        return Ok(Vec::new());
    };
    if identity != config.get_identity()
        || vector.len() != dimensions.get()
        || vector.iter().any(|value| !value.is_finite())
    {
        return Err(fail_invalid_search_data());
    }
    match read
        .find_nearest_vectors(&identity, &vector, dimensions.get())
        .map_err(map_search_error)?
    {
        VectorOutcome::Current(matches) => Ok(matches),
        VectorOutcome::ConfigChanged => Ok(Vec::new()),
    }
}

fn read_candidate_metadata<R: StorageRead>(
    read: &mut R,
    fts: &[TextMatch],
    vectors: &[VectorMatch],
) -> CommandResult<BTreeMap<KnowledgeId, KnowledgeMetadata>> {
    let mut metadata = BTreeMap::new();
    for hit in fts {
        let (row_id, node) = read_knowledge(read, &hit.id)?;
        if row_id != hit.row_id {
            return Err(fail_invalid_search_data());
        }
        metadata.insert(hit.id.clone(), node);
    }
    for hit in vectors {
        let (_, node) = read_knowledge(read, &hit.id)?;
        if node.search_hash != hit.search_hash {
            return Err(fail_invalid_search_data());
        }
        metadata.insert(hit.id.clone(), node);
    }
    Ok(metadata)
}

fn read_knowledge<R: StorageRead>(
    read: &mut R,
    id: &KnowledgeId,
) -> CommandResult<(i64, KnowledgeMetadata)> {
    let stored = read
        .read_node(&EdgeTarget::Knowledge(id.clone()))
        .map_err(map_search_error)?
        .ok_or_else(fail_invalid_search_data)?;
    let NodeMetadata::Knowledge(node) = stored.metadata else {
        return Err(fail_invalid_search_data());
    };
    if node.id != *id || node.uri != *id {
        return Err(fail_invalid_search_data());
    }
    Ok((stored.row_id, node))
}

fn build_link<F: FileSystem, M: MarkdownCodec>(
    node: &KnowledgeMetadata,
    revision: u64,
    filesystem: &F,
    markdown: &M,
) -> CommandResult<SearchLink> {
    let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
        .map_err(map_search_error)?;
    let flags =
        if is_current_knowledge(node, &path, filesystem, markdown).map_err(map_search_error)? {
            BTreeSet::new()
        } else {
            BTreeSet::from([SearchFlag::Stale])
        };
    Ok(SearchLink {
        element_id: node.id.clone(),
        path,
        lines: node.lines,
        revision,
        flags,
    })
}

fn fail_invalid_search_data() -> CommandError {
    CommandError::new(CommandErrorKind::InternalError, "invalid search index data")
}
fn map_revision_error(cause: SnapshotError) -> CommandError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::NotFound => CommandErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        SnapshotErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot search source revisions", cause)
}
fn map_search_error(cause: impl Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot search knowledge",
        cause,
    )
}

#[cfg(test)]
mod tests;
