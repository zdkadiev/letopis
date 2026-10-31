#[expect(
    dead_code,
    reason = "Shared child supervision also offers unrelated runtime fixture helpers"
)]
#[path = "support/cli.rs"]
mod cli;
#[expect(
    dead_code,
    reason = "Only detached ownership is used from the shared drain fixtures"
)]
#[path = "support/drain.rs"]
mod drain;
mod support;
#[path = "support/thin_slice.rs"]
mod thin_slice;

use letopis::domain::{LineRange, Sha256};
use letopis::service::{EmbeddingConfig, EmbeddingFailure, Storage, StorageRead, VectorOutcome};
use serde_json::json;
use std::collections::BTreeSet;
use std::{
    process::{Command, Stdio},
    time::Duration,
};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    git::isolate_command,
};
use thin_slice::{ProjectionSnapshot, SliceFixture, emit_slice, get_response};

const SUCCESS: &str = r#"{"model":"fixture","embeddings":[[1,0]]}"#;
const CACHE_TEXT: &str = "CacheScope rebuilds derived state.\nsource";
const CACHE_HASH: &str = "b3209c440805716a35f247dbecf963207fd2e7b102964a72bd6198251be5951d";
const SOURCE_HASH: &str = "551069b2bf2940ac6ce6ed7e27ee28b48097e5f13a70f132438ca4610b463e52";
const RANGE_HASH: &str = "8e0329bc53da6e5e8c823fd66d7840f3c8cbc6848040b04a58d1b10d651d5c17";

#[test]
fn binary_load_projects_sources_and_is_idempotent() {
    // Given: committed source and untracked knowledge, without embedding config.
    let fixture = SliceFixture::new();
    let before = fixture.capture_user_state();
    // When: only the product executable builds the projection.
    let response = fixture.load();
    let state = fixture.get_projection();
    // Then: independent identities, edge triples, timestamps and byte hashes.
    assert_eq!(
        (
            state.knowledge.len(),
            state.sources.len(),
            state.edges.len()
        ),
        (3, 2, 5)
    );
    assert_eq!(
        state
            .knowledge
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "knowledge/cache.md",
            "knowledge/flow.md#reuse",
            "knowledge/flow.md#scope"
        ]
    );
    assert_eq!(state.edges, build_expected_edges());
    assert_sources(&state);
    assert_eq!(
        state.knowledge["knowledge/cache.md"].search_hash.to_hex(),
        CACHE_HASH
    );
    for node in state.knowledge.values() {
        // When / Then
        assert_eq!(
            node.source_snapshot.as_ref(),
            Some(fixture.get_source_tree())
        );
    }
    assert!(
        response["result"]
            .as_array()
            .unwrap()
            .iter()
            .any(|r| r["element_id"] == "knowledge/cache.md")
    );
    fixture.load();
    assert_eq!(fixture.get_projection(), state);
    assert_eq!(fixture.capture_user_state(), before);
    emit_slice("load", "search", 0, &response);
}

