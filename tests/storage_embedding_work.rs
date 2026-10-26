#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::{EdgeTarget, KnowledgeId, Sha256},
    open_storage,
    service::{
        CacheWrite, EmbeddingWork, Storage, StorageErrorKind, StorageRead, VectorOutcome,
        VectorWrite,
    },
};
use rusqlite::{Connection, types::Value};
use tempfile::TempDir;

#[test]
fn cursor_orders_shared_hashes_and_excludes_sources_and_stored_hashes() {
    // Arrange: reverse insertion order and shared text with two representatives.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(
        &mut storage,
        &[("z.md", "stored"), ("b.md", "shared"), ("a.md", "shared")],
    );
    let mut unit = storage.begin_cache_write().unwrap();
    unit.upsert_node(&support::build_source("file://source.txt", None, 7))
        .unwrap();
    unit.commit().unwrap();
    store(&mut storage, &identity, "stored", 3);
    // Act
    let first = read_next_work(&mut storage, &identity, 4, None).unwrap();
    let second = read_next_work(&mut storage, &identity, 4, Some(&first.id)).unwrap();
    // Assert: skipping a stale representative can advance to its valid sibling.
    assert_eq!(first.id.as_str(), "a.md");
    assert_eq!(second.id.as_str(), "b.md");
    assert_eq!(first.search_hash, second.search_hash);
    assert_eq!(
        read_next_work(&mut storage, &identity, 4, Some(&second.id)),
        None
    );
    store(&mut storage, &identity, "shared", 3);
    assert_eq!(read_next_work(&mut storage, &identity, 4, None), None);
}

