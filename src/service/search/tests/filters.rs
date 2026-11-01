use super::{
    build_config, build_noop_refresher, make_text_match, make_vector_match, seed_dimension,
};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid,
    },
    filesystem::LocalFileSystem,
    markdown::MarkdownParser,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DocumentResult, DrainOutcome, Embedder,
        EmbeddingResult, KnowledgeMetadata, MarkdownCodec, MockRefresher, NodeMetadata,
        ProvenanceObservation, ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest,
        RecheckFilter, SearchFilters, SearchFlag, SearchRequest, Searcher, SnapshotError,
        SnapshotErrorKind, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
        Storage, TextMatch, VectorMatch,
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
    SearchService<MockRefresher, SqliteStorage, Query, LocalFileSystem, MarkdownParser, Probe>;

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
            request.filters.source_revisions = BTreeSet::from([parse_tree("1"), parse_tree("f")]);
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
        let Fixture { mut search, .. } = build_search(root.path(), Candidate::Present, None);
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
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
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
    fn observe_sources(&self, _: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation> {
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
fn build_search(root: &Path, candidate: Candidate, failure: Option<SnapshotErrorKind>) -> Fixture {
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
        storage,
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
