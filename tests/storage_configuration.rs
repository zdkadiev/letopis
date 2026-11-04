use crate::storage_support::get_current;
use letopis::service::StorageError;
use letopis::service::{CacheDatabase, VectorDatabase};
use storage_support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod storage_support;
use letopis::{
    domain::Sha256,
    service::{StorageErrorKind, StorageRead, VectorOutcome},
};
use rusqlite::{Connection, types::Value};
use tempfile::TempDir;

#[test]
fn reset_commits_only_for_the_observed_identity_and_keeps_a_pinned_read() {
    // Given: separate connections capture A before B is installed.
    let root = TempDir::new().unwrap();
    let first = open(&root);
    let second = open(&root);
    let a = Sha256::digest(b"A");
    let b = Sha256::digest(b"B");
    // When / Then: each conditional reset is attempted; only the observed identity changes and old reads remain pinned.
    assert_eq!(
        first.vectors.reset_vectors(None, &a).unwrap(),
        VectorOutcome::Current(true)
    );
    seed_state(&first, &a);
    first
        .cache
        .with_read_tx(|| {
            let pinned = &first.cache;
            // When / Then: each conditional reset is attempted; only the observed identity changes and old reads remain pinned.
            assert_eq!(
                second.vectors.reset_vectors(Some(&a), &b).unwrap(),
                VectorOutcome::Current(true)
            );
            // Then: old view still sees the exact old vector and failure.
            assert_eq!(pinned.meta_get_by_id().unwrap(), Some(a));
            assert_state(pinned, &a);
            second
                .cache
                .with_read_tx(|| {
                    let fresh = &second.cache;
                    assert_eq!(fresh.meta_get_by_id().unwrap(), Some(b));
                    assert_eq!(
                        fresh
                            .vector_keys_get_vector_by_search_hash(&b, &Sha256::digest(b"vector"))
                            .unwrap(),
                        VectorOutcome::Current(None)
                    );
                    assert_eq!(
                        fresh
                            .embed_failures_get_by_search_hash(&b, &Sha256::digest(b"attempt"))
                            .unwrap(),
                        VectorOutcome::Current(None)
                    );
                    Ok::<_, StorageError>(())
                })
                .unwrap();
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn same_desired_identity_preserves_vectors_and_failure_bytes() {
    // Given: stored vectors and failure bytes under the desired identity.
    let root = TempDir::new().unwrap();
    let first = open(&root);
    let second = open(&root);
    let a = Sha256::digest(b"A");
    let b = Sha256::digest(b"B");
    first.vectors.reset_vectors(None, &a).unwrap();
    second.vectors.reset_vectors(Some(&a), &b).unwrap();
    seed_state(&second, &b);
    let before = get_failure_bytes(&root);
    for observed in [Some(&b), Some(&a), None] {
        // When: a late observer may have captured B, A or initial absence.
        let outcome = first.vectors.reset_vectors(observed, &b).unwrap();
        // Then: vectors and failure bytes remain unchanged.
        first
            .cache
            .with_read_tx(|| {
                assert_state(&first.cache, &b);
                Ok::<_, StorageError>(())
            })
            .unwrap();
        assert_eq!(get_failure_bytes(&root), before);
        assert_eq!(outcome, VectorOutcome::Current(false));
    }
}

#[test]
fn obsolete_and_absent_observations_preserve_the_winning_identity_and_work() {
    // Given: obsolete or absent observations and a winning identity.
    for observed in [Some(Sha256::digest(b"A")), None] {
        let root = TempDir::new().unwrap();
        let first = open(&root);
        let second = open(&root);
        let a = Sha256::digest(b"A");
        let b = Sha256::digest(b"B");
        if let Some(identity) = observed.as_ref() {
            first.vectors.reset_vectors(None, identity).unwrap();
        }
        second.vectors.reset_vectors(observed.as_ref(), &b).unwrap();
        seed_state(&second, &b);
        let before = get_failure_bytes(&root);
        // When: a stale reset is attempted.
        let outcome = first.vectors.reset_vectors(observed.as_ref(), &a).unwrap();
        // Then: state before the result, so a compiling mutation fails on lost work.
        first
            .cache
            .with_read_tx(|| {
                assert_state(&first.cache, &b);
                Ok::<_, StorageError>(())
            })
            .unwrap();
        assert_eq!(get_failure_bytes(&root), before);
        assert_eq!(outcome, VectorOutcome::ConfigChanged);
    }
}

#[test]
fn malformed_stored_identity_is_a_typed_error_without_resetting_work() {
    // Given
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let b = Sha256::digest(b"B");
    storage.vectors.reset_vectors(None, &b).unwrap();
    seed_state(&storage, &b);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute("update meta set config_identity='malformed'", [])
        .unwrap();
    let before = get_failure_bytes(&root);
    // When
    let error = storage
        .vectors
        .reset_vectors(None, &Sha256::digest(b"A"))
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(get_failure_bytes(&root), before);
    assert_eq!(
        db.query_row("select count(*) from vector_keys", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("select config_identity from meta", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "malformed"
    );
}

fn open(root: &TempDir) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
    open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn seed_state<C: CacheDatabase, V: VectorDatabase>(storage: &Databases<C, V>, identity: &Sha256) {
    storage
        .vectors
        .with_write_tx(identity, || {
            let unit = &storage.vectors;
            unit.store_vector(&Sha256::digest(b"vector"), &[4.0, 5.0, 6.0], 3)
                .unwrap();
            assert!(
                unit.embed_failures_upsert_by_search_hash_and_attempts(
                    &Sha256::digest(b"attempt"),
                    4
                )
                .unwrap()
            );
            unit.embed_failures_update_by_search_hash(
                &Sha256::digest(b"attempt"),
                "current failure sentinel",
            )
            .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}

fn assert_state(read: &dyn StorageRead, identity: &Sha256) {
    assert_eq!(
        read.vector_keys_get_vector_by_search_hash(identity, &Sha256::digest(b"vector"))
            .unwrap(),
        VectorOutcome::Current(Some(vec![4.0, 5.0, 6.0]))
    );
    let VectorOutcome::Current(Some(failure)) = read
        .embed_failures_get_by_search_hash(identity, &Sha256::digest(b"attempt"))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failure.attempts, 1);
    assert_eq!(failure.error.as_deref(), Some("current failure sentinel"));
    assert_eq!(read.meta_get_by_id().unwrap(), Some(*identity));
}

fn get_failure_bytes(root: &TempDir) -> Vec<Vec<Value>> {
    let db = Connection::open(root.path().join("vectors")).unwrap();
    let mut statement = db
        .prepare("select search_hash, attempts, error from embed_failures order by search_hash")
        .unwrap();
    statement
        .query_map([], |row| (0..3).map(|column| row.get(column)).collect())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
