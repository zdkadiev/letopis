use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead};
mod support;
use letopis::{
    domain::LineRange,
    service::{
        CommandErrorKind, NodeMetadata, Refresher, SearchFlag, SearchRequest, Searcher,
        VectorOutcome,
    },
    wire_document_refresher, wire_searcher,
};
use std::{collections::BTreeSet, num::NonZeroUsize};
use support::{
    embedding::{FixtureReply, HttpFixture},
    reading::{ReadingRepo, run_with_credentials},
};

#[test]
fn service_caps_direct_requests_at_one_hundred_results() {
    // Given: 105 equally searchable knowledge files and a direct service caller.
    let repo = ReadingRepo::new();
    for index in 0..105 {
        repo.write_knowledge(&format!("n{index:03}.md"), "sqlite");
    }
    // When: the caller supplies a limit above the search maximum.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 1000))
        .unwrap();
    // Then: the effective limit caps both retrieval and the final response at 100.
    assert_eq!(links.len(), 100);
    assert_eq!(links.first().unwrap().element_id.as_str(), "n000.md");
    assert_eq!(links.last().unwrap().element_id.as_str(), "n099.md");
}

#[test]
fn restrictive_filter_does_not_refill_outside_sql_candidates() {
    // Given: the first lexical candidate has no sources; a lower candidate has one.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    repo.write_knowledge(
        "b.md",
        "sqlite extra [source](code.rs \"letopis:type=external;path=code.rs\")",
    );
    let mut request = make_request("sqlite", 1);
    request
        .filters
        .sources
        .insert(letopis::domain::SourceUri::parse("code.rs").unwrap());
    // When: a source filter excludes the single SQL-selected candidate.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&request)
        .unwrap();
    // Then: the service returns fewer hits rather than scanning/refilling the whole corpus.
    assert!(links.is_empty());
}

