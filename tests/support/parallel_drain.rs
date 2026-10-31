use crate::support::{
    git::{OwnedChild, isolate_command},
    reading::ReadingRepo,
};
use letopis::{
    domain::{EdgeTarget, KnowledgeId, Sha256},
    service::{EmbeddingConfig, NodeMetadata, Storage, StorageRead, VectorOutcome},
};
use serde_json::Value;
use std::process::{Command, Output, Stdio};

/// Read a published fixture knowledge hash by its literal ID.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn read_hash(repo: &ReadingRepo, id: &str) -> Sha256 {
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

/// Publish eight distinct documents in an isolated repository without a provider.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn prepare_repo() -> ReadingRepo {
    let repo = ReadingRepo::new();
    for i in 0..8 {
        repo.write_knowledge(&format!("k{i}.md"), &format!("parallel document {i}"));
    }
    assert_success(&repo.invoke(&["reindex"], b"{}"));
    repo
}

/// Own a real drain process with isolated Git settings and synthetic credentials.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn spawn_drain(repo: &ReadingRepo) -> OwnedChild {
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    OwnedChild::spawn(
        command
            .args(["embed", "--drain"])
            .env("OPENAI_API_KEY", "parallel-fixture-secret")
            .current_dir(repo.get_root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped()),
    )
}

/// Count actual durable hash reservations through an independent `SQLite` query.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn read_reserved(repo: &ReadingRepo) -> i64 {
    rusqlite::Connection::open(repo.get_root().join(".letopis/vectors.db"))
        .unwrap()
        .query_row("SELECT count(*) FROM embed_failures", [], |row| row.get(0))
        .unwrap()
}

/// Require a successful process and one successful product JSON envelope.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn assert_success(output: &Output) {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true, "{value}");
}

/// Read a vector under the expected captured identity without refreshing files.
///
/// # Panics
/// Panics if fixture I/O or the required captured-state assertion fails.
pub fn read_vector(
    repo: &ReadingRepo,
    config: &EmbeddingConfig,
    hash: &Sha256,
) -> Option<Vec<f32>> {
    let VectorOutcome::Current(vector) = repo
        .open_storage()
        .begin_read()
        .unwrap()
        .read_vector(&config.get_identity(), hash)
        .unwrap()
    else {
        panic!("configuration changed");
    };
    vector
}
