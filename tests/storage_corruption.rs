use crate::support::get_current;
use letopis::service::StorageError;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
use support::open_storage;
#[path = "support/storage.rs"]
mod support;

use letopis::{
    domain::{KnowledgeId, Sha256},
    service::{StorageErrorKind, VectorOutcome},
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
    // Given
    for target in [CACHE_DATABASE, VECTOR_DATABASE] {
        for damage in [DatabaseDamage::InvalidHeader, DatabaseDamage::Truncated] {
            // When / Then
            assert_damage_is_rejected(target, damage);
        }
    }
}

#[test]
fn incompatible_existing_schemas_are_never_repaired_implicitly() {
    // Given
    for target in [CACHE_DATABASE, VECTOR_DATABASE] {
        for damage in [
            DatabaseDamage::UnsupportedVersion,
            DatabaseDamage::PartialSchema,
        ] {
            // When / Then
            assert_damage_is_rejected(target, damage);
        }
    }
}

#[test]
fn missing_and_zero_length_cache_are_unpublished() {
    // Given: missing or zero-length cache databases.
    for create_empty_file in [false, true] {
        let root = create_private_directory();
        let cache = root.path().join(CACHE_DATABASE);
        let vectors = root.path().join(VECTOR_DATABASE);
        if create_empty_file {
            fs::File::create(&cache).unwrap();
        }

        let storage = open_storage(&cache, &vectors).unwrap();

        // When / Then: storage opens a read transaction; the cache remains unpublished.
        assert_eq!(
            storage
                .cache
                .with_read_tx(|| storage.cache.project_get_by_id())
                .unwrap(),
            0
        );
    }
}

#[test]
fn missing_and_zero_length_vectors_preserve_published_cache() {
    // Given: a published cache whose vector database is absent or empty.
    for create_empty_file in [false, true] {
        let root = create_private_directory();
        let cache = root.path().join(CACHE_DATABASE);
        let vectors = root.path().join(VECTOR_DATABASE);
        seed_vector(&cache, &vectors);
        remove_database(&vectors);
        if create_empty_file {
            fs::File::create(&vectors).unwrap();
        }

        let storage = open_storage(&cache, &vectors).unwrap();
        storage
            .cache
            .with_read_tx(|| {
                let view = &storage.cache;

                // When / Then: cache revision and lexical results survive with no vector identity.
                assert_eq!(view.project_get_by_id().unwrap(), 1);
                assert_eq!(view.meta_get_by_id().unwrap(), None);
                let matches = view.text_index_get_by_expression("searchable").unwrap();
                assert_eq!(matches.len(), 1);
                assert_eq!(matches[0].id.as_str(), NODE_ID);
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}

#[test]
fn recreated_empty_cache_preserves_healthy_vectors() {
    // Given: healthy persisted vectors and a recreated empty cache.
    let root = create_private_directory();
    let cache = root.path().join(CACHE_DATABASE);
    let vectors = root.path().join(VECTOR_DATABASE);
    let canonical = root.path().join("known.md");
    fs::write(&canonical, CANONICAL_TEXT).unwrap();
    let (identity, search_hash) = seed_vector(&cache, &vectors);
    // When / Then: the derived cache is removed and reopened; healthy vector bytes and identity survive.
    assert_healthy(&cache, &vectors, &identity, &search_hash);
    remove_database(&cache);

    let storage = open_storage(&cache, &vectors).unwrap();
    storage
        .cache
        .with_read_tx(|| {
            let view = &storage.cache;

            assert_eq!(view.project_get_by_id().unwrap(), 0);
            assert_eq!(view.meta_get_by_id().unwrap(), Some(identity));
            let stored_vector = unwrap_current(
                view.vector_keys_get_by_search_hash(&identity, &search_hash)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(stored_vector, vec![0.25_f32, 0.75_f32]);
            assert_eq!(fs::read(canonical).unwrap(), CANONICAL_TEXT);
            Ok::<_, StorageError>(())
        })
        .unwrap();
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
    let storage = open_storage(cache, vectors).unwrap();
    assert_eq!(
        storage.vectors.reset_vectors(None, &identity).unwrap(),
        VectorOutcome::Current(true)
    );
    let cache_write_result = storage
        .cache
        .with_write_tx(|| {
            let cache_write = &storage.cache;
            cache_write
                .nodes_upsert_by_id(&support::build_knowledge(NODE_ID, NODE_TEXT))
                .unwrap();
            cache_write
                .replace_text(&KnowledgeId::parse(NODE_ID).unwrap(), NODE_TEXT)
                .unwrap();

            cache_write.get_publication_revision()
        })
        .map_err(TransactionError::into_inner);
    assert_eq!(cache_write_result.unwrap(), 1);
    storage
        .vectors
        .with_write_tx(&identity, || {
            let vector_write = &storage.vectors;
            vector_write
                .store_vector(&search_hash, &[0.25_f32, 0.75_f32], 2)
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
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
                .execute_batch(&format!("drop table {table}"))
                .unwrap();
        }
    }
    fs::read(path).unwrap()
}

fn assert_healthy(cache: &Path, vectors: &Path, identity: &Sha256, search_hash: &Sha256) {
    let storage = open_storage(cache, vectors).unwrap();
    storage
        .cache
        .with_read_tx(|| {
            let view = &storage.cache;
            assert_eq!(view.project_get_by_id().unwrap(), 1);
            assert_eq!(view.meta_get_by_id().unwrap(), Some(*identity));
            let matches = view.text_index_get_by_expression("searchable").unwrap();
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].id.as_str(), NODE_ID);
            let vector = unwrap_current(
                view.vector_keys_get_by_search_hash(identity, search_hash)
                    .unwrap(),
            )
            .unwrap();
            assert_eq!(vector, vec![0.25_f32, 0.75_f32]);
            Ok::<_, StorageError>(())
        })
        .unwrap();
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
