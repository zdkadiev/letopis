use crate::support::get_current;
use letopis::service::StorageError;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
use support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::{EdgeTarget, KnowledgeId, Sha256},
    service::{EmbeddingWork, StorageErrorKind, VectorOutcome},
};
use rusqlite::{Connection, types::Value};
use tempfile::TempDir;

#[test]
fn cursor_orders_shared_hashes_and_excludes_sources_and_stored_hashes() {
    // Given: reverse insertion order and shared text with two representatives.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    publish(
        &storage,
        &[("z.md", "stored"), ("b.md", "shared"), ("a.md", "shared")],
    );
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            unit.nodes_upsert_by_id(&support::build_source("file://source.txt", None, 7))
                .unwrap();

            unit.get_publication_revision()
        })
        .unwrap();
    store(&storage, &identity, "stored", 3);
    // When: embedding work is retrieved in cursor order.
    let first = get_next_work(&storage, &identity, 4, None).unwrap();
    let second = get_next_work(&storage, &identity, 4, Some(&first.id)).unwrap();
    // Then: skipping a stale representative can advance to its valid sibling.
    assert_eq!(first.id.as_str(), "a.md");
    assert_eq!(second.id.as_str(), "b.md");
    assert_eq!(first.search_hash, second.search_hash);
    assert_eq!(
        get_next_work(&storage, &identity, 4, Some(&second.id)),
        None
    );
    store(&storage, &identity, "shared", 3);
    assert_eq!(get_next_work(&storage, &identity, 4, None), None);
}

#[test]
fn absent_and_null_error_rows_remain_eligible_until_four_attempts() {
    // Given: max_retries=3 means a total budget of four reservations.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"shared");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    publish(&storage, &[("a.md", "shared"), ("b.md", "shared")]);
    for attempts in 1..=4 {
        // When / Then: work remains eligible before the next reservation.
        assert!(get_next_work(&storage, &identity, 4, None).is_some());
        // When: reserve without a failure message, including the crash window.
        storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                // Then: the reservation succeeds within the four-attempt budget.
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 4)
                        .unwrap()
                );

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
        // Then: the committed attempt count advances with no failure message.
        let failure = unwrap_current(
            storage
                .cache
                .with_read_tx(|| {
                    Ok::<_, StorageError>(
                        storage
                            .cache
                            .embed_failures_get_by_search_hash(&identity, &hash),
                    )
                })
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(failure.attempts, attempts);
        assert_eq!(failure.error, None);
    }
    // When / Then: the exhausted hash yields no further work or reservation.
    assert_eq!(get_next_work(&storage, &identity, 4, None), None);
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            assert!(
                !unit
                    .embed_failures_upsert_by_search_hash_and_attempts(&hash, 4)
                    .unwrap()
            );

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}