#[test]
fn absent_and_null_error_rows_remain_eligible_until_four_attempts() {
    // Arrange: max_retries=3 means a total budget of four reservations.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"shared");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("b.md", "shared")]);
    for attempts in 1..=4 {
        assert!(read_next_work(&mut storage, &identity, 4, None).is_some());
        // Act: reserve without a failure message, including the crash window.
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        assert!(unit.reserve_attempt(&hash, 4).unwrap());
        unit.commit().unwrap();
        // Assert
        let failure = unwrap_current(
            storage
                .begin_read()
                .unwrap()
                .read_failure(&identity, &hash)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(failure.attempts, attempts);
        assert_eq!(failure.error, None);
    }
    assert_eq!(read_next_work(&mut storage, &identity, 4, None), None);
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    assert!(!unit.reserve_attempt(&hash, 4).unwrap());
    unit.commit().unwrap();
}

#[test]
fn work_and_dimensions_use_the_pinned_identity_and_membership() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let m1 = Sha256::digest(b"one");
    let m2 = Sha256::digest(b"two");
    first.reset_vectors(None, &m1).unwrap();
    publish(&mut first, &[("a.md", "pending")]);
    store(&mut first, &m1, "orphan", 3);
    let mut pinned = first.begin_read().unwrap();
    // Act: change both databases after the read view was captured.
    second.reset_vectors(Some(&m1), &m2).unwrap();
    store(&mut second, &m2, "pending", 4);
    publish(&mut second, &[("a.md", "changed")]);
    // Assert
    assert_eq!(
        unwrap_current(pinned.read_vector_dimensions(&m1).unwrap())
            .unwrap()
            .get(),
        3
    );
    assert_eq!(
        unwrap_current(pinned.read_next_embedding_work(&m1, 4, None).unwrap())
            .unwrap()
            .search_hash,
        Sha256::digest(b"pending")
    );
    assert!(matches!(
        pinned.read_vector_dimensions(&m2).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    let mut fresh = second.begin_read().unwrap();
    assert!(matches!(
        fresh.read_next_embedding_work(&m1, 4, None).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert!(matches!(
        fresh.read_vector_dimensions(&m1).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert_eq!(
        unwrap_current(fresh.read_vector_dimensions(&m2).unwrap())
            .unwrap()
            .get(),
        4
    );
}

#[test]
fn last_vector_cleanup_preserves_dimension_across_reopen() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    // Act
    prune(&mut storage, &identity);
    drop(storage);
    let mut storage = open(&root);
    // Assert
    assert_eq!(count_keys(&root), 0);
    assert_eq!(read_size(&mut storage, &identity), Some(3));
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    let error = unit
        .store_vector(&Sha256::digest(b"new"), &[1.0; 4], 4)
        .unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(unit.commit().is_err());
    assert_eq!(count_keys(&root), 0);
    assert_eq!(read_size(&mut storage, &identity), Some(3));
}

#[test]
fn first_committed_dimension_wins_between_competing_connections() {
    // Arrange: both connections observed an uninitialized dimension.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    assert_eq!(read_size(&mut first, &identity), None);
    assert_eq!(read_size(&mut second, &identity), None);
    // Act: hold the first write while the other tries to enter.
    let mut winner = unwrap_current(first.begin_vector_write(&identity).unwrap());
    winner.store_vector(&identity, &[1.0; 3], 3).unwrap();
    assert!(second.begin_vector_write(&identity).is_err());
    winner.commit().unwrap();
    let before = read_native_state(&root);
    let mut loser = unwrap_current(second.begin_vector_write(&identity).unwrap());
    let error = loser
        .store_vector(&Sha256::digest(b"loser"), &[1.0; 4], 4)
        .unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(loser.commit().is_err());
    // Assert
    assert_eq!(read_native_state(&root), before);
    assert_eq!(count_keys(&root), 1);
    assert_eq!(read_size(&mut second, &identity), Some(3));
    assert_eq!(
        unwrap_current(
            first
                .begin_read()
                .unwrap()
                .read_vector(&identity, &identity)
                .unwrap()
        ),
        Some(vec![1.0; 3])
    );
}

#[test]
fn rollback_of_first_insertion_does_not_pin_dimensions() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    // Act
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.store_vector(&identity, &[1.0; 3], 3).unwrap();
    drop(unit);
    // Assert, including reuse of the same native connection after rollback.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(read_size(&mut storage, &identity), None);
    store(&mut storage, &identity, "different", 4);
    assert_eq!(read_size(&mut storage, &identity), Some(4));
}

#[test]
fn cleanup_retains_referenced_hashes_and_diagnostics_without_cache_publication() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    let revision_before = publish(
        &mut storage,
        &[("a.md", "shared"), ("b.md", "shared"), ("c.md", "failed")],
    );
    let key = store(&mut storage, &identity, "shared", 3);
    store(&mut storage, &identity, "orphan", 3);
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    for text in ["failed", "unused"] {
        let hash = Sha256::digest(text.as_bytes());
        unit.reserve_attempt(&hash, 4).unwrap();
        unit.record_failure(&hash, "safe failure").unwrap();
    }
    unit.set_query_error(17, "query failure").unwrap();
    unit.commit().unwrap();
    // Act
    prune(&mut storage, &identity);
    // Assert
    assert_eq!(store(&mut storage, &identity, "shared", 3), key);
    let mut read = storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), revision_before);
    let orphan = Sha256::digest(b"orphan");
    let failed = Sha256::digest(b"failed");
    let unused = Sha256::digest(b"unused");
    assert_eq!(
        unwrap_current(read.read_vector(&identity, &orphan).unwrap()),
        None
    );
    assert!(unwrap_current(read.read_failure(&identity, &failed).unwrap()).is_some());
    assert_eq!(
        unwrap_current(read.read_failure(&identity, &unused).unwrap()),
        None
    );
    assert_eq!(
        read.read_query_error().unwrap().unwrap().error,
        "query failure"
    );
}

#[test]
fn cleanup_uses_a_pinned_cache_observation_and_new_publication_is_recomputable() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    publish(&mut first, &[("a.md", "keep")]);
    store(&mut first, &identity, "keep", 3);
    let mut cleanup = unwrap_current(first.begin_vector_write(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap(); // Pins cache membership.
    // Act: publish a new hash, then put its vector into the older cleanup view.
    publish(&mut second, &[("b.md", "late")]);
    cleanup
        .store_vector(&Sha256::digest(b"late"), &[1.0; 3], 3)
        .unwrap();
    cleanup.prune_unreferenced().unwrap();
    cleanup.commit().unwrap();
    // Assert: only derived work is missing; the knowledge survives for recompute.
    let work = read_next_work(&mut second, &identity, 4, None).unwrap();
    assert_eq!(work.id.as_str(), "b.md");
    assert_eq!(work.search_hash, Sha256::digest(b"late"));
    assert_eq!(count_keys(&root), 1);
}

#[test]
fn late_old_identity_cannot_cleanup_current_vectors_or_query_error() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let old = Sha256::digest(b"old");
    let current = Sha256::digest(b"current");
    storage.reset_vectors(None, &old).unwrap();
    storage.reset_vectors(Some(&old), &current).unwrap();
    publish(&mut storage, &[("a.md", "current")]);
    store(&mut storage, &current, "current", 3);
    let mut unit = unwrap_current(storage.begin_vector_write(&current).unwrap());
    unit.set_query_error(9, "current failure").unwrap();
    unit.commit().unwrap();
    // Act
    assert!(matches!(
        storage.begin_vector_write(&old).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    // Assert
    let mut read = storage.begin_read().unwrap();
    assert!(unwrap_current(read.read_vector(&current, &current).unwrap()).is_some());
    assert_eq!(
        read.read_query_error().unwrap().unwrap().error,
        "current failure"
    );
}

#[test]
fn deleting_one_shared_representative_retains_the_shared_vector() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("b.md", "shared")]);
    let key = store(&mut storage, &identity, "shared", 3);
    let mut unit = storage.begin_cache_write().unwrap();
    unit.delete_node(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        .unwrap();
    unit.commit().unwrap();
    // Act
    prune(&mut storage, &identity);
    // Assert
    assert_eq!(count_keys(&root), 1);
    assert_eq!(store(&mut storage, &identity, "shared", 3), key);
}

#[test]
fn reset_rollback_restores_native_dimensions_and_commit_releases_them() {
    // Arrange: a late failure after reset has recreated the native table.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let old = Sha256::digest(b"old");
    let new = Sha256::digest(b"new");
    storage.reset_vectors(None, &old).unwrap();
    store(&mut storage, &old, "retained", 3);
    let before = read_native_state(&root);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_reset BEFORE UPDATE ON meta
        BEGIN SELECT RAISE(ABORT, 'test reset failure'); END;",
    )
    .unwrap();
    // Act
    assert!(storage.reset_vectors(Some(&old), &new).is_err());
    db.execute_batch("DROP TRIGGER reject_reset;").unwrap();
    // Assert: all key/data/metadata bytes and schema are restored.
    assert_eq!(read_native_state(&root), before);
    assert_eq!(read_size(&mut storage, &old), Some(3));
    assert_eq!(
        storage.reset_vectors(None, &old).unwrap(),
        VectorOutcome::Current(false)
    );
    assert_eq!(read_size(&mut storage, &old), Some(3));
    // Act: commit the reset and establish a different first dimension.
    assert_eq!(
        storage.reset_vectors(Some(&old), &new).unwrap(),
        VectorOutcome::Current(true)
    );
    assert_eq!(read_size(&mut storage, &new), None);
    store(&mut storage, &new, "new", 4);
    assert_eq!(read_size(&mut storage, &new), Some(4));
}

