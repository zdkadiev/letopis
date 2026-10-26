use letopis::open_storage;
use letopis::service::{Storage, StorageErrorKind, StorageRead};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Barrier},
};
use tempfile::TempDir;

#[path = "support/storage.rs"]
mod storage_support;
use storage_support::assert_error;

#[test]
fn invalid_database_paths_return_safe_readable_diagnostics_without_side_effects() {
    for path in ["", "private-label\0cache.db"] {
        // Given: invalid input must be rejected before the vector database is opened.
        let root = TempDir::new().unwrap();
        let vectors = root.path().join("vectors.db");
        // When
        let error = open_storage(Path::new(path), &vectors).err().unwrap();
        // Then: verify the public diagnostic contract, not only an error category.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert_eq!(error.get_message(), "invalid storage database path");
        assert_eq!(error.to_string(), "invalid storage database path");
        assert!(!format!("{error:?}").contains("private-label"));
        assert!(!vectors.exists());
    }
}

#[test]
fn fresh_storage_reopens_with_empty_identity() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    // When / Then
    for _ in 0..2 {
        let mut storage = open_storage(&cache, &vectors).unwrap();
        let read = storage.begin_read().unwrap();
        // When / Then
        assert_eq!(read.get_revision(), 0);
        assert_eq!(read.get_config_identity(), None);
    }
    let connection = Connection::open(&cache).unwrap();
    connection
        .execute("UPDATE project SET revision=7", [])
        .unwrap();
    let connection = Connection::open(&vectors).unwrap();
    connection
        .execute("INSERT INTO meta VALUES(1,?1)", ["a".repeat(64)])
        .unwrap();
    let mut storage = open_storage(&cache, &vectors).unwrap();
    let read = storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 7);
    assert_eq!(read.get_config_identity().unwrap().to_hex(), "a".repeat(64));
}

#[test]
fn owned_storage_outlives_path_arguments() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = {
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        open_storage(&cache, &vectors).unwrap()
    };
    // When / Then
    assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
}

#[test]
fn same_database_path_is_rejected() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    drop(open_storage(&cache, &vectors).unwrap());
    let before = Sha256::digest(std::fs::read(&cache).unwrap());
    // When / Then
    assert_error(&cache, &cache, StorageErrorKind::InvalidRequest);
    assert_eq!(Sha256::digest(std::fs::read(&cache).unwrap()), before);
}

#[cfg(unix)]
#[test]
fn symlink_and_hardlink_aliases_are_rejected() {
    use std::os::unix::fs::symlink;
    let root = TempDir::new().unwrap();
    // Given an existing database; rejecting aliases must preserve its bytes.
    let cache = root.path().join("cache.db");
    drop(open_storage(&cache, &root.path().join("vectors.db")).unwrap());
    let before = Sha256::digest(std::fs::read(&cache).unwrap());
    let symbolic = root.path().join("symbolic.db");
    let hard = root.path().join("hard.db");
    symlink(&cache, &symbolic).unwrap();
    std::fs::hard_link(&cache, &hard).unwrap();
    // When / Then
    for alias in [&symbolic, &hard] {
        // When / Then
        assert_error(&cache, alias, StorageErrorKind::InvalidRequest);
        assert_eq!(Sha256::digest(std::fs::read(&cache).unwrap()), before);
    }
}

#[test]
fn literal_unicode_quote_and_question_mark_paths_open() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("кэш'?mode=memory.db");
    let vectors = root.path().join("векторы'?mode=ro.db");
    let mut storage = open_storage(&cache, &vectors).unwrap();
    // When / Then
    assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
    assert!(cache.is_file());
    assert!(vectors.is_file());
}

#[test]
fn missing_parent_and_nul_paths_are_rejected() {
    // Given
    let root = TempDir::new().unwrap();
    // When / Then
    assert_error(
        &root.path().join("missing/cache.db"),
        &root.path().join("v.db"),
        StorageErrorKind::InvalidRequest,
    );
    assert_error(
        &root.path().join("a\0.db"),
        &root.path().join("v.db"),
        StorageErrorKind::InvalidRequest,
    );
}

#[cfg(unix)]
#[test]
fn non_utf8_attachment_path_is_rejected() {
    // Given
    use std::os::unix::ffi::OsStringExt;
    let root = TempDir::new().unwrap();
    let bad = root.path().join(std::ffi::OsString::from_vec(vec![0xff]));
    // When
    let Err(error) = open_storage(&root.path().join("cache.db"), &bad) else {
        panic!("non-UTF-8 attachment accepted");
    };
    // Then
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
}

#[test]
fn unknown_or_partial_schema_is_not_recreated() {
    // Given
    for mutation in [
        "PRAGMA user_version=99",
        "DROP TABLE files",
        "DROP INDEX nodes_search_hash",
        "DROP INDEX edges_outgoing; CREATE INDEX edges_outgoing ON edges(source_id,target_id,type)",
        "DROP INDEX edges_incoming; CREATE INDEX edges_incoming ON edges(target_id,source_id,type)",
        "DROP INDEX nodes_source_snapshot; CREATE INDEX nodes_source_snapshot ON nodes(source_snapshot)",
        "DROP TABLE text_index; CREATE VIRTUAL TABLE text_index USING fts5(text,content='')",
        "CREATE TABLE raw_text(body TEXT); INSERT INTO raw_text VALUES('private')",
    ] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        drop(open_storage(&cache, &vectors).unwrap());
        let connection = Connection::open(&cache).unwrap();
        connection
            .execute("UPDATE project SET revision=9", [])
            .unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);
        // When / Then
        assert_error(&cache, &vectors, StorageErrorKind::InternalError);
        let connection = Connection::open(&cache).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT revision FROM project", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
        if mutation.contains("raw_text") {
            assert_eq!(
                connection
                    .query_row("SELECT body FROM raw_text", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "private"
            );
        }
    }
}

