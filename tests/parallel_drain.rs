#[path = "support/parallel_drain.rs"]
mod drain;
#[path = "support/parallel_provider.rs"]
mod provider;
mod support;

use drain::{assert_success, prepare_repo, read_hash, read_reserved, read_vector, spawn_drain};
use provider::ParallelProvider;
use std::time::{Duration, Instant};

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
    assert_eq!(read_vector(&repo, &config, &hash), Some(vec![0., 1., 0.]));
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
