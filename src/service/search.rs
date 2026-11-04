use super::utils::is_current_knowledge;
use super::{
    CommandError, CommandErrorKind, CommandResult, ConfigState, Embedder, EmbeddingConfig,
    FileSystem, KnowledgeMetadata, MarkdownCodec, NodeMetadata, ProvenanceRequest, QueryEmbedding,
    QueryEmbeddingRequest, Refresher, SearchFilters, SearchFlag, SearchLink, SearchRequest,
    Searcher, SnapshotError, SnapshotErrorKind, SourceProvenance, SourceRevisionComparison,
    SourceRevisionRequest, Storage, StorageRead, TextMatch, VectorMatch, VectorOutcome,
};
use crate::domain::{EdgeTarget, KnowledgeId, RepoPath, SourceUri};
use std::fmt;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    num::NonZeroUsize,
};

use comparisons::RevisionComparisons;
use ranking::{rank_candidates, validate_metrics};

/// Infer and validate vector candidates while storage owns the pinned read view.
struct QueryCandidates<'a, E> {
    embeddings: &'a mut E,
}

/// Observe candidate eligibility through disjoint read-only boundaries.
struct SearchEligibility<'a, F, M, P> {
    filesystem: &'a F,
    markdown: &'a M,
    provenance: &'a P,
}

/// Refreshes and ranks complete candidate channels from one scoped read.
///
/// Boundaries are independently owned; inference cannot upgrade the search read into a write.
pub(crate) struct SearchService<D, E, F, M, P> {
    /// Canonical document publication before pinning.
    pub(crate) refresher: D,
    /// Exclusive scoped search read connection.
    pub(crate) storage: Box<dyn Storage>,
    /// Query executor owning independent configuration and diagnostic connections.
    pub(crate) embeddings: E,
    /// Confined live knowledge observations.
    pub(crate) filesystem: F,
    /// Shared whole-file knowledge codec.
    pub(crate) markdown: M,
    /// Read-only observations and comparisons of candidate source bindings.
    pub(crate) provenance: P,
}

impl<D, E, F, M, P> fmt::Debug for SearchService<D, E, F, M, P> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SearchService")
            .finish_non_exhaustive()
    }
}
impl<D: Refresher, E: Embedder, F: FileSystem, M: MarkdownCodec, P: SourceProvenance> Searcher
    for SearchService<D, E, F, M, P>
{
    fn search(&mut self, request: &SearchRequest) -> CommandResult<Vec<SearchLink>> {
        self.refresher.refresh().map_err(map_search_error)?;
        self.provenance
            .validate_revisions(&request.filters.source_revisions)
            .map_err(map_revision_error)?;
        let config = self
            .embeddings
            .get_configuration()
            .map_err(map_search_error)?;
        let mut read = self.storage.begin_read_tx().map_err(map_search_error)?;
        let captured = capture_vector_config(read.as_mut(), &config)?;
        let Some(expression) = build_fts_expression(&request.query) else {
            return Ok(Vec::new());
        };
        let vectors = QueryCandidates {
            embeddings: &mut self.embeddings,
        }
        .find_vector_candidates(read.as_mut(), captured, &request.query)?;
        let fts = read
            .get_text_matches_by_expression(&expression)
            .map_err(map_search_error)?;
        validate_metrics(&fts, &vectors)?;
        let metadata = get_candidate_metadata(read.as_mut(), &fts, &vectors)?;
        let eligibility = SearchEligibility {
            filesystem: &self.filesystem,
            markdown: &self.markdown,
            provenance: &self.provenance,
        };
        let mut links =
            eligibility.get_eligible_links(read.as_mut(), &metadata, &request.filters)?;
        let eligible = links.keys().cloned().collect();
        let ranked = rank_candidates(&fts, &vectors, &eligible, request.limit)?;
        ranked
            .into_iter()
            .map(|id| links.remove(&id).ok_or_else(fail_invalid_search_data))
            .collect()
    }
}

