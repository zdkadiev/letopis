use letopis::{
    domain::Sha256,
    open_storage,
    service::{EmbeddingFailure, QueryError, Storage, StorageRead, VectorOutcome, VectorWrite},
};
use tempfile::TempDir;

#[test]
fn reservation_is_durable_only_after_commit_and_survives_reopen() {
    // Catches implicit commit, forgotten persisted counters, and off-by-one budgets.
    // Given
    for budget in [1, 3] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        let vectors = root.path().join("vectors");
        let identity = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"text");
        let mut storage = open_storage(&cache, &vectors).unwrap();
        storage.reset_vectors(None, &identity).unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        // When / Then
        assert!(unit.reserve_attempt(&hash, budget).unwrap());
        drop(unit);
        assert_eq!(read_failure(&mut storage, &identity, &hash), None);
        for expected in 1..=budget {
            let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
            // When / Then
            assert!(unit.reserve_attempt(&hash, budget).unwrap());
            unit.commit().unwrap();
            drop(storage);
            storage = open_storage(&cache, &vectors).unwrap();
            assert_eq!(
                read_failure(&mut storage, &identity, &hash),
                Some(EmbeddingFailure {
                    search_hash: hash,
                    attempts: expected,
                    error: None
                })
            );
        }
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        assert!(!unit.reserve_attempt(&hash, budget).unwrap());
        unit.commit().unwrap();
        assert_eq!(
            read_failure(&mut storage, &identity, &hash)
                .unwrap()
                .attempts,
            budget
        );
    }
}
#[test]
fn absent_failure_is_not_created_by_error() {
    // Catches errors inventing attempts for requests that were never reserved.
    // Given
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.reset_vectors(None, &identity).unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    // When / Then
    assert!(!unit.record_failure(&identity, "safe failure").unwrap());
    unit.commit().unwrap();
    assert_eq!(read_failure(&mut storage, &identity, &identity), None);
}
#[test]
fn success_clears_only_its_failure() {
    // Catches success deleting unrelated failures or uncommitted deletes leaking.
    // Given
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let identity = Sha256::digest(b"M1");
    let other = Sha256::digest(b"other");
    storage.reset_vectors(None, &identity).unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    // When / Then
    for hash in [identity, other] {
        // When / Then
        assert!(unit.reserve_attempt(&hash, 3).unwrap());
        assert!(unit.record_failure(&hash, "safe failure").unwrap());
    }
    unit.commit().unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.store_vector(&identity, &[1.0, 0.0], 2).unwrap();
    drop(unit);
    assert_eq!(
        read_failure(&mut storage, &identity, &identity)
            .unwrap()
            .attempts,
        1
    );
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.store_vector(&identity, &[1.0, 0.0], 2).unwrap();
    unit.commit().unwrap();
    assert_eq!(read_failure(&mut storage, &identity, &identity), None);
    assert_eq!(
        read_failure(&mut storage, &identity, &other),
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
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache");
    let vectors = root.path().join("vectors");
    let identity = Sha256::digest(b"M1");
    let mut storage = open_storage(&cache, &vectors).unwrap();
    storage.reset_vectors(None, &identity).unwrap();
    for (at, message) in [(10, "first safe message"), (20, "latest safe message")] {
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        unit.set_query_error(at, message).unwrap();
        unit.commit().unwrap();
    }
    drop(storage);
    let mut storage = open_storage(&cache, &vectors).unwrap();
    let next = Sha256::digest(b"M2");
    storage.reset_vectors(Some(&identity), &next).unwrap();
    // When / Then
    assert_eq!(
        storage.begin_read().unwrap().read_query_error().unwrap(),
        Some(QueryError {
            at: 20,
            error: "latest safe message".into()
        })
    );
    let raw = rusqlite::Connection::open(&vectors).unwrap();
    let count: i64 = raw
        .query_row("SELECT count(*) FROM query_error", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let mut tables = raw.prepare("SELECT name FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'vector_index%' ORDER BY name").unwrap();
    let names = tables
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        names,
        ["embed_failures", "meta", "query_error", "vector_keys"]
    );
    let mut unit = unwrap_current(storage.begin_vector_write(&next).unwrap());
    unit.clear_query_error().unwrap();
    unit.commit().unwrap();
    assert_eq!(
        storage.begin_read().unwrap().read_query_error().unwrap(),
        None
    );
    assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
}
#[test]
fn stale_identity_cannot_reserve_or_record_or_store() {
    // Every late action must be gated before it can alter populated M2 state.
    // Given
    let root = TempDir::new().unwrap();
    let mut storage =
        open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    let m1 = Sha256::digest(b"M1");
    let m2 = Sha256::digest(b"M2");
    storage.reset_vectors(None, &m1).unwrap();
    storage.reset_vectors(Some(&m1), &m2).unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&m2).unwrap());
    // When / Then
    assert!(unit.reserve_attempt(&m2, 3).unwrap());
    assert!(unit.record_failure(&m2, "M2 failure").unwrap());
    unit.set_query_error(42, "M2 query").unwrap();
    unit.store_vector(&m1, &[1.0, 0.0], 2).unwrap();
    unit.commit().unwrap();
    reject_stale_actions(&mut storage, &m1, &m2);
    let mut read = storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 0);
    assert_eq!(read.get_config_identity(), Some(m2));
    assert!(matches!(
        read.read_failure(&m1, &m2).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert_eq!(
        unwrap_current(read.read_failure(&m2, &m2).unwrap()),
        Some(EmbeddingFailure {
            search_hash: m2,
            attempts: 1,
            error: Some("M2 failure".into())
        })
    );
    assert_eq!(
        read.read_query_error().unwrap(),
        Some(QueryError {
            at: 42,
            error: "M2 query".into()
        })
    );
    assert_eq!(
        unwrap_current(read.read_vector(&m2, &m1).unwrap()),
        Some(vec![1.0, 0.0])
    );
    assert_eq!(unwrap_current(read.read_vector(&m2, &m2).unwrap()), None);
}
#[test]
fn invalid_budget_and_timestamp_poison_units_without_partial_writes() {
    // Catches wrapping conversions and committing writes preceding invalid input.
    // Given
    for invalid in [0, u64::MAX] {
        let root = TempDir::new().unwrap();
        let mut storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let identity = Sha256::digest(b"M1");
        storage.reset_vectors(None, &identity).unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        // When / Then
        assert!(unit.reserve_attempt(&identity, 3).unwrap());
        assert!(unit.reserve_attempt(&identity, invalid).is_err());
        assert!(unit.commit().is_err());
        assert_eq!(read_failure(&mut storage, &identity, &identity), None);
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        unit.set_query_error(10, "safe").unwrap();
        assert!(unit.set_query_error(u64::MAX, "overflow").is_err());
        assert!(unit.commit().is_err());
        assert_eq!(
            storage.begin_read().unwrap().read_query_error().unwrap(),
            None
        );
    }
}
#[test]
fn maximum_persisted_attempt_is_not_incremented_or_wrapped() {
    // Catches SQLite promoting integer overflow into a floating-point counter.
    // Given
    let root = TempDir::new().unwrap();
    let vectors = root.path().join("vectors");
    let mut storage = open_storage(&root.path().join("cache"), &vectors).unwrap();
    let identity = Sha256::digest(b"M1");
    storage.reset_vectors(None, &identity).unwrap();
    let raw = rusqlite::Connection::open(&vectors).unwrap();
    raw.execute(
        "INSERT INTO embed_failures(search_hash,attempts,error) VALUES(?1,?2,'safe')",
        rusqlite::params![identity.to_hex(), i64::MAX],
    )
    .unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    // When / Then
    assert!(!unit.reserve_attempt(&identity, i64::MAX as u64).unwrap());
    unit.set_query_error(i64::MAX as u64, "maximum time")
        .unwrap();
    unit.commit().unwrap();
    assert_eq!(
        read_failure(&mut storage, &identity, &identity),
        Some(EmbeddingFailure {
            search_hash: identity,
            attempts: i64::MAX as u64,
            error: Some("safe".into())
        })
    );
    assert_eq!(
        storage.begin_read().unwrap().read_query_error().unwrap(),
        Some(QueryError {
            at: i64::MAX as u64,
            error: "maximum time".into()
        })
    );
}

fn reject_stale_actions(storage: &mut impl Storage, m1: &Sha256, m2: &Sha256) {
    for action in 0..5 {
        match storage.begin_vector_write(m1).unwrap() {
            VectorOutcome::ConfigChanged => (),
            VectorOutcome::Current(mut unit) => {
                match action {
                    0 => {
                        unit.reserve_attempt(m2, 3).unwrap();
                    }
                    1 => {
                        unit.record_failure(m2, "late failure").unwrap();
                    }
                    2 => {
                        unit.store_vector(m2, &[0.0, 1.0], 2).unwrap();
                    }
                    3 => {
                        unit.set_query_error(99, "late query").unwrap();
                    }
                    _ => {
                        unit.clear_query_error().unwrap();
                    }
                }
                unit.commit().unwrap();
                panic!("stale writer accepted");
            }
        }
    }
}

fn read_failure(
    storage: &mut impl Storage,
    identity: &Sha256,
    hash: &Sha256,
) -> Option<EmbeddingFailure> {
    let mut read = storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 0);
    unwrap_current(read.read_failure(identity, hash).unwrap())
}

fn unwrap_current<T>(value: VectorOutcome<T>) -> T {
    match value {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("unexpected identity change"),
    }
}
