#[expect(
    dead_code,
    reason = "Shared cancellation fixtures expose unrelated wait observers"
)]
#[path = "support/cancellation.rs"]
mod cancellation;
#[expect(
    dead_code,
    reason = "Shared owned-process fixture exposes unrelated CLI runtime helpers"
)]
#[path = "support/cli.rs"]
mod cli;
mod support;

use cancellation::{
    GitReadBarrier, WAIT, assert_cancelled_output, assert_index_lock_held, get_lock_identity,
    observation::observe_index_flock, wait_until_index_lock_free,
};
use letopis::{
    domain::{EdgeTarget, KnowledgeId, Sha256},
    open_filesystem,
    service::{
        EmbeddingConfig, EmbeddingFailure, FileSystem, NodeMetadata, Storage, StorageRead,
        VectorOutcome,
    },
};
use serde_json::{Value, json};
use std::{
    ffi::OsStr,
    os::unix::process::ExitStatusExt,
    process::{Command, Output, Stdio},
};
use support::{
    embedding::{FixtureReply, HttpFixture, ResponseBarrier},
    git::isolate_command,
    reading::ReadingRepo,
};

#[test]
fn cancelling_holder_releases_index_lock() {
    // Given: real reindex holds index.lock and blocks in the Git ignore reader.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "HolderToken");
    let barrier_read = GitReadBarrier::new(repo.get_root());
    let mut child = spawn_product(&repo, &["reindex"], None);
    child.send_input(b"{}");
    barrier_read.observe(child.get_id());
    let lock = repo.get_root().join(".letopis/index.lock");
    assert_index_lock_held(&lock);
    let identity = get_lock_identity(&lock);
    // When
    child.send_sigint();
    let output = child.collect_output(WAIT);
    // Then: one cancelled envelope, and no replacement lock or surviving owner.
    assert_cancelled_output(&output, "index holder", &["HolderToken", "ls-files"]);
    barrier_read.assert_owned();
    barrier_read.release();
    wait_until_index_lock_free(&lock);
    assert_eq!(get_lock_identity(&lock), identity);
    assert_eq!(publish(&repo), 1);
    assert_search_result(&repo, "HolderToken", 1);
}

#[test]
fn cancelling_waiter_preserves_holder_and_lock_inode() {
    // Given: this test holds the real lock while the product blocks in flock.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "WaiterToken");
    let files = open_filesystem(repo.get_root()).expect("filesystem");
    let holder = files.lock_index().expect("holder lock");
    let lock = repo.get_root().join(".letopis/index.lock");
    let identity = get_lock_identity(&lock);
    let mut child = spawn_product(&repo, &["reindex"], None);
    child.send_input(b"{}");
    let observation = observe_index_flock(child.get_id(), &lock);
    assert!(observation.starts_with("inside=flock"), "{observation}");
    // When
    child.send_sigint();
    let output = child.collect_output(WAIT);
    // Then: the foreign holder and its lock file survive the waiter's death.
    assert_cancelled_output(&output, "index waiter", &["WaiterToken"]);
    assert_index_lock_held(&lock);
    assert_eq!(get_lock_identity(&lock), identity);
    drop(holder);
    wait_until_index_lock_free(&lock);
    assert_eq!(publish(&repo), 1);
    assert_search_result(&repo, "WaiterToken", 1);
}

#[test]
fn foreground_cancel_preserves_detached_drain() {
    // Given: a real drain committed its attempt and blocks on the provider response.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "DrainSurvivesToken");
    assert_eq!(publish(&repo), 1);
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,2,3]]}"#.into(),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    let hash = get_search_hash(&repo);
    let mut drain = spawn_drain(&repo);
    barrier.wait_until_entered();
    let reserved = get_failure_by_search_hash(&repo, &config, &hash).expect("durable reservation");
    assert_eq!(reserved.attempts, 1);
    let barrier_read = GitReadBarrier::new(repo.get_root());
    let mut search = spawn_product(&repo, &["search"], None);
    search.send_input(
        json!({"query":"DrainSurvivesToken","limit":1})
            .to_string()
            .as_bytes(),
    );
    barrier_read.observe(search.get_id());
    // When: only the foreground caller is cancelled while the drain stays blocked.
    search.send_sigint();
    let cancelled = search.collect_output(WAIT);
    // Then: the drain completes, stores its vector and serves the exact query later.
    assert_cancelled_output(&cancelled, "foreground search", &["DrainSurvivesToken"]);
    barrier_read.assert_owned();
    barrier_read.release();
    barrier.release();
    assert_drain_success(&drain.collect_output(WAIT));
    assert_eq!(
        get_vector_by_search_hash(&repo, &config, &hash),
        Some(vec![1., 2., 3.])
    );
    assert_search_result(&repo, "DrainSurvivesToken", 1);
}

