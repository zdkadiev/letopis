use crate::support::get_current;
use letopis::service::StorageError;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, VectorDatabase};
use support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::Sha256,
    open_status_storage,
    service::{EmbeddingStatusPolicy, StorageErrorKind, StorageRead, VectorOutcome},
};
use rusqlite::Connection;
use std::{
    fs::{self, File},
    path::Path,
};
use tempfile::TempDir;

#[test]
fn status_scope_skips_missing_publication_without_creating_databases() {
    // Given: a retained directory without either database.
    let root = TempDir::new().unwrap();
    let path = root.path().canonicalize().unwrap();
    let pinned = File::open(&path).unwrap();
    let storage = open_status_storage(&path, &pinned).unwrap();
    // When: status asks for a publication that does not exist.
    let result = storage
        .with_status_read_tx(|| -> letopis::service::StorageResult<()> {
            panic!("absent publication callback executed")
        })
        .unwrap();
    // Then: the callback was skipped and neither durable file exists.
    assert_eq!(result, None);
    assert!(!path.join("cache.db").exists());
    assert!(!path.join("vectors.db").exists());
}

#[test]
fn counts_distinct_current_hashes() {
    // Given: shared A, failed B, covered C, source and orphan D.
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(
        &writer,
        &[
            ("a.md", "A"),
            ("shared.md", "A"),
            ("b.md", "B"),
            ("c.md", "C"),
        ],
    );
    writer
        .cache
        .with_write_tx(|| {
            let cache = &writer.cache;
            cache
                .nodes_upsert_by_id(&support::build_source("file://source", None, 7))
                .unwrap();

            cache.get_publication_revision()
        })
        .unwrap();
    store(&writer, &identity, "C");
    store(&writer, &identity, "D");
    fail(&writer, &identity, "B", 2);
    fail(&writer, &identity, "obsolete", 2);
    // When: status counts the current publication under unavailable and ready policies.
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;
            let unavailable = read
                .get_embedding_status(&EmbeddingStatusPolicy::Unavailable)
                .unwrap();
            let ready = read.get_embedding_status(&ready_policy(identity)).unwrap();
            // Then: independent literal counts and one exhausted live hash.
            assert_eq!((unavailable.backlog, unavailable.failed), (3, 0));
            assert_eq!((ready.backlog, ready.failed), (1, 1));
            assert_eq!(ready.exhausted.len(), 1);
            assert_eq!(ready.exhausted[0].search_hash, Sha256::digest(b"B"));
            assert_eq!(ready.exhausted[0].attempts, 2);

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();
}

#[test]
fn respects_attempt_boundary() {
    // Given: a published corpus at the durable attempt boundary.
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
    fail(&writer, &identity, "A", 1);
    fail(&writer, &identity, "B", 2);
    fail(&writer, &identity, "obsolete", 2);
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When: status counts embedding work.
    let result = status
        .with_status_read_tx(|| {
            Ok::<_, StorageError>(status.get_embedding_status(&ready_policy(identity)))
        })
        .unwrap()
        .unwrap()
        .unwrap();
    // Then: backlog and exhausted attempts remain distinct.
    assert_eq!((result.backlog, result.failed), (1, 1));
    assert_eq!(result.exhausted.len(), 1);
    assert_eq!(result.exhausted[0].search_hash, Sha256::digest(b"B"));
}

#[test]
fn ignores_vectors_for_mismatched_identity() {
    // Given: stored vectors under an older identity.
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    let identity = Sha256::digest(b"old");
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&writer, &identity, "A");
    fail(&writer, &identity, "B", 2);
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;
            // When: status uses a different current identity.
            let result = read
                .get_embedding_status(&ready_policy(Sha256::digest(b"new")))
                .unwrap();
            // Then: all current knowledge remains backlog.
            assert_eq!((result.backlog, result.failed), (3, 0));
            assert!(result.exhausted.is_empty());
            assert_eq!(read.meta_get_by_id().unwrap(), Some(identity));
            assert!(matches!(
                read.vector_keys_get_vector_by_search_hash(&identity, &Sha256::digest(b"A"))
                    .unwrap(),
                VectorOutcome::Current(Some(_))
            ));

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();
}

