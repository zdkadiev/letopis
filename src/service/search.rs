use super::utils::is_current_knowledge;
use super::utils::map_document_error;
use super::utils::map_document_file;
use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandResult, ConfigState, Embedder,
    EmbeddingConfig, FileReader, KnowledgeMetadata, MarkdownCodec, NodeMetadata, ProvenanceRequest,
    QueryEmbedding, QueryEmbeddingRequest, Refresher, SearchFilters, SearchFlag, SearchLink,
    SearchRequest, Searcher, SnapshotError, SnapshotErrorKind, SourceProvenance,
    SourceRevisionComparison, SourceRevisionRequest, StorageRead, TextMatch, VectorMatch,
    VectorOutcome,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri, TreeOid};
use std::path::Path;
use std::{
    collections::{BTreeMap, BTreeSet},
    num::NonZeroUsize,
};
use std::{fmt, rc::Rc};

const REVISION_COMPARISON_LIMIT: usize = 4096;

/// Infer and validate vector candidates while storage owns the pinned read view.
struct QueryCandidates<'a, E> {
    storage_read: &'a dyn StorageRead,
    embeddings: &'a mut E,
}

/// Observe candidate eligibility through disjoint read-only boundaries.
struct SearchEligibility<'a, F, M, P> {
    storage_read: &'a dyn StorageRead,
    filesystem: &'a F,
    markdown: &'a M,
    provenance: &'a P,
}

struct CandidateReads<'a> {
    storage_read: &'a dyn StorageRead,
}

struct CandidateMetadata {
    row_id: i64,
    metadata: KnowledgeMetadata,
}

/// Refreshes and ranks complete candidate channels from one scoped read.
///
/// Boundaries are independently owned; inference cannot upgrade the search read into a write.
pub(crate) struct SearchService<D, E, F, M, P, C> {
    /// Canonical document publication before pinning.
    pub(crate) refresher: D,
    /// Exclusive scoped search read connection.
    pub(crate) cache: Rc<C>,
    /// Query executor owning independent configuration and diagnostic connections.
    pub(crate) embeddings: E,
    /// Confined live knowledge observations.
    pub(crate) filesystem: F,
    /// Shared whole-file knowledge codec.
    pub(crate) markdown: M,
    /// Read-only observations and comparisons of candidate source bindings.
    pub(crate) provenance: P,
}

/// Results demanded by one eligibility computation, after requested-root validation.
struct RevisionComparisons<'a> {
    provenance: &'a dyn SourceProvenance,
    results: BTreeMap<(Option<TreeOid>, SourceUri, TreeOid), SourceRevisionComparison>,
}

impl<D, E, F, M, P, C> fmt::Debug for SearchService<D, E, F, M, P, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchService")
            .finish_non_exhaustive()
    }
}

impl<
    D: Refresher,
    E: Embedder,
    F: FileReader,
    M: MarkdownCodec,
    P: SourceProvenance,
    C: CacheDatabase,
> Searcher for SearchService<D, E, F, M, P, C>
{
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>> {
        self.refresher
            .refresh(&[])
            .map_err(|cause| map_document_error(cause, "cannot search knowledge"))?;
        self.provenance
            .validate_revisions(&request.filters.source_revisions)
            .map_err(map_revision_error)?;
        let config = self.embeddings.get_configuration().map_err(|error| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot search knowledge",
                error,
            )
        })?;
        let cache = Rc::clone(&self.cache);
        cache
            .with_read_tx(|| {
                let read: &dyn StorageRead = self.cache.as_ref();
                let candidates = CandidateReads { storage_read: read };
                let captured = candidates.capture_vector_config(&config)?;
                let Some(expression) = build_fts_expression(&request.query) else {
                    return Ok(Vec::new());
                };
                let vector_matches = QueryCandidates {
                    storage_read: read,
                    embeddings: &mut self.embeddings,
                }
                .find_vector_candidates(captured, &request.query)?;
                let text_matches =
                    read.text_index_get_by_expression(&expression)
                        .map_err(|error| {
                            CommandError::new_with_cause(
                                CommandErrorKind::InternalError,
                                "cannot search knowledge",
                                error,
                            )
                        })?;
                validate_metrics(&text_matches, &vector_matches)?;
                let metadata = candidates.get_candidate_metadata(&text_matches, &vector_matches)?;
                let eligibility = SearchEligibility {
                    storage_read: read,
                    filesystem: &self.filesystem,
                    markdown: &self.markdown,
                    provenance: &self.provenance,
                };
                let links = eligibility.get_eligible_links(&metadata, &request.filters)?;
                let eligible = links.keys().cloned().collect();
                let ranked =
                    rank_candidates(&text_matches, &vector_matches, &eligible, request.limit)?;
                get_ranked_links(links, ranked)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                ),
            })
    }
}

