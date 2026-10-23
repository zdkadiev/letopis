use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigState, Documents, EmbeddingConfig,
    Embeddings, FileSystem, KnowledgeMetadata, Markdown, NodeMetadata, QueryEmbedding,
    QueryEmbeddingRequest, SearchFlag, SearchLink, SearchRequest, Searches, Storage, StorageRead,
    TextMatch, VectorMatch, VectorOutcome,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath};
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    num::NonZeroUsize,
};

/// Independently owned boundaries; inference cannot upgrade the search read into a write.
pub(crate) struct SearchDependencies<D, S, E, F, M> {
    /// Canonical document publication before pinning.
    pub(crate) documents: D,
    /// Exclusive scoped search read connection.
    pub(crate) storage: S,
    /// Query executor owning independent configuration and diagnostic connections.
    pub(crate) embeddings: E,
    /// Confined live knowledge observations.
    pub(crate) filesystem: F,
    /// Shared whole-file knowledge codec.
    pub(crate) markdown: M,
}
/// Refreshes and ranks complete candidate channels from one scoped read.
pub(crate) struct SearchService<D, S, E, F, M> {
    documents: D,
    storage: S,
    embeddings: E,
    filesystem: F,
    markdown: M,
}
impl<D, S, E, F, M> SearchService<D, S, E, F, M> {
    /// Assemble search from owned boundaries without performing I/O.
    pub(crate) fn new(dependencies: SearchDependencies<D, S, E, F, M>) -> Self {
        Self {
            documents: dependencies.documents,
            storage: dependencies.storage,
            embeddings: dependencies.embeddings,
            filesystem: dependencies.filesystem,
            markdown: dependencies.markdown,
        }
    }
}
impl<D: Documents, S: Storage, E: Embeddings, F: FileSystem, M: Markdown> Searches
    for SearchService<D, S, E, F, M>
{
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>> {
        self.documents.refresh().map_err(map_search_error)?;
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
        let metadata = read_candidate_metadata(&mut read, &fts, &vectors)?;
        let ranked = rank_candidates(&fts, &vectors, request.limit)?;
        ranked
            .into_iter()
            .map(|id| {
                let node = metadata.get(&id).ok_or_else(invalid_search_data)?;
                build_link(node, read.get_revision(), &self.filesystem, &self.markdown)
            })
            .collect()
    }
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

fn find_vector_candidates<R: StorageRead, E: Embeddings>(
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
        return Err(invalid_search_data());
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
            return Err(invalid_search_data());
        }
        metadata.insert(hit.id.clone(), node);
    }
    for hit in vectors {
        let (_, node) = read_knowledge(read, &hit.id)?;
        if node.search_hash != hit.search_hash {
            return Err(invalid_search_data());
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
        .ok_or_else(invalid_search_data)?;
    let NodeMetadata::Knowledge(node) = stored.metadata else {
        return Err(invalid_search_data());
    };
    if node.id != *id || node.uri != *id {
        return Err(invalid_search_data());
    }
    Ok((stored.row_id, node))
}

fn rank_candidates(
    fts: &[TextMatch],
    vectors: &[VectorMatch],
    limit: NonZeroUsize,
) -> CommandResult<Vec<KnowledgeId>> {
    let mut scores = BTreeMap::new();
    add_channel_ranks(
        fts.iter().map(|hit| (&hit.id, hit.bm25)).collect(),
        &mut scores,
    )?;
    add_channel_ranks(
        vectors.iter().map(|hit| (&hit.id, hit.distance)).collect(),
        &mut scores,
    )?;
    let mut ranked: Vec<_> = scores.into_iter().collect();
    ranked.sort_by(|(left_id, left), (right_id, right)| {
        right.total_cmp(left).then_with(|| left_id.cmp(right_id))
    });
    Ok(ranked
        .into_iter()
        .take(limit.get())
        .map(|(id, _)| id)
        .collect())
}

fn add_channel_ranks(
    mut channel: Vec<(&KnowledgeId, f64)>,
    scores: &mut BTreeMap<KnowledgeId, f64>,
) -> CommandResult<()> {
    if channel.iter().any(|(_, metric)| !metric.is_finite()) {
        return Err(invalid_search_data());
    }
    channel.sort_by(|(left_id, left), (right_id, right)| {
        // Signed zeros are equal metrics and therefore use the same identity tie-break.
        if left == right {
            left_id.cmp(right_id)
        } else {
            left.total_cmp(right)
        }
    });
    let mut seen = BTreeSet::new();
    for (id, _) in channel {
        if !seen.insert(id) {
            continue;
        }
        let rank = NonZeroUsize::new(seen.len()).ok_or_else(invalid_search_data)?;
        let score = scores.entry(id.clone()).or_insert(0.0);
        *score += calculate_rank_weight(rank)?;
        if !score.is_finite() {
            return Err(invalid_search_data());
        }
    }
    Ok(())
}

/// Restrict ranks to an exactly representable integer before computing their reciprocal.
fn calculate_rank_weight(rank: NonZeroUsize) -> CommandResult<f64> {
    let rank = u32::try_from(rank.get()).map_err(map_search_error)?;
    Ok(1.0 / f64::from(rank))
}

fn build_link<F: FileSystem, M: Markdown>(
    node: &KnowledgeMetadata,
    revision: u64,
    filesystem: &F,
    markdown: &M,
) -> CommandResult<SearchLink> {
    let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
        .map_err(map_search_error)?;
    let flags = if is_current_knowledge(node, &path, filesystem, markdown)? {
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

fn is_current_knowledge<F: FileSystem, M: Markdown>(
    node: &KnowledgeMetadata,
    path: &RepoPath,
    filesystem: &F,
    markdown: &M,
) -> CommandResult<bool> {
    let Some(file) = filesystem.read_file(path).map_err(map_search_error)? else {
        return Ok(false);
    };
    let Ok(Some(parsed)) = markdown.parse_file(path, &file.bytes) else {
        return Ok(false);
    };
    Ok(parsed.elements.iter().any(|current| {
        current.element.id == node.id
            && current.element.hash == node.hash
            && current.element.lines == node.lines
    }))
}

fn invalid_search_data() -> CommandError {
    CommandError::new(CommandErrorKind::InternalError, "invalid search index data")
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
