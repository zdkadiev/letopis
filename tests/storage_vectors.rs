#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::Sha256,
    open_storage,
    service::{Storage, StorageErrorKind, StorageRead, VectorOutcome},
};
use tempfile::TempDir;

#[test]
fn text_hash_shares_one_vector() {
    // Catches duplicate keys, lost bytes, and one-to-one rather than text joins.
    // Given: two knowledge nodes sharing the same search text.
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    let hash = Sha256::digest(b"shared");
    // When / Then: vector insertion preserves one key and joins both current node identities.
    assert_eq!(
        storage.reset_vectors(None, &identity).unwrap(),
        VectorOutcome::Current(true)
    );
    let mut cache = storage.begin_cache_write_tx().unwrap();
    for id in ["a.md", "b.md"] {
        cache
            .upsert_node(&support::build_knowledge(id, "shared"))
            .unwrap();
    }
    assert_eq!(cache.commit().unwrap(), 1);
    let row = store(&mut storage, &identity, &hash, &[1.0, 0.0, 0.0]);
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    let again = unit.store_vector(&hash, &[0.0, 1.0, 0.0], 3).unwrap();
    assert_eq!(again.row_id, row);
    assert!(!again.inserted);
    unit.commit().unwrap();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(
        unwrap_current(read.get_vector_by_search_hash(&identity, &hash).unwrap()),
        Some(vec![1.0, 0.0, 0.0])
    );
    let matches = unwrap_current(
        read.get_nearest_vectors(&identity, &[1.0, 0.0, 0.0], 3)
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
    assert_eq!(read.get_revision(), 1);
}
#[test]
fn vectors_require_matching_identity() {
    // Given: stored vectors under the original configuration identity.
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    storage.reset_vectors(None, &m1).unwrap();
    store(&mut storage, &m1, &m1, &[1.0, 0.0, 0.0]);
    // When / Then: reads and writes reject another identity without changing stored vectors.
    assert!(matches!(
        storage.begin_vector_write_tx(&m2).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    let mut read = storage.begin_read_tx().unwrap();
    assert!(matches!(
        read.get_vector_by_search_hash(&m2, &m1).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert!(matches!(
        read.get_nearest_vectors(&m2, &[], 0).unwrap(),
        VectorOutcome::ConfigChanged
    ));
}
#[test]
fn old_reader_survives_reset_and_late_reader_rejects_captured_old_identity() {
    // Catches shared vec1 state or validation against a later global identity.
    // Given: an old reader pinned before a model reset.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let mut first = open_storage(&cache, &vectors).unwrap();
    let mut second = open_storage(&cache, &vectors).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    first.reset_vectors(None, &m1).unwrap();
    store(&mut first, &m1, &m1, &[1.0, 0.0, 0.0]);
    publish_hash(&mut first, m1);
    let mut old = first.begin_read_tx().unwrap();
    // When / Then: old and new readers query vector state; the old view survives and late readers reject the old identity.
    assert_nearest(old.as_mut(), &m1, &[1.0, 0.0, 0.0]);
    assert_eq!(
        unwrap_current(old.get_vector_by_search_hash(&m1, &m1).unwrap()),
        Some(vec![1.0, 0.0, 0.0])
    );
    assert_eq!(
        second.reset_vectors(Some(&m1), &m2).unwrap(),
        VectorOutcome::Current(true)
    );
    store(&mut second, &m2, &m2, &[0.0, 1.0, 0.0, 0.0]);
    publish_hash(&mut second, m2);
    assert_eq!(
        second.reset_vectors(Some(&m1), &m2).unwrap(),
        VectorOutcome::Current(false)
    );
    assert_nearest(old.as_mut(), &m1, &[1.0, 0.0, 0.0]);
    assert_eq!(
        unwrap_current(old.get_vector_by_search_hash(&m1, &m1).unwrap()),
        Some(vec![1.0, 0.0, 0.0])
    );
    let mut fresh = second.begin_read_tx().unwrap();
    assert_nearest(fresh.as_mut(), &m2, &[0.0, 1.0, 0.0, 0.0]);
    assert!(matches!(
        fresh.get_vector_by_search_hash(&m1, &m1).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert_eq!(
        unwrap_current(fresh.get_vector_by_search_hash(&m2, &m2).unwrap()),
        Some(vec![0.0, 1.0, 0.0, 0.0])
    );
    assert_eq!(
        unwrap_current(fresh.get_vector_by_search_hash(&m2, &m1).unwrap()),
        None
    );
}
#[test]
fn current_node_hash_excludes_old_vector() {
    // Given: old and current text vectors with only the current hash published.
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    storage.reset_vectors(None, &m1).unwrap();
    store(&mut storage, &m1, &Sha256::digest(b"old"), &[1.0, 0.0, 0.0]);
    store(&mut storage, &m1, &Sha256::digest(b"new"), &[0.0, 1.0, 0.0]);
    let mut cache = storage.begin_cache_write_tx().unwrap();
    cache
        .upsert_node(&support::build_knowledge("a.md", "new"))
        .unwrap();
    cache.commit().unwrap();
    let mut read = storage.begin_read_tx().unwrap();
    // When: nearest vectors are joined to the currently published node hashes.
    let matches = unwrap_current(read.get_nearest_vectors(&m1, &[1.0, 0.0, 0.0], 3).unwrap());
    // Then: nearest-vector results join only the current node hash.
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].search_hash, Sha256::digest(b"new"));
    assert_eq!(matches[0].id.as_str(), "a.md");
    assert_eq!(matches[0].distance, 1.0);
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
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let m1 = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"valid");
        storage.reset_vectors(None, &m1).unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&m1).unwrap());
        unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        let error = unit.store_vector(&m1, &invalid, 3).unwrap_err();
        // When / Then: the invalid vector is stored and commit is attempted; the entire vector unit rolls back.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(unit.commit().is_err());
        let mut read = storage.begin_read_tx().unwrap();
        assert_eq!(
            unwrap_current(read.get_vector_by_search_hash(&m1, &hash).unwrap()),
            None
        );
        assert_eq!(
            unwrap_current(read.get_vector_by_search_hash(&m1, &m1).unwrap()),
            None
        );
    }
}
#[test]
fn dimension_reset_and_rollback() {
    // Given: stored native dimensions before reset and rollback.
    let root = TempDir::new().unwrap();
    let vectors = root.path().join("vectors");
    let mut storage = open_storage(&root.path().join("cache"), &vectors).unwrap();
    let m1 = Sha256::digest(b"M1");
    let hash = Sha256::digest(b"late");
    storage.reset_vectors(None, &m1).unwrap();
    store(&mut storage, &m1, &m1, &[1.0, 0.0, 0.0]);
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&m1).unwrap());
    // When / Then: mismatched insertion or identity reset is attempted; dimension changes remain atomic and rollback-safe.
    assert!(unit.store_vector(&hash, &[0.0, 1.0, 0.0, 0.0], 4).is_err());
    assert!(unit.commit().is_err());
    let raw = rusqlite::Connection::open(&vectors).unwrap();
    let keys: i64 = raw
        .query_row("select count(*) from vector_keys", [], |r| r.get(0))
        .unwrap();
    assert_eq!(keys, 1);
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(
        unwrap_current(read.get_vector_by_search_hash(&m1, &m1).unwrap()),
        Some(vec![1.0, 0.0, 0.0])
    );
    assert_eq!(
        unwrap_current(read.get_vector_by_search_hash(&m1, &hash).unwrap()),
        None
    );
}
#[test]
fn cache_and_vector_writers_are_independent() {
    // Given: independent cache and vector writers.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let mut first = open_storage(&cache, &vectors).unwrap();
    let mut second = open_storage(&cache, &vectors).unwrap();
    let identity = Sha256::digest(b"M1");
    first.reset_vectors(None, &identity).unwrap();
    let cache_unit = first.begin_cache_write_tx().unwrap();
    let mut vector_unit = unwrap_current(second.begin_vector_write_tx(&identity).unwrap());
    let key = vector_unit
        .store_vector(&identity, &[1.0, 0.0, 0.0], 3)
        .unwrap();
    vector_unit.commit().unwrap();
    // When / Then: the cache writer can commit after the independent vector write.
    assert_eq!(cache_unit.commit().unwrap(), 1);
    drop(first);
    drop(second);
    // Given: the derived cache is removed after the independent writes.
    std::fs::remove_file(&cache).unwrap();
    let mut rebuilt = open_storage(&cache, &vectors).unwrap();
    let mut unit = unwrap_current(rebuilt.begin_vector_write_tx(&identity).unwrap());
    let reused = unit.store_vector(&identity, &[0.0, 1.0, 0.0], 3).unwrap();
    // Then: rebuilding the cache reuses the same persisted vector key.
    assert_eq!(reused.row_id, key.row_id);
    assert!(!reused.inserted);
    unit.commit().unwrap();
    assert_eq!(rebuilt.begin_read_tx().unwrap().get_revision(), 0);
}

