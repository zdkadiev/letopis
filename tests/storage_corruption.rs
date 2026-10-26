#[path = "support/storage.rs"]
mod support;

use letopis::{
    domain::{KnowledgeId, Sha256},
    open_storage,
    service::{CacheWrite, Storage, StorageErrorKind, StorageRead, VectorOutcome, VectorWrite},
};
use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::{Builder, TempDir};

const CACHE_DATABASE: &str = "cache.db";
const VECTOR_DATABASE: &str = "vectors.db";
const NODE_ID: &str = "known.md";
const NODE_TEXT: &str = "known searchable text";
const CANONICAL_TEXT: &[u8] = b"canonical knowledge\n";
const CORRUPTION_CANARY: &str = "CORRUPTION_CANARY";

#[derive(Debug, Clone, Copy)]
enum DatabaseDamage {
    InvalidHeader,
    Truncated,
    UnsupportedVersion,
    PartialSchema,
}

#[test]
fn nonempty_corrupt_databases_are_rejected_without_reinitialization() {
    for target in [CACHE_DATABASE, VECTOR_DATABASE] {
        for damage in [DatabaseDamage::InvalidHeader, DatabaseDamage::Truncated] {
            assert_damage_is_rejected(target, damage);
        }
    }
}

#[test]
fn incompatible_existing_schemas_are_never_repaired_implicitly() {
    for target in [CACHE_DATABASE, VECTOR_DATABASE] {
        for damage in [
            DatabaseDamage::UnsupportedVersion,
            DatabaseDamage::PartialSchema,
        ] {
            assert_damage_is_rejected(target, damage);
        }
    }
}

#[test]
fn missing_and_zero_length_cache_are_unpublished() {
    for create_empty_file in [false, true] {
        let root = create_private_directory();
        let cache = root.path().join(CACHE_DATABASE);
        let vectors = root.path().join(VECTOR_DATABASE);
        if create_empty_file {
            fs::File::create(&cache).unwrap();
        }

        let mut storage = open_storage(&cache, &vectors).unwrap();

        assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
    }
}

#[test]
fn missing_and_zero_length_vectors_preserve_published_cache() {
    for create_empty_file in [false, true] {
        let root = create_private_directory();
        let cache = root.path().join(CACHE_DATABASE);
        let vectors = root.path().join(VECTOR_DATABASE);
        seed_vector(&cache, &vectors);
        remove_database(&vectors);
        if create_empty_file {
            fs::File::create(&vectors).unwrap();
        }

        let mut storage = open_storage(&cache, &vectors).unwrap();
        let mut view = storage.begin_read().unwrap();

        assert_eq!(view.get_revision(), 1);
        assert_eq!(view.get_config_identity(), None);
        let matches = view.match_text("searchable").unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id.as_str(), NODE_ID);
    }
}

#[test]
fn recreated_empty_cache_preserves_healthy_vectors() {
    let root = create_private_directory();
    let cache = root.path().join(CACHE_DATABASE);
    let vectors = root.path().join(VECTOR_DATABASE);
    let canonical = root.path().join("known.md");
    fs::write(&canonical, CANONICAL_TEXT).unwrap();
    let (identity, search_hash) = seed_vector(&cache, &vectors);
    assert_healthy(&cache, &vectors, &identity, &search_hash);
    remove_database(&cache);

    let mut storage = open_storage(&cache, &vectors).unwrap();
    let mut view = storage.begin_read().unwrap();

    assert_eq!(view.get_revision(), 0);
    assert_eq!(view.get_config_identity(), Some(identity));
    let stored_vector = unwrap_current(view.read_vector(&identity, &search_hash).unwrap()).unwrap();
    assert_eq!(stored_vector, vec![0.25_f32, 0.75_f32]);
    assert_eq!(fs::read(canonical).unwrap(), CANONICAL_TEXT);
}

