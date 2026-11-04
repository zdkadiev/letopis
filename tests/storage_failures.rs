use crate::storage_support::get_current;
use letopis::service::StorageError;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
use storage_support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod storage_support;
use letopis::{
    domain::Sha256,
    service::{EmbeddingFailure, QueryError, VectorOutcome},
};
use tempfile::TempDir;

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the sequential transaction scenario and its assertions together"
)]
fn reservation_is_durable_only_after_commit_and_survives_reopen() {
    // Catches implicit commit, forgotten persisted counters, and off-by-one budgets.
    // Given: reservation budgets and a fresh vector store.
    for budget in [1, 3] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        let vectors = root.path().join("vectors");
        let identity = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"text");
        let mut storage = open_storage(&cache, &vectors).unwrap();
        storage.vectors.reset_vectors(None, &identity).unwrap();
        assert!(matches!(
            storage.vectors.with_write_tx(&identity, || {
                let unit = &storage.vectors;
                // When / Then: a reservation is dropped or committed; only committed reservations survive reopening.
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, budget)
                        .unwrap()
                );

                Err::<(), _>(())
            }),
            Err(TransactionError::Operation(()))
        ));

        assert_eq!(get_failure_by_search_hash(&storage, &identity, &hash), None);
        for expected in 1..=budget {
            storage
                .vectors
                .with_write_tx(&identity, || {
                    let unit = &storage.vectors;
                    // When / Then: a reservation is dropped or committed; only committed reservations survive reopening.
                    assert!(
                        unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, budget)
                            .unwrap()
                    );

                    Ok::<_, StorageError>(())
                })
                .map(get_current)
                .unwrap();
            drop(storage);
            storage = open_storage(&cache, &vectors).unwrap();
            assert_eq!(
                get_failure_by_search_hash(&storage, &identity, &hash),
                Some(EmbeddingFailure {
                    search_hash: hash,
                    attempts: expected,
                    error: None
                })
            );
        }
        storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                assert!(
                    !unit
                        .embed_failures_upsert_by_search_hash_and_attempts(&hash, budget)
                        .unwrap()
                );

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
        assert_eq!(
            get_failure_by_search_hash(&storage, &identity, &hash)
                .unwrap()
                .attempts,
            budget
        );
    }
}
#[test]
fn absent_failure_is_not_created_by_error() {
    // Catches errors inventing attempts for requests that were never reserved.
    // Given: no reservation for the requested search hash.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            // When / Then: a failure message is recorded; no failure row is created without a reservation.
            assert!(
                !unit
                    .embed_failures_update_by_search_hash(&identity, "safe failure")
                    .unwrap()
            );

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert_eq!(
        get_failure_by_search_hash(&storage, &identity, &identity),
        None
    );
}
#[test]
fn success_clears_only_its_failure() {
    // Catches success deleting unrelated failures or uncommitted deletes leaking.
    // Given: two reserved search hashes with stored failures.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    let other = Sha256::digest(b"other");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            // When / Then: each hash receives a durable reservation and its own failure message.
            for hash in [identity, other] {
                // When / Then: each hash receives a durable reservation and its own failure message.
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                        .unwrap()
                );
                assert!(
                    unit.embed_failures_update_by_search_hash(&hash, "safe failure")
                        .unwrap()
                );
            }

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert!(matches!(
        storage.vectors.with_write_tx(&identity, || {
            let unit = &storage.vectors;
            unit.store_vector(&identity, &[1.0, 0.0], 2).unwrap();

            Err::<(), _>(())
        }),
        Err(TransactionError::Operation(()))
    ));

    assert_eq!(
        get_failure_by_search_hash(&storage, &identity, &identity)
            .unwrap()
            .attempts,
        1
    );
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            unit.store_vector(&identity, &[1.0, 0.0], 2).unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert_eq!(
        get_failure_by_search_hash(&storage, &identity, &identity),
        None
    );
    assert_eq!(
        get_failure_by_search_hash(&storage, &identity, &other),
        Some(EmbeddingFailure {
            search_hash: other,
            attempts: 1,
            error: Some("safe failure".into())
        })
    );
}
#[test]
fn query_error_replaces_and_clears_without_history() {
    // Catches reset clearing last query failure or upserts retaining history.
    // Given: a persisted singleton query error.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let identity = Sha256::digest(b"M1");
    let storage = open_storage(&cache, &vectors).unwrap();
    storage.vectors.reset_vectors(None, &identity).unwrap();
    for (at, message) in [(10, "first safe message"), (20, "latest safe message")] {
        storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                unit.query_error_upsert_by_id(at, message).unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
    }
    drop(storage);
    let storage = open_storage(&cache, &vectors).unwrap();
    let next = Sha256::digest(b"M2");
    storage
        .vectors
        .reset_vectors(Some(&identity), &next)
        .unwrap();
    // When / Then: the error is replaced and later cleared; only the latest query error is stored without history.
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.query_error_get_by_id())
            .unwrap(),
        Some(QueryError {
            at: 20,
            error: "latest safe message".into()
        })
    );
    assert_query_error_schema(&vectors);
    storage
        .vectors
        .with_write_tx(&next, || {
            let unit = &storage.vectors;
            unit.query_error_delete_by_id().unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.query_error_get_by_id())
            .unwrap(),
        None
    );
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.project_get_by_id())
            .unwrap(),
        0
    );
}
#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the sequential transaction scenario and its assertions together"
)]
fn stale_identity_cannot_reserve_or_record_or_store() {
    // Every late action must be gated before it can alter populated M2 state.
    // Given: an obsolete vector identity.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    storage.vectors.reset_vectors(None, &m1).unwrap();
    storage.vectors.reset_vectors(Some(&m1), &m2).unwrap();
    storage
        .vectors
        .with_write_tx(&m2, || {
            let unit = &storage.vectors;
            // When / Then: the obsolete writer tries to reserve, record or store; current vector state rejects the stale writer.
            assert!(
                unit.embed_failures_upsert_by_search_hash_and_attempts(&m2, 3)
                    .unwrap()
            );
            assert!(
                unit.embed_failures_update_by_search_hash(&m2, "M2 failure")
                    .unwrap()
            );
            unit.query_error_upsert_by_id(42, "M2 query").unwrap();
            unit.store_vector(&m1, &[1.0, 0.0], 2).unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    reject_stale_actions(&storage, &m1, &m2);
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(read.project_get_by_id().unwrap(), 0);
            assert_eq!(read.meta_get_by_id().unwrap(), Some(m2));
            assert!(matches!(
                read.embed_failures_get_by_search_hash(&m1, &m2).unwrap(),
                VectorOutcome::ConfigChanged
            ));
            assert_eq!(
                unwrap_current(read.embed_failures_get_by_search_hash(&m2, &m2).unwrap()),
                Some(EmbeddingFailure {
                    search_hash: m2,
                    attempts: 1,
                    error: Some("M2 failure".into())
                })
            );
            assert_eq!(
                read.query_error_get_by_id().unwrap(),
                Some(QueryError {
                    at: 42,
                    error: "M2 query".into()
                })
            );
            assert_eq!(
                unwrap_current(
                    read.vector_keys_get_vector_by_search_hash(&m2, &m1)
                        .unwrap()
                ),
                Some(vec![1.0, 0.0])
            );
            assert_eq!(
                unwrap_current(
                    read.vector_keys_get_vector_by_search_hash(&m2, &m2)
                        .unwrap()
                ),
                None
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}
#[test]
fn invalid_budget_and_timestamp_poison_units_without_partial_writes() {
    // Catches wrapping conversions and committing writes preceding invalid input.
    // Given: invalid attempt budgets and timestamps with prior valid writes.
    for invalid in [0, u64::MAX] {
        let root = TempDir::new().unwrap();
        let storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let identity = Sha256::digest(b"M1");
        storage.vectors.reset_vectors(None, &identity).unwrap();
        let unit_result = storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                // When / Then: the invalid diagnostic writes are attempted; poisoned units cannot commit partial changes.
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&identity, 3)
                        .unwrap()
                );
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&identity, invalid)
                        .is_err()
                );

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .map_err(TransactionError::into_inner);
        assert!(unit_result.is_err());
        assert_eq!(
            get_failure_by_search_hash(&storage, &identity, &identity),
            None
        );
        let unit_result = storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                unit.query_error_upsert_by_id(10, "safe").unwrap();
                assert!(unit.query_error_upsert_by_id(u64::MAX, "overflow").is_err());

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .map_err(TransactionError::into_inner);
        assert!(unit_result.is_err());
        assert_eq!(
            storage
                .cache
                .with_read_tx(|| storage.cache.query_error_get_by_id())
                .unwrap(),
            None
        );
    }
}
#[test]
fn maximum_persisted_attempt_is_not_incremented_or_wrapped() {
    // Catches SQLite promoting integer overflow into a floating-point counter.
    // Given: a persisted attempt count at SQLite's maximum.
    let root = TempDir::new().unwrap();
    let vectors = root.path().join("vectors");
    let storage = open_storage(&root.path().join("cache"), &vectors).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    let raw = rusqlite::Connection::open(&vectors).unwrap();
    raw.execute(
        "insert into embed_failures(search_hash,attempts,error) values(?1,?2,'safe')",
        rusqlite::params![identity.to_hex(), i64::MAX],
    )
    .unwrap();
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            // When / Then: another attempt is reserved; the count neither increments nor wraps.
            assert!(
                !unit
                    .embed_failures_upsert_by_search_hash_and_attempts(&identity, i64::MAX as u64)
                    .unwrap()
            );
            unit.query_error_upsert_by_id(i64::MAX as u64, "maximum time")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    assert_eq!(
        get_failure_by_search_hash(&storage, &identity, &identity),
        Some(EmbeddingFailure {
            search_hash: identity,
            attempts: i64::MAX as u64,
            error: Some("safe".into())
        })
    );
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.query_error_get_by_id())
            .unwrap(),
        Some(QueryError {
            at: i64::MAX as u64,
            error: "maximum time".into()
        })
    );
}