impl<F: FileReader, M: MarkdownCodec, P: SourceProvenance> SearchEligibility<'_, F, M, P> {
    fn get_eligible_links(
        &self,
        metadata: &BTreeMap<KnowledgeId, KnowledgeMetadata>,
        filters: &SearchFilters,
    ) -> CommandResult<BTreeMap<KnowledgeId, SearchLink>> {
        let mut comparisons = RevisionComparisons::new(self.provenance);
        let mut links = BTreeMap::new();
        for (id, node) in metadata {
            let sources = self.get_source_bindings(id)?;
            if !Self::is_matched_by_sources(&mut comparisons, node, &sources, filters)? {
                continue;
            }
            let mut link = self.build_link(
                node,
                self.storage_read.project_get_by_id().map_err(|error| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot search knowledge",
                        error,
                    )
                })?,
            )?;
            let observed = self
                .provenance
                .observe_sources(ProvenanceRequest {
                    source_snapshot: node.source_snapshot.as_ref(),
                    sources: &sources,
                })
                .map_err(|error| {
                    CommandError::new_with_cause(
                        CommandErrorKind::InternalError,
                        "cannot search knowledge",
                        error,
                    )
                })?;
            link.flags.extend(observed.flags);
            if filters.is_matched_by_flags(&link.flags) {
                links.insert(id.clone(), link);
            }
        }
        Ok(links)
    }

    fn build_link(&self, node: &KnowledgeMetadata, revision: u64) -> CommandResult<SearchLink> {
        let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?;
        let file = map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?;
        let parsed = file
            .as_ref()
            .and_then(|file| self.markdown.file_parse(&path, &file.bytes).ok().flatten());
        let flags = if is_current_knowledge(node, parsed.as_ref()) {
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

    fn is_matched_by_sources(
        comparisons: &mut RevisionComparisons<'_>,
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
            if Self::is_matched_by_revision(comparisons, node, source, filters)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn is_matched_by_revision(
        comparisons: &mut RevisionComparisons<'_>,
        node: &KnowledgeMetadata,
        source: &SourceUri,
        filters: &SearchFilters,
    ) -> CommandResult<bool> {
        for revision in &filters.source_revisions {
            let comparison = comparisons
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

    fn get_source_bindings(&self, id: &KnowledgeId) -> CommandResult<BTreeSet<SourceUri>> {
        Ok(self
            .storage_read
            .edges_get_by_source_id(Some(id))
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?
            .into_iter()
            .filter_map(|edge| match edge.get_target() {
                EdgeTarget::Source(source) => Some(source.clone()),
                EdgeTarget::Knowledge(_) => None,
            })
            .collect())
    }
}

impl<E: Embedder> QueryCandidates<'_, E> {
    fn find_vector_candidates(
        &mut self,
        captured: Option<(&EmbeddingConfig, NonZeroUsize)>,
        query: &str,
    ) -> CommandResult<Vec<VectorMatch>> {
        let Some((config, dimensions)) = captured else {
            return Ok(Vec::new());
        };
        let result = self
            .embeddings
            .embed_query_with_config(QueryEmbeddingRequest {
                text: query,
                config,
                dimensions: Some(dimensions),
            })
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?;
        let QueryEmbedding::Available { identity, vector } = result else {
            return Ok(Vec::new());
        };
        if identity != config.get_identity()
            || vector.len() != dimensions.get()
            || vector.iter().any(|value| !value.is_finite())
        {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid search index data",
            ));
        }
        match self
            .storage_read
            .vector_index_get_by_vector(&identity, &vector, dimensions.get())
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })? {
            VectorOutcome::Current(matches) => Ok(matches),
            VectorOutcome::ConfigChanged => Ok(Vec::new()),
        }
    }
}

