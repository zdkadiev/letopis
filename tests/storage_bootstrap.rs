use letopis::service::StorageError;
use letopis::service::StorageErrorKind;
use letopis::service::TransactionError;
use letopis::service::{CacheDatabase, StorageRead};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::{Arc, Barrier},
};
use storage_support::open_storage;
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
    // Given: fresh cache and vector database paths.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    // When / Then: storage opens and reopens; the initial vector identity remains absent.
    for _ in 0..2 {
        let storage = open_storage(&cache, &vectors).unwrap();
        storage
            .cache
            .with_read_tx(|| {
                let read = &storage.cache;
                // When / Then: storage opens and reopens; the initial vector identity remains absent.
                assert_eq!(read.project_get_by_id().unwrap(), 0);
                assert_eq!(read.meta_get_by_id().unwrap(), None);

                Ok::<_, StorageError>(())
            })
            .unwrap();
    }
    let connection = Connection::open(&cache).unwrap();
    connection
        .execute("update project set revision=7", [])
        .unwrap();
    let connection = Connection::open(&vectors).unwrap();
    connection
        .execute("insert into meta values(1,?1)", ["a".repeat(64)])
        .unwrap();
    let storage = open_storage(&cache, &vectors).unwrap();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(read.project_get_by_id().unwrap(), 7);
            assert_eq!(
                read.meta_get_by_id().unwrap().unwrap().to_hex(),
                "a".repeat(64)
            );

            Ok::<_, StorageError>(())
        })
        .unwrap();
}

#[test]
fn owned_storage_outlives_path_arguments() {
    // Given: owned storage created from temporary path arguments.
    let root = TempDir::new().unwrap();
    let storage = {
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        open_storage(&cache, &vectors).unwrap()
    };
    // When / Then: the caller drops its path arguments; storage remains usable through owned handles.
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.project_get_by_id())
            .unwrap(),
        0
    );
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
    // Given: literal Unicode, quote and question-mark paths.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("кэш'?mode=memory.db");
    let vectors = root.path().join("векторы'?mode=ro.db");
    let storage = open_storage(&cache, &vectors).unwrap();
    // When / Then: storage opens those paths; path bytes retain their literal meaning.
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.project_get_by_id())
            .unwrap(),
        0
    );
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
        "pragma user_version=99",
        "drop table files",
        "drop index nodes_search_hash",
        "drop index edges_outgoing; create index edges_outgoing on edges(source_id,target_id,type)",
        "drop index edges_incoming; create index edges_incoming on edges(target_id,source_id,type)",
        "drop index nodes_source_snapshot; create index nodes_source_snapshot on nodes(source_snapshot)",
        "drop table text_index; create virtual table text_index using fts5(text,content='')",
        "create table raw_text(body text); insert into raw_text values('private')",
    ] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        drop(open_storage(&cache, &vectors).unwrap());
        let connection = Connection::open(&cache).unwrap();
        connection
            .execute("update project set revision=9", [])
            .unwrap();
        connection.execute_batch(mutation).unwrap();
        drop(connection);
        // When / Then
        assert_error(&cache, &vectors, StorageErrorKind::InternalError);
        let connection = Connection::open(&cache).unwrap();
        assert_eq!(
            connection
                .query_row("select revision from project", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            9
        );
        if mutation.contains("raw_text") {
            assert_eq!(
                connection
                    .query_row("select body from raw_text", [], |r| r.get::<_, String>(0))
                    .unwrap(),
                "private"
            );
        }
    }
}

#[test]
fn parallel_initializers_leave_a_complete_schema() {
    // Given: parallel initializers for the same database paths.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    let barrier = Arc::new(Barrier::new(2));
    // When / Then: both initialization attempts finish; the resulting database schema is complete.
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
    let storage = open_storage(&cache, &vectors).unwrap();
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.project_get_by_id())
            .unwrap(),
        0
    );
}

#[test]
fn independent_thread_connections_register_native_vec1() {
    // Given: each thread creates its own temporary databases.
    // When / Then: each thread uses native vector storage; each connection has the native vec1 extension.
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                let root = TempDir::new().unwrap();
                let cache = root.path().join("cache.db");
                let vectors = root.path().join("vectors.db");
                let storage = open_storage(&cache, &vectors).unwrap();
                assert_eq!(
                    storage
                        .cache
                        .with_read_tx(|| storage.cache.project_get_by_id())
                        .unwrap(),
                    0
                );
                let connection = Connection::open(&vectors).unwrap();
                let info: String = connection
                    .query_row("select vec1_info()", [], |r| r.get(0))
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
            "create table sqlitexprivate(body text); insert into sqlitexprivate values('retain');",
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
        "pragma user_version=2",
        "drop table query_error",
        "alter table meta add column raw_text text",
    ] {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        drop(open_storage(&cache, &vectors).unwrap());
        let connection = Connection::open(&vectors).unwrap();
        connection
            .execute(
                "insert into embed_failures values(?1,3,'safe')",
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
                .query_row("select attempts from embed_failures", [], |r| r
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
        .execute_batch("create table existing(value text); insert into existing values('retain');")
        .unwrap();
    drop(connection);
    // When / Then
    assert_error(&cache, &vectors, StorageErrorKind::InternalError);
    let connection = Connection::open(&cache).unwrap();
    assert_eq!(
        connection
            .query_row("select value from existing", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "retain"
    );
}

#[test]
fn malformed_stored_observations_return_safe_typed_errors() {
    // Given: malformed persisted storage observations.
    for (target, mutation) in [
        ("cache", "update project set revision=-1"),
        (
            "vectors",
            "insert into meta values(1,'private-invalid-hash')",
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
        let storage = open_storage(&cache, &vectors).unwrap();
        let Err(error) = storage
            .cache
            .with_read_tx(|| Ok::<_, StorageError>(()))
            .map_err(TransactionError::into_inner)
        else {
            panic!("invalid observation accepted");
        };
        // When / Then: typed reads decode the stored values; safe typed errors preserve internal causes.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert!(!error.to_string().contains("private"));
        assert!(std::error::Error::source(&error).is_some());
    }
}

#[test]
fn initialized_open_succeeds_but_cache_write_still_returns_busy() {
    // Given: initialized databases with a contending cache writer.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let vectors = root.path().join("vectors.db");
    drop(open_storage(&cache, &vectors).unwrap());
    let connection = Connection::open(&cache).unwrap();
    connection.execute_batch("begin immediate").unwrap();
    let storage = open_storage(&cache, &vectors).unwrap();
    // When / Then: storage opens and another write is attempted; opening succeeds while the contending write reports busy.
    assert_eq!(
        storage
            .cache
            .with_read_tx(|| storage.cache.project_get_by_id())
            .unwrap(),
        0
    );
    let Err(error) = storage
        .cache
        .with_write_tx(|| storage.cache.project_get_revision())
        .map_err(TransactionError::into_inner)
    else {
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
    connection.execute_batch("rollback").unwrap();
    assert!(open_storage(&cache, &vectors).is_ok());
}