fn assert_damage_is_rejected(target: &str, damage: DatabaseDamage) {
    let root = create_private_directory();
    let cache = root.path().join(CACHE_DATABASE);
    let vectors = root.path().join(VECTOR_DATABASE);
    let (identity, search_hash) = seed_vector(&cache, &vectors);
    assert_healthy(&cache, &vectors, &identity, &search_hash);
    let original_cache = fs::read(&cache).unwrap();
    let original_vectors = fs::read(&vectors).unwrap();
    let damaged_path = root.path().join(target);
    let damaged_bytes = damage_database(&damaged_path, damage);

    for _ in 0..2 {
        let Err(error) = open_storage(&cache, &vectors) else {
            panic!("{target} with {damage:?} was accepted");
        };
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert_eq!(fs::read(&damaged_path).unwrap(), damaged_bytes);
        assert!(!format!("{error} {error:?}").contains(CORRUPTION_CANARY));
    }

    restore_databases(&cache, &original_cache, &vectors, &original_vectors);
    assert_healthy(&cache, &vectors, &identity, &search_hash);
}

fn seed_vector(cache: &Path, vectors: &Path) -> (Sha256, Sha256) {
    let identity = Sha256::digest(b"known model identity");
    let search_hash = Sha256::digest(NODE_TEXT.as_bytes());
    let mut storage = open_storage(cache, vectors).unwrap();
    assert_eq!(
        storage.reset_vectors(None, &identity).unwrap(),
        VectorOutcome::Current(true)
    );
    let mut cache_write = storage.begin_cache_write().unwrap();
    cache_write
        .upsert_node(&support::build_knowledge(NODE_ID, NODE_TEXT))
        .unwrap();
    cache_write
        .replace_text(&KnowledgeId::parse(NODE_ID).unwrap(), NODE_TEXT)
        .unwrap();
    assert_eq!(cache_write.commit().unwrap(), 1);
    let mut vector_write = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    vector_write
        .store_vector(&search_hash, &[0.25_f32, 0.75_f32], 2)
        .unwrap();
    vector_write.commit().unwrap();
    drop(storage);
    (identity, search_hash)
}

fn damage_database(path: &Path, damage: DatabaseDamage) -> Vec<u8> {
    match damage {
        DatabaseDamage::InvalidHeader => {
            let mut bytes = fs::read(path).unwrap();
            bytes[..16].copy_from_slice(b"invalid sqlite!!");
            fs::write(path, bytes).unwrap();
        }
        DatabaseDamage::Truncated => {
            let bytes = fs::read(path).unwrap();
            fs::write(path, &bytes[..15]).unwrap();
        }
        DatabaseDamage::UnsupportedVersion => {
            let connection = Connection::open(path).unwrap();
            connection.pragma_update(None, "user_version", 999).unwrap();
        }
        DatabaseDamage::PartialSchema => {
            let table = if path.ends_with(CACHE_DATABASE) {
                "project"
            } else {
                "meta"
            };
            let connection = Connection::open(path).unwrap();
            connection
                .execute_batch(&format!("DROP TABLE {table}"))
                .unwrap();
        }
    }
    fs::read(path).unwrap()
}

fn assert_healthy(cache: &Path, vectors: &Path, identity: &Sha256, search_hash: &Sha256) {
    let mut storage = open_storage(cache, vectors).unwrap();
    let mut view = storage.begin_read().unwrap();
    assert_eq!(view.get_revision(), 1);
    assert_eq!(view.get_config_identity(), Some(*identity));
    let matches = view.match_text("searchable").unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id.as_str(), NODE_ID);
    let vector = unwrap_current(view.read_vector(identity, search_hash).unwrap()).unwrap();
    assert_eq!(vector, vec![0.25_f32, 0.75_f32]);
}

fn restore_databases(cache: &Path, cache_bytes: &[u8], vectors: &Path, vector_bytes: &[u8]) {
    remove_sidecars(cache);
    remove_sidecars(vectors);
    fs::write(cache, cache_bytes).unwrap();
    fs::write(vectors, vector_bytes).unwrap();
}

fn remove_database(path: &Path) {
    fs::remove_file(path).unwrap();
    remove_sidecars(path);
}

fn remove_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
    }
}

fn create_private_directory() -> TempDir {
    Builder::new().prefix(CORRUPTION_CANARY).tempdir().unwrap()
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected vector identity change"),
    }
}
