mod support;

use letopis::{
    domain::TreeOid,
    service::{EmbeddingConfig, NodeMetadata, Refresher, Storage, StorageRead, VectorOutcome},
    wire_document_refresher,
};
use serde_json::{Value, json};
use support::{
    embedding::{FixtureReply, HttpFixture},
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

#[test]
fn filtered_failure_then_success_clears_query_error_without_identity_reset() {
    // Given: filtered searches with a failure followed by success.
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start_sequence(vec![
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
        FixtureReply::Status {
            status: 200,
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        },
    ]);
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    prepare_vectors(&repo, &config, 2);
    let input = build_input(&basis);

    // When: both searches run under one identity.
    let fallback = invoke_search(&repo, &input);

    // Then: success clears the query error without resetting identity.
    assert_eq!(get_ids(&fallback), ["a.md", "b.md"]);
    let error = repo
        .open_storage()
        .begin_read_tx()
        .unwrap()
        .get_query_error()
        .unwrap()
        .unwrap();
    assert_eq!(error.error, "embedding provider rejected request");
    assert!(!error.error.contains("PRIVATE"));
    assert!(!error.error.contains("fixture-secret"));
    assert_unchanged_vector_state(&repo, &config);

    let recovered = invoke_search(&repo, &input);

    // Successful query inference includes the eligible semantic-only note.
    assert_eq!(get_ids(&recovered), ["a.md", "b.md", "semantic.md"]);
    assert!(
        repo.open_storage()
            .begin_read_tx()
            .unwrap()
            .get_query_error()
            .unwrap()
            .is_none()
    );
    assert_unchanged_vector_state(&repo, &config);
    assert_eq!(
        server.take_requests(),
        vec![
            json!({"model":"fixture","input":["needle"],"truncate":false}),
            json!({"model":"fixture","input":["needle"],"truncate":false}),
        ]
    );
}

#[test]
fn missing_and_invalid_configuration_keep_filtered_fts_without_provider_requests() {
    // Given: missing or invalid embedding configuration.
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    prepare_vectors(&repo, &config, 2);
    std::fs::remove_file(repo.get_root().join(".letopis/config.json")).unwrap();

    // When / Then: filtered lexical search runs; results remain available without provider requests.
    assert_eq!(
        get_ids(&invoke_search(&repo, &build_input(&basis))),
        ["a.md", "b.md"]
    );
    // Given: missing or invalid embedding configuration.
    repo.write(".letopis/config.json", b"PRIVATE invalid configuration");
    // When / Then: filtered lexical search runs; results remain available without provider requests.
    assert_eq!(
        get_ids(&invoke_search(&repo, &build_input(&basis))),
        ["a.md", "b.md"]
    );

    assert!(server.take_requests().is_empty());
    assert!(
        repo.open_storage()
            .begin_read_tx()
            .unwrap()
            .get_query_error()
            .unwrap()
            .is_none()
    );
    assert_unchanged_vector_state(&repo, &config);
}

#[test]
fn disabled_configuration_keeps_filtered_fts_without_query_provider() {
    // Given: seeded vectors and a valid configured-disabled document.
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    prepare_vectors(&repo, &config, 2);
    repo.write(
        ".letopis/config.json",
        json!({"embedding_model": {
            "enabled": false,
            "provider": "ollama",
            "model": "fixture",
            "parameters": {"base_url": server.get_base_url()},
            "max_retries": 0
        }})
        .to_string()
        .as_bytes(),
    );

    // When: filtered search runs.
    let links = invoke_search(&repo, &build_input(&basis));

    // Then: filtered FTS remains and no query inference is attempted.
    assert_eq!(get_ids(&links), ["a.md", "b.md"]);
    assert!(server.take_requests().is_empty());
    assert!(
        repo.open_storage()
            .begin_read_tx()
            .unwrap()
            .get_query_error()
            .unwrap()
            .is_none()
    );
}

#[test]
fn incompatible_vectors_keep_filtered_fts_without_query_provider_misuse() {
    // Given: incompatible vector configuration with a lexical filter.
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "must not call".into(),
    });
    let configured = json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":server.get_base_url(),"dimensions":3},"max_retries":0});
    let raw = json!({"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture",
        "parameters":{"base_url":server.get_base_url(),"dimensions":3},"max_retries":0}})
    .to_string();
    let config = EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap();
    repo.write(".letopis/config.json", raw.as_bytes());
    prepare_vectors(&repo, &config, 2);

    // When: filtered search runs.
    let links = invoke_search(&repo, &build_input(&basis));

    // Then: lexical results survive without misusing the provider.
    assert_eq!(get_ids(&links), ["a.md", "b.md"]);
    assert!(server.take_requests().is_empty());
    assert!(
        repo.open_storage()
            .begin_read_tx()
            .unwrap()
            .get_query_error()
            .unwrap()
            .is_none()
    );
    assert_unchanged_vector_state(&repo, &config);
}

#[test]
fn query_failure_keeps_fts_and_appears_in_status() {
    // Given: one failing provider reply.
    let (repo, server, input) = prepare_query_scenario(vec![FixtureReply::Status {
        status: 500,
        body: "PRIVATE provider fixture-secret".into(),
    }]);

    // When: the query embedding fails.
    let links = invoke_search(&repo, &input);

    // Then: FTS results remain and status reports one current problem.
    assert_eq!(get_ids(&links), ["a.md", "b.md"]);
    assert_eq!(
        get_query_problems(&get_status(&repo)),
        vec![expected_query_problem()]
    );
    assert_eq!(server.take_requests().len(), 1);
}

