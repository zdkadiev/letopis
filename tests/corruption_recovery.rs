mod support;

use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, RepoPath, Sha256},
    open_filesystem,
    service::{EmbedLocker, EmbeddingConfig, NodeMetadata, Storage, StorageRead, VectorOutcome},
};
use rusqlite::{Connection, OpenFlags};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    num::NonZeroUsize,
    path::Path,
    process::{Command, Output},
};
use support::{
    embedding::{FixtureReply, HttpFixture},
    git::isolate_command,
    reading::ReadingRepo,
};

const CACHE: &str = "cache.db";
const VECTORS: &str = "vectors.db";
const CORRUPT_BYTES: &[u8] = b"not sqlite CORRUPTION_CANARY";
const FIRST_VECTOR: &[f32] = &[0.25_f32, 0.75_f32];
const SECOND_VECTOR: &[f32] = &[0.75_f32, 0.25_f32];
const CANONICAL_PATHS: &[&str] = &[
    "a.md",
    "b.md",
    "source.txt",
    "live.md",
    ".letopis/config.json",
    ".letopis/operator-sentinel",
    ".git/HEAD",
    ".git/index",
    ".git/config",
];

#[derive(Clone, Copy, Debug)]
enum Damage {
    Missing,
    NonemptyCorrupt,
}

#[derive(Debug, PartialEq, Eq)]
struct CanonicalState {
    files: Vec<Option<Vec<u8>>>,
    user_refs: Vec<u8>,
}

#[test]
fn reindex_recovers_explicitly_removed_cache_vectors_or_both() {
    // Given
    for targets in [&[CACHE][..], &[VECTORS][..], &[CACHE, VECTORS][..]] {
        for damage in [Damage::Missing, Damage::NonemptyCorrupt] {
            // When / Then: the helper performs recovery and checks the recovered state.
            verify_removed_databases_recover(targets, damage);
        }
    }
}

#[test]
fn reindex_preserves_healthy_vectors_after_corrupt_cache_removal() {
    // Given: every current knowledge hash has a known two-dimensional vector.
    let repo = publish_fixture();
    let provider = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "unexpected inference CORRUPTION_CANARY".to_owned(),
    });
    let config = repo.write_config("ollama", &provider.get_base_url(), "fixture");
    repo.seed_vectors(&config, &[("a.md", FIRST_VECTOR), ("b.md", SECOND_VECTOR)]);
    let hashes = read_search_hashes(&repo);
    let canonical = capture_canonical_state(&repo);
    let files = open_filesystem(repo.get_root()).unwrap();
    let _embed_lock = files.try_lock_embed().unwrap().unwrap();
    corrupt_owned_database(repo.get_root(), CACHE);

    // When / Then: damage persists until the operator explicitly removes cache only.
    assert_persistent_corruption(&repo, &[CACHE], &canonical);
    remove_owned_database(repo.get_root(), CACHE);
    assert_missing_cache_status(&repo);
    assert_eq!(
        invoke_reindex(&repo),
        json!({"ok":true,"result":{"revision":1}})
    );
    assert_projection(&repo, 1);
    assert_retained_vectors(&repo, &config, &hashes);
    assert_eq!(capture_canonical_state(&repo), canonical);

    // When / Then: reopen storage and use another CLI process to prove stability.
    assert_eq!(
        invoke_reindex(&repo),
        json!({"ok":true,"result":{"revision":2}})
    );
    assert_projection(&repo, 2);
    assert_retained_vectors(&repo, &config, &hashes);
    assert_eq!(capture_canonical_state(&repo), canonical);
    assert!(
        provider.take_requests().is_empty(),
        "reindex must not invoke inference"
    );
}