#[test]
fn unsupported_native_dimensions_poison_the_unit_without_keys() {
    for size in [1, 4097] {
        // Arrange
        let root = TempDir::new().unwrap();
        let mut storage = open(&root);
        let identity = Sha256::digest(b"model");
        storage.reset_vectors(None, &identity).unwrap();
        let before = read_native_state(&root);
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        // Act
        let error = unit
            .store_vector(&identity, &vec![1.0; size], size)
            .unwrap_err();
        // Assert
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(unit.commit().is_err());
        assert_eq!(read_native_state(&root), before);
        assert_eq!(read_size(&mut storage, &identity), None);
    }
}

#[test]
fn nullable_failure_hash_does_not_hide_unrelated_work() {
    // Arrange: SQLite rowid-table TEXT PRIMARY KEY permits a legacy NULL key.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "pending")]);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    // This malformed legacy row cannot be created by the typed writer.
    db.execute(
        "INSERT INTO embed_failures(search_hash, attempts) VALUES(NULL, 4)",
        [],
    )
    .unwrap();
    // Act
    let work = read_next_work(&mut storage, &identity, 4, None).unwrap();
    // Assert: a NOT IN anti-join would wrongly suppress every candidate.
    assert_eq!(work.id.as_str(), "a.md");
}

#[test]
fn invalid_attempt_budgets_are_rejected_without_changing_storage() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "pending")]);
    // Act and assert
    let mut read = storage.begin_read().unwrap();
    for budget in [0, u64::MAX] {
        let error = read
            .read_next_embedding_work(&identity, budget, None)
            .unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }
    assert_eq!(read.get_revision(), 1);
}

