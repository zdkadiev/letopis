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
fn excludes_every_representative_of_an_outstanding_hash() {
    // Given: two nodes share one in-flight hash and a third has distinct text.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    let shared = Sha256::digest(b"shared");
    let distinct = Sha256::digest(b"distinct");
    storage.reset_vectors(None, &identity).unwrap();
    publish(
        &mut storage,
        &[("a.md", "shared"), ("b.md", "shared"), ("c.md", "distinct")],
    );
    // When: select beyond all representatives of the active hash.
    let mut read = storage.begin_read().unwrap();
    let selected = unwrap_current(
        read.read_next_embedding_work(&identity, 1, None, &[shared])
            .unwrap(),
    );
    let excluded = read
        .read_next_embedding_work(&identity, 1, None, &[shared, distinct])
        .unwrap();
    // Then: distinct work progresses and excluding both leaves no eligible candidate.
    assert_eq!(selected.unwrap().id.as_str(), "c.md");
    assert_eq!(excluded, VectorOutcome::Current(None));
}

#[test]
fn empty_exclusions_preserve_cursor_and_attempt_policy() {
    // Given: the first hash is exhausted and another candidate follows the cursor.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(
        &mut storage,
        &[("a.md", "exhausted"), ("b.md", "ready"), ("c.md", "later")],
    );
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.reserve_attempt(&Sha256::digest(b"exhausted"), 1)
        .unwrap();
    unit.commit().unwrap();
    // When: the general caller has no outstanding hashes.
    let mut read = storage.begin_read().unwrap();
    let first = unwrap_current(
        read.read_next_embedding_work(&identity, 1, None, &[])
            .unwrap(),
    )
    .unwrap();
    let next = unwrap_current(
        read.read_next_embedding_work(&identity, 1, Some(&first.id), &[])
            .unwrap(),
    )
    .unwrap();
    // Then: exhausted work stays excluded and the original stable cursor still works.
    assert_eq!(first.id.as_str(), "b.md");
    assert_eq!(next.id.as_str(), "c.md");
}

#[test]
fn exclusions_preserve_pinned_config_and_read_snapshot() {
    // Given: a read pins old configuration and cache before a separate writer advances them.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let old = Sha256::digest(b"old model");
    let new = Sha256::digest(b"new model");
    let shared = Sha256::digest(b"shared");
    storage.reset_vectors(None, &old).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("c.md", "distinct")]);
    let mut writer = open(&root);
    let mut pinned = storage.begin_read().unwrap();
    let first = unwrap_current(
        pinned
            .read_next_embedding_work(&old, 1, None, &[shared])
            .unwrap(),
    )
    .unwrap();
    writer.reset_vectors(Some(&old), &new).unwrap();
    publish(&mut writer, &[("b.md", "new distinct")]);
    // When: the old view selects again with the same exclusion.
    let observed = unwrap_current(
        pinned
            .read_next_embedding_work(&old, 1, None, &[shared])
            .unwrap(),
    )
    .unwrap();
    drop(pinned);
    let current = storage
        .begin_read()
        .unwrap()
        .read_next_embedding_work(&old, 1, None, &[shared])
        .unwrap();
    // Then: the pinned row stays literal c.md; a fresh mismatched identity is rejected.
    assert_eq!(first.id.as_str(), "c.md");
    assert_eq!(observed.id.as_str(), "c.md");
    assert_eq!(current, VectorOutcome::ConfigChanged);
}

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
    let mut unit = storage.begin_cache_write().unwrap();
    unit.upsert_node(&support::build_source("file://source.txt", None, 7))
        .unwrap();
    unit.commit().unwrap();
    store(&mut storage, &identity, "stored", 3);
    // When
    let first = read_next_work(&mut storage, &identity, 4, None).unwrap();
    let second = read_next_work(&mut storage, &identity, 4, Some(&first.id)).unwrap();
    // Then: skipping a stale representative can advance to its valid sibling.
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
    // Given: max_retries=3 means a total budget of four reservations.
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"shared");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "shared"), ("b.md", "shared")]);
    for attempts in 1..=4 {
        // When / Then
        assert!(read_next_work(&mut storage, &identity, 4, None).is_some());
        // When: reserve without a failure message, including the crash window.
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        // Then
        assert!(unit.reserve_attempt(&hash, 4).unwrap());
        unit.commit().unwrap();
        // Then
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
    // When / Then
    assert_eq!(read_next_work(&mut storage, &identity, 4, None), None);
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    assert!(!unit.reserve_attempt(&hash, 4).unwrap());
    unit.commit().unwrap();
}

