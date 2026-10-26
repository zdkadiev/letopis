#[path = "support/storage.rs"]
mod support;
use letopis::{
    domain::Sha256,
    open_status_storage, open_storage,
    service::{
        CacheWrite, EmbeddingStatusPolicy, StatusStorage, Storage, StorageErrorKind, StorageRead,
        VectorOutcome, VectorWrite,
    },
};
use rusqlite::Connection;
use std::{
    fs::{self, File},
    path::Path,
};
use tempfile::TempDir;

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
    let mut cache = writer.begin_cache_write().unwrap();
    cache
        .upsert_node(&support::build_source("file://source", None, 7))
        .unwrap();
    cache.commit().unwrap();
    store(&mut writer, &identity, "C");
    store(&mut writer, &identity, "D");
    fail(&mut writer, &identity, "B", 2);
    fail(&mut writer, &identity, "obsolete", 2);
    // When.
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read().unwrap().unwrap();
    let unavailable = read
        .read_embedding_status(&EmbeddingStatusPolicy::Unavailable)
        .unwrap();
    let ready = read.read_embedding_status(&ready_policy(identity)).unwrap();
    // Then independent literal counts and one exhausted live hash.
    assert_eq!((unavailable.backlog, unavailable.failed), (3, 0));
    assert_eq!((ready.backlog, ready.failed), (1, 1));
    assert_eq!(ready.exhausted.len(), 1);
    assert_eq!(ready.exhausted[0].search_hash, Sha256::digest(b"B"));
    assert_eq!(ready.exhausted[0].attempts, 2);
}

#[test]
fn respects_attempt_boundary() {
    // Given
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
    // When
    let result = status
        .begin_status_read()
        .unwrap()
        .unwrap()
        .read_embedding_status(&ready_policy(identity))
        .unwrap();
    // Then
    assert_eq!((result.backlog, result.failed), (1, 1));
    assert_eq!(result.exhausted.len(), 1);
    assert_eq!(result.exhausted[0].search_hash, Sha256::digest(b"B"));
}

#[test]
fn ignores_vectors_for_mismatched_identity() {
    // Given
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"old");
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&mut writer, &identity, "A");
    fail(&mut writer, &identity, "B", 2);
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read().unwrap().unwrap();
    // When
    let result = read
        .read_embedding_status(&ready_policy(Sha256::digest(b"new")))
        .unwrap();
    // Then
    assert_eq!((result.backlog, result.failed), (3, 0));
    assert!(result.exhausted.is_empty());
    assert_eq!(read.get_config_identity(), Some(identity));
    assert!(matches!(
        read.read_vector(&identity, &Sha256::digest(b"A")).unwrap(),
        VectorOutcome::Current(Some(_))
    ));
}

#[test]
fn opens_missing_files_without_creating_them() {
    // Given
    let root = TempDir::new().unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut missing = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then
    assert!(missing.begin_status_read().unwrap().is_none());
    assert!(!root.path().join("cache.db").exists());
    assert!(!root.path().join("vectors.db").exists());
    drop(missing);
    let mut writer = open_writer(root.path());
    assert!(
        open_status_storage(&root.path().canonicalize().unwrap(), &pinned)
            .unwrap()
            .begin_status_read()
            .unwrap()
            .is_none()
    );
    publish(&mut writer, &[("a.md", "A"), ("b.md", "A"), ("c.md", "B")]);
    drop(writer);
    // Given
    fs::remove_file(root.path().join("vectors.db")).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let summary = status
        .begin_status_read()
        .unwrap()
        .unwrap()
        .read_embedding_status(&ready_policy(Sha256::digest(b"model")))
        .unwrap();
    // When / Then
    assert_eq!((summary.backlog, summary.failed), (2, 0));
    assert!(!root.path().join("vectors.db").exists());
}

#[test]
fn pins_status_revision_and_counts() {
    // Given
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A")]);
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read().unwrap().unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B")]);
    store(&mut writer, &identity, "A");
    // When
    let summary = read.read_embedding_status(&ready_policy(identity)).unwrap();
    // Then
    assert_eq!(read.get_revision(), 1);
    assert_eq!((summary.backlog, summary.failed), (1, 0));
}

