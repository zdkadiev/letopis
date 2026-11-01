mod support;
#[path = "support/target_corpus.rs"]
mod target_corpus;
#[expect(
    dead_code,
    reason = "The shared slice fixture also serves unrelated CLI scenarios"
)]
#[path = "support/thin_slice.rs"]
mod thin_slice;

use letopis::{
    domain::Sha256,
    service::{Configuration, EmbeddingConfig, NodeMetadata, Storage, VectorOutcome},
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};
use support::reading::ReadingRepo;
use target_corpus::observe_request;
use tempfile::TempDir;
use thin_slice::SliceFixture;

#[test]
#[ignore = "Requires LETOPIS_CORPUS_REQUEST pointing to an existing corpus request"]
fn observe_existing_corpus() {
    // Given
    let request = std::env::var_os("LETOPIS_CORPUS_REQUEST").expect("missing request path");
    // When / Then
    observe_request(Path::new(&request)).expect("corpus observation failed");
}

#[test]
fn reports_current_vectors_and_excludes_sources_from_coverage() {
    // Given: only the product CLI publishes the graph; vectors are test-only state.
    let fixture = SliceFixture::new();
    fixture.load();
    let config = configure_repo(fixture.get_repo(), 0);
    fixture.get_repo().seed_vectors(
        &config,
        &[
            ("knowledge/cache.md", &[1., 0.]),
            ("knowledge/flow.md#reuse", &[0., 1.]),
            ("knowledge/flow.md#scope", &[1., 1.]),
        ],
    );
    let before = capture_databases(fixture.get_repo().get_root());
    let request = write_request(fixture.get_repo().get_root(), (3, 2, 5));
    // When.
    observe_request(&request).unwrap();
    let receipt = get_receipt(fixture.get_repo().get_root());
    // Then: the two source nodes neither need nor receive vectors.
    assert_eq!(capture_databases(fixture.get_repo().get_root()), before);
    assert_eq!(
        receipt,
        json!({"schema_version":1,"project_revision":1,
        "config_identity":config.get_identity().to_hex(),"dimensions":2,
        "knowledge_nodes":3,"source_nodes":2,"edges":5,"unique_search_hashes":3,
        "covered_nodes":3,"missing_nodes":0,"eligible_hashes":0,"failed_hashes":0,
        "exhausted_hashes":0,"invalid_files":0,"verdict":"populated"})
    );
}

#[test]
fn partial_coverage_counts_current_and_missing_nodes_separately() {
    // Given
    let repo = build_pair("alpha", "beta");
    let config = get_config(&repo);
    repo.seed_vectors(&config, &[("a.md", &[1., 0.])]);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When
    let receipt = get_receipt(repo.get_root());
    // Then
    assert_eq!(receipt["covered_nodes"], 1);
    assert_eq!(receipt["missing_nodes"], 1);
    assert_eq!(receipt["eligible_hashes"], 1);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn invalid_files_prevent_populated_even_when_current_nodes_are_covered() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "alpha");
    repo.write(
        "broken.md",
        b"<!-- letopis: {\"created_at\":-1} -->\nInvalid metadata",
    );
    // When
    let output = repo.invoke(&["search"], br#"{"query":"alpha","limit":1}"#);
    // Then
    assert!(output.status.success(), "{output:?}");
    let config = configure_repo(&repo, 0);
    repo.seed_vectors(&config, &[("a.md", &[1., 0.])]);
    let request = write_request(repo.get_root(), (1, 0, 0));
    observe_request(&request).unwrap();
    let receipt = get_receipt(repo.get_root());
    assert_eq!(receipt["covered_nodes"], 1);
    assert_eq!(receipt["invalid_files"], 1);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn configured_dimension_mismatch_is_rejected() {
    // Given
    let repo = build_pair("alpha", "beta");
    let configured = json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://127.0.0.1:1/api","dimensions":3},"max_retries":0});
    let bytes = json!({"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://127.0.0.1:1/api","dimensions":3},"max_retries":0}})
    .to_string();
    repo.write(".letopis/config.json", bytes.as_bytes());
    let config = EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap();
    repo.seed_vectors(&config, &[("a.md", &[1., 0.])]);
    let before = capture_databases(repo.get_root());
    let request = write_request(repo.get_root(), (2, 0, 0));
    // When
    let error = observe_request(&request).unwrap_err();
    // Then
    assert!(error.to_string().contains("dimensions mismatch"), "{error}");
    assert_eq!(capture_databases(repo.get_root()), before);
    assert!(!repo.get_root().join("observation.json").exists());
}

