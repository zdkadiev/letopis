#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::Sha256,
    open_status_storage, open_storage,
    service::{
        EmbeddingStatusPolicy, StatusStorage, Storage, StorageErrorKind, StorageRead, VectorOutcome,
    },
};
use rusqlite::Connection;
use std::{
    fs::{self, File},
    path::Path,
};
use tempfile::TempDir;

#[test]
fn boxed_status_read_does_not_create_missing_publication() {
    // Given: a pinned root with no service databases.
    let root = TempDir::new().unwrap();
    let path = root.path().canonicalize().unwrap();
    let pinned = File::open(&path).unwrap();
    let mut storage = open_status_storage(&path, &pinned).unwrap();
    // When: status requests its narrow boxed read interface.
    let view: Option<Box<dyn StorageRead + '_>> = storage.begin_status_read_tx().unwrap();
    // Then: no publication or durable database is created.
    assert!(view.is_none());
    assert!(!path.join("cache.db").exists());
    assert!(!path.join("vectors.db").exists());
}

#[test]
fn counts_distinct_current_hashes() {
    // Given: shared A, failed B, covered C, source and orphan D.
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.reset_vectors(None, &identity).unwrap();
    publish(
        &mut writer,
        &[
            ("a.md", "A"),
            ("shared.md", "A"),
            ("b.md", "B"),
            ("c.md", "C"),
        ],
    );
    let mut cache = writer.begin_cache_write_tx().unwrap();
    cache
        .upsert_node(&support::build_source("file://source", None, 7))
        .unwrap();
    cache.commit().unwrap();
    store(&mut writer, &identity, "C");
    store(&mut writer, &identity, "D");
    fail(&mut writer, &identity, "B", 2);
    fail(&mut writer, &identity, "obsolete", 2);
    // When: status counts the current publication under unavailable and ready policies.
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();
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
}

#[test]
fn respects_attempt_boundary() {
    // Given: a published corpus at the durable attempt boundary.
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B")]);
    fail(&mut writer, &identity, "A", 1);
    fail(&mut writer, &identity, "B", 2);
    fail(&mut writer, &identity, "obsolete", 2);
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When: status counts embedding work.
    let result = status
        .begin_status_read_tx()
        .unwrap()
        .unwrap()
        .get_embedding_status(&ready_policy(identity))
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
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"old");
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&mut writer, &identity, "A");
    fail(&mut writer, &identity, "B", 2);
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();
    // When: status uses a different current identity.
    let result = read
        .get_embedding_status(&ready_policy(Sha256::digest(b"new")))
        .unwrap();
    // Then: all current knowledge remains backlog.
    assert_eq!((result.backlog, result.failed), (3, 0));
    assert!(result.exhausted.is_empty());
    assert_eq!(read.get_config_identity(), Some(identity));
    assert!(matches!(
        read.get_vector_by_search_hash(&identity, &Sha256::digest(b"A"))
            .unwrap(),
        VectorOutcome::Current(Some(_))
    ));
}

#[test]
fn opens_missing_files_without_creating_them() {
    // Given: a status reader with missing cache or vector files.
    let root = TempDir::new().unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut missing = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then: status reads the available publication; missing database files are never created.
    assert!(missing.begin_status_read_tx().unwrap().is_none());
    assert!(!root.path().join("cache.db").exists());
    assert!(!root.path().join("vectors.db").exists());
    drop(missing);
    let mut writer = open_writer(root.path());
    assert!(
        open_status_storage(&root.path().canonicalize().unwrap(), &pinned)
            .unwrap()
            .begin_status_read_tx()
            .unwrap()
            .is_none()
    );
    publish(&mut writer, &[("a.md", "A"), ("b.md", "A"), ("c.md", "B")]);
    drop(writer);
    // Given: a status reader with missing cache or vector files.
    fs::remove_file(root.path().join("vectors.db")).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let summary = status
        .begin_status_read_tx()
        .unwrap()
        .unwrap()
        .get_embedding_status(&ready_policy(Sha256::digest(b"model")))
        .unwrap();
    // When / Then: status reads the available publication; missing database files are never created.
    assert_eq!((summary.backlog, summary.failed), (2, 0));
    assert!(!root.path().join("vectors.db").exists());
}

#[test]
fn pins_status_revision_and_counts() {
    // Given: a status view pinned before a later publication.
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A")]);
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B")]);
    store(&mut writer, &identity, "A");
    // When: embedding status is counted.
    let summary = read.get_embedding_status(&ready_policy(identity)).unwrap();
    // Then: revision and counts remain from the pinned view.
    assert_eq!(read.get_revision(), 1);
    assert_eq!((summary.backlog, summary.failed), (1, 0));
}