#[test]
fn repeated_query_failure_keeps_one_current_problem() {
    // Given: two failing provider replies.
    let (repo, server, input) = prepare_query_scenario(vec![
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
    ]);

    // When: two query embeddings fail in separate processes.
    assert_eq!(get_ids(&invoke_search(&repo, &input)), ["a.md", "b.md"]);
    assert_eq!(get_ids(&invoke_search(&repo, &input)), ["a.md", "b.md"]);

    // Then: the one current record is replaced, not accumulated.
    assert_eq!(
        get_query_problems(&get_status(&repo)),
        vec![expected_query_problem()]
    );
    assert_eq!(server.take_requests().len(), 2);
}

#[test]
fn successful_query_clears_the_query_problem() {
    // Given: a failing reply followed by one successful reply.
    let (repo, server, input) = prepare_query_scenario(vec![
        FixtureReply::Status {
            status: 500,
            body: "PRIVATE provider fixture-secret".into(),
        },
        FixtureReply::Status {
            status: 200,
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        },
    ]);

    // When: a failed query is followed by a successful one.
    assert_eq!(get_ids(&invoke_search(&repo, &input)), ["a.md", "b.md"]);
    assert_eq!(
        get_query_problems(&get_status(&repo)),
        vec![expected_query_problem()]
    );
    let recovered = invoke_search(&repo, &input);

    // Then: semantic results appear and status no longer reports the problem.
    assert_eq!(get_ids(&recovered), ["a.md", "b.md", "semantic.md"]);
    assert!(get_query_problems(&get_status(&repo)).is_empty());
    assert_eq!(server.take_requests().len(), 2);
}

fn prepare_query_scenario(replies: Vec<FixtureReply>) -> (ReadingRepo, HttpFixture, Value) {
    let repo = ReadingRepo::new();
    let basis = prepare_notes(&repo);
    let server = HttpFixture::start_sequence(replies);
    let config = repo.write_config("ollama", &server.get_base_url(), "fixture");
    prepare_vectors(&repo, &config, 2);
    let input = build_input(&basis);
    (repo, server, input)
}

fn expected_query_problem() -> Value {
    json!({
        "code": "embedding_failed",
        "message": "query embedding failed",
        "details": {"reason": "query_embedding_failed"}
    })
}

fn prepare_notes(repo: &ReadingRepo) -> TreeOid {
    repo.write("code.rs", b"stable\n");
    let basis = create_basis(repo.get_root(), &[("code.rs", b"stable\n")]);
    for (id, sources, text) in [
        ("a.md", vec!["code.rs"], "needle"),
        ("b.md", vec!["code.rs"], "needle"),
        ("excluded.md", vec!["other.rs"], "needle needle needle"),
        ("semantic.md", vec!["code.rs"], "semantic"),
    ] {
        repo.write(id, build_note(Some(&basis), &sources, text).as_bytes());
    }
    basis
}
fn prepare_vectors(repo: &ReadingRepo, config: &EmbeddingConfig, dimensions: usize) {
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let vector = vec![1.0; dimensions];
    repo.seed_vectors(
        config,
        &[
            ("a.md", &vector),
            ("b.md", &vector),
            ("excluded.md", &vector),
            ("semantic.md", &vector),
        ],
    );
}
fn build_input(basis: &TreeOid) -> Value {
    json!({"query":"needle","limit":10,"filters":{
        "sources":["code.rs"],"source_revisions":[basis.as_str()],"needs_recheck":false}})
}
fn invoke_search(repo: &ReadingRepo, input: &Value) -> Value {
    let output = repo.invoke(&["search"], input.to_string().as_bytes());
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    for text in [&stdout, &stderr] {
        assert!(!text.contains("PRIVATE"));
        assert!(!text.contains("fixture-secret"));
        assert!(!text.contains("stable"));
    }
    let value: Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
    assert!(value.get("warnings").is_none(), "{value}");
    value["result"].clone()
}
fn get_ids(links: &Value) -> Vec<String> {
    links
        .as_array()
        .unwrap()
        .iter()
        .map(|link| link["element_id"].as_str().unwrap().to_owned())
        .collect()
}
fn get_status(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["status"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    for canary in ["PRIVATE", "fixture-secret", "stable"] {
        assert!(!stdout.contains(canary), "{stdout}");
    }
    serde_json::from_str(&stdout).unwrap()
}

fn get_query_problems(status: &Value) -> Vec<Value> {
    status["result"]["problems"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|problem| problem["details"]["reason"] == "query_embedding_failed")
        .cloned()
        .collect()
}

fn assert_unchanged_vector_state(repo: &ReadingRepo, config: &EmbeddingConfig) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_config_identity(), Some(config.get_identity()));
    assert_eq!(
        read.get_next_embedding_work_by_id_and_max_attempts(&config.get_identity(), 1, None)
            .unwrap(),
        VectorOutcome::Current(None)
    );
    for node in read.get_nodes().unwrap() {
        let NodeMetadata::Knowledge(node) = node.metadata else {
            continue;
        };
        assert_eq!(
            read.get_failure_by_search_hash(&config.get_identity(), &node.search_hash)
                .unwrap(),
            VectorOutcome::Current(None)
        );
    }
}