#[test]
fn missing_vectors_remain_missing_with_null_dimensions() {
    // Given
    let repo = build_pair("alpha", "beta");
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When
    let receipt = get_receipt(repo.get_root());
    // Then
    assert_eq!(receipt["dimensions"], Value::Null);
    assert_eq!(receipt["unique_search_hashes"], 2);
    assert_eq!(receipt["covered_nodes"], 0);
    assert_eq!(receipt["missing_nodes"], 2);
    assert_eq!(receipt["eligible_hashes"], 2);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn one_shared_hash_covers_two_nodes() {
    // Given
    let repo = build_pair("same searchable text", "same searchable text");
    let config = get_config(&repo);
    repo.seed_vectors(&config, &[("a.md", &[1., 0.])]);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When
    let receipt = get_receipt(repo.get_root());
    // Then
    assert_eq!(receipt["unique_search_hashes"], 1);
    assert_eq!(receipt["covered_nodes"], 2);
    assert_eq!(receipt["missing_nodes"], 0);
    assert_eq!(receipt["eligible_hashes"], 0);
    assert_eq!(receipt["verdict"], "populated");
}

#[test]
fn stale_hash_vector_does_not_cover_current_nodes() {
    // Given: stored vectors whose hashes no longer match current text.
    let repo = build_pair("current alpha", "current beta");
    let config = get_config(&repo);
    let mut storage = repo.open_storage();
    let mut write = unwrap_current(
        storage
            .begin_vector_write_tx(&config.get_identity())
            .unwrap(),
    );
    write
        .store_vector(&Sha256::digest(b"obsolete text"), &[1., 0.], 2)
        .unwrap();
    write.commit().unwrap();
    drop(storage);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When: the resource receipt measures coverage.
    let receipt = get_receipt(repo.get_root());
    // Then: stale hashes cover no current knowledge nodes.
    assert_eq!(receipt["dimensions"], 2);
    assert_eq!(receipt["covered_nodes"], 0);
    assert_eq!(receipt["missing_nodes"], 2);
    assert_eq!(receipt["eligible_hashes"], 2);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn exhausted_failures_have_zero_eligible_work_but_stay_missing() {
    // Given
    let repo = build_pair("alpha", "beta");
    seed_failures(&repo, 1);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When
    let receipt = get_receipt(repo.get_root());
    // Then
    assert_eq!(receipt["covered_nodes"], 0);
    assert_eq!(receipt["missing_nodes"], 2);
    assert_eq!(receipt["eligible_hashes"], 0);
    assert_eq!(receipt["failed_hashes"], 2);
    assert_eq!(receipt["exhausted_hashes"], 2);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn failed_hashes_with_budget_remaining_are_still_eligible() {
    // Given
    let repo = build_pair("alpha", "beta");
    configure_repo(&repo, 1);
    seed_failures(&repo, 2);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    // When
    let receipt = get_receipt(repo.get_root());
    // Then
    assert_eq!(receipt["eligible_hashes"], 2);
    assert_eq!(receipt["failed_hashes"], 2);
    assert_eq!(receipt["exhausted_hashes"], 0);
}

#[test]
fn exhausted_reservations_without_error_are_missing_but_not_failed() {
    // Given: exhausted reservations without failure messages.
    let repo = build_pair("alpha", "beta");
    let config = get_config(&repo);
    let mut storage = repo.open_storage();
    let nodes = storage.begin_read_tx().unwrap().get_nodes().unwrap();
    let mut write = unwrap_current(
        storage
            .begin_vector_write_tx(&config.get_identity())
            .unwrap(),
    );
    // When / Then: each current knowledge hash receives its only allowed reservation.
    for node in nodes {
        let NodeMetadata::Knowledge(node) = node.metadata else {
            continue;
        };
        // When / Then: each current knowledge hash receives its only allowed reservation.
        assert!(write.reserve_attempt(&node.search_hash, 1).unwrap());
    }
    write.commit().unwrap();
    drop(storage);
    let request = write_request(repo.get_root(), (2, 0, 0));
    observe_request(&request).unwrap();
    let receipt = get_receipt(repo.get_root());
    assert_eq!(receipt["missing_nodes"], 2);
    assert_eq!(receipt["eligible_hashes"], 0);
    assert_eq!(receipt["failed_hashes"], 0);
    assert_eq!(receipt["exhausted_hashes"], 2);
    assert_eq!(receipt["verdict"], "not_assessed");
}

#[test]
fn mismatched_configuration_fails_without_resetting_vectors() {
    // Given
    let repo = build_pair("alpha", "beta");
    let original = get_config(&repo);
    repo.seed_vectors(&original, &[("a.md", &[1., 0.])]);
    repo.write_config("ollama", "http://127.0.0.1:1/api", "different-model");
    let before = capture_databases(repo.get_root());
    let request = write_request(repo.get_root(), (2, 0, 0));
    // When
    let error = observe_request(&request).unwrap_err();
    // Then
    assert!(
        error
            .to_string()
            .contains("configuration identity mismatch"),
        "{error}"
    );
    assert_eq!(capture_databases(repo.get_root()), before);
    assert!(!repo.get_root().join("observation.json").exists());
}

#[test]
fn missing_databases_are_not_initialized() {
    // Given
    let root = TempDir::new().unwrap();
    let request = write_request(root.path(), (0, 0, 0));
    // When / Then
    assert!(observe_request(&request).is_err());
    assert!(!root.path().join(".letopis").exists());
    assert!(!root.path().join("observation.json").exists());
}

#[test]
fn empty_and_wrong_version_databases_are_unchanged() {
    // Given
    for (leaf, wrong_version) in [("cache.db", 3), ("vectors.db", 2)] {
        for version in [0, wrong_version] {
            let repo = build_pair("alpha", "beta");
            let path = repo.get_root().join(".letopis").join(leaf);
            fs::remove_file(&path).unwrap();
            let raw = rusqlite::Connection::open(&path).unwrap();
            raw.pragma_update(None, "user_version", version).unwrap();
            drop(raw);
            let before = capture_databases(repo.get_root());
            let request = write_request(repo.get_root(), (2, 0, 0));
            // When
            let error = observe_request(&request).unwrap_err();
            // Then
            assert!(
                error.to_string().contains("unsupported database schema"),
                "{error}"
            );
            assert_eq!(capture_databases(repo.get_root()), before);
            assert!(!repo.get_root().join("observation.json").exists());
        }
    }
}

#[test]
fn missing_or_non_regular_or_symlink_database_is_rejected() {
    // Given
    for kind in ["missing", "directory", "symlink"] {
        let repo = build_pair("alpha", "beta");
        let path = repo.get_root().join(".letopis/vectors.db");
        let original = fs::read(&path).unwrap();
        fs::remove_file(&path).unwrap();
        match kind {
            "directory" => fs::create_dir(&path).unwrap(),
            "symlink" => {
                fs::write(repo.get_root().join("target.db"), &original).unwrap();
                std::os::unix::fs::symlink(repo.get_root().join("target.db"), &path).unwrap();
            }
            _ => {}
        }
        let cache = fs::read(repo.get_root().join(".letopis/cache.db")).unwrap();
        let request = write_request(repo.get_root(), (2, 0, 0));
        // When / Then
        assert!(observe_request(&request).is_err());
        assert_eq!(
            fs::read(repo.get_root().join(".letopis/cache.db")).unwrap(),
            cache
        );
        assert!(!repo.get_root().join("observation.json").exists());
        if kind == "missing" {
            assert!(!path.exists());
        }
        if kind == "symlink" {
            assert_eq!(fs::read(path).unwrap(), original);
        }
    }
}

#[test]
fn expected_versions_outside_wal_mode_are_rejected_without_header_changes() {
    // Given
    for leaf in ["cache.db", "vectors.db"] {
        let repo = build_pair("alpha", "beta");
        let path = repo.get_root().join(".letopis").join(leaf);
        let raw = rusqlite::Connection::open(&path).unwrap();
        raw.pragma_update(None, "journal_mode", "delete").unwrap();
        drop(raw);
        let before = capture_databases(repo.get_root());
        let request = write_request(repo.get_root(), (2, 0, 0));
        // When
        let error = observe_request(&request).unwrap_err();
        // Then
        assert!(error.to_string().contains("product WAL mode"), "{error}");
        assert_eq!(capture_databases(repo.get_root()), before);
        assert!(!repo.get_root().join("observation.json").exists());
    }
}

#[test]
fn missing_or_invalid_configuration_does_not_reset_databases() {
    // Given
    for bytes in [None, Some(b"{}".as_slice())] {
        let repo = build_pair("alpha", "beta");
        fs::remove_file(repo.get_root().join(".letopis/config.json")).unwrap();
        if let Some(bytes) = bytes {
            repo.write(".letopis/config.json", bytes);
        }
        let before = capture_databases(repo.get_root());
        let request = write_request(repo.get_root(), (2, 0, 0));
        // When / Then
        assert!(observe_request(&request).is_err());
        assert_eq!(capture_databases(repo.get_root()), before);
        assert!(!repo.get_root().join("observation.json").exists());
    }
}

#[test]
fn expected_cache_version_without_product_schema_is_rejected() {
    // Given
    let repo = build_pair("alpha", "beta");
    let path = repo.get_root().join(".letopis/cache.db");
    fs::remove_file(&path).unwrap();
    let raw = rusqlite::Connection::open(&path).unwrap();
    raw.pragma_update(None, "user_version", 2).unwrap();
    raw.pragma_update(None, "journal_mode", "wal").unwrap();
    drop(raw);
    let before = capture_databases(repo.get_root());
    let request = write_request(repo.get_root(), (2, 0, 0));
    // When / Then
    assert!(observe_request(&request).is_err());
    assert_eq!(capture_databases(repo.get_root()), before);
}

#[test]
fn existing_receipt_is_never_overwritten() {
    // Given
    let repo = build_pair("alpha", "beta");
    repo.write("observation.json", b"preserve this receipt");
    let request = write_request(repo.get_root(), (2, 0, 0));
    // When / Then
    assert!(observe_request(&request).is_err());
    assert_eq!(
        fs::read(repo.get_root().join("observation.json")).unwrap(),
        b"preserve this receipt"
    );
}

#[test]
fn count_mismatch_is_not_a_populated_observation() {
    // Given
    let repo = build_pair("alpha", "beta");
    let before = capture_databases(repo.get_root());
    let request = write_request(repo.get_root(), (3, 0, 0));
    // When
    let error = observe_request(&request).unwrap_err();
    // Then
    assert!(
        error.to_string().contains("corpus counts mismatch"),
        "{error}"
    );
    assert_eq!(capture_databases(repo.get_root()), before);
    assert!(!repo.get_root().join("observation.json").exists());
}

#[test]
fn malformed_request_is_rejected_without_opening_databases() {
    // Given
    let root = TempDir::new().unwrap();
    let request = root.path().join("request.json");
    // When / Then
    for input in [
        "{}",
        "[]",
        "{broken",
        r#"{"root":"relative","output":"relative","expected_knowledge":0,"expected_sources":0,"expected_edges":0}"#,
    ] {
        fs::write(&request, input).unwrap();
        // When / Then
        assert!(observe_request(&request).is_err());
        assert!(!root.path().join(".letopis").exists());
    }
}

#[test]
fn ignored_entry_consumes_request_from_its_process_environment() {
    // Given
    let repo = build_pair("alpha", "beta");
    let request = write_request(repo.get_root(), (2, 0, 0));
    // When
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "observe_existing_corpus",
            "--nocapture",
        ])
        .env("LETOPIS_CORPUS_REQUEST", request)
        .output()
        .unwrap();
    // Then
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(get_receipt(repo.get_root())["knowledge_nodes"], 2);
}

fn build_pair(first: &str, second: &str) -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", first);
    repo.write_knowledge("b.md", second);
    let output = repo.invoke(&["search"], br#"{"query":"alpha","limit":1}"#);
    assert!(output.status.success(), "{output:?}");
    configure_repo(&repo, 0);
    repo
}

fn configure_repo(repo: &ReadingRepo, retries: u64) -> EmbeddingConfig {
    let configured = json!({"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://127.0.0.1:1/api"},"max_retries":retries});
    let bytes = serde_json::to_vec(&json!({"embedding_model":{"enabled":true,
        "provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:1/api"},
        "max_retries":retries}}))
    .unwrap();
    repo.write(".letopis/config.json", &bytes);
    let config = EmbeddingConfig::parse(configured.to_string().as_bytes()).unwrap();
    let mut storage = repo.open_storage();
    let observed = storage.begin_read_tx().unwrap().get_config_identity();
    storage
        .reset_vectors(observed.as_ref(), &config.get_identity())
        .unwrap();
    config
}