#[test]
fn abandoned_cleanup_restores_vectors_failures_and_dimensions() {
    // Arrange
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.reserve_attempt(&identity, 4).unwrap();
    unit.commit().unwrap();
    let before = read_native_state(&root);
    // Act
    let mut cleanup = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap();
    drop(cleanup);
    // Assert
    assert_eq!(read_native_state(&root), before);
    assert_eq!(read_size(&mut storage, &identity), Some(3));
    let mut read = storage.begin_read().unwrap();
    assert_eq!(
        unwrap_current(read.read_failure(&identity, &identity).unwrap())
            .unwrap()
            .attempts,
        1
    );
}

fn open(root: &TempDir) -> impl Storage + use<> {
    open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn publish(storage: &mut impl Storage, rows: &[(&str, &str)]) -> u64 {
    let mut unit = storage.begin_cache_write().unwrap();
    for (id, text) in rows {
        unit.upsert_node(&support::build_knowledge(id, text))
            .unwrap();
    }
    unit.commit().unwrap()
}

fn store(storage: &mut impl Storage, identity: &Sha256, text: &str, size: usize) -> i64 {
    let mut unit = unwrap_current(storage.begin_vector_write(identity).unwrap());
    let key = unit
        .store_vector(&Sha256::digest(text.as_bytes()), &vec![1.0; size], size)
        .unwrap();
    unit.commit().unwrap();
    key.row_id
}

fn read_next_work(
    storage: &mut impl Storage,
    identity: &Sha256,
    budget: u64,
    after: Option<&KnowledgeId>,
) -> Option<EmbeddingWork> {
    unwrap_current(
        storage
            .begin_read()
            .unwrap()
            .read_next_embedding_work(identity, budget, after)
            .unwrap(),
    )
}

fn read_size(storage: &mut impl Storage, identity: &Sha256) -> Option<usize> {
    unwrap_current(
        storage
            .begin_read()
            .unwrap()
            .read_vector_dimensions(identity)
            .unwrap(),
    )
    .map(Into::into)
}

fn prune(storage: &mut impl Storage, identity: &Sha256) {
    let mut unit = unwrap_current(storage.begin_vector_write(identity).unwrap());
    unit.prune_unreferenced().unwrap();
    unit.commit().unwrap();
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected identity change"),
    }
}

fn count_keys(root: &TempDir) -> i64 {
    Connection::open(root.path().join("vectors"))
        .unwrap()
        .query_row("SELECT count(*) FROM vector_keys", [], |row| row.get(0))
        .unwrap()
}

fn read_native_state(root: &TempDir) -> Vec<Vec<Vec<Value>>> {
    let db = Connection::open(root.path().join("vectors")).unwrap();
    [
        "SELECT row_id, search_hash FROM vector_keys ORDER BY row_id",
        "SELECT id, vector FROM vector_index_base ORDER BY id",
        "SELECT id, val FROM vector_index_config ORDER BY id",
        "SELECT id, val FROM vector_index_model ORDER BY id",
        "SELECT type, name, sql FROM sqlite_schema ORDER BY name",
    ]
    .into_iter()
    .map(|sql| {
        let mut statement = db.prepare(sql).unwrap();
        let count = statement.column_count();
        statement
            .query_map([], |row| (0..count).map(|i| row.get(i)).collect())
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    })
    .collect()
}
