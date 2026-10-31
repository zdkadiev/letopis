#[path = "support/parallel_provider.rs"]
mod provider;
mod support;

use provider::ParallelProvider;
use serde_json::Value;
use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::{git::OwnedChild, reading::ReadingRepo};

#[test]
fn overlaps_four_requests_without_dispatching_a_fifth() {
    // Given: eight distinct texts and a provider holding every response.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    repo.write_config("ollama", provider.get_base_url(), "fixture");
    // When: the actual drain starts without a real model.
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    // Then: four requests arrive before any response is released.
    assert_eq!(requests.len(), 4);
    assert_eq!(provider.get_maximum_active(), 4);
    assert_eq!(provider.get_active(), 4);
    assert_eq!(read_reserved(&repo), 4);
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    assert_eq!(provider.get_requests().len(), 8);
}

#[test]
fn publishes_a_completion_before_other_requests_finish() {
    // Given: every response is held independently and the second document has a literal expected vector.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    let config = repo.write_config("ollama", provider.get_base_url(), "fixture");
    let hash = read_hash(&repo, "k1.md");
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    let second = requests
        .iter()
        .find(|request| request.text.contains("parallel document 1"))
        .unwrap();
    // When: only that document completes, leaving the other three responses held.
    provider.release(second.index, &[0., 1., 0.]);
    provider.wait_requests(5);
    // Then: publication and replacement admission happen before the held responses finish.
    use letopis::service::{Storage, StorageRead, VectorOutcome};
    let stored = repo
        .open_storage()
        .begin_read()
        .unwrap()
        .read_vector(&config.get_identity(), &hash)
        .unwrap();
    assert_eq!(stored, VectorOutcome::Current(Some(vec![0., 1., 0.])));
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
}

#[test]
fn deduplicates_shared_hashes_while_other_slots_progress() {
    // Given: two files carry identical text among eight indexed documents.
    let repo = prepare_repo();
    repo.write_knowledge("k1.md", "parallel document 0");
    assert_success(&repo.invoke(&["reindex"], b"{}"));
    let provider = ParallelProvider::start();
    let config = serde_json::json!({"embedding_model": {
        "enabled": true, "provider": "ollama", "model": "fixture",
        "parameters": {"base_url": provider.get_base_url()}, "max_retries": 3
    }});
    repo.write(".letopis/config.json", config.to_string().as_bytes());
    // When: independent hashes fill the pool while the shared text remains active.
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    // Then: one shared request and three distinct requests consumed four reservations.
    assert_eq!(
        requests
            .iter()
            .filter(|request| request.text.contains("parallel document 0"))
            .count(),
        1
    );
    assert_eq!(read_reserved(&repo), 4);
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    assert_eq!(provider.get_requests().len(), 7);
}

#[test]
fn keeps_completed_unpublished_results_inside_the_four_slot_bound() {
    // Given: the initial four requests are held and another connection holds the write lock.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    repo.write_config("ollama", provider.get_base_url(), "fixture");
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    let writer = rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db")).unwrap();
    writer.execute_batch("BEGIN IMMEDIATE").unwrap();
    // When: all four HTTP responses complete while publication cannot commit.
    for request in requests {
        provider.release(request.index, &[1., 2., 3.]);
    }
    provider.wait_idle();
    // Then: those completions still own their four durable reservation slots.
    assert_eq!(read_reserved(&repo), 4);
    assert_eq!(provider.get_requests().len(), 4);
    writer.execute_batch("ROLLBACK").unwrap();
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    assert_eq!(provider.get_requests().len(), 8);
}

fn read_hash(repo: &ReadingRepo, id: &str) -> letopis::domain::Sha256 {
    use letopis::{
        domain::{EdgeTarget, KnowledgeId},
        service::{NodeMetadata, Storage, StorageRead},
    };
    let node = repo
        .open_storage()
        .begin_read()
        .unwrap()
        .read_node(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!("expected knowledge");
    };
    node.search_hash
}

fn prepare_repo() -> ReadingRepo {
    let repo = ReadingRepo::new();
    for i in 0..8 {
        repo.write_knowledge(&format!("k{i}.md"), &format!("parallel document {i}"));
    }
    assert_success(&repo.invoke(&["reindex"], b"{}"));
    repo
}

fn spawn_drain(repo: &ReadingRepo) -> OwnedChild {
    OwnedChild::spawn(
        Command::new(env!("CARGO_BIN_EXE_letopis"))
            .args(["embed", "--drain"])
            .current_dir(repo.get_root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
}

fn read_reserved(repo: &ReadingRepo) -> i64 {
    rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM embed_failures", [], |row| row.get(0))
        .unwrap()
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}