#[test]
fn search_refreshes_and_returns_only_knowledge_links() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "sqlite\n[source](src/sqlite.rs \"letopis:type=external;path=src/sqlite.rs\")",
    );
    repo.write("plain.md", b"sqlite");
    // When
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 10))
        .unwrap();
    // Then
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].element_id.as_str(), "a.md");
    assert_eq!(links[0].path.as_str(), "a.md");
    assert_eq!(links[0].lines, LineRange::new(1, 3).unwrap());
    assert_eq!(links[0].revision, 1);
    assert_eq!(
        links[0].flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound,])
    );
}
#[test]
fn fts_literals_work_on_native_tokenizer() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("all.md", "SQLite OR блокировки NEAR x title café 42");
    repo.write_knowledge("partial.md", "SQLite");
    let mut search = wire_searcher(repo.get_root()).unwrap();
    for query in [
        "SQLite OR блокировки",
        "\"NEAR(x*)\" title:café 42",
        "CAFÉ блокировки 42",
    ] {
        // When
        let links = search.search(&make_request(query, 10)).unwrap();
        // Then
        assert_eq!(
            links
                .iter()
                .map(|x| x.element_id.as_str())
                .collect::<Vec<_>>(),
            ["all.md"]
        );
    }
}
#[test]
fn empty_query_never_calls_match_or_provider() {
    // Given: an empty search expression and recorded dependencies.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    repo.write_knowledge("b.md", "new");
    for query in ["", " * : () \" "] {
        // When / Then: search processes the empty query; neither FTS matching nor the provider runs.
        assert!(
            wire_searcher(repo.get_root())
                .unwrap()
                .search(&make_request(query, 2))
                .unwrap()
                .is_empty()
        );
    }
    assert!(server.take_requests().is_empty());
    assert_eq!(
        {
            let scope_databases = repo.open_storage();
            scope_databases
                .cache
                .with_read_tx(|| Ok::<_, StorageError>(scope_databases.cache.project_get_by_id()))
                .unwrap()
        }
        .unwrap(),
        2
    );
}
#[test]
fn search_uses_prepared_vectors_and_actual_query_client() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite sqlite sqlite");
    repo.write_knowledge("b.md", "sqlite plus padding words");
    repo.write_knowledge("c.md", "unrelated");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    let revision = wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap()
        .revision;
    repo.seed_vectors(
        &config,
        &[
            ("a.md", &[0.0, 1.0]),
            ("b.md", &[1.0, 1.0]),
            ("c.md", &[1.0, 0.0]),
        ],
    );
    // When
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    // Then
    assert_eq!(
        links
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    assert!(
        links
            .iter()
            .all(|x| x.revision == revision && x.flags.is_empty())
    );
    assert_eq!(
        server.take_requests()[0]["input"],
        serde_json::json!(["sqlite"])
    );
}
#[test]
fn provider_failure_sets_query_error_without_search_warning_or_attempts() {
    // Given: a failing query provider.
    if run_with_credentials("provider_failure_sets_query_error_without_search_warning_or_attempts")
    {
        return;
    }
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "SECRET".into(),
    });
    let config = repo.write_config("openai", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    // When: search falls back to lexical matches.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    // Then: a query error is stored without warnings or document attempts.
    assert_eq!(links.len(), 1);
    assert!(links[0].flags.is_empty());
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            let error = read.query_error_get_by_id().unwrap().unwrap();
            assert!(!error.error.contains("SECRET"));
            let nodes = read.nodes_get().unwrap();
            let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
                panic!("knowledge")
            };
            assert_eq!(
                read.embed_failures_get_by_search_hash(&config.get_identity(), &node.search_hash)
                    .unwrap(),
                VectorOutcome::Current(None)
            );
            assert_eq!(server.take_requests().len(), 1);
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn invalid_live_knowledge_retains_pinned_link_as_stale() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    repo.write("a.md", b"<!-- letopis: {invalid} -->\ncurrent");
    // When
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    // Then
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].flags.iter().copied().collect::<Vec<_>>(),
        [SearchFlag::Stale]
    );
    assert_eq!(links[0].lines, LineRange::new(1, 2).unwrap());
}
#[test]
fn search_propagates_non_config_io_failures() {
    // Given: a tracked document whose parent becomes a symlink.
    let repo = ReadingRepo::new();
    repo.write_knowledge("dir/a.md", "sqlite");
    assert!(
        std::process::Command::new("git")
            .current_dir(repo.get_root())
            .args(["add", "dir/a.md"])
            .status()
            .unwrap()
            .success()
    );
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    std::fs::remove_dir_all(repo.get_root().join("dir")).unwrap();
    std::os::unix::fs::symlink("/private/tmp", repo.get_root().join("dir")).unwrap();
    // When: search refreshes the tracked path.
    let error = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap_err();
    // Then: the unsafe parent causes an infrastructure error.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
}
#[test]
fn missing_and_invalid_embedding_configuration_permit_fts_without_provider() {
    // Given: marked knowledge and either absent config or invalid embedding settings.
    for config in [
        None,
        Some(br#"{"embedding_model":null,"discovery":{"respect_gitignore":true}}"#.as_slice()),
    ] {
        let repo = ReadingRepo::new();
        repo.write_knowledge("a.md", "sqlite");
        if let Some(bytes) = config {
            repo.write(".letopis/config.json", bytes);
        }
        // When: search refreshes valid discovery independently of embedding availability.
        let links = wire_searcher(repo.get_root())
            .unwrap()
            .search(&make_request("sqlite", 2))
            .unwrap();
        // Then: the literal FTS owner remains available without a provider.
        assert_eq!(
            links
                .iter()
                .map(|link| link.element_id.as_str())
                .collect::<Vec<_>>(),
            ["a.md"]
        );
    }
}

#[test]
fn invalid_discovery_configuration_stops_fts_and_preserves_corpus() {
    use letopis::service::CommandErrorKind;
    // Given: each malformed or unreadable configuration starts from a published corpus.
    for state in ["invalid", "unreadable"] {
        let repo = ReadingRepo::new();
        repo.write_knowledge("a.md", "sqlite");
        wire_searcher(repo.get_root())
            .unwrap()
            .search(&make_request("sqlite", 2))
            .unwrap();
        let storage = repo.open_storage();
        let before = storage
            .cache
            .with_read_tx(|| storage.cache.files_get())
            .unwrap();
        if state == "invalid" {
            repo.write(".letopis/config.json", b"{PRIVATE_INVALID}");
        } else {
            std::fs::create_dir(repo.get_root().join(".letopis/config.json")).unwrap();
        }
        // When: refresh cannot safely interpret its policy inputs.
        let error = wire_searcher(repo.get_root())
            .unwrap()
            .search(&make_request("sqlite", 2))
            .unwrap_err();
        // Then: failure is typed, private bytes stay hidden, and the old file rows survive.
        assert_eq!(
            error.get_kind(),
            if state == "invalid" {
                CommandErrorKind::InvalidRequest
            } else {
                CommandErrorKind::InternalError
            }
        );
        assert!(!format!("{error:?}").contains("PRIVATE_INVALID"));
        assert_eq!(
            storage
                .cache
                .with_read_tx(|| storage.cache.files_get())
                .unwrap(),
            before
        );
    }
}

#[test]
fn missing_vector_dimensions_skip_inference_and_shared_hashes_keep_distinct_ids() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "same sqlite");
    repo.write_knowledge("b.md", "same sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    let mut search = wire_searcher(repo.get_root()).unwrap();
    // When
    let first = search.search(&make_request("sqlite", 10)).unwrap();
    // Then
    assert_eq!(first.len(), 2);
    assert!(server.take_requests().is_empty());
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let vector_only = search.search(&make_request("notlexical", 10)).unwrap();
    assert_eq!(
        vector_only
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "b.md"]
    );
    assert_eq!(server.take_requests().len(), 1);
}
#[test]
fn malformed_persisted_knowledge_uri_is_decoded_only_inside_sql_limit() {
    // Given: two searchable elements with one persisted address corrupted.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite sqlite sqlite");
    repo.write_knowledge("b.md", "sqlite plus extra words");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    assert_eq!(
        connection
            .execute(
                "update nodes set uri = ?1 where uri = ?2",
                ["../wrong.md", "b.md"],
            )
            .unwrap(),
        1
    );
    // When: SQL selects the stronger valid hit with limit one.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 1))
        .unwrap();
    // Then: an unselected malformed URI is not decoded as a search candidate.
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].element_id.as_str(), "a.md");
    // When: the candidate pool includes the malformed lower-scoring hit.
    let error = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap_err();
    // Then: selected corruption remains a safe internal error.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
}
#[test]
fn stale_coordinates_are_observed_without_replacing_pinned_lines() {
    // Given: a published element with deliberately changed cached coordinates.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    assert_eq!(
        connection
            .execute("update nodes set line_start = 2 where uri = ?1", ["a.md"])
            .unwrap(),
        1
    );
    // When: search compares the pinned metadata to the current canonical document.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    // Then: the original pinned coordinates return with a stale flag.
    assert_eq!(links[0].lines, LineRange::new(2, 2).unwrap());
    assert_eq!(
        links[0].flags.iter().copied().collect::<Vec<_>>(),
        [SearchFlag::Stale]
    );
}
#[test]
fn stale_only_vectors_allow_one_query_but_never_join_obsolete_hashes() {
    // Given: published vectors whose knowledge text has since changed.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "old sqlite");
    repo.write_knowledge("b.md", "old semantic");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[0.0, 1.0]), ("b.md", &[1.0, 0.0])]);
    repo.write_knowledge("a.md", "current sqlite");
    repo.write_knowledge("b.md", "current unrelated");
    // When: search refreshes the text and embeds the query once.
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 10))
        .unwrap();
    // Then: only current lexical results survive and stale vector hashes never join.
    assert_eq!(
        links
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["a.md"]
    );
    assert_eq!(links[0].revision, 2);
    assert!(links[0].flags.is_empty());
    assert_eq!(server.take_requests().len(), 1);
    assert_eq!(
        {
            let scope_databases = repo.open_storage();
            scope_databases
                .cache
                .with_read_tx(|| {
                    Ok::<_, StorageError>(scope_databases.cache.vector_index_get_by_vector(
                        &config.get_identity(),
                        &[1.0, 0.0],
                        2,
                        NonZeroUsize::new(100).unwrap(),
                    ))
                })
                .unwrap()
        }
        .unwrap(),
        VectorOutcome::Current(vec![])
    );
}
fn make_request(query: &str, limit: usize) -> SearchRequest {
    SearchRequest {
        filters: Default::default(),
        query: query.into(),
        limit: NonZeroUsize::new(limit).unwrap(),
    }
}
