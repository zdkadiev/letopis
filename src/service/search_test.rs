use crate::service::StorageError;
use crate::service::TransactionError;
use crate::storage::get_current;

use super::{
    CandidateReads, QueryCandidates, build_fts_expression, calculate_rank_weight, rank_candidates,
};
use crate::{
    domain::{ElementKind, KnowledgeId, LineRange, Sha256},
    service::{
        CacheDatabase, CommandErrorKind, ConfigState, DrainOutcome, Embedder, EmbeddingConfig,
        EmbeddingError, EmbeddingErrorKind, EmbeddingResult, KnowledgeMetadata, MockRefresher,
        NodeMetadata, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, StorageRead,
        TextMatch, VectorDatabase, VectorMatch,
    },
    storage::open_test_storage as open_storage,
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
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let unit_result = storage
        .cache_storage
        .with_write_tx(|| {
            let unit = &storage.cache_storage;
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
            let row = unit.nodes_upsert_by_id(&node).unwrap();

            Ok::<_, StorageError>((unit.project_get_revision()?, row))
        })
        .map_err(TransactionError::into_inner);
    let (_, row) = unit_result.unwrap();

    storage
        .cache_storage
        .with_read_tx(|| {
            let read = &storage.cache_storage;
            let mut bad_row = make_text_match("a.md", 0.0);
            bad_row.row_id = row + 1;
            // When / Then: candidate metadata is retrieved; mismatched rows and hashes are rejected.
            assert!(
                CandidateReads { storage_read: read }
                    .get_candidate_metadata_by_text_matches_and_vector_matches(&[bad_row], &[])
                    .is_err()
            );
            assert!(
                CandidateReads { storage_read: read }
                    .get_candidate_metadata_by_text_matches_and_vector_matches(
                        &[],
                        &[make_vector_match("a.md", 0.0)]
                    )
                    .is_err()
            );
            assert!(
                CandidateReads { storage_read: read }
                    .get_candidate_metadata_by_text_matches_and_vector_matches(
                        &[make_text_match("missing.md", 0.0)],
                        &[]
                    )
                    .is_err()
            );
            assert_eq!(read.project_get_by_id().unwrap(), 1);

            Ok::<_, StorageError>(())
        })
        .unwrap();
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
    // Given: a configuration and an unconfigured native database.
    let root = tempfile::TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    let ready = ConfigState::Ready(config.clone());
    let capture = |state: &ConfigState| {
        storage
            .cache_storage
            .with_read_tx(|| {
                CandidateReads {
                    storage_read: &storage.cache_storage,
                }
                .capture_vector_config(state)
                .map(|value| value.map(|(_, dimensions)| dimensions.get()))
            })
            .unwrap()
    };
    // When / Then: identity without a native vector table remains unavailable.
    assert_eq!(capture(&ready), None);
    storage
        .vector_storage
        .reset_vectors(None, &config.get_identity())
        .unwrap();
    assert_eq!(capture(&ready), None);
    // Given: matching native dimensions are present.
    seed_dimension(&storage, &config, 2);
    // When / Then: only a valid matching configuration enables vectors.
    assert_eq!(capture(&ready), Some(2));
    for state in [
        ConfigState::Missing,
        ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
        ConfigState::Changed,
    ] {
        assert_eq!(capture(&state), None);
    }
    let incompatible = build_config(3);
    seed_dimension(&storage, &incompatible, 2);
    assert_eq!(capture(&ConfigState::Ready(incompatible)), None);
}

