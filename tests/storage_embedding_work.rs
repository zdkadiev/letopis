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
    // Given: reverse insertion order and shared text with two representatives.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(
        &mut storage,
        &[("z.md", "stored"), ("b.md", "shared"), ("a.md", "shared")],
    );
    let mut unit = storage.begin_cache_write_tx().unwrap();
    unit.upsert_node(&support::build_source("file://source.txt", None, 7))
        .unwrap();
    unit.commit().unwrap();
    store(&mut storage, &identity, "stored", 3);
    // When: embedding work is retrieved in cursor order.
    let first = get_next_work(&mut storage, &identity, 4, None).unwrap();
    let second = get_next_work(&mut storage, &identity, 4, Some(&first.id)).unwrap();
    // Then: skipping a stale representative can advance to its valid sibling.
    assert_eq!(first.id.as_str(), "a.md");
    assert_eq!(second.id.as_str(), "b.md");
    assert_eq!(first.search_hash, second.search_hash);
    assert_eq!(
        get_next_work(&mut storage, &identity, 4, Some(&second.id)),
        None
    );
    store(&mut storage, &identity, "shared", 3);
    assert_eq!(get_next_work(&mut storage, &identity, 4, None), None);
}

#[test]
fn absent_and_null_error_rows_remain_eligible_until_four_attempts() {
    // Given: max_retries=3 means a total budget of four reservations.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"shared");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("b.md", "shared")]);
    for attempts in 1..=4 {
        // When / Then: work remains eligible before the next reservation.
        assert!(get_next_work(&mut storage, &identity, 4, None).is_some());
        // When: reserve without a failure message, including the crash window.
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        // Then: the reservation succeeds within the four-attempt budget.
        assert!(unit.reserve_attempt(&hash, 4).unwrap());
        unit.commit().unwrap();
        // Then: the committed attempt count advances with no failure message.
        let failure = unwrap_current(
            storage
                .begin_read_tx()
                .unwrap()
                .get_failure_by_search_hash(&identity, &hash)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(failure.attempts, attempts);
        assert_eq!(failure.error, None);
    }
    // When / Then: the exhausted hash yields no further work or reservation.
    assert_eq!(get_next_work(&mut storage, &identity, 4, None), None);
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    assert!(!unit.reserve_attempt(&hash, 4).unwrap());
    unit.commit().unwrap();
}

#[test]
fn work_and_dimensions_use_the_pinned_identity_and_membership() {
    // Given: a pinned work view before identity or membership changes.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let m1 = Sha256::digest(b"one");
    let m2 = Sha256::digest(b"two");
    first.reset_vectors(None, &m1).unwrap();
    publish(&mut first, &[("a.md", "pending")]);
    store(&mut first, &m1, "orphan", 3);
    let mut pinned = first.begin_read_tx().unwrap();
    // When: change both databases after the read view was captured.
    second.reset_vectors(Some(&m1), &m2).unwrap();
    store(&mut second, &m2, "pending", 4);
    publish(&mut second, &[("a.md", "changed")]);
    // Then: the pinned identity and membership determine the results.
    assert_eq!(
        unwrap_current(pinned.get_vector_dimensions(&m1).unwrap())
            .unwrap()
            .get(),
        3
    );
    assert_eq!(
        unwrap_current(
            pinned
                .get_next_embedding_work_by_id_and_max_attempts(&m1, 4, None)
                .unwrap()
        )
        .unwrap()
        .search_hash,
        Sha256::digest(b"pending")
    );
    assert!(matches!(
        pinned.get_vector_dimensions(&m2).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    let mut fresh = second.begin_read_tx().unwrap();
    assert!(matches!(
        fresh
            .get_next_embedding_work_by_id_and_max_attempts(&m1, 4, None)
            .unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert!(matches!(
        fresh.get_vector_dimensions(&m1).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    assert_eq!(
        unwrap_current(fresh.get_vector_dimensions(&m2).unwrap())
            .unwrap()
            .get(),
        4
    );
}

#[test]
fn last_vector_cleanup_preserves_dimension_across_reopen() {
    // Given: one final vector with native dimensions.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    // When: orphan cleanup runs and storage reopens.
    prune(&mut storage, &identity);
    drop(storage);
    let mut storage = open(&root);
    // Then: native dimensions survive after the last vector is removed.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&mut storage, &identity), Some(3));
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    let error = unit
        .store_vector(&Sha256::digest(b"new"), &[1.0; 4], 4)
        .unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(unit.commit().is_err());
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&mut storage, &identity), Some(3));
}

#[test]
fn first_committed_dimension_wins_between_competing_connections() {
    // Given: both connections observed an uninitialized dimension.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    // When / Then: neither connection initially observes a native dimension.
    assert_eq!(get_size(&mut first, &identity), None);
    assert_eq!(get_size(&mut second, &identity), None);
    // When: hold the first write while the other tries to enter.
    let mut winner = unwrap_current(first.begin_vector_write_tx(&identity).unwrap());
    winner.store_vector(&identity, &[1.0; 3], 3).unwrap();
    // Then: the second writer is blocked until the winning write commits.
    assert!(second.begin_vector_write_tx(&identity).is_err());
    winner.commit().unwrap();
    let before = get_native_state(&root);
    let mut loser = unwrap_current(second.begin_vector_write_tx(&identity).unwrap());
    let error = loser
        .store_vector(&Sha256::digest(b"loser"), &[1.0; 4], 4)
        .unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(loser.commit().is_err());
    // Then: rejecting the losing dimension preserves the winning native state.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(count_keys(&root), 1);
    assert_eq!(get_size(&mut second, &identity), Some(3));
    assert_eq!(
        unwrap_current(
            first
                .begin_read_tx()
                .unwrap()
                .get_vector_by_search_hash(&identity, &identity)
                .unwrap()
        ),
        Some(vec![1.0; 3])
    );
}

#[test]
fn rollback_of_first_insertion_does_not_pin_dimensions() {
    // Given: an uncommitted first vector insertion.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    // When: the first vector transaction is abandoned.
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    unit.store_vector(&identity, &[1.0; 3], 3).unwrap();
    drop(unit);
    // Then:  including reuse of the same native connection after rollback.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&mut storage, &identity), None);
    store(&mut storage, &identity, "different", 4);
    assert_eq!(get_size(&mut storage, &identity), Some(4));
}

#[test]
fn cleanup_retains_referenced_hashes_and_diagnostics_without_cache_publication() {
    // Given: referenced and orphan vectors with stored diagnostics.
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
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    for text in ["failed", "unused"] {
        let hash = Sha256::digest(text.as_bytes());
        unit.reserve_attempt(&hash, 4).unwrap();
        unit.record_failure(&hash, "safe failure").unwrap();
    }
    unit.set_query_error(17, "query failure").unwrap();
    unit.commit().unwrap();
    // When: orphan cleanup runs.
    prune(&mut storage, &identity);
    // Then: referenced vectors and diagnostics survive without cache publication.
    assert_eq!(store(&mut storage, &identity, "shared", 3), key);
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), revision_before);
    let orphan = Sha256::digest(b"orphan");
    let failed = Sha256::digest(b"failed");
    let unused = Sha256::digest(b"unused");
    assert_eq!(
        unwrap_current(read.get_vector_by_search_hash(&identity, &orphan).unwrap()),
        None
    );
    assert!(unwrap_current(read.get_failure_by_search_hash(&identity, &failed).unwrap()).is_some());
    assert_eq!(
        unwrap_current(read.get_failure_by_search_hash(&identity, &unused).unwrap()),
        None
    );
    assert_eq!(
        read.get_query_error().unwrap().unwrap().error,
        "query failure"
    );
}

#[test]
fn cleanup_uses_a_pinned_cache_observation_and_new_publication_is_recomputable() {
    // Given: a cleanup transaction with a pinned cache observation.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    publish(&mut first, &[("a.md", "keep")]);
    store(&mut first, &identity, "keep", 3);
    let mut cleanup = unwrap_current(first.begin_vector_write_tx(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap(); // Pins cache membership.
    // When: publish a new hash, then put its vector into the older cleanup view.
    publish(&mut second, &[("b.md", "late")]);
    cleanup
        .store_vector(&Sha256::digest(b"late"), &[1.0; 3], 3)
        .unwrap();
    cleanup.prune_unreferenced().unwrap();
    cleanup.commit().unwrap();
    // Then: only derived work is missing; the knowledge survives for recompute.
    let work = get_next_work(&mut second, &identity, 4, None).unwrap();
    assert_eq!(work.id.as_str(), "b.md");
    assert_eq!(work.search_hash, Sha256::digest(b"late"));
    assert_eq!(count_keys(&root), 1);
}

#[test]
fn late_old_identity_cannot_cleanup_current_vectors_or_query_error() {
    // Given: an obsolete identity and current vectors with query diagnostics.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let old = Sha256::digest(b"old");
    let current = Sha256::digest(b"current");
    storage.reset_vectors(None, &old).unwrap();
    storage.reset_vectors(Some(&old), &current).unwrap();
    publish(&mut storage, &[("a.md", "current")]);
    store(&mut storage, &current, "current", 3);
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&current).unwrap());
    unit.set_query_error(9, "current failure").unwrap();
    unit.commit().unwrap();
    // When / Then: the old identity attempts cleanup; current vectors and query diagnostics remain unchanged.
    assert!(matches!(
        storage.begin_vector_write_tx(&old).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    // Then: current vectors and query diagnostics remain unchanged.
    let mut read = storage.begin_read_tx().unwrap();
    assert!(unwrap_current(read.get_vector_by_search_hash(&current, &current).unwrap()).is_some());
    assert_eq!(
        read.get_query_error().unwrap().unwrap().error,
        "current failure"
    );
}

#[test]
fn deleting_one_shared_representative_retains_the_shared_vector() {
    // Given: two current knowledge nodes sharing one vector.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("b.md", "shared")]);
    let key = store(&mut storage, &identity, "shared", 3);
    let mut unit = storage.begin_cache_write_tx().unwrap();
    unit.delete_node(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
        .unwrap();
    unit.commit().unwrap();
    // When: one representative is deleted and cleanup runs.
    prune(&mut storage, &identity);
    // Then: the remaining representative retains the shared vector.
    assert_eq!(count_keys(&root), 1);
    assert_eq!(store(&mut storage, &identity, "shared", 3), key);
}

#[test]
fn reset_rollback_restores_native_dimensions_and_commit_releases_them() {
    // Given: a late failure after reset has recreated the native table.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let old = Sha256::digest(b"old");
    let new = Sha256::digest(b"new");
    storage.reset_vectors(None, &old).unwrap();
    store(&mut storage, &old, "retained", 3);
    let before = get_native_state(&root);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute_batch(
        "create trigger reject_reset before update on meta
        begin select raise(abort, 'test reset failure'); end;",
    )
    .unwrap();
    // When / Then
    assert!(storage.reset_vectors(Some(&old), &new).is_err());
    db.execute_batch("drop trigger reject_reset;").unwrap();
    // Then: all key/data/metadata bytes and schema are restored.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(get_size(&mut storage, &old), Some(3));
    assert_eq!(
        storage.reset_vectors(None, &old).unwrap(),
        VectorOutcome::Current(false)
    );
    assert_eq!(get_size(&mut storage, &old), Some(3));
    // When: commit the reset and establish a different first dimension.
    // Then
    assert_eq!(
        storage.reset_vectors(Some(&old), &new).unwrap(),
        VectorOutcome::Current(true)
    );
    assert_eq!(get_size(&mut storage, &new), None);
    store(&mut storage, &new, "new", 4);
    assert_eq!(get_size(&mut storage, &new), Some(4));
}

#[test]
fn unsupported_native_dimensions_poison_the_unit_without_keys() {
    for size in [1, 4097] {
        // Given: unsupported native dimension counts.
        let root = TempDir::new().unwrap();
        let mut storage = open(&root);
        let identity = Sha256::digest(b"model");
        storage.reset_vectors(None, &identity).unwrap();
        let before = get_native_state(&root);
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        // When: a vector is stored with each unsupported size.
        let error = unit
            .store_vector(&identity, &vec![1.0; size], size)
            .unwrap_err();
        // Then: the unit is poisoned and no key remains.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(unit.commit().is_err());
        assert_eq!(get_native_state(&root), before);
        assert_eq!(get_size(&mut storage, &identity), None);
    }
}

#[test]
fn nullable_failure_hash_does_not_hide_unrelated_work() {
    // Given: SQLite rowid-table TEXT PRIMARY KEY permits a legacy NULL key.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "pending")]);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    // This malformed legacy row cannot be created by the typed writer.
    db.execute(
        "insert into embed_failures(search_hash, attempts) values(null, 4)",
        [],
    )
    .unwrap();
    // When
    let work = get_next_work(&mut storage, &identity, 4, None).unwrap();
    // Then: a NOT IN anti-join would wrongly suppress every candidate.
    assert_eq!(work.id.as_str(), "a.md");
}

#[test]
fn invalid_attempt_budgets_are_rejected_without_changing_storage() {
    // Given: invalid durable attempt budgets.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "pending")]);
    let mut read = storage.begin_read_tx().unwrap();
    for budget in [0, u64::MAX] {
        // When: the next embedding work is requested.
        let error = read
            .get_next_embedding_work_by_id_and_max_attempts(&identity, budget, None)
            .unwrap_err();
        // Then: storage rejects the budget without mutation.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }
    assert_eq!(read.get_revision(), 1);
}

#[test]
fn abandoned_cleanup_restores_vectors_failures_and_dimensions() {
    // Given: vectors, failures and dimensions before orphan cleanup.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    unit.reserve_attempt(&identity, 4).unwrap();
    unit.commit().unwrap();
    let before = get_native_state(&root);
    // When: the cleanup transaction is abandoned.
    let mut cleanup = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap();
    drop(cleanup);
    // Then: all prior vector state is restored.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(get_size(&mut storage, &identity), Some(3));
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(
        unwrap_current(
            read.get_failure_by_search_hash(&identity, &identity)
                .unwrap()
        )
        .unwrap()
        .attempts,
        1
    );
}

fn open(root: &TempDir) -> impl Storage + use<> {
    open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn publish(storage: &mut impl Storage, rows: &[(&str, &str)]) -> u64 {
    let mut unit = storage.begin_cache_write_tx().unwrap();
    for (id, text) in rows {
        unit.upsert_node(&support::build_knowledge(id, text))
            .unwrap();
    }
    unit.commit().unwrap()
}

fn store(storage: &mut impl Storage, identity: &Sha256, text: &str, size: usize) -> i64 {
    let mut unit = unwrap_current(storage.begin_vector_write_tx(identity).unwrap());
    let key = unit
        .store_vector(&Sha256::digest(text.as_bytes()), &vec![1.0; size], size)
        .unwrap();
    unit.commit().unwrap();
    key.row_id
}

fn get_next_work(
    storage: &mut impl Storage,
    identity: &Sha256,
    budget: u64,
    after: Option<&KnowledgeId>,
) -> Option<EmbeddingWork> {
    unwrap_current(
        storage
            .begin_read_tx()
            .unwrap()
            .get_next_embedding_work_by_id_and_max_attempts(identity, budget, after)
            .unwrap(),
    )
}

fn get_size(storage: &mut impl Storage, identity: &Sha256) -> Option<usize> {
    unwrap_current(
        storage
            .begin_read_tx()
            .unwrap()
            .get_vector_dimensions(identity)
            .unwrap(),
    )
    .map(Into::into)
}

fn prune(storage: &mut impl Storage, identity: &Sha256) {
    let mut unit = unwrap_current(storage.begin_vector_write_tx(identity).unwrap());
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
        .query_row("select count(*) from vector_keys", [], |row| row.get(0))
        .unwrap()
}

fn get_native_state(root: &TempDir) -> Vec<Vec<Vec<Value>>> {
    let db = Connection::open(root.path().join("vectors")).unwrap();
    [
        "select row_id, search_hash from vector_keys order by row_id",
        "select id, vector from vector_index_base order by id",
        "select id, val from vector_index_config order by id",
        "select id, val from vector_index_model order by id",
        "select type, name, sql from sqlite_schema order by name",
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