impl CandidateReads<'_> {
    fn capture_vector_config<'a>(
        &self,
        state: &'a ConfigState,
    ) -> CommandResult<Option<(&'a EmbeddingConfig, NonZeroUsize)>> {
        let ConfigState::Ready(config) = state else {
            return Ok(None);
        };
        if self.storage_read.meta_get_by_id().map_err(|error| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot search knowledge",
                error,
            )
        })? != Some(config.get_identity())
        {
            return Ok(None);
        }
        let VectorOutcome::Current(Some(dimensions)) = self
            .storage_read
            .vector_index_config_get_by_id(&config.get_identity())
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?
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

    fn get_candidate_metadata(
        &self,
        text_matches: &[TextMatch],
        vector_matches: &[VectorMatch],
    ) -> CommandResult<BTreeMap<KnowledgeId, KnowledgeMetadata>> {
        let mut metadata = BTreeMap::new();
        for hit in text_matches {
            let candidate = self.get_knowledge(&hit.id)?;
            if candidate.row_id != hit.row_id {
                return Err(CommandError::new(
                    CommandErrorKind::InternalError,
                    "invalid search index data",
                ));
            }
            metadata.insert(hit.id.clone(), candidate.metadata);
        }
        for hit in vector_matches {
            let candidate = self.get_knowledge(&hit.id)?;
            if candidate.metadata.search_hash != hit.search_hash {
                return Err(CommandError::new(
                    CommandErrorKind::InternalError,
                    "invalid search index data",
                ));
            }
            metadata.insert(hit.id.clone(), candidate.metadata);
        }
        Ok(metadata)
    }

    fn get_knowledge(&self, id: &KnowledgeId) -> CommandResult<CandidateMetadata> {
        let stored = self
            .storage_read
            .nodes_get_by_uri(&EdgeTarget::Knowledge(id.clone()))
            .map_err(|error| {
                CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                )
            })?
            .ok_or_else(|| {
                CommandError::new(CommandErrorKind::InternalError, "invalid search index data")
            })?;
        let NodeMetadata::Knowledge(node) = stored.metadata else {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid search index data",
            ));
        };
        if node.id != *id || node.uri != *id {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid search index data",
            ));
        }
        Ok(CandidateMetadata {
            row_id: stored.row_id,
            metadata: node,
        })
    }
}

impl<'a> RevisionComparisons<'a> {
    /// Creates an empty comparison history without performing I/O.
    ///
    /// # Arguments
    ///
    /// * `provenance` — Source provenance used to compare requested revisions.
    fn new(provenance: &'a dyn SourceProvenance) -> Self {
        Self {
            provenance,
            results: BTreeMap::new(),
        }
    }

    /// Returns the comparison for one source revision, reusing stored results.
    ///
    /// Unavailable bases are compared again because an object can appear before the next call.
    ///
    /// # Arguments
    ///
    /// * `request` — Source address, basis and requested revision to compare.
    ///
    /// # Errors
    /// Reports comparison failures without storing them.
    fn compare_revision(
        &mut self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        let key = (
            request.source_snapshot.cloned(),
            request.source.clone(),
            request.revision.clone(),
        );
        if let Some(result) = self.results.get(&key) {
            return Ok(*result);
        }
        let result = self.provenance.compare_revision(request)?;
        if result != SourceRevisionComparison::BasisUnavailable
            && self.results.len() < REVISION_COMPARISON_LIMIT
        {
            self.results.insert(key, result);
        }
        Ok(result)
    }
}

impl fmt::Debug for RevisionComparisons<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RevisionComparisons")
            .finish_non_exhaustive()
    }
}

/// Builds an FTS expression from alphanumeric query segments.
///
/// Punctuation never becomes FTS grammar.
///
/// # Arguments
///
/// * `query` — User query split into quoted alphanumeric segments.
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

fn map_revision_error(cause: SnapshotError) -> CommandError {
    let kind = match cause.get_kind() {
        SnapshotErrorKind::NotFound => CommandErrorKind::NotFound,
        SnapshotErrorKind::InvalidRequest => CommandErrorKind::InvalidRequest,
        SnapshotErrorKind::InternalError => CommandErrorKind::InternalError,
    };
    CommandError::new_with_cause(kind, "cannot search source revisions", cause)
}