#[test]
fn parallel_initializers_leave_a_complete_schema() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    let barrier = Arc::new(Barrier::new(2));
    // When / Then
    let successes = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..2)
            .map(|_| {
                let barrier = Arc::clone(&barrier);
                let (cache, vectors) = (&cache, &vectors);
                scope.spawn(move || {
                    barrier.wait();
                    match open_storage(cache, vectors) {
                        Ok(_) => true,
                        Err(error) => {
                            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
                            let cause = std::error::Error::source(&error)
                                .unwrap()
                                .downcast_ref::<rusqlite::Error>()
                                .unwrap();
                            assert!(matches!(
                                cause.sqlite_error_code(),
                                Some(
                                    rusqlite::ErrorCode::DatabaseBusy
                                        | rusqlite::ErrorCode::DatabaseLocked
                                )
                            ));
                            false
                        }
                    }
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>()
    });
    assert_eq!(successes, 2, "both initializers must open successfully");
    let mut storage = open_storage(&cache, &vectors).unwrap();
    assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
}

#[test]
fn independent_thread_connections_register_native_vec1() {
    // Given: each thread creates its own temporary databases.
    // When / Then
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let root = TempDir::new().unwrap();
                let cache = root.path().join("cache.db");
                let vectors = root.path().join("vectors.db");
                let mut storage = open_storage(&cache, &vectors).unwrap();
                assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
                let connection = Connection::open(&vectors).unwrap();
                let info: String = connection
                    .query_row("SELECT vec1_info()", [], |r| r.get(0))
                    .unwrap();
                assert!(info.starts_with("version 0.8"), "{info}");
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
}

#[test]
fn sqlite_like_prefix_does_not_hide_extra_user_tables() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    drop(open_storage(&cache, &vectors).unwrap());
    let connection = Connection::open(&cache).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE sqliteXprivate(body TEXT); INSERT INTO sqliteXprivate VALUES('retain');",
        )
        .unwrap();
    drop(connection);
    // When / Then
    assert_error(&cache, &vectors, StorageErrorKind::InternalError);
}

#[test]
fn vector_schema_and_unversioned_user_data_are_not_recreated() {
    // Given
    for mutation in [
        "PRAGMA user_version=2",
        "DROP TABLE query_error",
        "ALTER TABLE meta ADD COLUMN raw_text TEXT",
    ] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        drop(open_storage(&cache, &vectors).unwrap());
        let connection = Connection::open(&vectors).unwrap();
        connection
            .execute(
                "INSERT INTO embed_failures VALUES(?1,3,'safe')",
                ["a".repeat(64)],
            )
            .unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);
        // When / Then
        assert_error(&cache, &vectors, StorageErrorKind::InternalError);
        let connection = Connection::open(&vectors).unwrap();
        assert_eq!(
            connection
                .query_row("SELECT attempts FROM embed_failures", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            3
        );
    }
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    let connection = Connection::open(&cache).unwrap();
    connection
        .execute_batch("CREATE TABLE existing(value TEXT); INSERT INTO existing VALUES('retain');")
        .unwrap();
    drop(connection);
    // When / Then
    assert_error(&cache, &vectors, StorageErrorKind::InternalError);
    let connection = Connection::open(&cache).unwrap();
    assert_eq!(
        connection
            .query_row("SELECT value FROM existing", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "retain"
    );
}

#[test]
fn malformed_stored_observations_return_safe_typed_errors() {
    // Given
    for (target, mutation) in [
        ("cache", "UPDATE project SET revision=-1"),
        (
            "vectors",
            "INSERT INTO meta VALUES(1,'private-invalid-hash')",
        ),
    ] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        drop(open_storage(&cache, &vectors).unwrap());
        let path = if target == "cache" { &cache } else { &vectors };
        Connection::open(path)
            .unwrap()
            .execute_batch(mutation)
            .unwrap();
        let mut storage = open_storage(&cache, &vectors).unwrap();
        let Err(error) = storage.begin_read() else {
            panic!("invalid observation accepted");
        };
        // When / Then
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert!(!error.to_string().contains("private"));
        assert!(std::error::Error::source(&error).is_some());
    }
}

#[test]
fn initialized_open_succeeds_but_cache_write_still_returns_busy() {
    // Given
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    drop(open_storage(&cache, &vectors).unwrap());
    let connection = Connection::open(&cache).unwrap();
    connection.execute_batch("BEGIN IMMEDIATE").unwrap();
    let mut storage = open_storage(&cache, &vectors).unwrap();
    // When / Then
    assert_eq!(storage.begin_read().unwrap().get_revision(), 0);
    let Err(error) = storage.begin_cache_write() else {
        panic!("competing cache writer was accepted");
    };
    let cause = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<rusqlite::Error>()
        .unwrap();
    assert_eq!(
        cause.sqlite_error_code(),
        Some(rusqlite::ErrorCode::DatabaseBusy)
    );
    connection.execute_batch("ROLLBACK").unwrap();
    assert!(open_storage(&cache, &vectors).is_ok());
}