#[test]
fn work_and_dimensions_use_the_pinned_identity_and_membership() {
    // Given
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let m1 = Sha256::digest(b"one");
    let m2 = Sha256::digest(b"two");
    first.reset_vectors(None, &m1).unwrap();
    publish(&mut first, &[("a.md", "pending")]);
    store(&mut first, &m1, "orphan", 3);
    let mut pinned = first.begin_read().unwrap();
    // When: change both databases after the read view was captured.
    second.reset_vectors(Some(&m1), &m2).unwrap();
    store(&mut second, &m2, "pending", 4);
    publish(&mut second, &[("a.md", "changed")]);
    // Then
    assert_eq!(
        unwrap_current(pinned.read_vector_dimensions(&m1).unwrap())
            .unwrap()
            .get(),
        3
    );
    assert_eq!(
        unwrap_current(pinned.read_next_embedding_work(&m1, 4, None, &[]).unwrap())
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
        fresh.read_next_embedding_work(&m1, 4, None, &[]).unwrap(),
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
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    // When
    prune(&mut storage, &identity);
    drop(storage);
    let mut storage = open(&root);
    // Then
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
    // Given: both connections observed an uninitialized dimension.
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    // When / Then
    assert_eq!(read_size(&mut first, &identity), None);
    assert_eq!(read_size(&mut second, &identity), None);
    // When: hold the first write while the other tries to enter.
    let mut winner = unwrap_current(first.begin_vector_write(&identity).unwrap());
    winner.store_vector(&identity, &[1.0; 3], 3).unwrap();
    // Then
    assert!(second.begin_vector_write(&identity).is_err());
    winner.commit().unwrap();
    let before = read_native_state(&root);
    let mut loser = unwrap_current(second.begin_vector_write(&identity).unwrap());
    let error = loser
        .store_vector(&Sha256::digest(b"loser"), &[1.0; 4], 4)
        .unwrap_err();
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert!(loser.commit().is_err());
    // Then
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
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    // When
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.store_vector(&identity, &[1.0; 3], 3).unwrap();
    drop(unit);
    // Then, including reuse of the same native connection after rollback.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(read_size(&mut storage, &identity), None);
    store(&mut storage, &identity, "different", 4);
    assert_eq!(read_size(&mut storage, &identity), Some(4));
}

#[test]
fn cleanup_retains_referenced_hashes_and_diagnostics_without_cache_publication() {
    // Given
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
    // When
    prune(&mut storage, &identity);
    // Then
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
    // Given
    let root = TempDir::new().unwrap();
    let mut first = open(&root);
    let mut second = open(&root);
    let identity = Sha256::digest(b"model");
    first.reset_vectors(None, &identity).unwrap();
    publish(&mut first, &[("a.md", "keep")]);
    store(&mut first, &identity, "keep", 3);
    let mut cleanup = unwrap_current(first.begin_vector_write(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap(); // Pins cache membership.
    // When: publish a new hash, then put its vector into the older cleanup view.
    publish(&mut second, &[("b.md", "late")]);
    cleanup
        .store_vector(&Sha256::digest(b"late"), &[1.0; 3], 3)
        .unwrap();
    cleanup.prune_unreferenced().unwrap();
    cleanup.commit().unwrap();
    // Then: only derived work is missing; the knowledge survives for recompute.
    let work = read_next_work(&mut second, &identity, 4, None).unwrap();
    assert_eq!(work.id.as_str(), "b.md");
    assert_eq!(work.search_hash, Sha256::digest(b"late"));
    assert_eq!(count_keys(&root), 1);
}

#[test]
fn late_old_identity_cannot_cleanup_current_vectors_or_query_error() {
    // Given
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
    // When / Then
    assert!(matches!(
        storage.begin_vector_write(&old).unwrap(),
        VectorOutcome::ConfigChanged
    ));
    // Then
    let mut read = storage.begin_read().unwrap();
    assert!(unwrap_current(read.read_vector(&current, &current).unwrap()).is_some());
    assert_eq!(
        read.read_query_error().unwrap().unwrap().error,
        "current failure"
    );
}

#[test]
fn deleting_one_shared_representative_retains_the_shared_vector() {
    // Given
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
    // When
    prune(&mut storage, &identity);
    // Then
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
    let before = read_native_state(&root);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute_batch(
        "CREATE TRIGGER reject_reset BEFORE UPDATE ON meta
        BEGIN SELECT RAISE(ABORT, 'test reset failure'); END;",
    )
    .unwrap();
    // When / Then
    assert!(storage.reset_vectors(Some(&old), &new).is_err());
    db.execute_batch("DROP TRIGGER reject_reset;").unwrap();
    // Then: all key/data/metadata bytes and schema are restored.
    assert_eq!(read_native_state(&root), before);
    assert_eq!(read_size(&mut storage, &old), Some(3));
    assert_eq!(
        storage.reset_vectors(None, &old).unwrap(),
        VectorOutcome::Current(false)
    );
    assert_eq!(read_size(&mut storage, &old), Some(3));
    // When: commit the reset and establish a different first dimension.
    // Then
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
        // Given
        let root = TempDir::new().unwrap();
        let mut storage = open(&root);
        let identity = Sha256::digest(b"model");
        storage.reset_vectors(None, &identity).unwrap();
        let before = read_native_state(&root);
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        // When
        let error = unit
            .store_vector(&identity, &vec![1.0; size], size)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(unit.commit().is_err());
        assert_eq!(read_native_state(&root), before);
        assert_eq!(read_size(&mut storage, &identity), None);
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
        "INSERT INTO embed_failures(search_hash, attempts) VALUES(NULL, 4)",
        [],
    )
    .unwrap();
    // When
    let work = read_next_work(&mut storage, &identity, 4, None).unwrap();
    // Then: a NOT IN anti-join would wrongly suppress every candidate.
    assert_eq!(work.id.as_str(), "a.md");
}

#[test]
fn invalid_attempt_budgets_are_rejected_without_changing_storage() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    publish(&mut storage, &[("a.md", "pending")]);
    let mut read = storage.begin_read().unwrap();
    for budget in [0, u64::MAX] {
        // When
        let error = read
            .read_next_embedding_work(&identity, budget, None, &[])
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    }
    assert_eq!(read.get_revision(), 1);
}

#[test]
fn abandoned_cleanup_restores_vectors_failures_and_dimensions() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    store(&mut storage, &identity, "orphan", 3);
    let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    unit.reserve_attempt(&identity, 4).unwrap();
    unit.commit().unwrap();
    let before = read_native_state(&root);
    // When
    let mut cleanup = unwrap_current(storage.begin_vector_write(&identity).unwrap());
    cleanup.prune_unreferenced().unwrap();
    drop(cleanup);
    // Then
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
            .read_next_embedding_work(identity, budget, after, &[])
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