fn verify_removed_databases_recover(targets: &[&str], damage: Damage) {
    // Given: a real publication counter of three must not be invented after loss.
    let repo = publish_fixture();
    let canonical = capture_canonical_state(&repo);
    if matches!(damage, Damage::NonemptyCorrupt) {
        for target in targets {
            corrupt_owned_database(repo.get_root(), target);
        }
        assert_persistent_corruption(&repo, targets, &canonical);
    }

    // When: all own connections and CLI children have closed before physical loss.
    for target in targets {
        remove_owned_database(repo.get_root(), target);
    }
    if targets.contains(&CACHE) {
        assert_missing_cache_status(&repo);
    }
    let revision = if targets.contains(&CACHE) { 1 } else { 4 };
    assert_eq!(
        invoke_reindex(&repo),
        json!({"ok":true,"result":{"revision":revision}})
    );

    // Then: only text projections recover without a configured inference provider.
    assert_projection(&repo, revision);
    assert_empty_vectors(&repo);
    assert_eq!(capture_canonical_state(&repo), canonical);
    assert_eq!(invoke_reindex(&repo)["result"]["revision"], revision + 1);
    assert_projection(&repo, revision + 1);
    assert_empty_vectors(&repo);
    assert_eq!(capture_canonical_state(&repo), canonical);
}

fn publish_fixture() -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken [peer](b.md \"letopis:type=context\")");
    repo.write_knowledge("b.md", "BravoToken");
    repo.write("source.txt", b"authored source CORRUPTION_CANARY\n");
    run_git(&repo, &["update-ref", "refs/heads/user-preserved", "HEAD"]);
    for revision in 1..=3 {
        assert_eq!(invoke_reindex(&repo)["result"]["revision"], revision);
    }
    repo.write(
        ".letopis/operator-sentinel",
        b"keep this owned non-database file\n",
    );
    assert_projection(&repo, 3);
    repo
}

fn assert_persistent_corruption(repo: &ReadingRepo, targets: &[&str], state: &CanonicalState) {
    for command in ["reindex", "reindex", "status", "status"] {
        let output = repo.invoke(&[command], b"{}");
        let response = read_response(&output, 1);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "internal_error");
        assert!(response.get("result").is_none());
        let visible = String::from_utf8(output.stdout).unwrap();
        for secret in [
            "CORRUPTION_CANARY",
            "AlphaToken",
            "BravoToken",
            "fixture-secret",
            "letopis-git-",
        ] {
            assert!(!visible.contains(secret), "unsafe diagnostic: {visible}");
        }
        for target in targets {
            assert_eq!(
                fs::read(repo.get_root().join(".letopis").join(target)).unwrap(),
                CORRUPT_BYTES
            );
        }
        assert_healthy_peer(repo, targets);
        assert_eq!(&capture_canonical_state(repo), state);
    }
}

fn invoke_reindex(repo: &ReadingRepo) -> Value {
    let response = read_response(&repo.invoke(&["reindex"], b"{}"), 0);
    assert_eq!(response["ok"], true);
    response
}

fn assert_missing_cache_status(repo: &ReadingRepo) {
    let response = read_response(&repo.invoke(&["status"], b"{}"), 0);
    assert_eq!(response["ok"], true);
    assert_eq!(response["result"]["revision"], Value::Null);
    assert!(!repo.get_root().join(".letopis/cache.db").exists());
}

fn assert_projection(repo: &ReadingRepo, revision: u64) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), revision);
    let nodes = read.list_nodes().unwrap();
    let ids: Vec<&str> = nodes
        .iter()
        .map(|node| match &node.metadata {
            NodeMetadata::Knowledge(node) => node.id.as_str(),
            NodeMetadata::Source(_) => panic!("unexpected source node"),
        })
        .collect();
    assert_eq!(ids, ["a.md", "b.md"]);
    for (token, id) in [("AlphaToken", "a.md"), ("BravoToken", "b.md")] {
        let matches = read.match_text(token).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id.as_str(), id);
    }
    let edge = Edge::new(
        KnowledgeId::parse("a.md").unwrap(),
        EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
        EdgeType::parse("context").unwrap(),
    )
    .unwrap();
    assert_eq!(read.list_edges(None).unwrap(), [edge]);
    let files = read.list_files().unwrap();
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a.md", "b.md", "live.md"]);
    for path in ["a.md", "b.md", "live.md"] {
        let file = read
            .read_file(&RepoPath::parse(path).unwrap())
            .unwrap()
            .unwrap();
        assert!(file.valid);
        assert_eq!(file.error, None);
        assert_eq!(
            file.size,
            fs::metadata(repo.get_root().join(path)).unwrap().len()
        );
    }
}

