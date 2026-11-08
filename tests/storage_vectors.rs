use crate::support::get_current;
use letopis::service::StorageError;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, VectorDatabase};
use support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::Sha256,
    service::{StorageErrorKind, StorageRead, VectorOutcome},
};
use tempfile::TempDir;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the sequential transaction scenario and its assertions together"
)]
fn text_hash_shares_one_vector() {
    // Catches duplicate keys, lost bytes, and one-to-one rather than text joins.
    // Given: two knowledge nodes sharing the same search text.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    let hash = Sha256::digest(b"shared");
    // When / Then: vector insertion preserves one key and joins both current node identities.
    assert_eq!(
        storage.vectors.reset_vectors(None, &identity).unwrap(),
        VectorOutcome::Current(true)
    );
    let cache_result = storage
        .cache
        .with_write_tx(|| {
            let cache = &storage.cache;
            for id in ["a.md", "b.md"] {
                cache
                    .nodes_upsert_by_id(&support::build_knowledge(id, "shared"))
                    .unwrap();
            }

            cache.project_get_revision()
        })
        .map_err(TransactionError::into_inner);
    assert_eq!(cache_result.unwrap(), 1);
    let row = store(&storage, &identity, &hash, &[1.0, 0.0, 0.0]);
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            let again = unit.store_vector(&hash, &[0.0, 1.0, 0.0], 3).unwrap();
            assert_eq!(again.row_id, row);
            assert!(!again.is_inserted);

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(
                unwrap_current(
                    read.vector_keys_get_by_search_hash(&identity, &hash)
                        .unwrap()
                ),
                Some(vec![1.0, 0.0, 0.0])
            );
            let matches = unwrap_current(
                read.vector_index_get_by_vector(&identity, &[1.0, 0.0, 0.0], 3)
                    .unwrap(),
            );
            assert_eq!(
                matches.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
                ["a.md", "b.md"]
            );
            assert!(
                matches
                    .iter()
                    .all(|m| m.search_hash == hash && m.distance == 0.0)
            );
            assert_eq!(read.project_get_by_id().unwrap(), 1);
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn vectors_require_matching_identity() {
    // Given: stored vectors under the original configuration identity.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    storage.vectors.reset_vectors(None, &m1).unwrap();
    store(&storage, &m1, &m1, &[1.0, 0.0, 0.0]);
    // When / Then: reads and writes reject another identity without changing stored vectors.
    assert!(matches!(
        storage
            .vectors
            .with_write_tx(&m2, || Ok::<_, StorageError>(()))
            .unwrap(),
        VectorOutcome::ConfigChanged
    ));
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert!(matches!(
                read.vector_keys_get_by_search_hash(&m2, &m1).unwrap(),
                VectorOutcome::ConfigChanged
            ));
            assert!(matches!(
                read.vector_index_get_by_vector(&m2, &[], 0).unwrap(),
                VectorOutcome::ConfigChanged
            ));
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn old_reader_survives_reset_and_late_reader_rejects_captured_old_identity() {
    // Catches shared vec1 state or validation against a later global identity.
    // Given: an old reader pinned before a model reset.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let first = open_storage(&cache, &vectors).unwrap();
    let second = open_storage(&cache, &vectors).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    first.vectors.reset_vectors(None, &m1).unwrap();
    store(&first, &m1, &m1, &[1.0, 0.0, 0.0]);
    publish_hash(&first, m1);
    first
        .cache
        .with_read_tx(|| {
            let old = &first.cache;
            // When / Then: old and new readers query vector state; the old view survives and late readers reject the old identity.
            assert_nearest(old, &m1, &[1.0, 0.0, 0.0]);
            assert_eq!(
                unwrap_current(old.vector_keys_get_by_search_hash(&m1, &m1).unwrap()),
                Some(vec![1.0, 0.0, 0.0])
            );
            assert_eq!(
                second.vectors.reset_vectors(Some(&m1), &m2).unwrap(),
                VectorOutcome::Current(true)
            );
            store(&second, &m2, &m2, &[0.0, 1.0, 0.0, 0.0]);
            publish_hash(&second, m2);
            assert_eq!(
                second.vectors.reset_vectors(Some(&m1), &m2).unwrap(),
                VectorOutcome::Current(false)
            );
            assert_nearest(old, &m1, &[1.0, 0.0, 0.0]);
            assert_eq!(
                unwrap_current(old.vector_keys_get_by_search_hash(&m1, &m1).unwrap()),
                Some(vec![1.0, 0.0, 0.0])
            );
            second
                .cache
                .with_read_tx(|| {
                    let fresh = &second.cache;
                    assert_nearest(fresh, &m2, &[0.0, 1.0, 0.0, 0.0]);
                    assert!(matches!(
                        fresh.vector_keys_get_by_search_hash(&m1, &m1).unwrap(),
                        VectorOutcome::ConfigChanged
                    ));
                    assert_eq!(
                        unwrap_current(fresh.vector_keys_get_by_search_hash(&m2, &m2).unwrap()),
                        Some(vec![0.0, 1.0, 0.0, 0.0])
                    );
                    assert_eq!(
                        unwrap_current(fresh.vector_keys_get_by_search_hash(&m2, &m1).unwrap()),
                        None
                    );
                    Ok::<_, StorageError>(())
                })
                .unwrap();
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn current_node_hash_excludes_old_vector() {
    // Given: old and current text vectors with only the current hash published.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    storage.vectors.reset_vectors(None, &m1).unwrap();
    store(&storage, &m1, &Sha256::digest(b"old"), &[1.0, 0.0, 0.0]);
    store(&storage, &m1, &Sha256::digest(b"new"), &[0.0, 1.0, 0.0]);
    storage
        .cache
        .with_write_tx(|| {
            let cache = &storage.cache;
            cache
                .nodes_upsert_by_id(&support::build_knowledge("a.md", "new"))
                .unwrap();

            cache.project_get_revision()
        })
        .unwrap();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            // When: nearest vectors are joined to the currently published node hashes.
            let matches = unwrap_current(
                read.vector_index_get_by_vector(&m1, &[1.0, 0.0, 0.0], 3)
                    .unwrap(),
            );
            // Then: nearest-vector results join only the current node hash.
            assert_eq!(matches.len(), 1);
            assert_eq!(matches[0].search_hash, Sha256::digest(b"new"));
            assert_eq!(matches[0].id.as_str(), "a.md");
            assert_eq!(matches[0].distance, 1.0);
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn caught_vector_error_cannot_commit_key_or_prior_write() {
    // Every malformed input must poison the whole unit, including a prior valid pair.
    // Given: invalid vector values after a prior valid write.
    for invalid in [
        vec![],
        vec![f32::NAN, 0.0, 0.0],
        vec![f32::INFINITY, 0.0, 0.0],
        vec![1.0, 0.0, 0.0, 0.0],
    ] {
        let root = TempDir::new().unwrap();
        let storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let m1 = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"valid");
        storage.vectors.reset_vectors(None, &m1).unwrap();
        let unit_result = storage
            .vectors
            .with_write_tx(&m1, || {
                let unit = &storage.vectors;
                unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
                let error = unit.store_vector(&m1, &invalid, 3).unwrap_err();
                // When / Then: the invalid vector is stored and commit is attempted; the entire vector unit rolls back.
                assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .map_err(TransactionError::into_inner);
        assert!(unit_result.is_err());
        storage
            .cache
            .with_read_tx(|| {
                let read = &storage.cache;
                assert_eq!(
                    unwrap_current(read.vector_keys_get_by_search_hash(&m1, &hash).unwrap()),
                    None
                );
                assert_eq!(
                    unwrap_current(read.vector_keys_get_by_search_hash(&m1, &m1).unwrap()),
                    None
                );
                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
}
#[test]
fn dimension_reset_and_rollback() {
    // Given: stored native dimensions before reset and rollback.
    let root = TempDir::new().unwrap();
    let vectors = root.path().join("vectors");
    let storage = open_storage(&root.path().join("cache"), &vectors).unwrap();
    let m1 = Sha256::digest(b"M1");
    let hash = Sha256::digest(b"late");
    storage.vectors.reset_vectors(None, &m1).unwrap();
    store(&storage, &m1, &m1, &[1.0, 0.0, 0.0]);
    let unit_result = storage
        .vectors
        .with_write_tx(&m1, || {
            let unit = &storage.vectors;
            // When / Then: mismatched insertion or identity reset is attempted; dimension changes remain atomic and rollback-safe.
            assert!(unit.store_vector(&hash, &[0.0, 1.0, 0.0, 0.0], 4).is_err());

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    assert!(unit_result.is_err());
    let raw = rusqlite::Connection::open(&vectors).unwrap();
    let keys: i64 = raw
        .query_row("select count(*) from vector_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(keys, 1);
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(
                unwrap_current(read.vector_keys_get_by_search_hash(&m1, &m1).unwrap()),
                Some(vec![1.0, 0.0, 0.0])
            );
            assert_eq!(
                unwrap_current(read.vector_keys_get_by_search_hash(&m1, &hash).unwrap()),
                None
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn cache_and_vector_writers_are_independent() {
    // Given: independent cache and vector writers.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let first = open_storage(&cache, &vectors).unwrap();
    let second = open_storage(&cache, &vectors).unwrap();
    let identity = Sha256::digest(b"M1");
    first.vectors.reset_vectors(None, &identity).unwrap();
    let cache_unit_result = first
        .cache
        .with_write_tx(|| {
            let cache_unit = &first.cache;
            let vector_unit_result = second
                .vectors
                .with_write_tx(&identity, || {
                    let vector_unit = &second.vectors;
                    let key = vector_unit
                        .store_vector(&identity, &[1.0, 0.0, 0.0], 3)
                        .unwrap();

                    Ok::<_, StorageError>(((), key))
                })
                .map(get_current)
                .map_err(TransactionError::into_inner);
            let (_, key) = vector_unit_result.unwrap();

            // When / Then: the cache writer can commit after the independent vector write.

            Ok::<_, StorageError>((cache_unit.project_get_revision()?, key))
        })
        .map_err(TransactionError::into_inner);
    let (revision, key) = cache_unit_result.unwrap();
    assert_eq!(revision, 1);
    drop(first);
    drop(second);
    // Given: the derived cache is removed after the independent writes.
    std::fs::remove_file(&cache).unwrap();
    let rebuilt = open_storage(&cache, &vectors).unwrap();
    rebuilt
        .vectors
        .with_write_tx(&identity, || {
            let unit = &rebuilt.vectors;
            let reused = unit.store_vector(&identity, &[0.0, 1.0, 0.0], 3).unwrap();
            // Then: rebuilding the cache reuses the same persisted vector key.
            assert_eq!(reused.row_id, key.row_id);
            assert!(!reused.is_inserted);

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert_eq!(
        rebuilt
            .cache
            .with_read_tx(|| rebuilt.cache.project_get_by_id())
            .unwrap(),
        0
    );
}

#[test]
fn nearest_validates_query_and_orders_all_candidates_by_distance_then_id() {
    // Catches premature limiting before hash joins, wrong metric or incorrect order.
    // Given: current nodes with tied distances and an unreferenced vector.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            for (id, text) in [("z.md", "close"), ("b.md", "far"), ("a.md", "far")] {
                unit.nodes_upsert_by_id(&support::build_knowledge(id, text))
                    .unwrap();
            }

            unit.project_get_revision()
        })
        .unwrap();
    for (text, vector) in [
        ("unused", [1.0, 0.0, 0.0]),
        ("close", [1.0, 0.0, 0.0]),
        ("far", [0.0, 1.0, 0.0]),
    ] {
        store(
            &storage,
            &identity,
            &Sha256::digest(text.as_bytes()),
            &vector,
        );
    }
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            // When / Then: invalid queries are rejected before valid distance ordering is checked.
            for (query, dimensions) in [
                (vec![], 0),
                (vec![f32::NAN], 1),
                (vec![f32::INFINITY], 1),
                (vec![1.0, 0.0], 3),
                (vec![1.0, 0.0, 0.0, 0.0], 4),
            ] {
                // When / Then: each invalid vector or dimension combination returns an error.
                assert!(
                    read.vector_index_get_by_vector(&identity, &query, dimensions)
                        .is_err()
                );
            }
            let matches = unwrap_current(
                read.vector_index_get_by_vector(&identity, &[1.0, 0.0, 0.0], 3)
                    .unwrap(),
            );
            assert_eq!(
                matches
                    .iter()
                    .map(|value| (value.id.as_str(), value.distance))
                    .collect::<Vec<_>>(),
                [("z.md", 0.0), ("a.md", 1.0), ("b.md", 1.0)]
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

fn store<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
    hash: &Sha256,
    vector: &[f32],
) -> i64 {
    let unit_result = storage
        .vectors
        .with_write_tx(identity, || {
            let unit = &storage.vectors;
            let key = unit.store_vector(hash, vector, vector.len()).unwrap();

            Ok::<_, StorageError>(((), key))
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    let (_, key) = unit_result.unwrap();

    key.row_id
}

fn publish_hash<C: CacheDatabase, V: VectorDatabase>(storage: &Databases<C, V>, hash: Sha256) {
    let mut node = support::build_knowledge("a.md", "fixture");
    let letopis::service::NodeMetadata::Knowledge(ref mut knowledge) = node else {
        panic!("knowledge fixture");
    };
    knowledge.search_hash = hash;
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            unit.nodes_upsert_by_id(&node).unwrap();

            unit.project_get_revision()
        })
        .unwrap();
}

fn assert_nearest(read: &dyn StorageRead, identity: &Sha256, query: &[f32]) {
    let matches = unwrap_current(
        read.vector_index_get_by_vector(identity, query, query.len())
            .unwrap(),
    );
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].id.as_str(), "a.md");
    assert_eq!(matches[0].search_hash, *identity);
    assert_eq!(matches[0].distance, 0.0);
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected identity change"),
    }
}