#[test]
fn rejects_malformed_persisted_counts_and_read_failure() {
    // Given: malformed durable attempt counts or a failing status read.
    for attempts in ["-1", "0", "'invalid'", "1.5"] {
        let root = TempDir::new().unwrap();
        let mut writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer.reset_vectors(None, &identity).unwrap();
        publish(&mut writer, &[("a.md", "A")]);
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
        let mut status =
            open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
        // When / Then: status decodes the stored state; malformed values and read failures propagate.
        assert!(
            status
                .begin_status_read_tx()
                .unwrap()
                .unwrap()
                .get_embedding_status(&ready_policy(identity))
                .is_err(),
            "{attempts}"
        );
    }
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    publish(&mut writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("cache.db")).unwrap();
    connection
        .execute_batch("update project set revision=-1")
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then: status decodes the stored state; malformed values and read failures propagate.
    assert!(status.begin_status_read_tx().is_err());
    drop(status);
    connection.execute_batch("drop table project").unwrap();
    assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
}

#[test]
fn preserves_files_and_orders_only_uncovered_current_failures() {
    // Given: known covered and missing hashes plus a durable query failure.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let mut writer = open_writer(root.path());
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&mut writer, &identity, "C");
    fail(&mut writer, &identity, "A", 2);
    fail(&mut writer, &identity, "B", 2);
    fail(&mut writer, &identity, "C", 2);
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write_tx(&identity).unwrap() else {
        panic!()
    };
    unit.set_query_error(7, "safe query failure").unwrap();
    unit.commit().unwrap();
    drop(writer);
    let before = [
        fs::read(root.path().join("cache.db")).unwrap(),
        fs::read(root.path().join("vectors.db")).unwrap(),
    ];
    // When: status reads embedding counts, failures and the query diagnostic.
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();
    let result = read.get_embedding_status(&ready_policy(identity)).unwrap();
    let query_error = read.get_query_error().unwrap().unwrap();
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
    drop(read);
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
        let mut writer = open_writer(root.path());
        publish(&mut writer, &[("a.md", "A")]);
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
    let mut writer = open_writer(root.path());
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B")]);
    fail(&mut writer, &identity, "A", 2);
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write_tx(&identity).unwrap() else {
        panic!()
    };
    unit.set_query_error(7, "safe query failure").unwrap();
    unit.commit().unwrap();
    let connection = Connection::open(root.path().join("vectors.db")).unwrap();
    connection
        .execute(
            "insert into vector_keys(search_hash) values(?1)",
            [Sha256::digest(b"B").to_hex()],
        )
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();

    // When: embedding status is requested.
    let disabled = read
        .get_embedding_status(&EmbeddingStatusPolicy::Disabled)
        .unwrap();

    // Then: no coverage or stored-failure read can fail or leak into disabled counts.
    assert_eq!((disabled.backlog, disabled.failed), (0, 0));
    assert!(disabled.exhausted.is_empty());
    assert!(read.get_query_error().unwrap().is_some());
}

#[test]
fn rejects_current_vector_key_without_native_row() {
    // Given: a published current hash whose key has no corresponding native vector.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"A");
    let mut writer = open_writer(root.path());
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("vectors.db")).unwrap();
    connection
        .execute(
            "insert into vector_keys(search_hash) values(?1)",
            [hash.to_hex()],
        )
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read_tx().unwrap().unwrap();
    // When: status compares unavailable and ready coverage policies.
    let unavailable = read
        .get_embedding_status(&EmbeddingStatusPolicy::Unavailable)
        .unwrap();
    let error = read
        .get_embedding_status(&ready_policy(identity))
        .unwrap_err();
    let vector_error = read
        .get_vector_by_search_hash(&identity, &hash)
        .unwrap_err();
    // Then: configuration-unavailable counting ignores old vectors, but ready coverage fails safely.
    assert_eq!((unavailable.backlog, unavailable.failed), (1, 0));
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(error.get_message(), "invalid stored vector data");
    assert_eq!(vector_error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(vector_error.get_message(), "invalid stored vector data");
}

fn open_writer(root: &Path) -> impl Storage + use<> {
    open_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
}
fn ready_policy(identity: Sha256) -> EmbeddingStatusPolicy {
    EmbeddingStatusPolicy::Ready {
        identity,
        max_attempts: 2,
    }
}
fn publish(writer: &mut impl Storage, rows: &[(&str, &str)]) {
    let mut unit = writer.begin_cache_write_tx().unwrap();
    for (id, text) in rows {
        unit.upsert_node(&support::build_knowledge(id, text))
            .unwrap();
    }
    unit.commit().unwrap();
}
fn store(writer: &mut impl Storage, identity: &Sha256, text: &str) {
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write_tx(identity).unwrap() else {
        panic!()
    };
    unit.store_vector(&Sha256::digest(text.as_bytes()), &[1.0, 0.0, 0.0], 3)
        .unwrap();
    unit.commit().unwrap();
}
fn fail(writer: &mut impl Storage, identity: &Sha256, text: &str, attempts: u64) {
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write_tx(identity).unwrap() else {
        panic!()
    };
    for _ in 0..attempts {
        assert!(
            unit.reserve_attempt(&Sha256::digest(text.as_bytes()), 2)
                .unwrap()
        );
    }
    unit.record_failure(&Sha256::digest(text.as_bytes()), "safe failure")
        .unwrap();
    unit.commit().unwrap();
}