#[test]
fn work_and_dimensions_use_the_pinned_identity_and_membership() {
    // Given: a pinned work view before identity or membership changes.
    let root = TempDir::new().unwrap();
    let first = open(&root);
    let second = open(&root);
    let m1 = Sha256::digest(b"one");
    let m2 = Sha256::digest(b"two");
    first.vectors.reset_vectors(None, &m1).unwrap();
    publish(&first, &[("a.md", "pending")]);
    store(&first, &m1, "orphan", 3);
    first
        .cache
        .with_read_tx(|| {
            let pinned = &first.cache;
            // When: change both databases after the read view was captured.
            second.vectors.reset_vectors(Some(&m1), &m2).unwrap();
            store(&second, &m2, "pending", 4);
            publish(&second, &[("a.md", "changed")]);
            // Then: the pinned identity and membership determine the results.
            let dimensions =
                unwrap_current(pinned.vector_index_config_get_by_id(&m1).unwrap()).unwrap();
            assert_eq!(dimensions.get(), 3);
            assert_eq!(
                unwrap_current(
                    pinned
                        .nodes_get_by_kind_and_search_hash_and_attempts_and_id(&m1, 4, None)
                        .unwrap()
                )
                .unwrap()
                .search_hash,
                Sha256::digest(b"pending")
            );
            assert!(matches!(
                pinned.vector_index_config_get_by_id(&m2).unwrap(),
                VectorOutcome::ConfigChanged
            ));
            second
                .cache
                .with_read_tx(|| {
                    let fresh = &second.cache;
                    assert!(matches!(
                        fresh
                            .nodes_get_by_kind_and_search_hash_and_attempts_and_id(&m1, 4, None)
                            .unwrap(),
                        VectorOutcome::ConfigChanged
                    ));
                    assert!(matches!(
                        fresh.vector_index_config_get_by_id(&m1).unwrap(),
                        VectorOutcome::ConfigChanged
                    ));
                    assert_eq!(
                        unwrap_current(fresh.vector_index_config_get_by_id(&m2).unwrap())
                            .unwrap()
                            .get(),
                        4
                    );
                    Ok::<_, StorageError>(())
                })
                .unwrap();
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn last_vector_cleanup_preserves_dimension_across_reopen() {
    // Given: one final vector with native dimensions.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    store(&storage, &identity, "orphan", 3);
    // When: orphan cleanup runs and storage reopens.
    prune(&storage, &identity);
    drop(storage);
    let storage = open(&root);
    // Then: native dimensions survive after the last vector is removed.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&storage, &identity), Some(3));
    let unit_result = storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            let error = unit
                .store_vector(&Sha256::digest(b"new"), &[1.0; 4], 4)
                .unwrap_err();
            assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    assert!(unit_result.is_err());
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&storage, &identity), Some(3));
}

#[test]
fn first_committed_dimension_wins_between_competing_connections() {
    // Given: both connections observed an uninitialized dimension.
    let root = TempDir::new().unwrap();
    let first = open(&root);
    let second = open(&root);
    let identity = Sha256::digest(b"model");
    first.vectors.reset_vectors(None, &identity).unwrap();
    // When / Then: neither connection initially observes a native dimension.
    assert_eq!(get_size(&first, &identity), None);
    assert_eq!(get_size(&second, &identity), None);
    // When: hold the first write while the other tries to enter.
    first
        .vectors
        .with_write_tx(&identity, || {
            let winner = &first.vectors;
            winner.store_vector(&identity, &[1.0; 3], 3).unwrap();
            // Then: the second writer is blocked until the winning write commits.
            assert!(
                second
                    .vectors
                    .with_write_tx(&identity, || Ok::<_, StorageError>(()))
                    .map_err(TransactionError::into_inner)
                    .is_err()
            );

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = get_native_state(&root);
    let loser_result = second
        .vectors
        .with_write_tx(&identity, || {
            let loser = &second.vectors;
            let error = loser
                .store_vector(&Sha256::digest(b"loser"), &[1.0; 4], 4)
                .unwrap_err();
            assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    assert!(loser_result.is_err());
    // Then: rejecting the losing dimension preserves the winning native state.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(count_keys(&root), 1);
    assert_eq!(get_size(&second, &identity), Some(3));
    assert_eq!(
        unwrap_current(
            first
                .cache
                .with_read_tx(|| first
                    .cache
                    .vector_keys_get_by_search_hash(&identity, &identity))
                .unwrap()
        ),
        Some(vec![1.0; 3])
    );
}

#[test]
fn rollback_of_first_insertion_does_not_pin_dimensions() {
    // Given: an uncommitted first vector insertion.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    // When: the first vector transaction is abandoned.
    assert!(matches!(
        storage.vectors.with_write_tx(&identity, || {
            let unit = &storage.vectors;
            unit.store_vector(&identity, &[1.0; 3], 3).unwrap();

            Err::<(), _>(())
        }),
        Err(TransactionError::Operation(()))
    ));

    // Then:  including reuse of the same native connection after rollback.
    assert_eq!(count_keys(&root), 0);
    assert_eq!(get_size(&storage, &identity), None);
    store(&storage, &identity, "different", 4);
    assert_eq!(get_size(&storage, &identity), Some(4));
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the sequential transaction scenario and its assertions together"
)]
fn cleanup_retains_referenced_hashes_and_diagnostics_without_cache_publication() {
    // Given: referenced and orphan vectors with stored diagnostics.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    let revision_before = publish(
        &storage,
        &[("a.md", "shared"), ("b.md", "shared"), ("c.md", "failed")],
    );
    let key = store(&storage, &identity, "shared", 3);
    store(&storage, &identity, "orphan", 3);
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            for text in ["failed", "unused"] {
                let hash = Sha256::digest(text.as_bytes());
                unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 4)
                    .unwrap();
                unit.embed_failures_update_by_search_hash(&hash, "safe failure")
                    .unwrap();
            }
            unit.query_error_upsert_by_id(17, "query failure").unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    // When: orphan cleanup runs.
    prune(&storage, &identity);
    // Then: referenced vectors and diagnostics survive without cache publication.
    assert_eq!(store(&storage, &identity, "shared", 3), key);
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(read.project_get_by_id().unwrap(), revision_before);
            let orphan = Sha256::digest(b"orphan");
            let failed = Sha256::digest(b"failed");
            let unused = Sha256::digest(b"unused");
            assert_eq!(
                unwrap_current(
                    read.vector_keys_get_by_search_hash(&identity, &orphan)
                        .unwrap()
                ),
                None
            );
            assert!(
                unwrap_current(
                    read.embed_failures_get_by_search_hash(&identity, &failed)
                        .unwrap()
                )
                .is_some()
            );
            assert_eq!(
                unwrap_current(
                    read.embed_failures_get_by_search_hash(&identity, &unused)
                        .unwrap()
                ),
                None
            );
            assert_eq!(
                read.query_error_get_by_id().unwrap().unwrap().error,
                "query failure"
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn cleanup_uses_a_pinned_cache_observation_and_new_publication_is_recomputable() {
    // Given: a cleanup transaction with a pinned cache observation.
    let root = TempDir::new().unwrap();
    let first = open(&root);
    let second = open(&root);
    let identity = Sha256::digest(b"model");
    first.vectors.reset_vectors(None, &identity).unwrap();
    publish(&first, &[("a.md", "keep")]);
    store(&first, &identity, "keep", 3);
    first
        .vectors
        .with_write_tx(&identity, || {
            let cleanup = &first.vectors;
            cleanup.prune_unreferenced().unwrap(); // Pins cache membership.
            // When: publish a new hash, then put its vector into the older cleanup view.
            publish(&second, &[("b.md", "late")]);
            cleanup
                .store_vector(&Sha256::digest(b"late"), &[1.0; 3], 3)
                .unwrap();
            cleanup.prune_unreferenced().unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    // Then: only derived work is missing; the knowledge survives for recompute.
    let work = get_next_work(&second, &identity, 4, None).unwrap();
    assert_eq!(work.id.as_str(), "b.md");
    assert_eq!(work.search_hash, Sha256::digest(b"late"));
    assert_eq!(count_keys(&root), 1);
}

#[test]
fn late_old_identity_cannot_cleanup_current_vectors_or_query_error() {
    // Given: an obsolete identity and current vectors with query diagnostics.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let old = Sha256::digest(b"old");
    let current = Sha256::digest(b"current");
    storage.vectors.reset_vectors(None, &old).unwrap();
    storage.vectors.reset_vectors(Some(&old), &current).unwrap();
    publish(&storage, &[("a.md", "current")]);
    store(&storage, &current, "current", 3);
    storage
        .vectors
        .with_write_tx(&current, || {
            let unit = &storage.vectors;
            unit.query_error_upsert_by_id(9, "current failure").unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    // When / Then: the old identity attempts cleanup; current vectors and query diagnostics remain unchanged.
    assert!(matches!(
        storage
            .vectors
            .with_write_tx(&old, || Ok::<_, StorageError>(()))
            .unwrap(),
        VectorOutcome::ConfigChanged
    ));
    // Then: current vectors and query diagnostics remain unchanged.
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert!(
                unwrap_current(
                    read.vector_keys_get_by_search_hash(&current, &current)
                        .unwrap()
                )
                .is_some()
            );
            assert_eq!(
                read.query_error_get_by_id().unwrap().unwrap().error,
                "current failure"
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn deleting_one_shared_representative_retains_the_shared_vector() {
    // Given: two current knowledge nodes sharing one vector.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    publish(&storage, &[("a.md", "shared"), ("b.md", "shared")]);
    let key = store(&storage, &identity, "shared", 3);
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            unit.delete_node(&EdgeTarget::Knowledge(KnowledgeId::parse("a.md").unwrap()))
                .unwrap();

            unit.get_publication_revision()
        })
        .unwrap();
    // When: one representative is deleted and cleanup runs.
    prune(&storage, &identity);
    // Then: the remaining representative retains the shared vector.
    assert_eq!(count_keys(&root), 1);
    assert_eq!(store(&storage, &identity, "shared", 3), key);
}

#[test]
fn reset_rollback_restores_native_dimensions_and_commit_releases_them() {
    // Given: a late failure after reset has recreated the native table.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let old = Sha256::digest(b"old");
    let new = Sha256::digest(b"new");
    storage.vectors.reset_vectors(None, &old).unwrap();
    store(&storage, &old, "retained", 3);
    let before = get_native_state(&root);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    db.execute_batch(
        "create trigger reject_reset before update on meta
        begin select raise(abort, 'test reset failure'); end;",
    )
    .unwrap();
    // When / Then
    assert!(storage.vectors.reset_vectors(Some(&old), &new).is_err());
    db.execute_batch("drop trigger reject_reset;").unwrap();
    // Then: all key/data/metadata bytes and schema are restored.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(get_size(&storage, &old), Some(3));
    assert_eq!(
        storage.vectors.reset_vectors(None, &old).unwrap(),
        VectorOutcome::Current(false)
    );
    assert_eq!(get_size(&storage, &old), Some(3));
    // When: commit the reset and establish a different first dimension.
    // Then
    assert_eq!(
        storage.vectors.reset_vectors(Some(&old), &new).unwrap(),
        VectorOutcome::Current(true)
    );
    assert_eq!(get_size(&storage, &new), None);
    store(&storage, &new, "new", 4);
    assert_eq!(get_size(&storage, &new), Some(4));
}

#[test]
fn unsupported_native_dimensions_poison_the_unit_without_keys() {
    for size in [1, 4097] {
        // Given: unsupported native dimension counts.
        let root = TempDir::new().unwrap();
        let storage = open(&root);
        let identity = Sha256::digest(b"model");
        storage.vectors.reset_vectors(None, &identity).unwrap();
        let before = get_native_state(&root);
        let unit_result = storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
                // When: a vector is stored with each unsupported size.
                let error = unit
                    .store_vector(&identity, &vec![1.0; size], size)
                    .unwrap_err();
                // Then: the unit is poisoned and no key remains.
                assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .map_err(TransactionError::into_inner);
        assert!(unit_result.is_err());
        assert_eq!(get_native_state(&root), before);
        assert_eq!(get_size(&storage, &identity), None);
    }
}

#[test]
fn nullable_failure_hash_does_not_hide_unrelated_work() {
    // Given: SQLite rowid-table TEXT PRIMARY KEY permits a legacy NULL key.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    publish(&storage, &[("a.md", "pending")]);
    let db = Connection::open(root.path().join("vectors")).unwrap();
    // This malformed legacy row cannot be created by the typed writer.
    db.execute(
        "insert into embed_failures(search_hash, attempts) values(null, 4)",
        [],
    )
    .unwrap();
    // When
    let work = get_next_work(&storage, &identity, 4, None).unwrap();
    // Then: a NOT IN anti-join would wrongly suppress every candidate.
    assert_eq!(work.id.as_str(), "a.md");
}

#[test]
fn invalid_attempt_budgets_are_rejected_without_changing_storage() {
    // Given: invalid durable attempt budgets.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    publish(&storage, &[("a.md", "pending")]);
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            for budget in [0, u64::MAX] {
                // When: the next embedding work is requested.
                let error = read
                    .nodes_get_by_kind_and_search_hash_and_attempts_and_id(&identity, budget, None)
                    .unwrap_err();
                // Then: storage rejects the budget without mutation.
                assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
            }
            assert_eq!(read.project_get_by_id().unwrap(), 1);
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn abandoned_cleanup_restores_vectors_failures_and_dimensions() {
    // Given: vectors, failures and dimensions before orphan cleanup.
    let root = TempDir::new().unwrap();
    let storage = open(&root);
    let identity = Sha256::digest(b"model");
    storage.vectors.reset_vectors(None, &identity).unwrap();
    store(&storage, &identity, "orphan", 3);
    storage
        .vectors
        .with_write_tx(&identity, || {
            let unit = &storage.vectors;
            unit.embed_failures_upsert_by_search_hash_and_attempts(&identity, 4)
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let before = get_native_state(&root);
    // When: the cleanup transaction is abandoned.
    assert!(matches!(
        storage.vectors.with_write_tx(&identity, || {
            let cleanup = &storage.vectors;
            cleanup.prune_unreferenced().unwrap();

            Err::<(), _>(())
        }),
        Err(TransactionError::Operation(()))
    ));

    // Then: all prior vector state is restored.
    assert_eq!(get_native_state(&root), before);
    assert_eq!(get_size(&storage, &identity), Some(3));
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(
                unwrap_current(
                    read.embed_failures_get_by_search_hash(&identity, &identity)
                        .unwrap()
                )
                .unwrap()
                .attempts,
                1
            );
            Ok::<_, StorageError>(())
        })
        .unwrap();
}

fn open(root: &TempDir) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
    open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap()
}

fn publish<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    rows: &[(&str, &str)],
) -> u64 {
    let unit_result = storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            for (id, text) in rows {
                unit.nodes_upsert_by_id(&support::build_knowledge(id, text))
                    .unwrap();
            }

            unit.get_publication_revision()
        })
        .map_err(TransactionError::into_inner);
    unit_result.unwrap()
}

fn store<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
    text: &str,
    size: usize,
) -> i64 {
    let unit_result = storage
        .vectors
        .with_write_tx(identity, || {
            let unit = &storage.vectors;
            let key = unit
                .store_vector(&Sha256::digest(text.as_bytes()), &vec![1.0; size], size)
                .unwrap();

            Ok::<_, StorageError>(((), key))
        })
        .map(get_current)
        .map_err(TransactionError::into_inner);
    let (_, key) = unit_result.unwrap();

    key.row_id
}

fn get_next_work<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
    budget: u64,
    after: Option<&KnowledgeId>,
) -> Option<EmbeddingWork> {
    unwrap_current(
        storage
            .cache
            .with_read_tx(|| {
                Ok::<_, StorageError>(
                    storage
                        .cache
                        .nodes_get_by_kind_and_search_hash_and_attempts_and_id(
                            identity, budget, after,
                        ),
                )
            })
            .unwrap()
            .unwrap(),
    )
}

fn get_size<C: CacheDatabase, V: VectorDatabase>(
    storage: &Databases<C, V>,
    identity: &Sha256,
) -> Option<usize> {
    unwrap_current(
        storage
            .cache
            .with_read_tx(|| {
                Ok::<_, StorageError>(storage.cache.vector_index_config_get_by_id(identity))
            })
            .unwrap()
            .unwrap(),
    )
    .map(Into::into)
}

fn prune<C: CacheDatabase, V: VectorDatabase>(storage: &Databases<C, V>, identity: &Sha256) {
    storage
        .vectors
        .with_write_tx(identity, || {
            let unit = &storage.vectors;
            unit.prune_unreferenced().unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
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