/// Returns the ranked eligible text and vector matches up to `limit`.
///
/// # Arguments
///
/// * `text_matches` — Full-text matches contributing the text ranking channel.
/// * `vector_matches` — Vector matches contributing the vector ranking channel.
/// * `eligible` — Candidate identities allowed in the final ranking.
/// * `limit` — Positive maximum number of identities returned.
fn rank_candidates(
    text_matches: &[TextMatch],
    vector_matches: &[VectorMatch],
    eligible: &BTreeSet<KnowledgeId>,
    limit: NonZeroUsize,
) -> CommandResult<Vec<KnowledgeId>> {
    validate_metrics(text_matches, vector_matches)?;
    let text_ranks = rank_channel(
        text_matches.iter().map(|hit| (&hit.id, hit.bm25)).collect(),
        eligible,
    )?;
    let vector_ranks = rank_channel(
        vector_matches
            .iter()
            .map(|hit| (&hit.id, hit.distance))
            .collect(),
        eligible,
    )?;
    let mut scores = BTreeMap::new();
    for (id, weight) in text_ranks.into_iter().chain(vector_ranks) {
        let score = scores.entry(id.clone()).or_insert(0.0);
        *score += weight;
        if !score.is_finite() {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid search index data",
            ));
        }
    }
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

/// Returns the links for the ranked identities in ranking order.
///
/// # Arguments
///
/// * `links` — Eligible links keyed by knowledge identity.
/// * `ranked` — Ranked identities that must all exist in `links`.
///
/// # Errors
/// Reports an internal error for an identity missing from `links`.
fn get_ranked_links(
    mut links: BTreeMap<KnowledgeId, SearchLink>,
    ranked: Vec<KnowledgeId>,
) -> CommandResult<Vec<SearchLink>> {
    ranked
        .into_iter()
        .map(|id| {
            links.remove(&id).ok_or_else(|| {
                CommandError::new(CommandErrorKind::InternalError, "invalid search index data")
            })
        })
        .collect()
}

/// Rejects non-finite metrics from text and vector matches.
///
/// The check runs before eligibility can discard their candidates.
///
/// # Arguments
///
/// * `text_matches` — Full-text matches whose metrics are validated.
/// * `vector_matches` — Vector matches whose metrics are validated.
fn validate_metrics(
    text_matches: &[TextMatch],
    vector_matches: &[VectorMatch],
) -> CommandResult<()> {
    let metrics = text_matches
        .iter()
        .map(|hit| hit.bm25)
        .chain(vector_matches.iter().map(|hit| hit.distance));
    if metrics.into_iter().any(|metric| !metric.is_finite()) {
        return Err(CommandError::new(
            CommandErrorKind::InternalError,
            "invalid search index data",
        ));
    }
    Ok(())
}

fn rank_channel<'a>(
    mut channel: Vec<(&'a KnowledgeId, f64)>,
    eligible: &BTreeSet<KnowledgeId>,
) -> CommandResult<Vec<(&'a KnowledgeId, f64)>> {
    channel.retain(|(id, _)| eligible.contains(*id));
    channel.sort_by(|(left_id, left), (right_id, right)| {
        // Signed zeros are equal metrics and therefore use the same identity tie-break.
        if left == right {
            left_id.cmp(right_id)
        } else {
            left.total_cmp(right)
        }
    });
    let mut seen = BTreeSet::new();
    channel.retain(|(id, _)| seen.insert(*id));
    for (index, (_, weight)) in channel.iter_mut().enumerate() {
        let rank = index
            .checked_add(1)
            .and_then(NonZeroUsize::new)
            .ok_or_else(|| {
                CommandError::new(CommandErrorKind::InternalError, "invalid search index data")
            })?;
        *weight = calculate_rank_weight(rank)?;
    }
    Ok(channel)
}

/// Returns the reciprocal of the rank as the channel weight.
///
/// Ranks that do not fit a `u32` are rejected.
///
/// # Arguments
///
/// * `rank` — Positive one-based rank whose reciprocal becomes the channel weight.
fn calculate_rank_weight(rank: NonZeroUsize) -> CommandResult<f64> {
    let rank = u32::try_from(rank.get()).map_err(|error| {
        CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "cannot search knowledge",
            error,
        )
    })?;
    Ok(1.0 / f64::from(rank))
}

#[cfg(test)]
#[path = "search_test.rs"]
mod tests;
