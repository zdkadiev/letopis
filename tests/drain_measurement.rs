#[path = "support/storage.rs"]
mod fixtures;

use letopis::{
    domain::{EdgeTarget, KnowledgeId, Sha256},
    open_storage,
    service::{
        CacheWrite, Configuration, NodeMetadata, Storage, StorageRead, VectorOutcome, VectorWrite,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    path::PathBuf,
};
use tempfile::TempDir;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    root: PathBuf,
    expected: BTreeMap<String, Vec<f32>>,
    output: PathBuf,
}

#[derive(Debug, Serialize)]
struct Observation {
    vectors: BTreeMap<String, Vec<f32>>,
    unique_vectors: usize,
    missing: Vec<String>,
    failed_hashes: usize,
    query_error: Option<String>,
    config_identity: String,
}

#[test]
#[ignore = "Requires LETOPIS_DRAIN_OBSERVATION for a prepared synthetic fixture"]
fn observe_existing_drain() {
    // Given: an explicit standalone fixture and independently supplied literal vectors.
    let path = std::env::var_os("LETOPIS_DRAIN_OBSERVATION").unwrap();
    let request: Request = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    // When: observe existing native vector state outside the measured drain interval.
    let observation = observe(&request);
    // Then: create a new receipt and never replace an earlier observation.
    let output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&request.output)
        .unwrap();
    serde_json::to_writer_pretty(output, &observation).unwrap();
}

#[test]
fn observes_literal_vectors_from_the_prepared_cache() {
    // Given: one actual stored vector with an independent literal expected value.
    let (_root, request) = prepare_fixture(&[1., 0., 0.]);
    // When: the native observation reads one pinned view.
    let observed = observe(&request);
    // Then: bytes, coverage and errors reflect actual stored state.
    assert_eq!(
        observed.vectors,
        BTreeMap::from([("k.md".into(), vec![1., 0., 0.])])
    );
    assert_eq!(observed.unique_vectors, 1);
    assert!(observed.missing.is_empty());
    assert_eq!(observed.failed_hashes, 0);
}

#[test]
#[should_panic(expected = "stored vector differs from fixture oracle")]
fn rejects_a_vector_associated_with_the_wrong_fixture_text() {
    // Given: valid finite bytes belonging to a different expected fixture vector.
    let (_root, request) = prepare_fixture(&[0., 1., 0.]);
    // When / Then: the native oracle must reject correct coverage with wrong contents.
    observe(&request);
}

fn observe(request: &Request) -> Observation {
    let cache = request.root.join(".letopis/cache.db");
    let vectors = request.root.join(".letopis/vectors.db");
    assert!(
        cache.is_file() && vectors.is_file(),
        "existing fixture databases required"
    );
    let config =
        Configuration::parse(&fs::read(request.root.join(".letopis/config.json")).unwrap())
            .unwrap();
    let config = config.get_embedding_config().unwrap();
    let mut storage = open_storage(&cache, &vectors).unwrap();
    let mut read = storage.begin_read().unwrap();
    read_observation(&mut read, &config.get_identity(), &request.expected)
}

fn read_observation(
    read: &mut dyn StorageRead,
    identity: &Sha256,
    expected: &BTreeMap<String, Vec<f32>>,
) -> Observation {
    assert_eq!(read.list_nodes().unwrap().len(), expected.len());
    let mut observed = BTreeMap::new();
    let mut covered = BTreeSet::new();
    let mut failed = BTreeSet::new();
    let mut missing = Vec::new();
    for id in expected.keys() {
        let hash = read_knowledge_hash(read, id);
        let VectorOutcome::Current(vector) = read.read_vector(identity, &hash).unwrap() else {
            panic!("configuration changed");
        };
        if let Some(vector) = vector {
            assert_eq!(
                vector, expected[id],
                "stored vector differs from fixture oracle"
            );
            observed.insert(id.clone(), vector);
            covered.insert(hash.to_hex());
        } else {
            missing.push(id.clone());
        }
        let VectorOutcome::Current(failure) = read.read_failure(identity, &hash).unwrap() else {
            panic!("configuration changed");
        };
        if failure.is_some_and(|failure| failure.error.is_some()) {
            failed.insert(hash.to_hex());
        }
    }
    Observation {
        vectors: observed,
        unique_vectors: covered.len(),
        missing,
        failed_hashes: failed.len(),
        query_error: read.read_query_error().unwrap().map(|error| error.error),
        config_identity: identity.to_hex(),
    }
}

fn read_knowledge_hash(read: &mut dyn StorageRead, id: &str) -> Sha256 {
    let node = read
        .read_node(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
        .unwrap()
        .unwrap();
    let NodeMetadata::Knowledge(node) = node.metadata else {
        panic!("fixture knowledge required");
    };
    node.search_hash
}

fn prepare_fixture(vector: &[f32]) -> (TempDir, Request) {
    let root = TempDir::new().unwrap();
    let service = root.path().join(".letopis");
    fs::create_dir(&service).unwrap();
    let raw = serde_json::json!({"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture",
        "parameters":{"base_url":"http://127.0.0.1:1/api"},"max_retries":0}});
    fs::write(service.join("config.json"), raw.to_string()).unwrap();
    let config = Configuration::parse(raw.to_string().as_bytes()).unwrap();
    let config = config.get_embedding_config().unwrap();
    let mut storage = open_storage(&service.join("cache.db"), &service.join("vectors.db")).unwrap();
    let mut unit = storage.begin_cache_write().unwrap();
    unit.upsert_node(&fixtures::build_knowledge("k.md", "text"))
        .unwrap();
    unit.commit().unwrap();
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    let VectorOutcome::Current(mut unit) =
        storage.begin_vector_write(&config.get_identity()).unwrap()
    else {
        panic!("configuration changed");
    };
    unit.store_vector(&Sha256::digest(b"text"), vector, 3)
        .unwrap();
    unit.commit().unwrap();
    let request = Request {
        root: root.path().to_path_buf(),
        expected: BTreeMap::from([("k.md".into(), vec![1., 0., 0.])]),
        output: root.path().join("observation.json"),
    };
    (root, request)
}