#[test]
fn binary_inspect_retains_knowledge_bytes_separately_from_source_basis() {
    // Given: live bytes and unrelated user Git state are independent oracles.
    let fixture = SliceFixture::new();
    let before = fixture.capture_user_state();
    let cache = fixture.get_bytes("knowledge/cache.md");
    let flow = fixture.get_bytes("knowledge/flow.md");
    let body = json!({"element_ids":["knowledge/cache.md","knowledge/flow.md#reuse"]}).to_string();
    // When: actual CLI captures both files in one retained knowledge tree.
    let response = get_response(&fixture.get_repo().invoke(&["inspect"], body.as_bytes()), 0);
    let tree = response["result"]["snapshot"].as_str().unwrap();
    // Then: metadata identifies the exact bytes in the returned tree.
    assert_eq!(
        response["result"]["elements"]["knowledge/cache.md"]["lines"],
        json!([1, 3])
    );
    assert_eq!(
        response["result"]["elements"]["knowledge/cache.md"]["hash"],
        Sha256::digest(&cache).to_hex()
    );
    assert_eq!(
        response["result"]["elements"]["knowledge/flow.md#reuse"]["lines"],
        json!([1, 6])
    );
    assert_eq!(fixture.get_tree_file(tree, "knowledge/cache.md"), cache);
    assert_eq!(fixture.get_tree_file(tree, "knowledge/flow.md"), flow);
    assert_eq!(
        fixture.get_tree_file(fixture.get_source_tree().as_str(), "src/cache.rs"),
        b"pub fn use_cache() -> bool {\n    true\n}\n"
    );
    assert_ne!(tree, fixture.get_source_tree().as_str());
    assert_eq!(fixture.capture_user_state(), before);
    // Given: an edit is made after the retained tree was captured.
    fixture.get_repo().write(
        "knowledge/cache.md",
        String::from_utf8(cache.clone())
            .unwrap()
            .replace("rebuilds", "reconstructs")
            .as_bytes(),
    );
    // When
    fixture.load();
    // Then
    assert_eq!(fixture.get_tree_file(tree, "knowledge/cache.md"), cache);
    emit_slice("inspect", "inspect", 0, &response);
}

#[test]
fn binary_search_returns_before_background_embedding_and_reuses_vectors() {
    // Given: no vectors, and a provider that cannot return until released.
    let fixture = SliceFixture::new();
    fixture.load();
    let before = fixture.capture_user_state();
    let revision = fixture.get_projection().revision;
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture
        .get_repo()
        .write_config("ollama", &http.get_base_url(), "fixture");
    let mut foreground = spawn_product(
        &fixture,
        &["search"],
        Some(br#"{"query":"CacheScope","limit":10}"#),
    );
    let mut owner = drain::DetachedOwner::new(fixture.get_repo().get_root(), foreground.get_id());
    // When: foreground EOF arrives while document inference is still blocked.
    let response = get_response(&foreground.collect_output(Duration::from_secs(10)), 0);
    barrier.wait_until_entered();
    barrier.release();
    owner.wait_for_exit();
    // Then: persisted literal hash/vector and exactly one inference per text.
    let requests = http.take_requests();
    assert_eq!(requests.len(), 3);
    assert_eq!(requests[0]["input"], json!([CACHE_TEXT]));
    assert_eq!(
        get_vector_by_search_hash(&fixture, &config, &Sha256::parse(CACHE_HASH).unwrap()),
        Some(vec![1., 0.])
    );
    get_response(&fixture.get_repo().invoke(&["embed", "--drain"], b""), 0);
    assert!(http.take_requests().is_empty());
    assert_eq!(fixture.get_projection().revision, revision);
    assert_eq!(fixture.capture_user_state(), before);
    emit_slice("background", "search", 0, &response);
}

#[test]
fn binary_provider_failure_preserves_fts_and_bounds_attempts() {
    // Given: a fixed one-attempt budget and a private provider payload.
    let fixture = SliceFixture::new();
    fixture.load();
    let before = fixture.capture_user_state();
    let state = fixture.get_projection();
    let secret = "TK14_PROVIDER_BODY_SECRET";
    let http = HttpFixture::start(FixtureReply::Status {
        status: 503,
        body: secret.into(),
    });
    let config = fixture
        .get_repo()
        .write_config("ollama", &http.get_base_url(), "fixture");
    // When: drain consumes bounded durable attempts; FTS remains available.
    let output = fixture.get_repo().invoke(&["embed", "--drain"], b"");
    get_response(&output, 0);
    let response = fixture.load();
    // Then: neither retrying drain nor searching creates fresh document work.
    for node in state.knowledge.values() {
        let failure = get_failure_by_search_hash(&fixture, &config, &node.search_hash);
        // When / Then
        assert_eq!(failure.attempts, 1);
        assert_eq!(
            failure.error.as_deref(),
            Some("embedding provider rejected request")
        );
        assert_eq!(
            get_vector_by_search_hash(&fixture, &config, &node.search_hash),
            None
        );
    }
    assert_eq!(http.take_requests().len(), 3);
    get_response(&fixture.get_repo().invoke(&["embed", "--drain"], b""), 0);
    assert!(http.take_requests().is_empty());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(secret));
    assert!(!response["result"].as_array().unwrap().is_empty());
    assert_eq!(fixture.get_projection(), state);
    assert_eq!(fixture.capture_user_state(), before);
    emit_slice("provider_failure", "search", 0, &response);
}

