use letopis::{
    domain::Sha256,
    open_storage,
    service::{Storage, StorageErrorKind, StorageRead, VectorOutcome, VectorWrite},
};
use rusqlite::{Connection, types::Value};
use tempfile::TempDir;

#[test]
fn reset_commits_only_for_the_observed_identity_and_keeps_a_pinned_read() {
    // Given: separate connections capture A before B is installed.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let a = Sha256::digest(b"A");
    let b = Sha256::digest(b"B");
    // When / Then
    assert_eq!(
        first.reset_vectors(None, &a).unwrap(),
        VectorOutcome::Current(true)
    );
    seed_state(&mut first, &a);
    let mut pinned = first.begin_read().unwrap();
    // When / Then
    assert_eq!(
        second.reset_vectors(Some(&a), &b).unwrap(),
        VectorOutcome::Current(true)
    );
    // Then: old view still sees the exact old vector and failure.
    assert_eq!(pinned.get_config_identity(), Some(a));
    assert_state(&mut pinned, &a);
    let mut fresh = second.begin_read().unwrap();
    assert_eq!(fresh.get_config_identity(), Some(b));
    assert_eq!(
        fresh.read_vector(&b, &Sha256::digest(b"vector")).unwrap(),
        VectorOutcome::Current(None)
    );
    assert_eq!(
        fresh.read_failure(&b, &Sha256::digest(b"attempt")).unwrap(),
        VectorOutcome::Current(None)
    );
}

#[test]
fn same_desired_identity_preserves_vectors_and_failure_bytes() {
    // Given
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let a = Sha256::digest(b"A");
    let b = Sha256::digest(b"B");
    first.reset_vectors(None, &a).unwrap();
    second.reset_vectors(Some(&a), &b).unwrap();
    seed_state(&mut second, &b);
    let before = read_failure_bytes(&root);
    for observed in [Some(&b), Some(&a), None] {
        // When: a late observer may have captured B, A or initial absence.
        let outcome = first.reset_vectors(observed, &b).unwrap();
        // Then
        assert_state(&mut first.begin_read().unwrap(), &b);
        assert_eq!(read_failure_bytes(&root), before);
        assert_eq!(outcome, VectorOutcome::Current(false));
    }
}

#[test]
fn obsolete_and_absent_observations_preserve_the_winning_identity_and_work() {
    // Given
    for observed in [Some(Sha256::digest(b"A")), None] {
        let root = TempDir::new().unwrap();
        let mut first = open(&root);
        let mut second = open(&root);
        let a = Sha256::digest(b"A");
        let b = Sha256::digest(b"B");
        if let Some(identity) = observed.as_ref() {
            first.reset_vectors(None, identity).unwrap();
        }
        second.reset_vectors(observed.as_ref(), &b).unwrap();
        seed_state(&mut second, &b);
        let before = read_failure_bytes(&root);
        // When
        let outcome = first.reset_vectors(observed.as_ref(), &a).unwrap();
        // Then state before the result, so a compiling mutation fails on lost work.
        assert_state(&mut first.begin_read().unwrap(), &b);
        assert_eq!(read_failure_bytes(&root), before);
        assert_eq!(outcome, VectorOutcome::ConfigChanged);
    }
}

#[test]
fn malformed_stored_identity_is_a_typed_error_without_resetting_work() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let b = Sha256::digest(b"B");
    storage.reset_vectors(None, &b).unwrap();
    seed_state(&mut storage, &b);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute("UPDATE meta SET config_identity='malformed'", [])
        .unwrap();
    let before = read_failure_bytes(&root);
    // When
    let error = storage
        .reset_vectors(None, &Sha256::digest(b"A"))
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(read_failure_bytes(&root), before);
    assert_eq!(
        db.query_row("SELECT count(*) FROM vector_keys", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT config_identity FROM meta", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "malformed"
    );
}

fn open(root: &TempDir) -> impl Storage + use<> {
    open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn seed_state(storage: &mut impl Storage, identity: &Sha256) {
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write(identity).unwrap() else {
        panic!()
    };
    unit.store_vector(&Sha256::digest(b"vector"), &[4.0, 5.0, 6.0], 3)
        .unwrap();
    assert!(
        unit.reserve_attempt(&Sha256::digest(b"attempt"), 4)
            .unwrap()
    );
    unit.record_failure(&Sha256::digest(b"attempt"), "current failure sentinel")
        .unwrap();
    unit.commit().unwrap();
}

fn assert_state(read: &mut impl StorageRead, identity: &Sha256) {
    assert_eq!(
        read.read_vector(identity, &Sha256::digest(b"vector"))
            .unwrap(),
        VectorOutcome::Current(Some(vec![4.0, 5.0, 6.0]))
    );
    let VectorOutcome::Current(Some(failure)) = read
        .read_failure(identity, &Sha256::digest(b"attempt"))
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(failure.attempts, 1);
    assert_eq!(failure.error.as_deref(), Some("current failure sentinel"));
    assert_eq!(read.get_config_identity(), Some(*identity));
}

fn read_failure_bytes(root: &TempDir) -> Vec<Vec<Value>> {
    let db = Connection::open(root.path().join("vectors")).unwrap();
    let mut statement = db
        .prepare("SELECT search_hash, attempts, error FROM embed_failures ORDER BY search_hash")
        .unwrap();
    statement
        .query_map([], |row| (0..3).map(|column| row.get(column)).collect())
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap()
}
