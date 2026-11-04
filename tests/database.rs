use letopis::{
    domain::Sha256,
    service::{CacheDatabase, StorageError, TransactionError, VectorDatabase, VectorOutcome},
};
use rusqlite::Connection;
use std::path::Path;
use storage_support::{Databases, get_current, open_storage};
use tempfile::TempDir;
#[path = "support/storage.rs"]
mod storage_support;

#[test]
fn reset_late_meta_failure_preserves_identity_key_vector_and_query_error() {
    // Given: a populated native table, then fail after DROP/recreation/deletion.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let storage = open_storage(&cache, &vectors).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    let key = seed_vector(&storage, &m1);
    let observer = Connection::open(&vectors).unwrap();
    observer.execute_batch("insert into embed_failures values('failed-text',1,'safe failure'); insert into query_error values(1,7,'safe failure'); create trigger deny_meta_update before update on meta begin select raise(abort, 'blocked meta write'); end;").unwrap();
    // When: the metadata update fails after native schema changes.
    assert!(storage.vectors.reset_vectors(Some(&m1), &m2).is_err());
    assert_eq!(count_failures(&vectors), 1);
    // Then: the same and a newly opened read both retain exact M1 bytes.
    assert_eq!(
        observer
            .query_row("select row_id from vector_keys", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        key
    );
    assert_original_vector(&storage, &m1);

    observer
        .execute_batch("drop trigger deny_meta_update")
        .unwrap();
    drop(storage);
    let reopened = open_storage(&cache, &vectors).unwrap();
    assert_original_vector(&reopened, &m1);

    assert_eq!(
        reopened.vectors.reset_vectors(Some(&m1), &m2).unwrap(),
        VectorOutcome::Current(true)
    );
    assert_eq!(count_failures(&vectors), 0);
    assert_eq!(
        observer
            .query_row("select error from query_error", [], |r| r
                .get::<_, String>(0))
            .unwrap(),
        "safe failure"
    );
}

#[test]
fn success_deletes_failure_atomically_and_rollback_preserves_it() {
    // Catches lost cleanup, uncommitted cleanup, and acceptance of orphan keys.
    // Given: reserved failures and a vector key that can be corrupted.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let observer = Connection::open(root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    let hash = Sha256::digest(b"text");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    observer
        .execute(
            "insert into embed_failures values(?1,1,'safe failure')",
            [hash.to_hex()],
        )
        .unwrap();
    // When: a successful vector write is abandoned before commit.
    assert!(matches!(
        storage.vectors.with_write_tx(&identity, || {
            storage
                .vectors
                .store_vector(&hash, &[1.0, 0.0, 0.0], 3)
                .unwrap();

            Err::<(), _>(())
        }),
        Err(TransactionError::Operation(()))
    ));

    // Then: rollback retains the previously recorded failure.
    assert_eq!(
        observer
            .query_row("select count(*) from embed_failures", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    // When: repeat the vector write and commit it.
    storage
        .vectors
        .with_write_tx(&identity, || {
            storage
                .vectors
                .store_vector(&hash, &[1.0, 0.0, 0.0], 3)
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    // Then: committing the successful vector write removes its failure.
    assert_eq!(
        observer
            .query_row("select count(*) from embed_failures", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn corrupt_key_poisons_the_vector_scope() {
    // Given: a configured database with a key missing its native vector.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let observer = Connection::open(root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    observer
        .execute(
            "insert into vector_keys(search_hash) values(?1)",
            [identity.to_hex()],
        )
        .unwrap();
    let unit_result = storage
        .vectors
        .with_write_tx(&identity, || {
            // When / Then: a key without its native vector poisons the write and prevents commit.
            assert!(
                storage
                    .vectors
                    .store_vector(&identity, &[1.0, 0.0, 0.0], 3)
                    .is_err()
            );

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    assert!(unit_result.is_err());
}

fn assert_original_vector<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
) {
    storage
        .cache
        .with_read_tx(|| {
            assert_eq!(storage.cache.meta_get_by_id().unwrap(), Some(*identity));
            assert_eq!(
                get_current(
                    storage
                        .cache
                        .vector_keys_get_vector_by_search_hash(identity, identity)
                        .unwrap()
                ),
                Some(vec![1.0, 0.0, 0.0])
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

fn seed_vector<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
) -> i64 {
    storage.vectors.reset_vectors(None, identity).unwrap();
    storage
        .vectors
        .with_write_tx(identity, || {
            storage
                .vectors
                .store_vector(identity, &[1.0, 0.0, 0.0], 3)
                .map(|key| key.row_id)
        })
        .map(get_current)
        .unwrap()
}

fn count_failures(path: &Path) -> i64 {
    Connection::open(path)
        .unwrap()
        .query_row("select count(*) from embed_failures", [], |r| r.get(0))
        .unwrap()
}
