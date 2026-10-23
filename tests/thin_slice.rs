mod support;
#[path = "support/thin_slice.rs"]
mod thin_slice;

use letopis::domain::{LineRange, Sha256};
use serde_json::json;
use std::collections::BTreeSet;
use thin_slice::{ProjectionSnapshot, SliceFixture, emit_slice, read_response};

const CACHE_HASH: &str = "b3209c440805716a35f247dbecf963207fd2e7b102964a72bd6198251be5951d";
const SOURCE_HASH: &str = "551069b2bf2940ac6ce6ed7e27ee28b48097e5f13a70f132438ca4610b463e52";
const RANGE_HASH: &str = "8e0329bc53da6e5e8c823fd66d7840f3c8cbc6848040b04a58d1b10d651d5c17";

#[test]
fn binary_load_projects_sources_and_is_idempotent() {
    // Arrange: committed source and untracked knowledge, without embedding config.
    let fixture = SliceFixture::new();
    let before = fixture.capture_user_state();
    // Act: only the product executable builds the projection.
    let response = fixture.load();
    let state = fixture.read_projection();
    // Assert: independent identities, edge triples, timestamps and byte hashes.
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
    assert_eq!(fixture.read_projection(), state);
    assert_eq!(fixture.capture_user_state(), before);
    emit_slice("load", "search", 0, &response);
}

#[test]
fn binary_inspect_retains_knowledge_bytes_separately_from_source_basis() {
    // Arrange: live bytes and unrelated user Git state are independent oracles.
    let fixture = SliceFixture::new();
    let before = fixture.capture_user_state();
    let cache = fixture.read_bytes("knowledge/cache.md");
    let flow = fixture.read_bytes("knowledge/flow.md");
    let body = json!({"element_ids":["knowledge/cache.md","knowledge/flow.md#reuse"]}).to_string();
    // Act: actual CLI captures both files in one retained knowledge tree.
    let response = read_response(&fixture.get_repo().invoke(&["inspect"], body.as_bytes()), 0);
    let tree = response["result"]["snapshot"].as_str().unwrap();
    // Assert: metadata identifies the exact bytes in the returned tree.
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
    assert_eq!(fixture.read_tree_file(tree, "knowledge/cache.md"), cache);
    assert_eq!(fixture.read_tree_file(tree, "knowledge/flow.md"), flow);
    assert_eq!(
        fixture.read_tree_file(fixture.get_source_tree().as_str(), "src/cache.rs"),
        b"pub fn use_cache() -> bool {\n    true\n}\n"
    );
    assert_ne!(tree, fixture.get_source_tree().as_str());
    assert_eq!(fixture.capture_user_state(), before);
    // Act / Assert: a fresh CLI publishes an edit; the retained tree stays immutable.
    fixture.get_repo().write(
        "knowledge/cache.md",
        String::from_utf8(cache.clone())
            .unwrap()
            .replace("rebuilds", "reconstructs")
            .as_bytes(),
    );
    fixture.load();
    assert_eq!(fixture.read_tree_file(tree, "knowledge/cache.md"), cache);
    emit_slice("inspect", "inspect", 0, &response);
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
