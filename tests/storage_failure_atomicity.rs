use letopis::{
    domain::Sha256,
    open_storage,
    service::{Storage, StorageErrorKind, VectorOutcome, VectorWrite},
};
use rusqlite::Connection;
use tempfile::TempDir;

#[test]
fn a_failed_failure_update_poisons_the_entire_vector_unit() {
    // Arrange: reject diagnostic updates after a reservation can be written.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let mut storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"document");
    storage.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "CREATE TRIGGER deny_failure_update BEFORE UPDATE OF error ON embed_failures
         BEGIN SELECT RAISE(ABORT, 'controlled update refusal'); END;",
        )
        .unwrap();
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("configuration unexpectedly changed");
    };
    assert!(unit.reserve_attempt(&hash, 3).unwrap());
    // Act
    let error = unit.record_failure(&hash, "safe diagnostic").unwrap_err();
    let commit_error = unit.commit().unwrap_err();
    // Assert: even the earlier valid reservation must roll back.
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InvalidRequest);
    let count: i64 = observer
        .query_row("SELECT count(*) FROM embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_failed_query_error_cleanup_poisons_the_entire_vector_unit() {
    // Arrange: retain an existing query diagnostic while rejecting its deletion.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let mut storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"document");
    storage.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "INSERT INTO query_error VALUES(1, 17, 'existing diagnostic');
         CREATE TRIGGER deny_query_cleanup BEFORE DELETE ON query_error
         BEGIN SELECT RAISE(ABORT, 'controlled deletion refusal'); END;",
        )
        .unwrap();
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("configuration unexpectedly changed");
    };
    assert!(unit.reserve_attempt(&hash, 3).unwrap());
    // Act
    let error = unit.clear_query_error().unwrap_err();
    let commit_error = unit.commit().unwrap_err();
    // Assert: the diagnostic survives and the earlier reservation rolls back.
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InvalidRequest);
    let count: i64 = observer
        .query_row("SELECT count(*) FROM embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let diagnostic: String = observer
        .query_row("SELECT error FROM query_error", [], |row| row.get(0))
        .unwrap();
    assert_eq!(diagnostic, "existing diagnostic");
}

#[test]
fn a_failed_orphan_prune_poisons_the_entire_vector_unit() {
    // Arrange: an orphan reservation reaches the final cleanup statement.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let mut storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"unreferenced document");
    storage.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "CREATE TRIGGER deny_orphan_cleanup BEFORE DELETE ON embed_failures
         BEGIN SELECT RAISE(ABORT, 'controlled orphan cleanup refusal'); END;",
        )
        .unwrap();
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(&identity).unwrap() else {
        panic!("configuration unexpectedly changed");
    };
    assert!(unit.reserve_attempt(&hash, 3).unwrap());
    // Act
    let error = unit.prune_unreferenced().unwrap_err();
    let commit_error = unit.commit().unwrap_err();
    // Assert: a failed cleanup cannot commit the earlier reservation.
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InvalidRequest);
    let count: i64 = observer
        .query_row("SELECT count(*) FROM embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