#[test]
fn direct_drain_sigint_keeps_durable_attempt() {
    // Given: a real drain reserved one attempt and waits on the provider response.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "InterruptedDrainToken");
    assert_eq!(publish(&repo), 1);
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start(FixtureReply::Barrier {
        barrier: barrier.clone(),
        body: r#"{"model":"fixture","embeddings":[[1,2,3]]}"#.into(),
    });
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    let hash = get_search_hash(&repo);
    let mut drain = spawn_drain(&repo);
    barrier.wait_until_entered();
    let reserved = get_failure_by_search_hash(&repo, &config, &hash).expect("durable reservation");
    assert_eq!(reserved.attempts, 1);
    assert_eq!(reserved.error, None);
    // When: the drain receives SIGINT directly and has no handled response.
    drain.send_sigint();
    let interrupted = drain.collect_output(WAIT);
    // Then: default disposition, no JSON, and the committed attempt stays durable.
    assert_eq!(
        interrupted.status.signal(),
        Some(libc::SIGINT),
        "{interrupted:?}"
    );
    assert!(
        interrupted.stdout.is_empty(),
        "drain must not answer SIGINT: {interrupted:?}"
    );
    assert!(interrupted.stderr.is_empty(), "{interrupted:?}");
    barrier.release();
    assert_eq!(
        get_failure_by_search_hash(&repo, &config, &hash)
            .expect("durable attempt")
            .attempts,
        1
    );
    assert_eq!(get_vector_by_search_hash(&repo, &config, &hash), None);
    assert_eq!(http.take_requests().len(), 1);
    let status = get_success(&repo.invoke(&["status"], b"{}"));
    assert_eq!(status["result"]["revision"], 1);
    assert_eq!(status["result"]["embedding_backlog"], 0);
    assert_eq!(status["result"]["embedding_failed"], 1);
    let problems = status["result"]["problems"]
        .as_array()
        .expect("status problems");
    assert_eq!(problems.len(), 1);
    assert_eq!(
        problems[0]["details"]["reason"],
        "embedding_attempts_exhausted"
    );
    assert_eq!(problems[0]["details"]["target"], json!(hash.to_hex()));
}