#[test]
fn binary_invalid_file_keeps_last_projection_and_publishes_valid_neighbor() {
    // Given: two indexed files, a bad edit and a valid neighboring edit.
    let fixture = SliceFixture::new();
    fixture.load();
    let state = fixture.get_projection();
    let cache = fixture.get_bytes("knowledge/cache.md");
    fixture.get_repo().write(
        "knowledge/cache.md",
        b"<!-- letopis: {broken} -->\nInvalidToken\n",
    );
    let flow = String::from_utf8(fixture.get_bytes("knowledge/flow.md"))
        .unwrap()
        .replace("CacheReuse", "FreshNeighbor");
    fixture
        .get_repo()
        .write("knowledge/flow.md", flow.as_bytes());
    // When: one real refresh must isolate file validity.
    let response = get_response(
        &fixture
            .get_repo()
            .invoke(&["search"], br#"{"query":"FreshNeighbor","limit":10}"#),
        0,
    );
    // Then: valid membership advances, invalid current inspection rejects.
    assert_eq!(
        response["result"][0]["element_id"],
        "knowledge/flow.md#reuse"
    );
    assert_eq!(
        fixture.get_projection().knowledge["knowledge/cache.md"],
        state.knowledge["knowledge/cache.md"]
    );
    assert_invalid_row(&fixture);
    // When
    let invalid = get_response(
        &fixture
            .get_repo()
            .invoke(&["inspect"], br#"{"element_ids":["knowledge/cache.md"]}"#),
        1,
    );
    // Then
    assert_eq!(invalid["error"]["code"], "validation_failed");
    assert!(invalid.get("result").is_none());
    emit_slice("invalid_inspect", "inspect", 1, &invalid);
    fixture.get_repo().write("knowledge/cache.md", &cache);
    fixture.load();
    assert!(
        fixture
            .get_repo()
            .open_storage()
            .begin_read()
            .unwrap()
            .get_files()
            .unwrap()
            .iter()
            .all(|row| row.valid)
    );
    emit_slice("invalid_neighbor", "search", 0, &response);
}

#[test]
fn binary_text_edit_during_document_http_requires_its_own_vector() {
    // Given: the first cache inference is held before any vector is published.
    let fixture = SliceFixture::new();
    fixture.load();
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: SUCCESS.into(),
    });
    let config = fixture
        .get_repo()
        .write_config("ollama", &http.get_base_url(), "fixture");
    let mut child = spawn_product(&fixture, &["embed", "--drain"], None);
    barrier.wait_until_entered();
    // When: publication uses CLI while HTTP owns no cache write transaction.
    let cache = String::from_utf8(fixture.get_bytes("knowledge/cache.md"))
        .unwrap()
        .replace("rebuilds", "reconstructs");
    fixture
        .get_repo()
        .write("knowledge/cache.md", cache.as_bytes());
    let response = fixture.load();
    let state = fixture.get_projection();
    let new_hash = state.knowledge["knowledge/cache.md"].search_hash;
    let old_hash = Sha256::parse(CACHE_HASH).unwrap();
    // Then
    assert_ne!(new_hash, old_hash);
    assert_eq!(
        get_vector_by_search_hash(&fixture, &config, &new_hash),
        None
    );
    barrier.release();
    get_response(&child.collect_output(Duration::from_secs(15)), 0);
    // Then: both texts required inference; the obsolete result cannot stand in.
    let requests = http.take_requests();
    assert_eq!(requests.len(), 4);
    assert_eq!(requests[0]["input"], json!([CACHE_TEXT]));
    assert_eq!(
        requests[1]["input"],
        json!(["CacheScope reconstructs derived state.\nsource"])
    );
    assert_eq!(
        get_vector_by_search_hash(&fixture, &config, &new_hash),
        Some(vec![1., 0.])
    );
    assert_eq!(
        get_vector_by_search_hash(&fixture, &config, &old_hash),
        None
    );
    assert_eq!(fixture.get_bytes("knowledge/cache.md"), cache.as_bytes());
    emit_slice("text_race", "search", 0, &response);
}

