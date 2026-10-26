mod support;

use letopis::{
    domain::{EdgeTarget, KnowledgeId},
    service::{NodeMetadata, Storage, StorageRead, VectorOutcome},
};
use serde_json::{Value, json};
use std::process::Output;
use support::{
    embedding::{FixtureReply, HttpFixture},
    reading::ReadingRepo,
};

const FINAL_MARKER: &str = "EndOfLongKnowledgeToken";
const PRIVATE_RESPONSE: &str = "private-over-context-response-canary";

#[test]
fn over_context_rejection_keeps_full_text_searchable_and_exhausts_one_attempt() {
    // Given: this synthetic length tests preservation, not a universal model limit.
    let repo = ReadingRepo::new();
    let text = format!("{}{FINAL_MARKER}", "LongKnowledgeBodyToken ".repeat(4096));
    repo.write_knowledge("long.md", &text);
    assert_success(&repo.invoke(&["reindex"], b"{}"));
    let http = HttpFixture::start(FixtureReply::Status {
        status: 400,
        body: format!(r#"{{"error":"context exceeded {PRIVATE_RESPONSE}"}}"#),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    // When: the real drain submits the whole input, then a repeated drain does no work.
    assert_success(&repo.invoke(&["embed", "--drain"], b""));
    assert_eq!(
        http.take_requests(),
        vec![json!({
            "model":"fixture", "input":[text], "truncate":false
        })]
    );
    assert_success(&repo.invoke(&["embed", "--drain"], b""));
    assert!(http.take_requests().is_empty());
    // Then: the final token is independently retrievable despite provider rejection.
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    let matches = read.match_text(FINAL_MARKER).unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id.as_str(), "long.md");
    let node = read
        .read_node(&EdgeTarget::Knowledge(
            KnowledgeId::parse("long.md").unwrap(),
        ))
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!()
    };
    let VectorOutcome::Current(Some(failure)) = read
        .read_failure(&config.get_identity(), &node.search_hash)
        .unwrap()
    else {
        panic!("expected a durable current failure")
    };
    assert_eq!(failure.attempts, 1);
    assert_eq!(
        failure.error.as_deref(),
        Some("embedding provider rejected request")
    );
    assert!(!failure.error.unwrap().contains(PRIVATE_RESPONSE));
    assert_eq!(
        read.read_vector(&config.get_identity(), &node.search_hash)
            .unwrap(),
        VectorOutcome::Current(None)
    );
}

fn assert_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true);
    assert!(!String::from_utf8_lossy(&output.stdout).contains(PRIVATE_RESPONSE));
}