#[test]
fn late_drain_vector_does_not_represent_changed_text() {
    // Given: the drain reads the old text and parks before its first response.
    let repo = ReadingRepo::new();
    repo.write_knowledge("knowledge.md", "OldDrainTextToken");
    assert_eq!(publish(&repo), 1);
    let barrier = ResponseBarrier::default();
    let http = HttpFixture::start_sequence(vec![
        FixtureReply::Barrier {
            barrier: barrier.clone(),
            body: r#"{"model":"fixture","embeddings":[[1,0]]}"#.into(),
        },
        FixtureReply::Status {
            status: 200,
            body: r#"{"model":"fixture","embeddings":[[0,1]]}"#.into(),
        },
    ]);
    let config = repo.write_config("ollama", &http.get_base_url(), "fixture");
    let old_hash = get_search_hash(&repo);
    let mut drain = spawn_drain(&repo);
    barrier.wait_until_entered();
    let first = http.take_requests();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0]["input"], json!(["OldDrainTextToken"]));
    // When: another real publication changes the text while the response is late.
    repo.write_knowledge("knowledge.md", "NewDrainTextToken");
    assert_eq!(publish(&repo), 2);
    let new_hash = get_search_hash(&repo);
    assert_ne!(new_hash, old_hash);
    assert_eq!(get_vector_by_search_hash(&repo, &config, &new_hash), None);
    barrier.release();
    assert_drain_success(&drain.collect_output(WAIT));
    // Then: the new hash is the later correct vector, never the late old response.
    assert_eq!(
        get_vector_by_search_hash(&repo, &config, &new_hash),
        Some(vec![0., 1.])
    );
    assert_ne!(
        get_vector_by_search_hash(&repo, &config, &new_hash),
        Some(vec![1., 0.])
    );
    let old = get_vector_by_search_hash(&repo, &config, &old_hash);
    assert!(old.is_none() || old == Some(vec![1., 0.]), "{old:?}");
    let second = http.take_requests();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0]["input"], json!(["NewDrainTextToken"]));
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().expect("storage read");
    let matches = match read
        .get_nearest_vectors(&config.get_identity(), &[0., 1.], 2)
        .expect("vector query")
    {
        VectorOutcome::Current(matches) => matches,
        VectorOutcome::ConfigChanged => panic!("configuration changed"),
    };
    assert_eq!(matches.len(), 1, "{matches:?}");
    assert_eq!(matches[0].id.as_str(), "knowledge.md");
    assert_eq!(matches[0].search_hash, new_hash);
    assert!(matches[0].distance.abs() < 1e-6, "{matches:?}");
}

fn spawn_product(repo: &ReadingRepo, args: &[&str], path: Option<&OsStr>) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    if let Some(path) = path {
        process.env("PATH", path);
    }
    process
        .current_dir(repo.get_root())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut process)
}

fn spawn_drain(repo: &ReadingRepo) -> cli::OwnedChild {
    let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut process);
    process
        .args(["embed", "--drain"])
        .current_dir(repo.get_root())
        .env("OPENAI_API_KEY", "fixture-secret")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut process)
}

fn get_success(output: &Output) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("one JSON response")
}

fn publish(repo: &ReadingRepo) -> u64 {
    let response = get_success(&repo.invoke(&["reindex"], b"{}"));
    assert_eq!(response["ok"], true);
    response["result"]["revision"]
        .as_u64()
        .expect("published revision")
}

fn assert_search_result(repo: &ReadingRepo, query: &str, revision: u64) {
    let input = json!({"query": query, "limit": 1}).to_string();
    let response = get_success(&repo.invoke(&["search"], input.as_bytes()));
    assert_eq!(response["ok"], true);
    assert_eq!(
        response["result"],
        json!([{
            "element_id": "knowledge.md",
            "path": "knowledge.md",
            "lines": [1, 2],
            "revision": revision,
            "flags": []
        }])
    );
}

fn get_search_hash(repo: &ReadingRepo) -> Sha256 {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().expect("storage read");
    let node = read
        .get_node_by_id(&EdgeTarget::Knowledge(
            KnowledgeId::parse("knowledge.md").expect("knowledge id"),
        ))
        .expect("read node")
        .expect("published knowledge");
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!("expected knowledge metadata");
    };
    node.search_hash
}

fn get_vector_by_search_hash(
    repo: &ReadingRepo,
    config: &EmbeddingConfig,
    search_hash: &Sha256,
) -> Option<Vec<f32>> {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().expect("storage read");
    match read
        .get_vector_by_search_hash(&config.get_identity(), search_hash)
        .expect("read vector")
    {
        VectorOutcome::Current(vector) => vector,
        VectorOutcome::ConfigChanged => panic!("configuration changed"),
    }
}

fn get_failure_by_search_hash(
    repo: &ReadingRepo,
    config: &EmbeddingConfig,
    search_hash: &Sha256,
) -> Option<EmbeddingFailure> {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().expect("storage read");
    match read
        .get_failure_by_search_hash(&config.get_identity(), search_hash)
        .expect("read failure")
    {
        VectorOutcome::Current(failure) => failure,
        VectorOutcome::ConfigChanged => panic!("configuration changed"),
    }
}

fn assert_drain_success(output: &Output) {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).expect("one JSON response"),
        json!({"ok": true, "result": {}})
    );
}