fn build_expected_edges() -> BTreeSet<(String, String, String)> {
    [
        ("knowledge/cache.md", "src/cache.rs", "external"),
        (
            "knowledge/flow.md#reuse",
            "knowledge/cache.md",
            "depends_on",
        ),
        ("knowledge/flow.md#reuse", "src/cache.rs#L2-L2", "external"),
        ("knowledge/flow.md#scope", "knowledge/cache.md", "includes"),
        ("knowledge/flow.md#scope", "src/cache.rs", "external"),
    ]
    .into_iter()
    .map(|(a, b, c)| (a.into(), b.into(), c.into()))
    .collect()
}

fn assert_sources(state: &ProjectionSnapshot) {
    let whole = &state.sources["src/cache.rs"];
    let range = &state.sources["src/cache.rs#L2-L2"];
    assert_eq!(
        (whole.created_at, whole.lines, whole.hash),
        (10, None, Some(Sha256::parse(SOURCE_HASH).unwrap()))
    );
    assert_eq!(
        (range.created_at, range.lines, range.hash),
        (
            20,
            Some(LineRange::new(2, 2).unwrap()),
            Some(Sha256::parse(RANGE_HASH).unwrap())
        )
    );
}

fn spawn_product(fixture: &SliceFixture, args: &[&str], input: Option<&[u8]>) -> cli::OwnedChild {
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    command
        .current_dir(fixture.get_repo().get_root())
        .args(args)
        .env("OPENAI_API_KEY", "fixture-secret")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.stdin(if input.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    let mut child = cli::OwnedChild::spawn(&mut command);
    if let Some(input) = input {
        child.send_input(input);
    }
    child
}
fn get_vector_by_search_hash(
    fixture: &SliceFixture,
    config: &EmbeddingConfig,
    hash: &Sha256,
) -> Option<Vec<f32>> {
    let mut storage = fixture.get_repo().open_storage();
    let result = storage
        .begin_read()
        .unwrap()
        .get_vector_by_search_hash(&config.get_identity(), hash)
        .unwrap();
    let VectorOutcome::Current(vector) = result else {
        panic!("configuration changed")
    };
    vector
}
fn get_failure_by_search_hash(
    fixture: &SliceFixture,
    config: &EmbeddingConfig,
    hash: &Sha256,
) -> EmbeddingFailure {
    let mut storage = fixture.get_repo().open_storage();
    let result = storage
        .begin_read()
        .unwrap()
        .get_failure_by_search_hash(&config.get_identity(), hash)
        .unwrap();
    let VectorOutcome::Current(Some(failure)) = result else {
        panic!("expected failure")
    };
    failure
}
fn assert_invalid_row(fixture: &SliceFixture) {
    let mut storage = fixture.get_repo().open_storage();
    let files = storage.begin_read().unwrap().get_files().unwrap();
    let row = files
        .iter()
        .find(|row| row.path.as_str() == "knowledge/cache.md")
        .unwrap();
    assert!(!row.valid);
    assert!(row.error.is_some());
}
