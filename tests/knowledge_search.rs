mod support;
use letopis::{
    domain::LineRange,
    service::{
        CommandErrorKind, NodeMetadata, Refresher, SearchFlag, SearchRequest, Searcher, Storage,
        StorageRead, VectorOutcome,
    },
    wire_document_refresher, wire_searcher,
};
use std::num::NonZeroUsize;
use support::{
    embedding::{FixtureReply, HttpFixture},
    reading::{ReadingRepo, run_with_credentials},
};

#[test]
fn search_refreshes_and_returns_only_knowledge_links() {
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "sqlite\n[source](src/sqlite.rs \"letopis:type=external\")",
    );
    repo.write("plain.md", b"sqlite");
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 10))
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].element_id.as_str(), "a.md");
    assert_eq!(links[0].path.as_str(), "a.md");
    assert_eq!(links[0].lines, LineRange::new(1, 3).unwrap());
    assert_eq!(links[0].revision, 1);
    assert!(links[0].flags.is_empty());
}
#[test]
fn fts_literals_work_on_native_tokenizer() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("all.md", "SQLite OR блокировки NEAR x title café 42");
    repo.write_knowledge("partial.md", "SQLite");
    let mut search = wire_searcher(repo.get_root()).unwrap();
    for query in [
        "SQLite OR блокировки",
        "\"NEAR(x*)\" title:café 42",
        "CAFÉ блокировки 42",
    ] {
        let links = search.search(&make_request(query, 10)).unwrap();
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
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    repo.write_knowledge("b.md", "new");
    for query in ["", " * : () \" "] {
        assert!(
            wire_searcher(repo.get_root())
                .unwrap()
                .search(&make_request(query, 2))
                .unwrap()
                .is_empty()
        );
    }
    assert!(server.take_requests().is_empty());
    assert_eq!(repo.open_storage().begin_read().unwrap().get_revision(), 2);
}
#[test]
fn search_uses_prepared_vectors_and_actual_query_client() {
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
        .refresh()
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
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
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
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1.0, 0.0])]);
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    assert_eq!(links.len(), 1);
    assert!(links[0].flags.is_empty());
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    let error = read.read_query_error().unwrap().unwrap();
    assert!(!error.error.contains("SECRET"));
    let nodes = read.list_nodes().unwrap();
    let NodeMetadata::Knowledge(node) = &nodes[0].metadata else {
        panic!("knowledge")
    };
    assert_eq!(
        read.read_failure(&config.get_identity(), &node.search_hash)
            .unwrap(),
        VectorOutcome::Current(None)
    );
    assert_eq!(server.take_requests().len(), 1);
}
#[test]
fn invalid_live_knowledge_retains_pinned_link_as_stale() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.write("a.md", b"<!-- letopis: {invalid} -->\ncurrent");
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(
        links[0].flags.iter().copied().collect::<Vec<_>>(),
        [SearchFlag::Stale]
    );
    assert_eq!(links[0].lines, LineRange::new(1, 2).unwrap());
}
#[test]
fn search_propagates_non_config_io_failures() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    std::fs::remove_file(repo.get_root().join("a.md")).unwrap();
    std::os::unix::fs::symlink("/private/tmp", repo.get_root().join("a.md")).unwrap();
    let error = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
}
#[test]
fn unreadable_invalid_and_missing_configuration_permit_fts_without_provider() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    for state in ["missing", "invalid", "unreadable"] {
        match state {
            "invalid" => repo.write(".letopis/config.json", b"{invalid}"),
            "unreadable" => {
                std::fs::remove_file(repo.get_root().join(".letopis/config.json")).unwrap();
                std::fs::create_dir(repo.get_root().join(".letopis/config.json")).unwrap();
            }
            _ => {}
        }
        let links = wire_searcher(repo.get_root())
            .unwrap()
            .search(&make_request("sqlite", 2))
            .unwrap();
        assert_eq!(
            links
                .iter()
                .map(|x| x.element_id.as_str())
                .collect::<Vec<_>>(),
            ["a.md"]
        );
    }
}
#[test]
fn missing_vector_dimensions_skip_inference_and_shared_hashes_keep_distinct_ids() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "same sqlite");
    repo.write_knowledge("b.md", "same sqlite");
    let server = HttpFixture::start(FixtureReply::Status {
        status: 200,
        body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    let mut search = wire_searcher(repo.get_root()).unwrap();
    let first = search.search(&make_request("sqlite", 10)).unwrap();
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
fn mismatched_persisted_knowledge_uri_fails_even_beyond_final_limit() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite sqlite sqlite");
    repo.write_knowledge("b.md", "sqlite plus extra words");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    connection
        .execute(
            "UPDATE nodes SET uri = ?1 WHERE id = ?2",
            ["wrong.md", "b.md"],
        )
        .unwrap();
    let error = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 1))
        .unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
}
#[test]
fn stale_coordinates_are_observed_without_replacing_pinned_lines() {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "sqlite");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    connection
        .execute("UPDATE nodes SET line_start = 2 WHERE id = ?1", ["a.md"])
        .unwrap();
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 2))
        .unwrap();
    assert_eq!(links[0].lines, LineRange::new(2, 2).unwrap());
    assert_eq!(
        links[0].flags.iter().copied().collect::<Vec<_>>(),
        [SearchFlag::Stale]
    );
}
#[test]
fn stale_only_vectors_allow_one_query_but_never_join_obsolete_hashes() {
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
        .refresh()
        .unwrap();
    repo.seed_vectors(&config, &[("a.md", &[0.0, 1.0]), ("b.md", &[1.0, 0.0])]);
    repo.write_knowledge("a.md", "current sqlite");
    repo.write_knowledge("b.md", "current unrelated");
    let links = wire_searcher(repo.get_root())
        .unwrap()
        .search(&make_request("sqlite", 10))
        .unwrap();
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
        repo.open_storage()
            .begin_read()
            .unwrap()
            .find_nearest_vectors(&config.get_identity(), &[1.0, 0.0], 2)
            .unwrap(),
        VectorOutcome::Current(vec![])
    );
}
fn make_request(query: &str, limit: usize) -> SearchRequest {
    SearchRequest {
        query: query.into(),
        limit: NonZeroUsize::new(limit).unwrap(),
    }
}