fn reject_stale_actions<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    m1: &Sha256,
    m2: &Sha256,
) {
    for action in 0..5 {
        let result = storage
            .vectors
            .with_write_tx(m1, || {
                match action {
                    0 => {
                        storage
                            .vectors
                            .embed_failures_upsert_by_search_hash_and_attempts(m2, 3)?;
                    }
                    1 => {
                        storage
                            .vectors
                            .embed_failures_update_by_search_hash(m2, "late failure")?;
                    }
                    2 => {
                        storage.vectors.store_vector(m2, &[0.0, 1.0], 2)?;
                    }
                    3 => storage.vectors.query_error_upsert_by_id(99, "late query")?,
                    _ => storage.vectors.query_error_delete_by_id()?,
                }
                Ok::<_, StorageError>(())
            })
            .unwrap();
        assert!(matches!(result, VectorOutcome::ConfigChanged));
    }
}

fn get_failure_by_search_hash<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
    search_hash: &Sha256,
) -> Option<EmbeddingFailure> {
    storage
        .cache
        .with_read_tx(|| {
            Ok::<_, StorageError>({
                let read = &storage.cache;
                assert_eq!(read.project_get_by_id().unwrap(), 0);
                unwrap_current(
                    read.embed_failures_get_by_search_hash(identity, search_hash)
                        .unwrap(),
                )
            })
        })
        .unwrap()
}

fn unwrap_current<T>(value: VectorOutcome<T>) -> T {
    match value {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected identity change"),
    }
}

fn assert_query_error_schema(vectors: &std::path::Path) {
    let raw = rusqlite::Connection::open(vectors).unwrap();
    let count: i64 = raw
        .query_row("select count(*) from query_error", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let mut tables = raw.prepare("select name from sqlite_schema where type='table' and name not like 'vector_index%' order by name").unwrap();
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        names,
        ["embed_failures", "meta", "query_error", "vector_keys"]
    );
}
