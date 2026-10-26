use super::{
    build_fts_expression, capture_vector_config, find_vector_candidates,
    ranking::{calculate_rank_weight, rank_candidates},
    read_candidate_metadata,
};
use crate::{
    domain::{ElementKind, KnowledgeId, LineRange, Sha256},
    open_storage,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DrainOutcome, Embedder, EmbeddingConfig,
        EmbeddingError, EmbeddingErrorKind, EmbeddingResult, KnowledgeMetadata, NodeMetadata,
        QueryEmbedding, QueryEmbeddingRequest, Storage, StorageRead, TextMatch, VectorMatch,
        VectorOutcome, VectorWrite,
    },
};
use std::{collections::BTreeSet, num::NonZeroUsize};
struct FixedQuery(Option<EmbeddingResult<QueryEmbedding>>);

#[test]
fn literal_query_quotes_unicode_tokens_without_fts_operators() {
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
        assert_eq!(build_fts_expression(query).as_deref(), expected);
    }
}
#[test]
fn signed_zero_metrics_tie_by_identity_in_both_channels() {
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
    let fts = [make_text_match("a.md", -2.0), make_text_match("b.md", -1.0)];
    let vectors = [
        make_vector_match("c.md", 0.0),
        make_vector_match("b.md", 0.5),
        make_vector_match("a.md", 1.0),
    ];
    for (limit, expected) in [(3, vec!["a.md", "b.md", "c.md"]), (2, vec!["a.md", "b.md"])] {
        assert_eq!(rank_ids(&fts, &vectors, limit), expected);
    }
}
#[test]
fn ranking_orders_ties_deduplicates_and_keeps_disjoint_channels() {
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
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for (fts, vectors) in [
            (vec![make_text_match("a.md", value)], vec![]),
            (vec![], vec![make_vector_match("a.md", value)]),
        ] {
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
    let fts = [make_text_match("z.md", -2.0), make_text_match("a.md", -1.0)];
    let vectors = [
        make_vector_match("y.md", 0.0),
        make_vector_match("a.md", 1.0),
    ];
    assert_eq!(rank_ids(&fts, &vectors, 1), ["a.md"]);
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
    let root = tempfile::TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let mut unit = storage.begin_cache_write().unwrap();
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
    let mut read = storage.begin_read().unwrap();
    let mut bad_row = make_text_match("a.md", 0.0);
    bad_row.row_id = row + 1;
    assert!(read_candidate_metadata(&mut read, &[bad_row], &[]).is_err());
    assert!(read_candidate_metadata(&mut read, &[], &[make_vector_match("a.md", 0.0)]).is_err());
    assert!(
        read_candidate_metadata(&mut read, &[make_text_match("missing.md", 0.0)], &[]).is_err()
    );
    assert_eq!(read.get_revision(), 1);
}
#[test]
fn rank_weight_checks_integer_conversion_and_preserves_reciprocals() {
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
    let root = tempfile::TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    let ready = ConfigState::Ready(config.clone());
    assert!(
        capture_vector_config(&mut storage.begin_read().unwrap(), &ready)
            .unwrap()
            .is_none()
    );
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    assert!(
        capture_vector_config(&mut storage.begin_read().unwrap(), &ready)
            .unwrap()
            .is_none()
    );
    seed_dimension(&mut storage, &config, 2);
    let mut read = storage.begin_read().unwrap();
    assert_eq!(
        capture_vector_config(&mut read, &ready)
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
        assert!(capture_vector_config(&mut read, &state).unwrap().is_none());
    }
    drop(read);
    let incompatible = build_config(3);
    seed_dimension(&mut storage, &incompatible, 2);
    assert!(
        capture_vector_config(
            &mut storage.begin_read().unwrap(),
            &ConfigState::Ready(incompatible)
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn malformed_query_vectors_and_embedding_infrastructure_errors_fail_closed() {
    let root = tempfile::TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    seed_dimension(&mut storage, &config, 2);
    let mut read = storage.begin_read().unwrap();
    for (identity, vector) in [
        (Sha256::digest(b"wrong model"), vec![1.0, 0.0]),
        (config.get_identity(), vec![1.0]),
        (config.get_identity(), vec![f32::NAN, 0.0]),
    ] {
        let mut embeddings = FixedQuery(Some(Ok(QueryEmbedding::Available { identity, vector })));
        let error = find_vector_candidates(
            &mut read,
            &mut embeddings,
            Some((&config, NonZeroUsize::new(2).unwrap())),
            "query",
        )
        .unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "invalid search index data");
    }
    let mut embeddings = FixedQuery(Some(Err(EmbeddingError::new(
        EmbeddingErrorKind::InternalError,
    ))));
    assert!(
        find_vector_candidates(
            &mut read,
            &mut embeddings,
            Some((&config, NonZeroUsize::new(2).unwrap())),
            "query"
        )
        .is_err()
    );
}
#[test]
fn unavailable_or_changed_vector_channel_keeps_fts_fallback() {
    let root = tempfile::TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    let mut read = storage.begin_read().unwrap();
    let mut embeddings = FixedQuery(Some(Ok(QueryEmbedding::Available {
        identity: config.get_identity(),
        vector: vec![1.0, 0.0],
    })));
    assert!(
        find_vector_candidates(
            &mut read,
            &mut embeddings,
            Some((&config, NonZeroUsize::new(2).unwrap())),
            "query"
        )
        .unwrap()
        .is_empty()
    );
    let mut unavailable = FixedQuery(Some(Ok(QueryEmbedding::Unavailable)));
    assert!(
        find_vector_candidates(
            &mut read,
            &mut unavailable,
            Some((&config, NonZeroUsize::new(2).unwrap())),
            "query"
        )
        .unwrap()
        .is_empty()
    );
    let mut unused = FixedQuery(None);
    assert!(
        find_vector_candidates(&mut read, &mut unused, None, "query")
            .unwrap()
            .is_empty()
    );
}

impl Embedder for FixedQuery {
    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        panic!("must use captured configuration")
    }
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
        panic!("queries do not drain documents")
    }
    fn embed_query(&mut self, _: &str) -> EmbeddingResult<QueryEmbedding> {
        panic!("must use captured query API")
    }
    fn embed_query_with_config(
        &mut self,
        request: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding> {
        assert_eq!(request.text, "query");
        assert_eq!(request.dimensions, NonZeroUsize::new(2));
        self.0.take().expect("unexpected inference")
    }
}
fn build_config(dimensions: usize) -> EmbeddingConfig {
    EmbeddingConfig::parse(format!(r#"{{"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://localhost/api","dimensions":{dimensions}}},"max_retries":0}}"#).as_bytes()).unwrap()
}
fn seed_dimension<S: Storage>(storage: &mut S, config: &EmbeddingConfig, dimensions: usize) {
    let observed = storage.begin_read().unwrap().get_config_identity();
    storage
        .reset_vectors(observed.as_ref(), &config.get_identity())
        .unwrap();
    let VectorOutcome::Current(mut write) =
        storage.begin_vector_write(&config.get_identity()).unwrap()
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

mod scoped;

mod filters;
