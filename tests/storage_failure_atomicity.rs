use letopis::service::StorageError;
use letopis::service::VectorDatabase;
use storage_support::open_storage;
#[path = "support/storage.rs"]
mod storage_support;
use letopis::{domain::Sha256, service::StorageErrorKind};
use rusqlite::Connection;
use std::cell::Cell;
use tempfile::TempDir;

#[test]
fn a_failed_failure_update_poisons_the_entire_vector_unit() {
    // Given: reject diagnostic updates after a reservation can be written.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"document");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "create trigger deny_failure_update before update of error on embed_failures
         begin select raise(abort, 'controlled update refusal'); end;",
        )
        .unwrap();
    let entered = Cell::new(false);
    let result = storage.vectors.with_write_tx(&identity, || {
        entered.set(true);
        let unit = &storage.vectors;
        // When / Then: an earlier valid reservation succeeds inside the same transaction.
        assert!(
            unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                .unwrap()
        );
        // When: the failure update and commit are attempted.
        let error = unit
            .embed_failures_update_by_search_hash(&hash, "safe diagnostic")
            .unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        Ok::<_, StorageError>(())
    });
    assert!(entered.get());
    let commit_error = result.unwrap_err().into_inner();
    // Then: even the earlier valid reservation must roll back.
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InternalError);
    let count: i64 = observer
        .query_row("select count(*) from embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}

#[test]
fn a_failed_query_error_cleanup_poisons_the_entire_vector_unit() {
    // Given: retain an existing query diagnostic while rejecting its deletion.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"document");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "insert into query_error values(1, 17, 'existing diagnostic');
         create trigger deny_query_cleanup before delete on query_error
         begin select raise(abort, 'controlled deletion refusal'); end;",
        )
        .unwrap();
    let entered = Cell::new(false);
    let result = storage.vectors.with_write_tx(&identity, || {
        entered.set(true);
        let unit = &storage.vectors;
        // When / Then: an earlier valid reservation succeeds inside the same transaction.
        assert!(
            unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                .unwrap()
        );
        // When: query-error cleanup and commit are attempted.
        let error = unit.query_error_delete_by_id().unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        Ok::<_, StorageError>(())
    });
    assert!(entered.get());
    let commit_error = result.unwrap_err().into_inner();
    // Then: the diagnostic survives and the earlier reservation rolls back.
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InternalError);
    let count: i64 = observer
        .query_row("select count(*) from embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
    let diagnostic: String = observer
        .query_row("select error from query_error", [], |row| row.get(0))
        .unwrap();
    assert_eq!(diagnostic, "existing diagnostic");
}

#[test]
fn a_failed_orphan_prune_poisons_the_entire_vector_unit() {
    // Given: an orphan reservation reaches the final cleanup statement.
    let root = TempDir::new().unwrap();
    let vector_path = root.path().join("vectors.db");
    let storage = open_storage(&root.path().join("cache.db"), &vector_path).unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"unreferenced document");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    let observer = Connection::open(&vector_path).unwrap();
    observer
        .execute_batch(
            "create trigger deny_orphan_cleanup before delete on embed_failures
         begin select raise(abort, 'controlled orphan cleanup refusal'); end;",
        )
        .unwrap();
    let entered = Cell::new(false);
    let result = storage.vectors.with_write_tx(&identity, || {
        entered.set(true);
        let unit = &storage.vectors;
        // When / Then: an earlier valid reservation succeeds inside the same transaction.
        assert!(
            unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                .unwrap()
        );
        // When: orphan pruning and commit are attempted.
        let error = unit.prune_unreferenced().unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        Ok::<_, StorageError>(())
    });
    assert!(entered.get());
    let commit_error = result.unwrap_err().into_inner();
    // Then: a failed cleanup cannot commit the earlier reservation.
    assert_eq!(commit_error.get_kind(), StorageErrorKind::InternalError);
    let count: i64 = observer
        .query_row("select count(*) from embed_failures", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 0);
}
