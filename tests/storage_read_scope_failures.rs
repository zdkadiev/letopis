use letopis::{
    domain::{RepoPath, Sha256},
    service::{
        CacheDatabase, StorageError, StorageErrorKind, TransactionError, VectorDatabase,
        VectorOutcome,
    },
};
use storage_support::open_storage;
use tempfile::TempDir;

#[path = "support/storage.rs"]
mod storage_support;

#[test]
fn caught_direct_cache_write_makes_read_scope_rollback_only() {
    // Given: a cache owner with an active read transaction.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let path = RepoPath::parse("missing.md").unwrap();

    // When: the callback catches a rejected direct write and returns success.
    let result = storage.cache.with_read_tx(|| {
        let error = storage.cache.files_delete_by_path(&path).unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        Ok::<_, StorageError>(())
    });

    // Then: the outer transaction fails and the owner remains usable afterward.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert!(
        storage
            .cache
            .with_read_tx(|| Ok::<_, StorageError>(()))
            .is_ok()
    );
}

#[test]
fn caught_direct_vector_write_makes_read_scope_rollback_only() {
    // Given: a vector owner with an active read transaction for its identity.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();

    // When: the callback catches a rejected direct write and returns success.
    let result = storage.vectors.with_read_tx(&identity, || {
        let error = storage.vectors.query_error_delete_by_id().unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        Ok::<_, StorageError>(())
    });

    // Then: the outer transaction fails and the owner remains usable afterward.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert!(matches!(
        storage
            .vectors
            .with_read_tx(&identity, || Ok::<_, StorageError>(())),
        Ok(VectorOutcome::Current(()))
    ));
}

#[test]
fn rejected_revision_accessor_does_not_poison_read_scope() {
    // Given: a cache owner in a read scope, where a publication revision is unavailable.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();

    // When: the callback catches the read-only accessor's precondition error.
    let result = storage.cache.with_read_tx(|| {
        let error = storage.cache.get_publication_revision().unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        Ok::<_, StorageError>(())
    });

    // Then: the surrounding read scope succeeds because no write was attempted.
    assert!(matches!(result, Ok(())));
}