impl<F: FileSystem, M: MarkdownCodec, P: SourceProvenance> SearchEligibility<'_, F, M, P> {
    fn get_eligible_links(
        &self,
        read: &mut dyn StorageRead,
        metadata: &BTreeMap<KnowledgeId, KnowledgeMetadata>,
        filters: &SearchFilters,
    ) -> CommandResult<BTreeMap<KnowledgeId, SearchLink>> {
        let mut comparisons = RevisionComparisons::new(self.provenance);
        let mut links = BTreeMap::new();
        for (id, node) in metadata {
            let sources = get_source_bindings(read, id)?;
            if !Self::matches_sources(&mut comparisons, node, &sources, filters)? {
                continue;
            }
            let mut link = self.build_link(node, read.get_revision())?;
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

    fn build_link(&self, node: &KnowledgeMetadata, revision: u64) -> CommandResult<SearchLink> {
        let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
            .map_err(map_search_error)?;
        let file = self.filesystem.get_file(&path).map_err(map_search_error)?;
        let parsed = file
            .as_ref()
            .and_then(|file| self.markdown.parse_file(&path, &file.bytes).ok().flatten());
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

    fn matches_sources(
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
            if Self::matches_revision(comparisons, node, source, filters)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn matches_revision(
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
}

impl<E: Embedder> QueryCandidates<'_, E> {
    fn find_vector_candidates(
        &mut self,
        read: &mut dyn StorageRead,
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
            .get_nearest_vectors(&identity, &vector, dimensions.get())
            .map_err(map_search_error)?
        {
            VectorOutcome::Current(matches) => Ok(matches),
            VectorOutcome::ConfigChanged => Ok(Vec::new()),
        }
    }
}

fn get_source_bindings(
    read: &mut dyn StorageRead,
    id: &KnowledgeId,
) -> CommandResult<BTreeSet<SourceUri>> {
    Ok(read
        .get_edges_by_source_id(Some(id))
        .map_err(map_search_error)?
        .into_iter()
        .filter_map(|edge| match edge.get_target() {
            EdgeTarget::Source(source) => Some(source.clone()),
            EdgeTarget::Knowledge(_) => None,
        })
        .collect())
}

fn capture_vector_config<'a>(
    read: &mut dyn StorageRead,
    state: &'a ConfigState,
) -> CommandResult<Option<(&'a EmbeddingConfig, NonZeroUsize)>> {
    let ConfigState::Ready(config) = state else {
        return Ok(None);
    };
    if read.get_config_identity() != Some(config.get_identity()) {
        return Ok(None);
    }
    let VectorOutcome::Current(Some(dimensions)) = read
        .get_vector_dimensions(&config.get_identity())
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

fn get_candidate_metadata(
    read: &mut dyn StorageRead,
    fts: &[TextMatch],
    vectors: &[VectorMatch],
) -> CommandResult<BTreeMap<KnowledgeId, KnowledgeMetadata>> {
    let mut metadata = BTreeMap::new();
    for hit in fts {
        let (row_id, node) = get_knowledge(read, &hit.id)?;
        if row_id != hit.row_id {
            return Err(fail_invalid_search_data());
        }
        metadata.insert(hit.id.clone(), node);
    }
    for hit in vectors {
        let (_, node) = get_knowledge(read, &hit.id)?;
        if node.search_hash != hit.search_hash {
            return Err(fail_invalid_search_data());
        }
        metadata.insert(hit.id.clone(), node);
    }
    Ok(metadata)
}

fn get_knowledge(
    read: &mut dyn StorageRead,
    id: &KnowledgeId,
) -> CommandResult<(i64, KnowledgeMetadata)> {
    let stored = read
        .get_node_by_id(&EdgeTarget::Knowledge(id.clone()))
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

mod ranking {
    use super::{fail_invalid_search_data, map_search_error};
    use crate::{
        domain::KnowledgeId,
        service::{CommandResult, TextMatch, VectorMatch},
    };
    use std::{
        collections::{BTreeMap, BTreeSet},
        num::NonZeroUsize,
    };

    /// Validate complete metrics, rank eligible channels, and fuse before the final limit.
    pub(super) fn rank_candidates(
        fts: &[TextMatch],
        vectors: &[VectorMatch],
        eligible: &BTreeSet<KnowledgeId>,
        limit: NonZeroUsize,
    ) -> CommandResult<Vec<KnowledgeId>> {
        validate_metrics(fts, vectors)?;
        let text_ranks = rank_channel(
            fts.iter().map(|hit| (&hit.id, hit.bm25)).collect(),
            eligible,
        )?;
        let vector_ranks = rank_channel(
            vectors.iter().map(|hit| (&hit.id, hit.distance)).collect(),
            eligible,
        )?;
        let mut scores = BTreeMap::new();
        for (id, weight) in text_ranks.into_iter().chain(vector_ranks) {
            let score = scores.entry(id.clone()).or_insert(0.0);
            *score += weight;
            if !score.is_finite() {
                return Err(fail_invalid_search_data());
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

    /// Reject corrupt native metrics before eligibility can discard their candidates.
    pub(super) fn validate_metrics(
        fts: &[TextMatch],
        vectors: &[VectorMatch],
    ) -> CommandResult<()> {
        let metrics = fts
            .iter()
            .map(|hit| hit.bm25)
            .chain(vectors.iter().map(|hit| hit.distance));
        if metrics.into_iter().any(|metric| !metric.is_finite()) {
            return Err(fail_invalid_search_data());
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
                .ok_or_else(fail_invalid_search_data)?;
            *weight = calculate_rank_weight(rank)?;
        }
        Ok(channel)
    }

    /// Restrict ranks to an exactly representable integer before computing their reciprocal.
    pub(super) fn calculate_rank_weight(rank: NonZeroUsize) -> CommandResult<f64> {
        let rank = u32::try_from(rank.get()).map_err(map_search_error)?;
        Ok(1.0 / f64::from(rank))
    }
}

mod comparisons {
    use crate::{
        domain::{SourceUri, TreeOid},
        service::{
            SnapshotError, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
        },
    };
    use std::{collections::BTreeMap, fmt};

    const REVISION_COMPARISON_LIMIT: usize = 4096;

    /// Results demanded by one eligibility computation, after requested-root validation.
    pub(super) struct RevisionComparisons<'a> {
        provenance: &'a dyn SourceProvenance,
        results: BTreeMap<(Option<TreeOid>, SourceUri, TreeOid), SourceRevisionComparison>,
    }

    impl<'a> RevisionComparisons<'a> {
        /// Start one empty operation-local history without performing I/O.
        pub(super) fn new(provenance: &'a dyn SourceProvenance) -> Self {
            Self {
                provenance,
                results: BTreeMap::new(),
            }
        }

        /// Compare only when demanded, reusing verified immutable results within this search.
        /// Unavailable bases are rechecked because an object can appear before the next demand.
        ///
        /// # Errors
        /// Propagates comparison failures without retaining them.
        pub(super) fn compare_revision(
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

    #[cfg(test)]
    mod tests {
        use super::RevisionComparisons;
        use crate::{
            domain::{SourceUri, TreeOid},
            service::{
                DocumentResult, ProvenanceObservation, ProvenanceRequest, SnapshotError,
                SnapshotErrorKind, SourceProvenance, SourceRevisionComparison,
                SourceRevisionRequest,
            },
        };
        use std::{
            cell::{Cell, RefCell},
            collections::BTreeSet,
        };

        type ComparisonKey = (Option<TreeOid>, SourceUri, TreeOid);
        #[derive(Default)]
        struct Recorded {
            calls: RefCell<Vec<ComparisonKey>>,
            failure: Cell<bool>,
            result: Cell<Option<SourceRevisionComparison>>,
        }

        #[test]
        fn exact_basis_address_range_and_revision_are_distinct() {
            // Given: requests differ in exactly one key component, including an absent basis.
            let provenance = Recorded::default();
            let mut comparisons = RevisionComparisons::new(&provenance);
            let first = TreeOid::parse(&"a".repeat(40)).unwrap();
            let second = TreeOid::parse(&"b".repeat(40)).unwrap();
            let whole = SourceUri::parse("a").unwrap();
            let range = SourceUri::parse("a#L1-L1").unwrap();
            let other = SourceUri::parse("b").unwrap();
            let requests = [
                (None, &whole, &first),
                (Some(&first), &whole, &first),
                (Some(&second), &whole, &first),
                (Some(&first), &range, &first),
                (Some(&first), &other, &first),
                (Some(&first), &whole, &second),
            ];
            // When: each request is repeated, without changing their exact identities.
            for (basis, source, revision) in requests {
                let request = SourceRevisionRequest {
                    source_snapshot: basis,
                    source,
                    revision,
                };
                let left = comparisons.compare_revision(request).unwrap();
                let right = comparisons.compare_revision(request).unwrap();
                assert_eq!(left, right);
            }
            // Then: each exact key is demanded once, in its first-use order.
            assert_eq!(provenance.calls.borrow().len(), 6);
            assert_eq!(
                provenance.calls.borrow()[0],
                (None, whole.clone(), first.clone())
            );
            assert_eq!(provenance.calls.borrow()[5], (Some(first), whole, second));
        }

        #[test]
        fn revision_failures_are_retried_and_overflow_recomputes() {
            // Given: one failing comparison is retried after repair.
            let provenance = Recorded::default();
            let mut comparisons = RevisionComparisons::new(&provenance);
            let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
            let source = SourceUri::parse("a").unwrap();
            let request = SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            };
            provenance.failure.set(true);
            assert_eq!(
                comparisons
                    .compare_revision(request)
                    .unwrap_err()
                    .get_kind(),
                SnapshotErrorKind::InternalError
            );
            provenance.failure.set(false);
            // When: the repaired result and 4096 additional distinct addresses are demanded.
            assert_eq!(
                comparisons.compare_revision(request).unwrap(),
                SourceRevisionComparison::Equal
            );
            for n in 0..4096 {
                let source = SourceUri::parse(&format!("source{n}")).unwrap();
                comparisons
                    .compare_revision(SourceRevisionRequest {
                        source: &source,
                        ..request
                    })
                    .unwrap();
            }
            let overflow = SourceUri::parse("source4095").unwrap();
            comparisons.compare_revision(request).unwrap();
            comparisons
                .compare_revision(SourceRevisionRequest {
                    source: &overflow,
                    ..request
                })
                .unwrap();
            // Then: the failure was retried, a cached success reused and overflow recomputed.
            assert_eq!(provenance.calls.borrow().len(), 4099);
        }

        #[test]
        fn successful_comparison_variants_are_preserved() {
            for expected in [
                SourceRevisionComparison::Different,
                SourceRevisionComparison::SourceNotFound,
                SourceRevisionComparison::Unfetched,
            ] {
                // Given: a demanded typed result, independent of the later collaborator value.
                let provenance = Recorded::default();
                provenance.result.set(Some(expected));
                let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
                let source = SourceUri::parse("a").unwrap();
                let request = SourceRevisionRequest {
                    source_snapshot: Some(&tree),
                    source: &source,
                    revision: &tree,
                };
                let mut comparisons = RevisionComparisons::new(&provenance);
                // When: the successful exact comparison is requested again.
                assert_eq!(comparisons.compare_revision(request).unwrap(), expected);
                provenance.result.set(Some(SourceRevisionComparison::Equal));
                let result = comparisons.compare_revision(request).unwrap();
                // Then: the original typed result is returned without more historical work.
                assert_eq!(result, expected);
                assert_eq!(provenance.calls.borrow().len(), 1);
            }
        }

        #[test]
        fn unavailable_basis_is_rechecked_when_the_same_key_becomes_available() {
            // Given: a declared basis is unavailable on the first demanded comparison.
            let provenance = Recorded::default();
            provenance
                .result
                .set(Some(SourceRevisionComparison::BasisUnavailable));
            let tree = TreeOid::parse(&"a".repeat(40)).unwrap();
            let source = SourceUri::parse("a").unwrap();
            let request = SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            };
            let mut comparisons = RevisionComparisons::new(&provenance);
            assert_eq!(
                comparisons.compare_revision(request).unwrap(),
                SourceRevisionComparison::BasisUnavailable
            );
            // When: the same basis appears before its next demand.
            provenance.result.set(Some(SourceRevisionComparison::Equal));
            let result = comparisons.compare_revision(request).unwrap();
            // Then: the negative classification cannot hide the now available bytes.
            assert_eq!(result, SourceRevisionComparison::Equal);
            assert_eq!(provenance.calls.borrow().len(), 2);
        }

        impl SourceProvenance for Recorded {
            fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
                panic!("caller validates roots before eligibility")
            }
            fn observe_sources(
                &self,
                _: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                panic!("comparisons never observe live bytes")
            }
            fn compare_revision(
                &self,
                request: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                self.calls.borrow_mut().push((
                    request.source_snapshot.cloned(),
                    request.source.clone(),
                    request.revision.clone(),
                ));
                if self.failure.get() {
                    return Err(SnapshotError::new(
                        SnapshotErrorKind::InternalError,
                        "comparison failure",
                    ));
                }
                Ok(self.result.get().unwrap_or(SourceRevisionComparison::Equal))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        QueryCandidates, build_fts_expression, capture_vector_config, get_candidate_metadata,
        ranking::{calculate_rank_weight, rank_candidates},
    };
    use crate::{
        domain::{ElementKind, KnowledgeId, LineRange, Sha256},
        open_storage,
        service::{
            CommandErrorKind, ConfigState, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingError,
            EmbeddingErrorKind, EmbeddingResult, KnowledgeMetadata, MockRefresher, NodeMetadata,
            QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, Storage, TextMatch, VectorMatch,
            VectorOutcome,
        },
    };
    use std::{collections::BTreeSet, num::NonZeroUsize};
    mockall::mock! {
        Query {}
        impl Embedder for Query {
            fn get_configuration(&mut self) -> EmbeddingResult<ConfigState>;
            fn drain(&mut self) -> EmbeddingResult<DrainOutcome>;
            fn embed_query(&mut self, text: &str) -> EmbeddingResult<QueryEmbedding>;
            fn embed_query_with_config<'a>(&mut self, request: QueryEmbeddingRequest<'a>) -> EmbeddingResult<QueryEmbedding>;
        }
    }

    #[test]
    fn literal_query_quotes_unicode_tokens_without_fts_operators() {
        // Given
        for (query, expected) in [
            (
                "SQLite OR блокировки",
                Some("\"SQLite\" AND \"OR\" AND \"блокировки\""),
            ),
            (
                "\"NEAR(x*)\" title:café 42",
                Some("\"NEAR\" AND \"x\" AND \"title\" AND \"café\" AND \"42\""),
            ),
            ("a-b_١２", Some("\"a\" AND \"b\" AND \"١２\"")),
            ("", None),
            (" * : () \" ", None),
        ] {
            // When / Then
            assert_eq!(build_fts_expression(query).as_deref(), expected);
        }
    }
    #[test]
    fn signed_zero_metrics_tie_by_identity_in_both_channels() {
        // Given / When / Then
        assert_eq!(
            rank_ids(
                &[make_text_match("b.md", -0.0), make_text_match("a.md", 0.0)],
                &[],
                2
            ),
            ["a.md", "b.md"]
        );
        assert_eq!(
            rank_ids(
                &[],
                &[
                    make_vector_match("b.md", -0.0),
                    make_vector_match("a.md", 0.0)
                ],
                2
            ),
            ["a.md", "b.md"]
        );
    }
    #[test]
    fn reciprocal_rank_unions_channels_before_limit() {
        // Given
        let fts = [make_text_match("a.md", -2.0), make_text_match("b.md", -1.0)];
        let vectors = [
            make_vector_match("c.md", 0.0),
            make_vector_match("b.md", 0.5),
            make_vector_match("a.md", 1.0),
        ];
        for (limit, expected) in [(3, vec!["a.md", "b.md", "c.md"]), (2, vec!["a.md", "b.md"])] {
            // When / Then
            assert_eq!(rank_ids(&fts, &vectors, limit), expected);
        }
    }
    #[test]
    fn ranking_orders_ties_deduplicates_and_keeps_disjoint_channels() {
        // Given / When / Then
        assert_eq!(
            rank_ids(
                &[make_text_match("b.md", 0.0), make_text_match("a.md", 0.0)],
                &[],
                9
            ),
            ["a.md", "b.md"]
        );
        assert_eq!(
            rank_ids(
                &[],
                &[
                    make_vector_match("b.md", 0.0),
                    make_vector_match("a.md", 0.0)
                ],
                9
            ),
            ["a.md", "b.md"]
        );
        assert_eq!(
            rank_ids(
                &[make_text_match("b.md", 0.0)],
                &[make_vector_match("a.md", 0.0)],
                9
            ),
            ["a.md", "b.md"]
        );
        let fts = [
            make_text_match("a.md", -3.0),
            make_text_match("a.md", -2.0),
            make_text_match("b.md", -1.0),
        ];
        assert_eq!(
            rank_ids(
                &fts,
                &[
                    make_vector_match("c.md", 0.0),
                    make_vector_match("b.md", 0.1)
                ],
                9
            ),
            ["a.md", "b.md", "c.md"]
        );
        assert!(rank_ids(&[], &[], 9).is_empty());
    }
    #[test]
    fn ranking_rejects_nonfinite_native_metrics() {
        // Given
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            for (fts, vectors) in [
                (vec![make_text_match("a.md", value)], vec![]),
                (vec![], vec![make_vector_match("a.md", value)]),
            ] {
                // When / Then
                assert_eq!(
                    rank_candidates(&fts, &vectors, &BTreeSet::new(), NonZeroUsize::MIN)
                        .unwrap_err()
                        .get_kind(),
                    CommandErrorKind::InternalError
                );
            }
        }
    }
    #[test]
    fn reciprocal_ranks_reward_overlap_before_applying_small_limit() {
        // Given
        let fts = [make_text_match("z.md", -2.0), make_text_match("a.md", -1.0)];
        let vectors = [
            make_vector_match("y.md", 0.0),
            make_vector_match("a.md", 1.0),
        ];
        // When / Then
        assert_eq!(rank_ids(&fts, &vectors, 1), ["a.md"]);
        // Given
        let fts = [
            make_text_match("a.md", -3.0),
            make_text_match("b.md", -2.0),
            make_text_match("c.md", -1.0),
        ];
        let vectors = [
            make_vector_match("d.md", 0.0),
            make_vector_match("b.md", 0.5),
            make_vector_match("c.md", 1.0),
        ];
        // When / Then
        assert_eq!(
            rank_ids(&fts, &vectors, 4),
            ["a.md", "b.md", "d.md", "c.md"]
        );
        assert_eq!(
            rank_ids(
                &[],
                &[
                    make_vector_match("b.md", 0.0),
                    make_vector_match("b.md", 0.1),
                    make_vector_match("a.md", 0.2)
                ],
                9
            ),
            ["b.md", "a.md"]
        );
    }
    #[test]
    fn candidate_metadata_rejects_wrong_rows_and_search_hashes() {
        // Given: candidate rows with mismatched identities or search hashes.
        let root = tempfile::TempDir::new().unwrap();
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let mut unit = storage.begin_cache_write_tx().unwrap();
        let id = KnowledgeId::parse("a.md").unwrap();
        let node = NodeMetadata::Knowledge(KnowledgeMetadata {
            id: id.clone(),
            uri: id,
            kind: ElementKind::Vertex,
            hash: Sha256::digest(b"body"),
            created_at: 1,
            source_snapshot: None,
            lines: LineRange::new(1, 2).unwrap(),
            search_hash: Sha256::digest(b"search"),
        });
        let row = unit.upsert_node(&node).unwrap();
        unit.commit().unwrap();
        let mut read = storage.begin_read_tx().unwrap();
        let mut bad_row = make_text_match("a.md", 0.0);
        bad_row.row_id = row + 1;
        // When / Then: candidate metadata is retrieved; mismatched rows and hashes are rejected.
        assert!(get_candidate_metadata(read.as_mut(), &[bad_row], &[]).is_err());
        assert!(
            get_candidate_metadata(read.as_mut(), &[], &[make_vector_match("a.md", 0.0)]).is_err()
        );
        assert!(
            get_candidate_metadata(read.as_mut(), &[make_text_match("missing.md", 0.0)], &[])
                .is_err()
        );
        assert_eq!(read.get_revision(), 1);
    }
    #[test]
    fn rank_weight_checks_integer_conversion_and_preserves_reciprocals() {
        // Given / When / Then
        assert_eq!(calculate_rank_weight(NonZeroUsize::MIN).unwrap(), 1.0);
        assert_eq!(
            calculate_rank_weight(NonZeroUsize::new(4).unwrap()).unwrap(),
            0.25
        );
        assert_eq!(
            calculate_rank_weight(NonZeroUsize::MAX)
                .unwrap_err()
                .get_kind(),
            CommandErrorKind::InternalError
        );
    }
    #[test]
    fn captured_vector_configuration_requires_matching_identity_and_native_dimensions() {
        // Given: captured configuration and native vector dimensions.
        let root = tempfile::TempDir::new().unwrap();
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let config = build_config(2);
        let ready = ConfigState::Ready(config.clone());
        // When / Then: the vector channel configuration is captured; identity and native dimensions must agree.
        assert!(
            capture_vector_config(storage.begin_read_tx().unwrap().as_mut(), &ready)
                .unwrap()
                .is_none()
        );
        storage.reset_vectors(None, &config.get_identity()).unwrap();
        assert!(
            capture_vector_config(storage.begin_read_tx().unwrap().as_mut(), &ready)
                .unwrap()
                .is_none()
        );
        seed_dimension(&mut storage, &config, 2);
        let mut read = storage.begin_read_tx().unwrap();
        assert_eq!(
            capture_vector_config(read.as_mut(), &ready)
                .unwrap()
                .unwrap()
                .1
                .get(),
            2
        );
        for state in [
            ConfigState::Missing,
            ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
            ConfigState::Changed,
        ] {
            assert!(
                capture_vector_config(read.as_mut(), &state)
                    .unwrap()
                    .is_none()
            );
        }
        drop(read);
        let incompatible = build_config(3);
        seed_dimension(&mut storage, &incompatible, 2);
        assert!(
            capture_vector_config(
                storage.begin_read_tx().unwrap().as_mut(),
                &ConfigState::Ready(incompatible)
            )
            .unwrap()
            .is_none()
        );
    }
    #[test]
    fn malformed_query_vectors_and_embedding_infrastructure_errors_fail_closed() {
        // Given: malformed query vectors and embedding failures.
        let root = tempfile::TempDir::new().unwrap();
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let config = build_config(2);
        seed_dimension(&mut storage, &config, 2);
        let mut read = storage.begin_read_tx().unwrap();
        for (identity, vector) in [
            (Sha256::digest(b"wrong model"), vec![1.0, 0.0]),
            (config.get_identity(), vec![1.0]),
            (config.get_identity(), vec![f32::NAN, 0.0]),
        ] {
            // Given: the query provider returns a mismatched identity or invalid vector.
            let mut embeddings =
                build_query(Some(Ok(QueryEmbedding::Available { identity, vector })));
            // When: vector candidates are requested.
            let error = QueryCandidates {
                embeddings: &mut embeddings,
            }
            .find_vector_candidates(
                read.as_mut(),
                Some((&config, NonZeroUsize::new(2).unwrap())),
                "query",
            )
            .unwrap_err();
            // Then: invalid index data and infrastructure failures propagate.
            assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
            assert_eq!(error.get_message(), "invalid search index data");
        }
        // Given: the query provider returns an infrastructure error.
        let mut embeddings = build_query(Some(Err(EmbeddingError::new(
            EmbeddingErrorKind::InternalError,
        ))));
        // When / Then: vector candidates are requested; invalid index data and infrastructure failures propagate.
        assert!(
            QueryCandidates {
                embeddings: &mut embeddings
            }
            .find_vector_candidates(
                read.as_mut(),
                Some((&config, NonZeroUsize::new(2).unwrap())),
                "query"
            )
            .is_err()
        );
    }
    #[test]
    fn unavailable_or_changed_vector_channel_keeps_fts_fallback() {
        // Given: an unavailable or changed vector channel.
        let root = tempfile::TempDir::new().unwrap();
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let config = build_config(2);
        let mut read = storage.begin_read_tx().unwrap();
        let mut embeddings = build_query(Some(Ok(QueryEmbedding::Available {
            identity: config.get_identity(),
            vector: vec![1.0, 0.0],
        })));
        // When / Then: vector candidates are requested; the lexical fallback remains available.
        assert!(
            QueryCandidates {
                embeddings: &mut embeddings
            }
            .find_vector_candidates(
                read.as_mut(),
                Some((&config, NonZeroUsize::new(2).unwrap())),
                "query"
            )
            .unwrap()
            .is_empty()
        );
        // Given: the query provider reports unavailable embeddings.
        let mut unavailable = build_query(Some(Ok(QueryEmbedding::Unavailable)));
        // When / Then: vector candidates are requested; the lexical fallback remains available.
        assert!(
            QueryCandidates {
                embeddings: &mut unavailable
            }
            .find_vector_candidates(
                read.as_mut(),
                Some((&config, NonZeroUsize::new(2).unwrap())),
                "query"
            )
            .unwrap()
            .is_empty()
        );
        // Given: no configured vector channel is available.
        let mut unused = build_query(None);
        // When / Then: vector candidates are requested; the lexical fallback remains available.
        assert!(
            QueryCandidates {
                embeddings: &mut unused
            }
            .find_vector_candidates(read.as_mut(), None, "query")
            .unwrap()
            .is_empty()
        );
    }

    fn build_query(result: Option<EmbeddingResult<QueryEmbedding>>) -> MockQuery {
        let mut query = MockQuery::new();
        query.expect_get_configuration().times(0);
        query.expect_drain().times(0);
        query.expect_embed_query().times(0);
        if let Some(result) = result {
            query
                .expect_embed_query_with_config()
                .times(1)
                .withf(|request| {
                    request.text == "query" && request.dimensions == NonZeroUsize::new(2)
                })
                .return_once(move |_| result);
        } else {
            query.expect_embed_query_with_config().times(0);
        }
        query
    }
    fn build_config(dimensions: usize) -> EmbeddingConfig {
        EmbeddingConfig::parse(format!(r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","dimensions":{dimensions}}},"max_retries":0}}"#).as_bytes()).unwrap()
    }
    fn seed_dimension<S: Storage>(storage: &mut S, config: &EmbeddingConfig, dimensions: usize) {
        let observed = storage.begin_read_tx().unwrap().get_config_identity();
        storage
            .reset_vectors(observed.as_ref(), &config.get_identity())
            .unwrap();
        let VectorOutcome::Current(mut write) = storage
            .begin_vector_write_tx(&config.get_identity())
            .unwrap()
        else {
            panic!("identity")
        };
        write
            .store_vector(&Sha256::digest(b"seed"), &vec![1.0; dimensions], dimensions)
            .unwrap();
        write.commit().unwrap();
    }
    fn rank_ids(fts: &[TextMatch], vectors: &[VectorMatch], limit: usize) -> Vec<String> {
        rank_candidates(
            fts,
            vectors,
            &fts.iter()
                .map(|hit| hit.id.clone())
                .chain(vectors.iter().map(|hit| hit.id.clone()))
                .collect(),
            NonZeroUsize::new(limit).unwrap(),
        )
        .unwrap()
        .iter()
        .map(|id| id.as_str().to_owned())
        .collect()
    }
    fn make_text_match(id: &str, bm25: f64) -> TextMatch {
        TextMatch {
            id: KnowledgeId::parse(id).unwrap(),
            row_id: 1,
            bm25,
        }
    }
    fn make_vector_match(id: &str, distance: f64) -> VectorMatch {
        VectorMatch {
            id: KnowledgeId::parse(id).unwrap(),
            search_hash: Sha256::digest(b"shared"),
            distance,
        }
    }

    mod scoped {
        use crate::service::{ReadTransaction, VectorWrite};
        mod revisions {
            use super::{ProvenanceFailure, ScopedFixture, build_filtered_request, open_storage};
            use crate::{
                domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri, TreeOid},
                service::{CommandErrorKind, NodeMetadata, Searcher, Storage},
            };
            use std::path::Path;

            #[test]
            fn revision_cache_keeps_demand_order_and_resets_between_searches() {
                // Given: two candidates share one exact basis/source/revision comparison.
                let fixture = ScopedFixture::new();
                add_second_candidate(fixture.root.path(), 'a');
                let mut search = fixture.build_search(None, None);
                // When: the same service executes two searches.
                let first = search.search(&build_filtered_request()).unwrap();
                fixture.reads.set(0);
                let second = search.search(&build_filtered_request()).unwrap();
                // Then: comparison is shared only within each search; every live binding is observed.
                assert_eq!(first.len(), 2);
                assert_eq!(second.len(), 2);
                assert_eq!(
                    *search.provenance.events.borrow(),
                    [
                        "validate", "compare", "observe", "observe", "validate", "compare",
                        "observe", "observe"
                    ]
                );
            }

            #[test]
            fn first_observation_failure_precedes_later_candidate_comparison() {
                // Given: two candidates and a first-observation failure.
                let fixture = ScopedFixture::new();
                add_second_candidate(fixture.root.path(), 'b');
                let mut search = fixture.build_search(None, Some(ProvenanceFailure::Observation));
                // When: the first candidate cannot be observed.
                let error = search.search(&build_filtered_request()).unwrap_err();
                // Then: no later candidate comparison or observation is demanded.
                assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
                assert_eq!(
                    *search.provenance.events.borrow(),
                    ["validate", "compare", "observe"]
                );
            }

            fn add_second_candidate(root: &Path, basis: char) {
                let mut storage = open_storage(root);
                let mut read = storage.begin_read_tx().unwrap();
                let stored = read
                    .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
                    .unwrap()
                    .unwrap();
                drop(read);
                let NodeMetadata::Knowledge(mut node) = stored.metadata else {
                    panic!("knowledge fixture")
                };
                let id = KnowledgeId::parse("b.md").unwrap();
                node.source_snapshot = Some(TreeOid::parse(&basis.to_string().repeat(40)).unwrap());
                node.id = id.clone();
                node.uri = id.clone();
                let mut write = storage.begin_cache_write_tx().unwrap();
                write.upsert_node(&NodeMetadata::Knowledge(node)).unwrap();
                write.replace_text(&id, "query").unwrap();
                write
                    .insert_edge(
                        &Edge::new(
                            id,
                            EdgeTarget::Source(SourceUri::parse("old.rs").unwrap()),
                            EdgeType::parse("external").unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
                write.commit().unwrap();
            }
        }

        use super::{build_config, build_noop_refresher, seed_dimension};
        use crate::{
            domain::{
                Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri,
                TreeOid,
            },
            filesystem::LocalFileSystem,
            markdown::MarkdownParser,
            service::{
                CacheWrite, CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind,
                DocumentResult, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind,
                EmbeddingResult, KnowledgeMetadata, MockRefresher, NodeMetadata,
                ProvenanceObservation, ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest,
                RecheckFilter, SearchFilters, SearchFlag, SearchRequest, Searcher, SnapshotError,
                SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, Storage,
                StorageResult, VectorOutcome, search::SearchService,
            },
            storage::SqliteStorage,
        };
        use std::{
            cell::{Cell, RefCell},
            collections::BTreeSet,
            num::NonZeroUsize,
            path::Path,
            rc::Rc,
        };

        struct Query {
            config: EmbeddingConfig,
            state: Option<ConfigState>,
            calls: Rc<Cell<usize>>,
            hook: Option<Box<dyn FnOnce()>>,
        }
        type RecordedSources = Vec<(Option<TreeOid>, BTreeSet<SourceUri>)>;
        #[derive(Clone, Copy)]
        enum ProvenanceFailure {
            Observation,
            Comparison,
        }
        struct Observations {
            events: RefCell<Vec<String>>,
            observed: Rc<RefCell<RecordedSources>>,
            failure: Option<ProvenanceFailure>,
        }
        struct ReadOnlyStorage {
            inner: SqliteStorage,
            reads: Rc<Cell<usize>>,
        }

        type ScopedSearch =
            SearchService<MockRefresher, Query, LocalFileSystem, MarkdownParser, Observations>;
        struct ScopedFixture {
            root: tempfile::TempDir,
            config: EmbeddingConfig,
            calls: Rc<Cell<usize>>,
            reads: Rc<Cell<usize>>,
            observed: Rc<RefCell<RecordedSources>>,
        }

        #[test]
        fn non_ready_configuration_preserves_fts_and_one_pinned_consumer_view() {
            for state in [
                ConfigState::Missing,
                ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
                ConfigState::Changed,
            ] {
                // Given: vectors exist, but readiness must still disable query inference.
                let fixture = ScopedFixture::new();
                let mut search = fixture.build_search(None, None);
                search.embeddings.state = Some(state);
                // When
                let links = search.search(&build_request()).unwrap();
                // Then
                assert_eq!(links.len(), 1);
                assert_eq!(links[0].element_id.as_str(), "a.md");
                assert_eq!(fixture.calls.get(), 0);
                assert_eq!(fixture.reads.get(), 1);
                assert_can_checkpoint(fixture.root.path());
            }
        }

        #[test]
        fn metadata_failure_after_query_releases_pinned_view() {
            // Given: a metadata failure after query embedding.
            let fixture = ScopedFixture::new();
            let root = fixture.root.path();
            let connection = rusqlite::Connection::open(root.join("cache.db")).unwrap();
            assert_eq!(
                connection
                    .execute("update nodes set created_at = -1 where uri = ?1", ["a.md"])
                    .unwrap(),
                1
            );
            let mut search = fixture.build_search(None, None);

            // When: search executes the query.
            let error = search.search(&build_request()).unwrap_err();

            // Then: the pinned view is released after failure.
            assert_eq!(fixture.calls.get(), 1);
            assert_eq!(fixture.reads.get(), 1);
            assert!(fixture.observed.borrow().is_empty());
            assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
            assert_eq!(error.get_message(), "cannot search knowledge");
            assert!(std::error::Error::source(&error).is_some());
            assert!(!format!("{error:?} {error}").contains("created_at"));
            // Keep the service alive: returning the error itself must release its reader.
            assert_can_checkpoint(root);
            assert_eq!(
                connection
                    .execute("update nodes set created_at = 1 where uri = ?1", ["a.md"])
                    .unwrap(),
                1
            );
            publish_candidate(&mut open_storage(root), "b", "new.rs");
            assert_eq!(
                open_storage(root).begin_read_tx().unwrap().get_revision(),
                2
            );
            assert_can_checkpoint(root);
            drop(search);
        }

        #[test]
        fn search_observes_metadata_and_bindings_from_original_pinned_view() {
            // Given: an original publication and a writer changing metadata.
            let fixture = ScopedFixture::new();
            let root = fixture.root.path();
            let mut writer = open_storage(root);
            let mut search = fixture.build_search(
                Some(Box::new(move || {
                    publish_candidate(&mut writer, "b", "new.rs");
                })),
                None,
            );

            // When: filtered search observes the pinned view.
            let links = search.search(&build_filtered_request()).unwrap();

            // Then: metadata and bindings retain the original revision.
            assert_eq!(fixture.calls.get(), 1);
            assert_eq!(fixture.reads.get(), 1);
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].revision, 1);
            assert_eq!(
                links[0].flags,
                BTreeSet::from([SearchFlag::Stale, SearchFlag::SourceChanged])
            );
            assert_eq!(
                *fixture.observed.borrow(),
                vec![(
                    Some(TreeOid::parse(&"a".repeat(40)).unwrap()),
                    BTreeSet::from([SourceUri::parse("old.rs").unwrap()])
                )]
            );
            assert_eq!(
                open_storage(root).begin_read_tx().unwrap().get_revision(),
                2
            );
            assert_can_checkpoint(root);
            drop(search);
        }

        #[test]
        fn late_provenance_failure_releases_pinned_view_while_service_stays_alive() {
            // Given: late source-observation or provenance failures.
            for failure in [
                ProvenanceFailure::Observation,
                ProvenanceFailure::Comparison,
            ] {
                let fixture = ScopedFixture::new();
                let root = fixture.root.path();
                let mut search = fixture.build_search(None, Some(failure));

                // When: filtered search reaches the failing observation.
                let error = search.search(&build_filtered_request()).unwrap_err();

                // Then: the pinned view is released while the service remains alive.
                assert_eq!(fixture.calls.get(), 1);
                assert_eq!(fixture.reads.get(), 1);
                assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
                assert!(std::error::Error::source(&error).is_some());
                assert!(!format!("{error:?} {error}").contains("PRIVATE"));
                assert_can_checkpoint(root);
                publish_candidate(&mut open_storage(root), "b", "new.rs");
                assert_eq!(
                    open_storage(root).begin_read_tx().unwrap().get_revision(),
                    2
                );
                assert_can_checkpoint(root);
                // Ownership must release the read before the still-live service is dropped.
                drop(search);
            }
        }

        impl ScopedFixture {
            fn new() -> Self {
                let root = tempfile::tempdir().unwrap();
                let mut storage = open_storage(root.path());
                publish_candidate(&mut storage, "a", "old.rs");
                let config = build_config(2);
                seed_dimension(&mut storage, &config, 2);
                Self {
                    root,
                    config,
                    calls: Rc::new(Cell::new(0)),
                    reads: Rc::new(Cell::new(0)),
                    observed: Rc::new(RefCell::new(Vec::new())),
                }
            }
            fn build_search(
                &self,
                hook: Option<Box<dyn FnOnce()>>,
                failure: Option<ProvenanceFailure>,
            ) -> ScopedSearch {
                SearchService {
                    refresher: build_noop_refresher(),
                    storage: Box::new(ReadOnlyStorage {
                        inner: open_storage(self.root.path()),
                        reads: self.reads.clone(),
                    }),
                    embeddings: Query {
                        config: self.config.clone(),
                        state: None,
                        calls: self.calls.clone(),
                        hook,
                    },
                    filesystem: LocalFileSystem::open(self.root.path()).unwrap(),
                    markdown: MarkdownParser::new(),
                    provenance: Observations {
                        events: RefCell::new(Vec::new()),
                        observed: self.observed.clone(),
                        failure,
                    },
                }
            }
        }

        impl Embedder for Query {
            fn get_configuration(&mut self) -> EmbeddingResult<ConfigState> {
                Ok(self
                    .state
                    .take()
                    .unwrap_or_else(|| ConfigState::Ready(self.config.clone())))
            }
            fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
                panic!("search cannot drain")
            }
            fn embed_query(&mut self, _: &str) -> EmbeddingResult<QueryEmbedding> {
                panic!("use captured configuration")
            }
            fn embed_query_with_config(
                &mut self,
                request: QueryEmbeddingRequest<'_>,
            ) -> EmbeddingResult<QueryEmbedding> {
                assert_eq!(request.text, "query");
                assert_eq!(request.config.get_identity(), self.config.get_identity());
                self.calls.set(self.calls.get() + 1);
                if let Some(hook) = self.hook.take() {
                    hook();
                }
                Ok(QueryEmbedding::Available {
                    identity: self.config.get_identity(),
                    vector: vec![1.0, 0.0],
                })
            }
        }
        impl SourceProvenance for Observations {
            fn validate_revisions(
                &self,
                revisions: &BTreeSet<TreeOid>,
            ) -> Result<(), SnapshotError> {
                self.events.borrow_mut().push("validate".into());
                assert!(revisions.is_empty() || *revisions == BTreeSet::from([build_revision()]));
                Ok(())
            }
            fn observe_sources(
                &self,
                request: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                self.events.borrow_mut().push("observe".into());
                if matches!(self.failure, Some(ProvenanceFailure::Observation)) {
                    return Err(DocumentError::new_with_cause(
                        DocumentErrorKind::InternalError,
                        "cannot observe sources",
                        std::io::Error::other("PRIVATE provenance payload"),
                    ));
                }
                self.observed
                    .borrow_mut()
                    .push((request.source_snapshot.cloned(), request.sources.clone()));
                Ok(ProvenanceObservation {
                    flags: BTreeSet::from([SearchFlag::SourceChanged]),
                    issues: BTreeSet::new(),
                })
            }
            fn compare_revision(
                &self,
                request: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                self.events.borrow_mut().push("compare".into());
                assert_eq!(
                    request.source_snapshot,
                    Some(&TreeOid::parse(&"a".repeat(40)).unwrap())
                );
                assert_eq!(request.source.as_str(), "old.rs");
                assert_eq!(request.revision, &build_revision());
                if matches!(self.failure, Some(ProvenanceFailure::Comparison)) {
                    return Err(SnapshotError::new_with_cause(
                        "cannot compare sources",
                        std::io::Error::other("PRIVATE comparison payload"),
                    ));
                }
                Ok(SourceRevisionComparison::Equal)
            }
        }
        impl Storage for ReadOnlyStorage {
            fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>> {
                self.reads.set(self.reads.get() + 1);
                assert_eq!(self.reads.get(), 1, "search opened a second view");
                self.inner.begin_read_tx()
            }
            fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>> {
                panic!("search cannot publish")
            }
            fn begin_vector_write_tx(
                &mut self,
                _: &Sha256,
            ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>> {
                panic!("search cannot write vectors")
            }
            fn reset_vectors(
                &mut self,
                _: Option<&Sha256>,
                _: &Sha256,
            ) -> StorageResult<VectorOutcome<bool>> {
                panic!("search cannot reset vectors")
            }
        }
        fn open_storage(root: &Path) -> SqliteStorage {
            SqliteStorage::open(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
        }
        fn publish_candidate(storage: &mut SqliteStorage, oid: &str, source: &str) {
            let id = KnowledgeId::parse("a.md").unwrap();
            let mut unit = storage.begin_cache_write_tx().unwrap();
            unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
                id: id.clone(),
                uri: id.clone(),
                kind: ElementKind::Vertex,
                hash: Sha256::digest(b"body"),
                created_at: 1,
                source_snapshot: Some(TreeOid::parse(&oid.repeat(40)).unwrap()),
                lines: LineRange::new(1, 2).unwrap(),
                search_hash: Sha256::digest(b"query"),
            }))
            .unwrap();
            unit.replace_text(&id, "query").unwrap();
            unit.delete_outgoing(&id).unwrap();
            unit.insert_edge(
                &Edge::new(
                    id,
                    EdgeTarget::Source(SourceUri::parse(source).unwrap()),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            unit.commit().unwrap();
        }
        fn build_filtered_request() -> SearchRequest {
            SearchRequest {
                filters: SearchFilters {
                    needs_recheck: RecheckFilter::Required,
                    sources: BTreeSet::from([SourceUri::parse("old.rs").unwrap()]),
                    source_revisions: BTreeSet::from([build_revision()]),
                },
                ..build_request()
            }
        }
        fn build_request() -> SearchRequest {
            SearchRequest {
                filters: Default::default(),
                query: "query".into(),
                limit: NonZeroUsize::new(10).unwrap(),
            }
        }
        fn build_revision() -> TreeOid {
            TreeOid::parse(&"c".repeat(40)).unwrap()
        }
        fn assert_can_checkpoint(root: &Path) {
            for name in ["cache.db", "vectors.db"] {
                let connection = rusqlite::Connection::open(root.join(name)).unwrap();
                let busy: i64 = connection
                    .query_row("pragma wal_checkpoint(truncate)", [], |row| row.get(0))
                    .unwrap();
                assert_eq!(busy, 0, "leaked read prevented checkpoint of {name}");
            }
        }
    }

    mod filters {
        use super::{
            build_config, build_noop_refresher, make_text_match, make_vector_match, seed_dimension,
        };
        use crate::{
            domain::{
                Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, RepoPath, Sha256, SourceUri,
                TreeOid,
            },
            filesystem::LocalFileSystem,
            markdown::MarkdownParser,
            service::{
                CommandErrorKind, ConfigState, DocumentResult, DrainOutcome, Embedder,
                EmbeddingResult, KnowledgeMetadata, MarkdownCodec, MockRefresher, NodeMetadata,
                ProvenanceObservation, ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest,
                RecheckFilter, SearchFilters, SearchFlag, SearchRequest, Searcher, SnapshotError,
                SnapshotErrorKind, SourceProvenance, SourceRevisionComparison,
                SourceRevisionRequest, Storage, TextMatch, VectorMatch,
                search::{SearchService, ranking::rank_candidates},
            },
            storage::SqliteStorage,
        };
        use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, path::Path, rc::Rc};

        #[derive(Clone, Copy)]
        enum Candidate {
            Present,
            Absent,
        }
        struct Fixture {
            search: TestSearch,
            queries: Rc<Cell<usize>>,
            validations: Rc<Cell<usize>>,
            observations: Rc<Cell<usize>>,
        }
        struct Query(Rc<Cell<usize>>);
        struct Probe {
            failure: Option<SnapshotErrorKind>,
            compare_failure: Option<SnapshotErrorKind>,
            observations: Rc<Cell<usize>>,
            validations: Rc<Cell<usize>>,
        }
        type TestSearch =
            SearchService<MockRefresher, Query, LocalFileSystem, MarkdownParser, Probe>;

        #[test]
        fn eligible_channel_ranks_are_recomputed_before_fusion() {
            // Given
            let fts = [
                make_text_match("excluded.md", -3.0),
                make_text_match("a.md", -2.0),
                make_text_match("b.md", -1.0),
            ];
            let vectors = [
                make_vector_match("b.md", 0.0),
                make_vector_match("a.md", 1.0),
            ];
            // When / Then
            assert_eq!(
                rank_filtered(&fts, &vectors, &["a.md", "b.md"], 2),
                ["a.md", "b.md"]
            );
        }

        #[test]
        fn excluded_channel_heads_do_not_consume_limit() {
            // Given
            let fts = [
                make_text_match("excluded.md", -3.0),
                make_text_match("a.md", -2.0),
            ];
            let vectors = [
                make_vector_match("excluded.md", 0.0),
                make_vector_match("b.md", 1.0),
            ];
            // When / Then
            assert_eq!(rank_filtered(&fts, &[], &["a.md"], 1), ["a.md"]);
            assert_eq!(rank_filtered(&[], &vectors, &["b.md"], 1), ["b.md"]);
        }

        #[test]
        fn eligible_ties_and_duplicates_keep_identity_order() {
            // Given
            let fts = [
                make_text_match("excluded.md", -3.0),
                make_text_match("b.md", -2.0),
                make_text_match("a.md", -2.0),
                make_text_match("a.md", -1.0),
            ];
            // When / Then
            assert_eq!(
                rank_filtered(&fts, &[], &["a.md", "b.md"], 2),
                ["a.md", "b.md"]
            );
            // Given
            let vectors = [
                make_vector_match("b.md", 0.0),
                make_vector_match("b.md", 1.0),
                make_vector_match("a.md", 0.0),
            ];
            // When / Then
            assert_eq!(
                rank_filtered(&[], &vectors, &["a.md", "b.md"], 2),
                ["a.md", "b.md"]
            );
        }

        #[test]
        fn excluded_nonfinite_metrics_remain_integrity_errors() {
            // Given
            for metric in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                for (fts, vectors) in [
                    (vec![make_text_match("excluded.md", metric)], vec![]),
                    (vec![], vec![make_vector_match("excluded.md", metric)]),
                ] {
                    // When / Then
                    assert_eq!(
                        rank_candidates(&fts, &vectors, &BTreeSet::new(), NonZeroUsize::MIN)
                            .unwrap_err()
                            .get_kind(),
                        CommandErrorKind::InternalError
                    );
                }
            }
        }

        #[test]
        fn revision_preflight_precedes_provider_and_candidate_dependent_exits() {
            // Given
            for (snapshot, command) in [
                (SnapshotErrorKind::NotFound, CommandErrorKind::NotFound),
                (
                    SnapshotErrorKind::InvalidRequest,
                    CommandErrorKind::InvalidRequest,
                ),
                (
                    SnapshotErrorKind::InternalError,
                    CommandErrorKind::InternalError,
                ),
            ] {
                for (query, candidate, unmatched) in [
                    ("", Candidate::Present, false),
                    (" * : ", Candidate::Present, false),
                    ("absent", Candidate::Present, false),
                    ("query", Candidate::Absent, false),
                    ("query", Candidate::Present, true),
                ] {
                    let root = tempfile::tempdir().unwrap();
                    let Fixture {
                        mut search,
                        queries,
                        validations,
                        ..
                    } = build_search(root.path(), candidate, Some(snapshot));
                    let mut request = build_request(query);
                    request.filters.source_revisions =
                        BTreeSet::from([parse_tree("1"), parse_tree("f")]);
                    if unmatched {
                        request
                            .filters
                            .sources
                            .insert(SourceUri::parse("missing.rs").unwrap());
                    }

                    // When
                    let result = search.search(&request);

                    // Then
                    assert_eq!(result.unwrap_err().get_kind(), command, "{query:?}");
                    assert_eq!(
                        queries.get(),
                        0,
                        "provider/config invoked before root validation"
                    );
                    assert_eq!(validations.get(), 1);
                }
            }
        }

        #[test]
        fn revision_comparison_errors_preserve_typed_codes() {
            // Given
            for (snapshot, command) in [
                (SnapshotErrorKind::NotFound, CommandErrorKind::NotFound),
                (
                    SnapshotErrorKind::InvalidRequest,
                    CommandErrorKind::InvalidRequest,
                ),
                (
                    SnapshotErrorKind::InternalError,
                    CommandErrorKind::InternalError,
                ),
            ] {
                let root = tempfile::tempdir().unwrap();
                let Fixture { mut search, .. } =
                    build_search(root.path(), Candidate::Present, None);
                search.provenance.compare_failure = Some(snapshot);
                let mut request = build_request("query");
                request.filters.source_revisions.insert(parse_tree("1"));

                // When / Then
                assert_eq!(search.search(&request).unwrap_err().get_kind(), command);
            }
        }

        #[test]
        fn changing_observer_is_called_once_and_retained_flags_drive_rendering() {
            // Given
            let root = tempfile::tempdir().unwrap();
            let Fixture {
                mut search,
                observations,
                ..
            } = build_search(root.path(), Candidate::Present, None);
            let mut request = build_request("query");
            request.filters.needs_recheck = RecheckFilter::Required;

            // When
            let links = search.search(&request).unwrap();

            // Then
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].element_id.as_str(), "note.md");
            assert_eq!(links[0].flags, BTreeSet::from([SearchFlag::SourceChanged]));
            assert_eq!(observations.get(), 1);
            assert!(search.search(&request).unwrap().is_empty());
            assert_eq!(observations.get(), 2);
            request.filters.needs_recheck = RecheckFilter::NotRequired;
            assert_eq!(search.search(&request).unwrap().len(), 1);
            assert_eq!(observations.get(), 3);
        }

        impl Embedder for Query {
            fn get_configuration(&mut self) -> EmbeddingResult<ConfigState> {
                self.0.set(self.0.get() + 1);
                Ok(ConfigState::Ready(build_config(2)))
            }
            fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
                panic!("no drain")
            }
            fn embed_query(&mut self, _: &str) -> EmbeddingResult<QueryEmbedding> {
                self.0.set(self.0.get() + 1);
                Ok(QueryEmbedding::Unavailable)
            }
            fn embed_query_with_config(
                &mut self,
                _: QueryEmbeddingRequest<'_>,
            ) -> EmbeddingResult<QueryEmbedding> {
                self.0.set(self.0.get() + 1);
                Ok(QueryEmbedding::Unavailable)
            }
        }
        impl SourceProvenance for Probe {
            fn validate_revisions(
                &self,
                revisions: &BTreeSet<TreeOid>,
            ) -> Result<(), SnapshotError> {
                self.validations.set(self.validations.get() + 1);
                assert!(
                    revisions.is_empty()
                        || revisions == &BTreeSet::from([parse_tree("1"), parse_tree("f")])
                        || revisions == &BTreeSet::from([parse_tree("1")])
                );
                if let Some(kind) = self.failure {
                    return Err(SnapshotError::new(kind, "cannot validate requested tree"));
                }
                Ok(())
            }
            fn observe_sources(
                &self,
                _: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                self.observations.set(self.observations.get() + 1);
                let flags = if self.observations.get() == 1 {
                    BTreeSet::from([SearchFlag::SourceChanged])
                } else {
                    BTreeSet::new()
                };
                Ok(ProvenanceObservation {
                    flags,
                    issues: BTreeSet::new(),
                })
            }
            fn compare_revision(
                &self,
                _: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                if let Some(kind) = self.compare_failure {
                    return Err(SnapshotError::new(kind, "cannot compare requested tree"));
                }
                Ok(SourceRevisionComparison::Equal)
            }
        }
        fn build_search(
            root: &Path,
            candidate: Candidate,
            failure: Option<SnapshotErrorKind>,
        ) -> Fixture {
            let mut storage =
                SqliteStorage::open(&root.join("cache.db"), &root.join("vectors.db")).unwrap();
            if matches!(candidate, Candidate::Present) {
                publish_note(root, &mut storage);
            }
            seed_dimension(&mut storage, &build_config(2), 2);
            let queries = Rc::new(Cell::new(0));
            let validations = Rc::new(Cell::new(0));
            let observations = Rc::new(Cell::new(0));
            let search = SearchService {
                refresher: build_noop_refresher(),
                storage: Box::new(storage),
                embeddings: Query(queries.clone()),
                filesystem: LocalFileSystem::open(root).unwrap(),
                markdown: MarkdownParser::new(),
                provenance: Probe {
                    failure,
                    compare_failure: None,
                    observations: observations.clone(),
                    validations: validations.clone(),
                },
            };
            Fixture {
                search,
                queries,
                validations,
                observations,
            }
        }
        fn publish_note(root: &Path, storage: &mut SqliteStorage) {
            let bytes = b"<!-- letopis: {\"created_at\":1} -->\nquery\n";
            std::fs::write(root.join("note.md"), bytes).unwrap();
            let parsed = MarkdownParser::new()
                .parse_file(&RepoPath::parse("note.md").unwrap(), bytes)
                .unwrap()
                .unwrap();
            let element = &parsed.elements[0].element;
            let id = KnowledgeId::parse("note.md").unwrap();
            let mut unit = storage.begin_cache_write_tx().unwrap();
            unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
                id: id.clone(),
                uri: id.clone(),
                kind: ElementKind::Vertex,
                hash: element.hash,
                created_at: 1,
                source_snapshot: Some(parse_tree("1")),
                lines: element.lines,
                search_hash: Sha256::digest(b"query"),
            }))
            .unwrap();
            unit.replace_text(&id, "query").unwrap();
            unit.insert_edge(
                &Edge::new(
                    id,
                    EdgeTarget::Source(SourceUri::parse("code.rs").unwrap()),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            unit.commit().unwrap();
        }
        fn build_request(query: &str) -> SearchRequest {
            SearchRequest {
                filters: SearchFilters::default(),
                query: query.into(),
                limit: NonZeroUsize::new(10).unwrap(),
            }
        }
        fn parse_tree(digit: &str) -> TreeOid {
            TreeOid::parse(&digit.repeat(40)).unwrap()
        }
        fn rank_filtered(
            fts: &[TextMatch],
            vectors: &[VectorMatch],
            eligible: &[&str],
            limit: usize,
        ) -> Vec<String> {
            rank_candidates(
                fts,
                vectors,
                &eligible
                    .iter()
                    .map(|id| KnowledgeId::parse(id).unwrap())
                    .collect(),
                NonZeroUsize::new(limit).unwrap(),
            )
            .unwrap()
            .into_iter()
            .map(|id| id.as_str().to_owned())
            .collect()
        }
    }

    fn build_noop_refresher() -> MockRefresher {
        let mut refresher = MockRefresher::new();
        refresher.expect_reindex().times(0);
        refresher.expect_refresh().returning(|| {
            Ok(RefreshOutcome {
                revision: 1,
                published: false,
                files_updated: 0,
                files_removed: 0,
                invalid_files_updated: 0,
            })
        });
        refresher
    }
}