#[test]
fn opens_missing_files_without_creating_them() {
    // Given: a status reader with missing cache or vector files.
    let root = TempDir::new().unwrap();
    let pinned = File::open(root.path()).unwrap();
    let missing = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then: status reads the available publication; missing database files are never created.
    assert!(
        missing
            .with_status_read_tx(|| Ok::<_, StorageError>(()))
            .unwrap()
            .is_none()
    );
    assert!(!root.path().join("cache.db").exists());
    assert!(!root.path().join("vectors.db").exists());
    drop(missing);
    let writer = open_writer(root.path());
    assert!(
        {
            let scope_databases =
                open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
            scope_databases
                .with_status_read_tx(|| Ok::<_, StorageError>(()))
                .map_err(TransactionError::into_inner)
        }
        .unwrap()
        .is_none()
    );
    publish(&writer, &[("a.md", "A"), ("b.md", "A"), ("c.md", "B")]);
    drop(writer);
    // Given: a status reader with missing cache or vector files.
    fs::remove_file(root.path().join("vectors.db")).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let summary = status
        .with_status_read_tx(|| {
            Ok::<_, StorageError>(
                status.get_embedding_status(&ready_policy(Sha256::digest(b"model"))),
            )
        })
        .unwrap()
        .unwrap()
        .unwrap();
    // When / Then: status reads the available publication; missing database files are never created.
    assert_eq!((summary.backlog, summary.failed), (2, 0));
    assert!(!root.path().join("vectors.db").exists());
}

#[test]
fn pins_status_revision_and_counts() {
    // Given: a status view pinned before a later publication.
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A")]);
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;
            publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
            store(&writer, &identity, "A");
            // When: embedding status is counted.
            let summary = read.get_embedding_status(&ready_policy(identity)).unwrap();
            // Then: revision and counts remain from the pinned view.
            assert_eq!(read.project_get_by_id().unwrap(), 1);
            assert_eq!((summary.backlog, summary.failed), (1, 0));

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();
}

#[test]
fn rejects_malformed_persisted_counts_and_read_failure() {
    // Given: malformed durable attempt counts or a failing status read.
    for attempts in ["-1", "0", "'invalid'", "1.5"] {
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer.vectors.reset_vectors(None, &identity).unwrap();
        publish(&writer, &[("a.md", "A")]);
        let connection = Connection::open(root.path().join("vectors.db")).unwrap();
        connection
            .execute_batch("pragma ignore_check_constraints=on")
            .unwrap();
        connection
            .execute(
                &format!("insert into embed_failures values(?1,{attempts},null)"),
                [Sha256::digest(b"A").to_hex()],
            )
            .unwrap();
        let pinned = File::open(root.path()).unwrap();
        let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
        // When / Then: status decodes the stored state; malformed values and read failures propagate.
        assert!(
            status
                .with_status_read_tx(|| Ok::<_, StorageError>(
                    status.get_embedding_status(&ready_policy(identity))
                ))
                .unwrap()
                .unwrap()
                .is_err(),
            "{attempts}"
        );
    }
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    publish(&writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("cache.db")).unwrap();
    connection
        .execute_batch("update project set revision=-1")
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then: status decodes the stored state; malformed values and read failures propagate.
    assert!(
        status
            .with_status_read_tx(|| Ok::<_, StorageError>(()))
            .map_err(TransactionError::into_inner)
            .is_err()
    );
    drop(status);
    connection.execute_batch("drop table project").unwrap();
    assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
}

#[test]
fn preserves_files_and_orders_only_uncovered_current_failures() {
    // Given: known covered and missing hashes plus a durable query failure.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let writer = open_writer(root.path());
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&writer, &identity, "C");
    fail(&writer, &identity, "A", 2);
    fail(&writer, &identity, "B", 2);
    fail(&writer, &identity, "C", 2);
    writer
        .vectors
        .with_write_tx(&identity, || {
            let unit = &writer.vectors;
            unit.query_error_upsert_by_id(7, "safe query failure")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    drop(writer);
    let before = [
        fs::read(root.path().join("cache.db")).unwrap(),
        fs::read(root.path().join("vectors.db")).unwrap(),
    ];
    // When: status reads embedding counts, failures and the query diagnostic.
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;
            let result = read.get_embedding_status(&ready_policy(identity)).unwrap();
            let query_error = read.query_error_get_by_id().unwrap().unwrap();
            // Then: hashes have known independent lexicographic order (A=559a..., B=df7e...).
            assert_eq!((result.backlog, result.failed), (0, 2));
            assert_eq!(
                result
                    .exhausted
                    .iter()
                    .map(|f| f.search_hash)
                    .collect::<Vec<_>>(),
                vec![Sha256::digest(b"A"), Sha256::digest(b"B")]
            );
            assert_eq!(query_error.at, 7);
            assert_eq!(query_error.error, "safe query failure");

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();

    drop(status);
    assert_eq!(fs::read(root.path().join("cache.db")).unwrap(), before[0]);
    assert_eq!(fs::read(root.path().join("vectors.db")).unwrap(), before[1]);
}

#[test]
fn rejects_existing_invalid_vector_schema_without_cache() {
    // Given
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("vectors.db"), b"not sqlite").unwrap();
    let pinned = File::open(root.path()).unwrap();
    // When / Then
    assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    assert!(!root.path().join("cache.db").exists());
}