fn assert_retained_vectors(repo: &ReadingRepo, config: &EmbeddingConfig, hashes: &[Sha256]) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    let identity = config.get_identity();
    assert_eq!(read.get_config_identity(), Some(identity));
    assert_eq!(
        read.read_vector_dimensions(&identity).unwrap(),
        VectorOutcome::Current(NonZeroUsize::new(2))
    );
    for ((id, hash), expected) in ["a.md", "b.md"]
        .into_iter()
        .zip(hashes)
        .zip([FIRST_VECTOR, SECOND_VECTOR])
    {
        let node = read
            .read_node(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
            .unwrap()
            .unwrap();
        let NodeMetadata::Knowledge(node) = node.metadata else {
            panic!("expected knowledge")
        };
        assert_eq!(&node.search_hash, hash);
        assert_eq!(
            read.read_vector(&identity, hash).unwrap(),
            VectorOutcome::Current(Some(expected.to_vec()))
        );
    }
}

fn read_search_hashes(repo: &ReadingRepo) -> Vec<Sha256> {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read().unwrap();
    ["a.md", "b.md"]
        .into_iter()
        .map(|id| {
            let node = read
                .read_node(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
                .unwrap()
                .unwrap();
            let NodeMetadata::Knowledge(node) = node.metadata else {
                panic!("expected knowledge")
            };
            node.search_hash
        })
        .collect()
}

fn assert_empty_vectors(repo: &ReadingRepo) {
    let mut storage = repo.open_storage();
    assert_eq!(storage.begin_read().unwrap().get_config_identity(), None);
    drop(storage);
    let db = open_database(repo.get_root(), VECTORS);
    let count: i64 = db
        .query_row("SELECT count(*) FROM vector_keys", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

fn assert_healthy_peer(repo: &ReadingRepo, damaged: &[&str]) {
    if !damaged.contains(&CACHE) {
        let db = open_database(repo.get_root(), CACHE);
        let revision: i64 = db
            .query_row("SELECT revision FROM project WHERE id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(revision, 3);
        let mut statement = db.prepare("SELECT id FROM nodes ORDER BY id").unwrap();
        let ids: Vec<String> = statement
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(ids, ["a.md", "b.md"]);
    }
    if !damaged.contains(&VECTORS) {
        let db = open_database(repo.get_root(), VECTORS);
        let count: i64 = db
            .query_row("SELECT count(*) FROM vector_keys", [], |row| row.get(0))
            .unwrap();
        // The configured retention case seeds both hashes; other cases have no config.
        let expected = if repo.get_root().join(".letopis/config.json").exists() {
            2
        } else {
            0
        };
        assert_eq!(count, expected);
    }
}

fn open_database(root: &Path, name: &str) -> Connection {
    Connection::open_with_flags(
        root.join(".letopis").join(name),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

fn corrupt_owned_database(root: &Path, name: &str) {
    remove_owned_database(root, name);
    fs::write(root.join(".letopis").join(name), CORRUPT_BYTES).unwrap();
}

/// Remove only the exact owned database and WAL/SHM after callers close resources.
fn remove_owned_database(root: &Path, name: &str) {
    assert!([CACHE, VECTORS].contains(&name));
    let owned_root = fs::canonicalize(root).unwrap();
    assert_eq!(owned_root, root);
    let parent = root.join(".letopis");
    assert_eq!(fs::canonicalize(&parent).unwrap(), parent);
    for suffix in ["", "-wal", "-shm"] {
        let path = parent.join(format!("{name}{suffix}"));
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                assert!(
                    metadata.file_type().is_file(),
                    "refuse non-file database target"
                );
                assert_eq!(fs::canonicalize(path.parent().unwrap()).unwrap(), parent);
                fs::remove_file(path).unwrap();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => panic!("cannot inspect owned database: {error}"),
        }
    }
}

fn capture_canonical_state(repo: &ReadingRepo) -> CanonicalState {
    let refs = run_git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]);
    CanonicalState {
        files: CANONICAL_PATHS
            .iter()
            .map(|path| fs::read(repo.get_root().join(path)).ok())
            .collect(),
        user_refs: refs
            .split_inclusive(|byte| *byte == b'\n')
            .filter(|line| !line.starts_with(b"refs/letopis/snapshots/"))
            .flatten()
            .copied()
            .collect(),
    }
}

fn run_git(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

fn read_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    serde_json::from_slice(&output.stdout).unwrap()
}