#[test]
fn nearest_validates_query_and_orders_all_candidates_by_distance_then_id() {
    // Catches premature limiting before hash joins, wrong metric or incorrect order.
    // Given: current nodes with tied distances and an unreferenced vector.
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.reset_vectors(None, &identity).unwrap();
    let mut unit = storage.begin_cache_write_tx().unwrap();
    for (id, text) in [("z.md", "close"), ("b.md", "far"), ("a.md", "far")] {
        unit.upsert_node(&support::build_knowledge(id, text))
            .unwrap();
    }
    unit.commit().unwrap();
    store(
        &mut storage,
        &identity,
        &Sha256::digest(b"unused"),
        &[1.0, 0.0, 0.0],
    );
    store(
        &mut storage,
        &identity,
        &Sha256::digest(b"close"),
        &[1.0, 0.0, 0.0],
    );
    store(
        &mut storage,
        &identity,
        &Sha256::digest(b"far"),
        &[0.0, 1.0, 0.0],
    );
    let mut read = storage.begin_read_tx().unwrap();
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
            read.get_nearest_vectors(&identity, &query, dimensions)
                .is_err()
        );
    }
    let matches = unwrap_current(
        read.get_nearest_vectors(&identity, &[1.0, 0.0, 0.0], 3)
            .unwrap(),
    );
    assert_eq!(
        matches
            .iter()
            .map(|value| (value.id.as_str(), value.distance))
            .collect::<Vec<_>>(),
        [("z.md", 0.0), ("a.md", 1.0), ("b.md", 1.0)]
    );
}

fn store(storage: &mut impl Storage, identity: &Sha256, hash: &Sha256, vector: &[f32]) -> i64 {
    let mut unit = unwrap_current(storage.begin_vector_write_tx(identity).unwrap());
    let key = unit.store_vector(hash, vector, vector.len()).unwrap();
    unit.commit().unwrap();
    key.row_id
}

fn publish_hash(storage: &mut impl Storage, hash: Sha256) {
    let mut node = support::build_knowledge("a.md", "fixture");
    let letopis::service::NodeMetadata::Knowledge(ref mut knowledge) = node else {
        panic!("knowledge fixture");
    };
    knowledge.search_hash = hash;
    let mut unit = storage.begin_cache_write_tx().unwrap();
    unit.upsert_node(&node).unwrap();
    unit.commit().unwrap();
}

fn assert_nearest(read: &mut dyn StorageRead, identity: &Sha256, query: &[f32]) {
    let matches = unwrap_current(
        read.get_nearest_vectors(identity, query, query.len())
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
