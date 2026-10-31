#[path = "support/parallel_drain.rs"]
mod drain;
#[path = "support/parallel_provider.rs"]
mod provider;
mod support;

use drain::{assert_success, prepare_repo, read_hash, read_reserved, read_vector, spawn_drain};
use letopis::{
    open_filesystem,
    service::{EmbedLocker, Refresher, Storage, StorageRead, VectorOutcome},
    wire_document_refresher,
};
use provider::ParallelProvider;
use std::{
    fs,
    time::{Duration, Instant},
};

#[test]
fn keeps_one_drain_owner_with_four_active_requests() {
    // Given: one actual drain owns four held HTTP requests.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    repo.write_config("ollama", provider.get_base_url(), "fixture");
    let first = spawn_drain(&repo);
    provider.wait_requests(4);
    assert_eq!(provider.get_active(), 4);
    // When: another actual drain tries to acquire the same ownership.
    let second = spawn_drain(&repo);
    assert_success(&second.wait(Instant::now() + Duration::from_secs(3)));
    // Then: the second process returns without sending another request.
    assert_eq!(provider.get_requests().len(), 4);
    assert_eq!(read_reserved(&repo), 4);
    provider.release_all();
    assert_success(&first.wait(Instant::now() + Duration::from_secs(10)));
    provider.wait_idle();
    assert_eq!(provider.get_maximum_active(), 4);
}

#[test]
fn preserves_reserved_attempts_after_kill_and_restart() {
    // Given: four durable reservations precede their still-held HTTP responses.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    let config = repo.write_config("ollama", provider.get_base_url(), "fixture");
    let first_hash = read_hash(&repo, "k0.md");
    let mut first = spawn_drain(&repo);
    provider.wait_requests(4);
    assert_eq!(read_reserved(&repo), 4);
    // When: terminate and reap only this fixture's process, then run a new drain.
    first.kill();
    let files = open_filesystem(repo.get_root()).unwrap();
    let guard = files
        .try_lock_embed()
        .unwrap()
        .expect("kernel released the dead owner's lock");
    drop(guard);
    provider.release_all();
    assert_success(&spawn_drain(&repo).wait(Instant::now() + Duration::from_secs(10)));
    // Then: zero-retry reservations stay spent; only the other four texts are requested.
    assert_eq!(provider.get_requests().len(), 8);
    assert_eq!(read_reserved(&repo), 4);
    assert_eq!(read_vector(&repo, &config, &first_hash), None);
    let attempts: i64 = rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db"))
        .unwrap()
        .query_row("SELECT sum(attempts) FROM embed_failures", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(attempts, 4);
}

#[test]
fn discards_old_identity_completions_after_reset() {
    // Given: old-identity requests are held while another writer establishes a new model.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    repo.write_config("ollama", provider.get_base_url(), "fixture");
    let hash = read_hash(&repo, "k7.md");
    let other = read_hash(&repo, "k0.md");
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    let new = repo.write_config("ollama", provider.get_base_url(), "new-model");
    repo.seed_vectors(&new, &[("k7.md", &[1., 0., 0.])]);
    // When: all old requests return different vectors after the reset.
    for request in requests {
        provider.release(request.index, &[0., 1., 0.]);
    }
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    // Then: no old result or attempt changes the new identity's literal vector.
    assert_eq!(read_vector(&repo, &new, &hash), Some(vec![1., 0., 0.]));
    assert_eq!(read_vector(&repo, &new, &other), None);
    let keys: i64 = rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM vector_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(keys, 1);
    assert_eq!(read_reserved(&repo), 0);
    assert_eq!(provider.get_requests().len(), 4);
}

#[test]
fn never_assigns_old_results_to_edited_or_deleted_text() {
    // Given: original text hashes are captured before four document requests complete.
    let repo = prepare_repo();
    let provider = ParallelProvider::start();
    let config = repo.write_config("ollama", provider.get_base_url(), "fixture");
    let old = read_hash(&repo, "k0.md");
    let deleted = read_hash(&repo, "k1.md");
    let child = spawn_drain(&repo);
    let requests = provider.wait_requests(4);
    // When: edit one file and delete another, then publish while HTTP remains held.
    repo.write_knowledge("k0.md", "replacement text requiring its own request");
    fs::remove_file(repo.get_root().join("k1.md")).unwrap();
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let current = read_hash(&repo, "k0.md");
    assert_ne!(current, old);
    for request in requests {
        provider.release(request.index, &[1., 0., 0.]);
    }
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    // Then: new text got its own fallback vector and obsolete hashes were pruned.
    assert_eq!(
        read_vector(&repo, &config, &current),
        Some(vec![1., 2., 3.])
    );
    assert_eq!(read_vector(&repo, &config, &old), None);
    assert_eq!(read_vector(&repo, &config, &deleted), None);
    assert_eq!(provider.get_requests().len(), 9);
}

#[test]
fn rejects_out_of_order_dimension_mismatch() {
    // Given: four requests capture an unknown dimension and their responses are held.
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
    // When: the second document establishes two dimensions; all other replies have three.
    provider.release(second.index, &[1., 2.]);
    provider.wait_requests(5);
    provider.release_all();
    assert_success(&child.wait(Instant::now() + Duration::from_secs(10)));
    // Then: one literal vector survives, while seven wrong-dimension attempts fail safely.
    assert_eq!(read_vector(&repo, &config, &hash), Some(vec![1., 2.]));
    let size = repo
        .open_storage()
        .begin_read()
        .unwrap()
        .read_vector_dimensions(&config.get_identity())
        .unwrap();
    assert_eq!(size, VectorOutcome::Current(std::num::NonZeroUsize::new(2)));
    assert_eq!(read_reserved(&repo), 7);
    let errors: i64 = rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db")).unwrap()
        .query_row("SELECT count(*) FROM embed_failures WHERE attempts = 1 AND error = 'invalid embedding response'", [], |row| row.get(0)).unwrap();
    assert_eq!(errors, 7);
}