#[test]
fn rejects_unsafe_paths_and_existing_invalid_schemas() {
    // Given
    use std::os::unix::fs::symlink;
    // When / Then
    for leaf in ["cache.db", "vectors.db"] {
        let root = TempDir::new().unwrap();
        let pinned = File::open(root.path()).unwrap();
        symlink("outside", root.path().join(leaf)).unwrap();
        // When / Then
        assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    }
    for leaf in ["cache.db", "vectors.db"] {
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        publish(&writer, &[("a.md", "A")]);
        drop(writer);
        let connection = Connection::open(root.path().join(leaf)).unwrap();
        connection.execute_batch("pragma user_version=99").unwrap();
        let pinned = File::open(root.path()).unwrap();
        // When / Then
        assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    }
    let root = TempDir::new().unwrap();
    let pinned = File::open(root.path()).unwrap();
    let different = TempDir::new().unwrap();
    assert!(open_status_storage(&different.path().canonicalize().unwrap(), &pinned).is_err());
}

#[test]
fn disabled_policy_returns_zero_without_touching_execution_state() {
    // Given: current knowledge, a broken executable vector row and stored failures.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let writer = open_writer(root.path());
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
    fail(&writer, &identity, "A", 2);
    writer
        .vectors
        .with_write_tx(&identity, || {
            let unit = &writer.vectors;
            unit.query_error_upsert_by_id(7, "safe query failure")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let connection = Connection::open(root.path().join("vectors.db")).unwrap();
    connection
        .execute(
            "insert into vector_keys(search_hash) values(?1)",
            [Sha256::digest(b"B").to_hex()],
        )
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;

            // When: embedding status is requested.
            let disabled = read
                .get_embedding_status(&EmbeddingStatusPolicy::Disabled)
                .unwrap();

            // Then: no coverage or stored-failure read can fail or leak into disabled counts.
            assert_eq!((disabled.backlog, disabled.failed), (0, 0));
            assert!(disabled.exhausted.is_empty());
            assert!(read.query_error_get_by_id().unwrap().is_some());

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();
}

#[test]
fn rejects_current_vector_key_without_native_row() {
    // Given: a published current hash whose key has no corresponding native vector.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"A");
    let writer = open_writer(root.path());
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("vectors.db")).unwrap();
    connection
        .execute(
            "insert into vector_keys(search_hash) values(?1)",
            [hash.to_hex()],
        )
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    status
        .with_status_read_tx(|| {
            let read = &status;
            // When: status compares unavailable and ready coverage policies.
            let unavailable = read
                .get_embedding_status(&EmbeddingStatusPolicy::Unavailable)
                .unwrap();
            let error = read
                .get_embedding_status(&ready_policy(identity))
                .unwrap_err();
            let vector_error = read
                .vector_keys_get_vector_by_search_hash(&identity, &hash)
                .unwrap_err();
            // Then: configuration-unavailable counting ignores old vectors, but ready coverage fails safely.
            assert_eq!((unavailable.backlog, unavailable.failed), (1, 0));
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
            assert_eq!(error.get_message(), "invalid stored vector data");
            assert_eq!(vector_error.get_kind(), StorageErrorKind::InternalError);
            assert_eq!(vector_error.get_message(), "invalid stored vector data");

            Ok::<_, StorageError>(())
        })
        .unwrap()
        .unwrap();
}

fn open_writer(root: &Path) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
    open_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
}
fn ready_policy(identity: Sha256) -> EmbeddingStatusPolicy {
    EmbeddingStatusPolicy::Ready {
        identity,
        max_attempts: 2,
    }
}
fn publish<C: CacheDatabase, V: VectorDatabase>(writer: &Databases<C, V>, rows: &[(&str, &str)]) {
    writer
        .cache
        .with_write_tx(|| {
            let unit = &writer.cache;
            for (id, text) in rows {
                unit.nodes_upsert_by_id(&support::build_knowledge(id, text))
                    .unwrap();
            }

            unit.get_publication_revision()
        })
        .unwrap();
}
fn store<C: CacheDatabase, V: VectorDatabase>(
    writer: &Databases<C, V>,
    identity: &Sha256,
    text: &str,
) {
    writer
        .vectors
        .with_write_tx(identity, || {
            let unit = &writer.vectors;
            unit.store_vector(&Sha256::digest(text.as_bytes()), &[1.0, 0.0, 0.0], 3)
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}
fn fail<C: CacheDatabase, V: VectorDatabase>(
    writer: &Databases<C, V>,
    identity: &Sha256,
    text: &str,
    attempts: u64,
) {
    writer
        .vectors
        .with_write_tx(identity, || {
            let unit = &writer.vectors;
            for _ in 0..attempts {
                assert!(
                    unit.embed_failures_upsert_by_search_hash_and_attempts(
                        &Sha256::digest(text.as_bytes()),
                        2
                    )
                    .unwrap()
                );
            }
            unit.embed_failures_update_by_search_hash(
                &Sha256::digest(text.as_bytes()),
                "safe failure",
            )
            .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}