#[test]
fn malformed_query_vectors_and_embedding_infrastructure_errors_fail_closed() {
    // Given: malformed query vectors and embedding failures.
    let root = tempfile::TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    seed_dimension(&storage, &config, 2);
    storage
        .cache_storage
        .with_read_tx(|| {
            let read = &storage.cache_storage;
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
                    storage_read: read,
                    embeddings: &mut embeddings,
                }
                .find_vector_candidates(
                    Some((&config, NonZeroUsize::new(2).unwrap())),
                    "query",
                    NonZeroUsize::new(100).unwrap(),
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
                    storage_read: read,
                    embeddings: &mut embeddings
                }
                .find_vector_candidates(
                    Some((&config, NonZeroUsize::new(2).unwrap())),
                    "query",
                    NonZeroUsize::new(100).unwrap(),
                )
                .is_err()
            );

            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn unavailable_or_changed_vector_channel_keeps_fts_fallback() {
    // Given: an unavailable or changed vector channel.
    let root = tempfile::TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let config = build_config(2);
    storage
        .cache_storage
        .with_read_tx(|| {
            let read = &storage.cache_storage;
            let mut embeddings = build_query(Some(Ok(QueryEmbedding::Available {
                identity: config.get_identity(),
                vector: vec![1.0, 0.0],
            })));
            // When / Then: vector candidates are requested; the lexical fallback remains available.
            assert!(
                QueryCandidates {
                    storage_read: read,
                    embeddings: &mut embeddings
                }
                .find_vector_candidates(
                    Some((&config, NonZeroUsize::new(2).unwrap())),
                    "query",
                    NonZeroUsize::new(100).unwrap(),
                )
                .unwrap()
                .is_empty()
            );
            // Given: the query provider reports unavailable embeddings.
            let mut unavailable = build_query(Some(Ok(QueryEmbedding::Unavailable)));
            // When / Then: vector candidates are requested; the lexical fallback remains available.
            assert!(
                QueryCandidates {
                    storage_read: read,
                    embeddings: &mut unavailable
                }
                .find_vector_candidates(
                    Some((&config, NonZeroUsize::new(2).unwrap())),
                    "query",
                    NonZeroUsize::new(100).unwrap(),
                )
                .unwrap()
                .is_empty()
            );
            // Given: no configured vector channel is available.
            let mut unused = build_query(None);
            // When / Then: vector candidates are requested; the lexical fallback remains available.
            assert!(
                QueryCandidates {
                    storage_read: read,
                    embeddings: &mut unused
                }
                .find_vector_candidates(None, "query", NonZeroUsize::new(100).unwrap())
                .unwrap()
                .is_empty()
            );

            Ok::<_, StorageError>(())
        })
        .unwrap();
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

fn seed_dimension(
    storage: &crate::storage::TestDatabases,
    config: &EmbeddingConfig,
    dimensions: usize,
) {
    let observed = storage
        .cache_storage
        .with_read_tx(|| Ok::<_, StorageError>(storage.cache_storage.meta_get_by_id()))
        .unwrap()
        .unwrap();
    storage
        .vector_storage
        .reset_vectors(observed.as_ref(), &config.get_identity())
        .unwrap();
    storage
        .vector_storage
        .with_write_tx(&config.get_identity(), || {
            let write = &storage.vector_storage;
            write
                .store_vector(&Sha256::digest(b"seed"), &vec![1.0; dimensions], dimensions)
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
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
    use crate::service::StorageError;
    use crate::service::TransactionError;

    mod revisions {
        use crate::service::StorageError;

        use super::{ProvenanceFailure, ScopedFixture, build_filtered_request, open_storage};
        use crate::{
            domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, Sha256, SourceUri, TreeOid},
            service::{
                CacheDatabase, CommandErrorKind, NodeMetadata, Searcher, StorageRead,
                VectorDatabase,
            },
        };
        use std::{num::NonZeroUsize, path::Path};

        #[test]
        fn revision_cache_keeps_demand_order_and_resets_between_searches() {
            // Given: two candidates share one exact basis/source/revision comparison.
            let fixture = ScopedFixture::new();
            add_candidate(fixture.root.path(), "b.md", 'a');
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
                    "validate", "compare", "observe", "observe", "validate", "compare", "observe",
                    "observe"
                ]
            );
        }

        #[test]
        fn sql_pool_limits_fresh_candidate_observations() {
            // Given: two equally ranked candidates share one source comparison.
            let fixture = ScopedFixture::new();
            add_candidate(fixture.root.path(), "b.md", 'a');
            let mut search = fixture.build_search(None, None);
            let mut request = build_filtered_request();
            request.limit = NonZeroUsize::MIN;
            // When: each SQL channel is limited to one knowledge hit.
            let links = search.search(&request).unwrap();
            // Then: only the selected candidate is captured and observed; there is no tail walk.
            assert_eq!(links.len(), 1);
            assert_eq!(links[0].element_id.as_str(), "a.md");
            assert_eq!(fixture.reads.get(), 1);
            assert_eq!(
                *search.provenance.events.borrow(),
                ["validate", "compare", "observe"]
            );
        }

        #[test]
        fn explicit_five_limits_both_hybrid_channels_before_observation() {
            // Given: seven equally ranked nodes occur in both SQL channels.
            let fixture = ScopedFixture::new();
            for candidate in ["b.md", "c.md", "d.md", "e.md", "f.md", "g.md"] {
                add_candidate(fixture.root.path(), candidate, 'a');
            }
            let storage = open_storage(fixture.root.path());
            let identity = fixture.config.get_identity();
            storage
                .vector_storage
                .with_write_tx(&identity, || {
                    storage
                        .vector_storage
                        .store_vector(&Sha256::digest(b"query"), &[1.0, 0.0], 2)
                        .map(|_| ())
                })
                .unwrap();
            let mut search = fixture.build_search(None, None);
            let mut request = build_filtered_request();
            request.filters = Default::default();
            request.limit = NonZeroUsize::new(5).unwrap();
            // When: search forwards the explicit bound to lexical and vector retrieval.
            let links = search.search(&request).unwrap();
            // Then: either channel returning more than five would enlarge this observed union.
            assert_eq!(
                links
                    .iter()
                    .map(|link| link.element_id.as_str())
                    .collect::<Vec<_>>(),
                ["a.md", "b.md", "c.md", "d.md", "e.md"]
            );
            assert_eq!(fixture.observed.borrow().len(), 5);
            assert_eq!(
                *search.provenance.events.borrow(),
                [
                    "validate", "observe", "observe", "observe", "observe", "observe"
                ]
            );
        }

        #[test]
        fn first_observation_failure_precedes_later_candidate_comparison() {
            // Given: two candidates and a first-observation failure.
            let fixture = ScopedFixture::new();
            add_candidate(fixture.root.path(), "b.md", 'b');
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

        fn add_candidate(root: &Path, candidate: &str, basis: char) {
            let storage = open_storage(root);
            let stored = storage
                .cache_storage
                .with_read_tx(|| {
                    let read = &storage.cache_storage;
                    let stored = read
                        .nodes_get_by_uri(&EdgeTarget::Knowledge(
                            KnowledgeId::parse("a.md").unwrap(),
                        ))
                        .unwrap()
                        .unwrap();

                    Ok::<_, StorageError>(stored)
                })
                .unwrap();

            let NodeMetadata::Knowledge(mut node) = stored.metadata else {
                panic!("knowledge fixture")
            };
            let id = KnowledgeId::parse(candidate).unwrap();
            node.source_snapshot = Some(TreeOid::parse(&basis.to_string().repeat(40)).unwrap());
            node.id = id.clone();
            node.uri = id.clone();
            storage
                .cache_storage
                .with_write_tx(|| {
                    let write = &storage.cache_storage;
                    write
                        .nodes_upsert_by_id(&NodeMetadata::Knowledge(node))
                        .unwrap();
                    write.replace_text(&id, "query").unwrap();
                    write
                        .edges_save_by_source_id_and_target_id_and_type(
                            &Edge::new(
                                id,
                                EdgeTarget::Source(SourceUri::parse("old.rs").unwrap()),
                                EdgeType::parse("external").unwrap(),
                            )
                            .unwrap(),
                        )
                        .unwrap();

                    write.project_get_revision()
                })
                .unwrap();
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
            CacheDatabase, CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind,
            DocumentResult, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind,
            EmbeddingResult, KnowledgeMetadata, MockRefresher, NodeMetadata, ProvenanceObservation,
            ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RecheckFilter, SearchFilters,
            SearchFlag, SearchRequest, Searcher, SnapshotError, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest, StorageRead, search::SearchService,
        },
        storage::TestDatabases,
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
        inner: crate::storage::CacheStorage,
        reads: Rc<Cell<usize>>,
    }

    type ScopedSearch = SearchService<
        MockRefresher,
        Query,
        LocalFileSystem,
        MarkdownParser,
        Observations,
        ReadOnlyStorage,
    >;
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
        publish_candidate(&open_storage(root), "b", "new.rs");
        assert_eq!(
            {
                let scope_databases = open_storage(root);
                scope_databases
                    .cache_storage
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(scope_databases.cache_storage.project_get_by_id())
                    })
                    .unwrap()
            }
            .unwrap(),
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
        let writer = open_storage(root);
        let mut search = fixture.build_search(
            Some(Box::new(move || {
                publish_candidate(&writer, "b", "new.rs");
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
            {
                let scope_databases = open_storage(root);
                scope_databases
                    .cache_storage
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(scope_databases.cache_storage.project_get_by_id())
                    })
                    .unwrap()
            }
            .unwrap(),
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
            publish_candidate(&open_storage(root), "b", "new.rs");
            assert_eq!(
                {
                    let scope_databases = open_storage(root);
                    scope_databases
                        .cache_storage
                        .with_read_tx(|| {
                            Ok::<_, StorageError>(scope_databases.cache_storage.project_get_by_id())
                        })
                        .unwrap()
                }
                .unwrap(),
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
            let storage = open_storage(root.path());
            publish_candidate(&storage, "a", "old.rs");
            let config = build_config(2);
            seed_dimension(&storage, &config, 2);
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
                cache: Rc::new(ReadOnlyStorage {
                    inner: open_storage(self.root.path()).cache_storage,
                    reads: self.reads.clone(),
                }),
                embeddings: Query {
                    config: self.config.clone(),
                    state: None,
                    calls: self.calls.clone(),
                    hook,
                },
                filesystem: LocalFileSystem::new(self.root.path()).unwrap(),
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
        fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
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
    use crate::service::cache_test_support::{
        delegate_cache_reads, delegate_cache_writes, delegate_graph_reads,
    };

    impl crate::service::StorageRead for ReadOnlyStorage {
        delegate_cache_reads!();
    }

    impl crate::service::GraphRead for ReadOnlyStorage {
        delegate_graph_reads!();
    }

    impl crate::service::CacheDatabase for ReadOnlyStorage {
        delegate_cache_writes!();

        fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.reads.set(self.reads.get() + 1);
            assert_eq!(self.reads.get(), 1, "search opened a second view");
            self.inner.with_read_tx(operation)
        }

        fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_write_tx(operation)
        }

        fn with_status_read_tx<T, E, F>(
            &self,
            operation: F,
        ) -> Result<Option<T>, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_status_read_tx(operation)
        }
    }

    fn open_storage(root: &Path) -> TestDatabases {
        crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
    }
    fn publish_candidate(storage: &TestDatabases, oid: &str, source: &str) {
        let id = KnowledgeId::parse("a.md").unwrap();
        storage
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
                unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
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
                unit.edges_delete_by_source_id(&id).unwrap();
                unit.edges_save_by_source_id_and_target_id_and_type(
                    &Edge::new(
                        id,
                        EdgeTarget::Source(SourceUri::parse(source).unwrap()),
                        EdgeType::parse("external").unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();

                unit.project_get_revision()
            })
            .unwrap();
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
            CacheDatabase, CommandErrorKind, ConfigState, DocumentResult, DrainOutcome, Embedder,
            EmbeddingResult, KnowledgeMetadata, MarkdownCodec, MockRefresher, NodeMetadata,
            ProvenanceObservation, ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest,
            RecheckFilter, SearchFilters, SearchFlag, SearchRequest, Searcher, SnapshotError,
            SnapshotErrorKind, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
            TextMatch, VectorMatch,
            search::{SearchService, rank_candidates},
        },
        storage::TestDatabases,
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
    type TestSearch = SearchService<
        MockRefresher,
        Query,
        LocalFileSystem,
        MarkdownParser,
        Probe,
        crate::storage::CacheStorage,
    >;

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
            crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db"))
                .unwrap();
        if matches!(candidate, Candidate::Present) {
            publish_note(root, &mut storage);
        }
        seed_dimension(&storage, &build_config(2), 2);
        let queries = Rc::new(Cell::new(0));
        let validations = Rc::new(Cell::new(0));
        let observations = Rc::new(Cell::new(0));
        let search = SearchService {
            refresher: build_noop_refresher(),
            cache: Rc::new(storage.cache_storage),
            embeddings: Query(queries.clone()),
            filesystem: LocalFileSystem::new(root).unwrap(),
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
    fn publish_note(root: &Path, storage: &mut TestDatabases) {
        let bytes = b"<!-- letopis: {\"created_at\":1} -->\nquery\n";
        std::fs::write(root.join("note.md"), bytes).unwrap();
        let parsed = MarkdownParser::new()
            .file_parse(&RepoPath::parse("note.md").unwrap(), bytes)
            .unwrap()
            .unwrap();
        let element = &parsed.elements[0].element;
        let id = KnowledgeId::parse("note.md").unwrap();
        storage
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
                unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
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
                unit.edges_save_by_source_id_and_target_id_and_type(
                    &Edge::new(
                        id,
                        EdgeTarget::Source(SourceUri::parse("code.rs").unwrap()),
                        EdgeType::parse("external").unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();

                unit.project_get_revision()
            })
            .unwrap();
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
    refresher.expect_refresh().returning(|_| {
        Ok(RefreshOutcome {
            revision: 1,
            is_published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    });
    refresher
}

mod comparisons {
    use super::super::RevisionComparisons;

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

mod parse_reuse {
    use crate::{
        domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, TreeOid},
        markdown::{Diagnostic, MarkdownParser},
        service::{
            CacheDatabase, CapturedFile, CommandError, CommandErrorKind, CommandResult,
            DocumentResult, FileError, FileErrorKind, FileStat, KnowledgeMetadata, MarkdownCodec,
            MockFileReader, ParsedFile, ProvenanceObservation, ProvenanceRequest, SearchFilters,
            SearchFlag, SearchLink, SnapshotError, SourceProvenance, SourceRevisionComparison,
            SourceRevisionRequest, TransactionError,
            search::{CurrentKnowledge, SearchEligibility},
        },
        storage::{TestDatabases, open_test_storage},
    };
    use std::{
        cell::Cell,
        collections::{BTreeMap, BTreeSet},
        path::Path,
    };

    struct CountedParser(Cell<usize>);
    struct NoSources;

    #[test]
    fn repeated_bindings_capture_every_time_but_parse_once_per_operation() {
        // Given: eight published sections of one unchanged current file.
        let bytes = build_sections(8);
        let nodes = build_metadata("note.md", &bytes);
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let files = build_files(vec![Some(bytes.clone()); 16]);
        // When: the same live collaborators serve two independent eligibility operations.
        let first = get_links(&storage, &files, &parser, &nodes);
        let second = get_links(&storage, &files, &parser, &nodes);
        // Then: all links remain current, all sixteen captures occur, and neither parse survives its operation.
        assert_eq!(first.len(), 8);
        assert_eq!(second.len(), 8);
        assert!(
            first
                .values()
                .chain(second.values())
                .all(|link| link.flags.is_empty())
        );
        assert_eq!(parser.0.get(), 2);
    }

    #[test]
    fn changed_invalid_missing_and_restored_captures_retain_their_flags() {
        // Given: eight section bindings encounter same-size edits, invalid bytes, absence, and restoration.
        let original = build_sections(8);
        let edited = String::from_utf8(original.clone())
            .unwrap()
            .replace("body", "edit")
            .into_bytes();
        let invalid = b"<!-- letopis: {} -->\ninvalid\n".to_vec();
        let files = build_files(vec![
            Some(original.clone()),
            Some(edited),
            Some(original.clone()),
            Some(invalid.clone()),
            Some(invalid),
            None,
            Some(original.clone()),
            Some(original.clone()),
        ]);
        let nodes = build_metadata("note.md", &original);
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        // When: eligibility follows sorted bindings while every capture independently changes.
        let links = get_links(&storage, &files, &parser, &nodes);
        // Then: exact current-byte changes are visible, and invalid parses are retried rather than retained.
        let stale: Vec<_> = links
            .values()
            .map(|link| link.flags.contains(&SearchFlag::Stale))
            .collect();
        assert_eq!(stale, [false, true, false, true, true, true, false, false]);
        assert_eq!(parser.0.get(), 6);
    }

    #[test]
    fn equal_bytes_at_distinct_paths_need_distinct_parses() {
        // Given: two files contain identical section bytes but have distinct knowledge identities.
        let bytes = build_sections(2);
        let mut nodes = build_metadata("a.md", &bytes);
        nodes.extend(build_metadata("b.md", &bytes));
        let files = build_files(vec![Some(bytes); 4]);
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        // When: sorted demand moves from a.md to b.md.
        let links = get_links(&storage, &files, &parser, &nodes);
        // Then: each file is parsed once and no path-specific identity leaks to the next file.
        assert_eq!(links.len(), 4);
        assert!(links.values().all(|link| link.flags.is_empty()));
        assert_eq!(parser.0.get(), 2);
    }

    #[test]
    fn oversized_inputs_and_entry_sets_bypass_retention() {
        // Given: byte and entry budgets are independently exceeded by valid files.
        let long_path = format!("{}.md", "n".repeat(300));
        for (path, bytes) in [
            (
                "note.md",
                [build_sections(2), vec![b'x'; 256 * 1024]].concat(),
            ),
            ("note.md", build_sections(1025)),
            (long_path.as_str(), build_sections(1024)),
        ] {
            let mut nodes = build_metadata(path, &bytes);
            nodes.retain(|id, _| id.as_str().ends_with("s0000") || id.as_str().ends_with("s0001"));
            let files = build_files(vec![Some(bytes); 2]);
            let parser = CountedParser(Cell::new(0));
            let root = tempfile::tempdir().unwrap();
            let storage =
                open_test_storage(&root.path().join("cache"), &root.path().join("vectors"))
                    .unwrap();
            // When: both bindings are evaluated using the ordinary path.
            let links = get_links(&storage, &files, &parser, &nodes);
            // Then: bypass changes no flags and retains no reusable parse.
            assert_eq!(links.len(), 2);
            assert!(links.values().all(|link| link.flags.is_empty()));
            assert_eq!(parser.0.get(), 2);
        }
    }

    #[test]
    fn missing_capture_discards_the_previous_success() {
        // Given: a valid file disappears between its three bindings and then returns unchanged.
        let bytes = build_sections(3);
        let nodes = build_metadata("note.md", &bytes);
        let files = build_files(vec![Some(bytes.clone()), None, Some(bytes)]);
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        // When: each binding captures its own current file state.
        let links = get_links(&storage, &files, &parser, &nodes);
        // Then: absence marks only its binding stale and restoration is parsed again.
        assert_eq!(
            links
                .values()
                .map(|link| link.flags.contains(&SearchFlag::Stale))
                .collect::<Vec<_>>(),
            [false, true, false]
        );
        assert_eq!(parser.0.get(), 2);
    }

    #[test]
    fn every_binding_still_checks_its_own_hash_and_lines() {
        // Given: three pinned bindings include a wrong hash and wrong line range.
        let bytes = build_sections(3);
        let mut nodes = build_metadata("note.md", &bytes);
        nodes
            .get_mut(&KnowledgeId::parse("note.md#s0001").unwrap())
            .unwrap()
            .hash = Sha256::digest(b"wrong");
        nodes
            .get_mut(&KnowledgeId::parse("note.md#s0002").unwrap())
            .unwrap()
            .lines = LineRange::new(1, 2).unwrap();
        let files = build_files(vec![Some(bytes); 3]);
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        // When: all bindings reuse one successful parse after their fresh captures.
        let links = get_links(&storage, &files, &parser, &nodes);
        // Then: stale predicates use each pinned node rather than the first node's answer.
        assert_eq!(
            links
                .values()
                .map(|link| link.flags.contains(&SearchFlag::Stale))
                .collect::<Vec<_>>(),
            [false, true, true]
        );
        assert_eq!(parser.0.get(), 1);
    }

    #[test]
    fn retained_projection_releases_search_and_graph_payloads() {
        // Given: a valid whole-file element contains searchable text and a graph link.
        let bytes = b"<!-- letopis: {\"created_at\":1} -->\nbody [source](code.rs \"letopis:type=external;path=code.rs\")\n";
        let nodes = build_metadata("note.md", bytes);
        let files = build_files(vec![Some(bytes.to_vec())]);
        let parser = CountedParser(Cell::new(0));
        let mut current = CurrentKnowledge {
            filesystem: &files,
            markdown: &parser,
            last_parsed: None,
        };
        // When: one current binding leaves a reusable eligibility projection.
        let link = current
            .build_link(nodes.values().next().unwrap(), 1)
            .unwrap();
        let parsed = &current.last_parsed.as_ref().unwrap().parsed;
        // Then: only bounded element metadata is retained, while text buffers and graph allocations are released.
        assert!(link.flags.is_empty());
        assert!(parsed.edges.is_empty());
        assert_eq!(parsed.elements.len(), 1);
        assert_eq!(parsed.elements[0].search_text.capacity(), 0);
    }

    #[test]
    fn fresh_capture_errors_abort_without_retaining_state_for_retry() {
        // Given: a safe first capture is followed by an I/O error at the second binding.
        let bytes = build_sections(2);
        let nodes = build_metadata("note.md", &bytes);
        let mut files = MockFileReader::new();
        let mut demand = 0;
        files
            .expect_file_get_by_path()
            .times(4)
            .returning(move |_| {
                demand += 1;
                if demand == 2 {
                    return Err(FileError::new(FileErrorKind::Io));
                }
                Ok(Some(build_capture(bytes.clone())))
            });
        let parser = CountedParser(Cell::new(0));
        let root = tempfile::tempdir().unwrap();
        let storage =
            open_test_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        // When: the failed operation is retried using the same collaborators.
        let error = get_links_result(&storage, &files, &parser, &nodes).unwrap_err();
        let links = get_links(&storage, &files, &parser, &nodes);
        // Then: I/O remains an error rather than stale or partial success, and the retry parses afresh.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(links.len(), 2);
        assert!(links.values().all(|link| link.flags.is_empty()));
        assert_eq!(parser.0.get(), 2);
    }

    impl MarkdownCodec for CountedParser {
        fn file_parse(
            &self,
            path: &RepoPath,
            bytes: &[u8],
        ) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
            self.0.set(self.0.get() + 1);
            MarkdownParser::new().file_parse(path, bytes)
        }
    }
    impl SourceProvenance for NoSources {
        fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            Ok(())
        }
        fn observe_sources(
            &self,
            _: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            Ok(ProvenanceObservation {
                flags: BTreeSet::new(),
                issues: BTreeSet::new(),
            })
        }
        fn compare_revision(
            &self,
            _: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            panic!("unfiltered search cannot compare revisions")
        }
    }
    fn get_links(
        storage: &TestDatabases,
        files: &MockFileReader,
        parser: &CountedParser,
        nodes: &BTreeMap<KnowledgeId, KnowledgeMetadata>,
    ) -> BTreeMap<KnowledgeId, SearchLink> {
        get_links_result(storage, files, parser, nodes).unwrap()
    }
    fn get_links_result(
        storage: &TestDatabases,
        files: &MockFileReader,
        parser: &CountedParser,
        nodes: &BTreeMap<KnowledgeId, KnowledgeMetadata>,
    ) -> CommandResult<BTreeMap<KnowledgeId, SearchLink>> {
        storage
            .cache_storage
            .with_read_tx(|| {
                SearchEligibility {
                    storage_read: &storage.cache_storage,
                    filesystem: files,
                    markdown: parser,
                    provenance: &NoSources,
                }
                .get_eligible_links_by_metadata_and_filters(nodes, &SearchFilters::default())
            })
            .map_err(|error| match error {
                TransactionError::Operation(error) => error,
                TransactionError::Storage(error) => CommandError::new_with_cause(
                    CommandErrorKind::InternalError,
                    "cannot search knowledge",
                    error,
                ),
            })
    }
    fn build_files(captures: Vec<Option<Vec<u8>>>) -> MockFileReader {
        let mut files = MockFileReader::new();
        let count = captures.len();
        let mut captures = captures.into_iter();
        files
            .expect_file_get_by_path()
            .times(count)
            .returning(move |_: &Path| {
                Ok(captures.next().expect("extra live read").map(build_capture))
            });
        files
    }
    fn build_capture(bytes: Vec<u8>) -> CapturedFile {
        CapturedFile {
            bytes,
            stat: FileStat {
                size: 1,
                mtime_ns: 1,
                inode: 1,
                device: 1,
            },
            checked_at_ns: 1,
        }
    }
    fn build_sections(count: usize) -> Vec<u8> {
        (0..count)
            .map(|index| {
                format!(
                    "## Section {{#s{index:04}}}\n<!-- letopis: {{\"created_at\":1}} -->\nbody\n"
                )
            })
            .collect::<String>()
            .into_bytes()
    }
    fn build_metadata(path: &str, bytes: &[u8]) -> BTreeMap<KnowledgeId, KnowledgeMetadata> {
        MarkdownParser::new()
            .file_parse(&RepoPath::parse(path).unwrap(), bytes)
            .unwrap()
            .unwrap()
            .elements
            .into_iter()
            .map(|parsed| {
                let element = parsed.element;
                (
                    element.id.clone(),
                    KnowledgeMetadata {
                        uri: element.id.clone(),
                        id: element.id,
                        kind: ElementKind::Vertex,
                        hash: element.hash,
                        created_at: 1,
                        source_snapshot: None,
                        lines: element.lines,
                        search_hash: parsed.search_hash,
                    },
                )
            })
            .collect()
    }
}
