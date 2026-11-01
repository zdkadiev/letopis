use super::{
    build_fts_expression, capture_vector_config, find_vector_candidates, get_candidate_metadata,
    ranking::{calculate_rank_weight, rank_candidates},
};
use crate::{
    domain::{ElementKind, KnowledgeId, LineRange, Sha256},
    open_storage,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DrainOutcome, Embedder, EmbeddingConfig,
        EmbeddingError, EmbeddingErrorKind, EmbeddingResult, KnowledgeMetadata, MockRefresher,
        NodeMetadata, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, Storage, StorageRead,
        TextMatch, VectorMatch, VectorOutcome, VectorWrite,
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
    assert!(get_candidate_metadata(&mut read, &[bad_row], &[]).is_err());
    assert!(get_candidate_metadata(&mut read, &[], &[make_vector_match("a.md", 0.0)]).is_err());
    assert!(get_candidate_metadata(&mut read, &[make_text_match("missing.md", 0.0)], &[]).is_err());
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
        capture_vector_config(&mut storage.begin_read_tx().unwrap(), &ready)
            .unwrap()
            .is_none()
    );
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    assert!(
        capture_vector_config(&mut storage.begin_read_tx().unwrap(), &ready)
            .unwrap()
            .is_none()
    );
    seed_dimension(&mut storage, &config, 2);
    let mut read = storage.begin_read_tx().unwrap();
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
            &mut storage.begin_read_tx().unwrap(),
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
        let mut embeddings = build_query(Some(Ok(QueryEmbedding::Available { identity, vector })));
        // When: vector candidates are requested.
        let error = find_vector_candidates(
            &mut read,
            &mut embeddings,
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
        find_vector_candidates(
            &mut read,
            &mut embeddings,
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
        find_vector_candidates(
            &mut read,
            &mut unavailable,
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
        find_vector_candidates(&mut read, &mut unused, None, "query")
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
            .withf(|request| request.text == "query" && request.dimensions == NonZeroUsize::new(2))
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

mod scoped;

mod filters;

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