#[test]
fn rejects_malformed_persisted_counts_and_read_failure() {
    // Given
    for attempts in ["-1", "0", "'invalid'", "1.5"] {
        let root = TempDir::new().unwrap();
        let mut writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer.reset_vectors(None, &identity).unwrap();
        publish(&mut writer, &[("a.md", "A")]);
        let connection = Connection::open(root.path().join("vectors.db")).unwrap();
        connection
            .execute_batch("PRAGMA ignore_check_constraints=ON")
            .unwrap();
        connection
            .execute(
                &format!("INSERT INTO embed_failures VALUES(?1,{attempts},NULL)"),
                [Sha256::digest(b"A").to_hex()],
            )
            .unwrap();
        let pinned = File::open(root.path()).unwrap();
        let mut status =
            open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
        // When / Then
        assert!(
            status
                .begin_status_read()
                .unwrap()
                .unwrap()
                .read_embedding_status(&ready_policy(identity))
                .is_err(),
            "{attempts}"
        );
    }
    let root = TempDir::new().unwrap();
    let mut writer = open_writer(root.path());
    publish(&mut writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("cache.db")).unwrap();
    connection
        .execute_batch("UPDATE project SET revision=-1")
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When / Then
    assert!(status.begin_status_read().is_err());
    drop(status);
    connection.execute_batch("DROP TABLE project").unwrap();
    assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
}

#[test]
fn preserves_files_and_orders_only_uncovered_current_failures() {
    // Given known covered and missing hashes plus a durable query failure.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let mut writer = open_writer(root.path());
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
    store(&mut writer, &identity, "C");
    fail(&mut writer, &identity, "A", 2);
    fail(&mut writer, &identity, "B", 2);
    fail(&mut writer, &identity, "C", 2);
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write(&identity).unwrap() else {
        panic!()
    };
    unit.set_query_error(7, "safe query failure").unwrap();
    unit.commit().unwrap();
    drop(writer);
    let before = [
        fs::read(root.path().join("cache.db")).unwrap(),
        fs::read(root.path().join("vectors.db")).unwrap(),
    ];
    // When.
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read().unwrap().unwrap();
    let result = read.read_embedding_status(&ready_policy(identity)).unwrap();
    let query_error = read.read_query_error().unwrap().unwrap();
    // Then hashes have known independent lexicographic order (A=559a..., B=df7e...).
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
        connection.execute_batch("PRAGMA user_version=99").unwrap();
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
fn rejects_current_vector_key_without_native_row() {
    // Given a published current hash whose key has no corresponding native vector.
    let root = TempDir::new().unwrap();
    let identity = Sha256::digest(b"model");
    let hash = Sha256::digest(b"A");
    let mut writer = open_writer(root.path());
    writer.reset_vectors(None, &identity).unwrap();
    publish(&mut writer, &[("a.md", "A")]);
    let connection = Connection::open(root.path().join("vectors.db")).unwrap();
    connection
        .execute(
            "INSERT INTO vector_keys(search_hash) VALUES(?1)",
            [hash.to_hex()],
        )
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let mut status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    let mut read = status.begin_status_read().unwrap().unwrap();
    // When.
    let unavailable = read
        .read_embedding_status(&EmbeddingStatusPolicy::Unavailable)
        .unwrap();
    let error = read
        .read_embedding_status(&ready_policy(identity))
        .unwrap_err();
    let vector_error = read.read_vector(&identity, &hash).unwrap_err();
    // Then configuration-unavailable counting ignores old vectors, but ready coverage fails safely.
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
    let mut unit = writer.begin_cache_write().unwrap();
    for (id, text) in rows {
        unit.upsert_node(&support::build_knowledge(id, text))
            .unwrap();
    }
    unit.commit().unwrap();
}
fn store(writer: &mut impl Storage, identity: &Sha256, text: &str) {
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write(identity).unwrap() else {
        panic!()
    };
    unit.store_vector(&Sha256::digest(text.as_bytes()), &[1.0, 0.0, 0.0], 3)
        .unwrap();
    unit.commit().unwrap();
}
fn fail(writer: &mut impl Storage, identity: &Sha256, text: &str, attempts: u64) {
    let VectorOutcome::Current(mut unit) = writer.begin_vector_write(identity).unwrap() else {
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