fn write_request(root: &Path, counts: (usize, usize, usize)) -> PathBuf {
    let path = root.join("request.json");
    fs::write(
        &path,
        serde_json::to_vec(&json!({"root":root,"output":root.join("observation.json"),
        "expected_knowledge":counts.0,"expected_sources":counts.1,"expected_edges":counts.2}))
        .unwrap(),
    )
    .unwrap();
    path
}

fn get_receipt(root: &Path) -> Value {
    serde_json::from_slice(&fs::read(root.join("observation.json")).unwrap()).unwrap()
}

fn seed_failures(repo: &ReadingRepo, budget: u64) {
    let config = get_config(repo);
    let mut storage = repo.open_storage();
    let nodes = storage.begin_read_tx().unwrap().get_nodes().unwrap();
    let mut write = unwrap_current(
        storage
            .begin_vector_write_tx(&config.get_identity())
            .unwrap(),
    );
    for node in nodes {
        let NodeMetadata::Knowledge(node) = node.metadata else {
            continue;
        };
        assert!(write.reserve_attempt(&node.search_hash, budget).unwrap());
        assert!(
            write
                .record_failure(&node.search_hash, "safe test failure")
                .unwrap()
        );
    }
    write.commit().unwrap();
}

fn get_config(repo: &ReadingRepo) -> EmbeddingConfig {
    Configuration::parse(&fs::read(repo.get_root().join(".letopis/config.json")).unwrap())
        .unwrap()
        .get_embedding_config()
        .unwrap()
        .clone()
}

fn capture_databases(root: &Path) -> Vec<Vec<u8>> {
    ["cache.db", "vectors.db"]
        .into_iter()
        .map(|leaf| fs::read(root.join(".letopis").join(leaf)).unwrap())
        .collect()
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("test identity changed"),
    }
}
