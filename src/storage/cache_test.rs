use super::CacheStorage;
use crate::service::{
    CacheDatabase, StorageError, StorageErrorKind, StorageRead, TransactionError,
};
use rusqlite::Connection;

#[test]
fn absent_status_storage_skips_observation_without_opening_a_database() {
    // Given: status found no existing cache database.
    let storage = CacheStorage::new_observed(None);
    // When: status requests a view of the absent publication.
    let result = storage.with_status_read_tx(|| -> Result<(), StorageError> {
        panic!("absent storage must skip observation")
    });
    // Then: absence is returned without constructing database connections.
    assert_eq!(result.unwrap(), None);
    assert!(
        storage
            .with_write_tx(|| -> Result<(), StorageError> { Ok(()) })
            .is_err()
    );
}

#[test]
fn status_skips_callback_until_a_revision_is_published() {
    // Given: a bootstrapped cache without a published revision.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text)").unwrap();
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("attach database ':memory:' as vec; create table vec.meta(id integer primary key,config_identity text)").unwrap();
    let storage = CacheStorage::new(reader, writer);
    // When: status runs before publication.
    let result = storage
        .with_status_read_tx(|| -> Result<(), StorageError> {
            panic!("unpublished status must skip its callback")
        })
        .unwrap();
    // Then: no result is produced; after publication status sees the pinned revision.
    assert_eq!(result, None);
    storage
        .with_write_tx(|| -> Result<(), StorageError> { Ok(()) })
        .unwrap();
    assert_eq!(
        storage
            .with_status_read_tx(|| storage.project_get_by_id())
            .unwrap(),
        Some(1)
    );
}

#[test]
fn caught_write_attempt_cannot_upgrade_a_read_scope() {
    // Given: a cache with an initialized read view and vector identity table.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text)").unwrap();
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("attach database ':memory:' as vec; create table vec.meta(id integer primary key,config_identity text)").unwrap();
    let storage = CacheStorage::new(reader, writer);
    // When: the caller catches an attempt to start a write inside the read.
    let result = storage.with_read_tx(|| -> Result<(), StorageError> {
        let attempt = storage.with_write_tx(|| -> Result<(), StorageError> {
            panic!("write callback must not execute")
        });
        assert!(matches!(attempt, Err(TransactionError::Storage(_))));
        Ok(())
    });
    // Then: the read is rollback-only, and a later write can start normally.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    storage
        .with_write_tx(|| -> Result<(), StorageError> { Ok(()) })
        .unwrap();
}

#[test]
fn caught_write_attempt_poisons_an_observed_read_scope() {
    // Given: an observation-only cache with a published revision.
    let reader = Connection::open_in_memory().unwrap();
    reader.execute_batch("create table project(id integer primary key, revision integer); insert into project values(1,1); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); attach database ':memory:' as vec; create table vec.meta(id integer primary key, config_identity text)").unwrap();
    let storage = CacheStorage::new_observed(Some(reader));
    // When: the read callback catches a rejected write attempt.
    let result = storage.with_read_tx(|| -> Result<(), StorageError> {
        let attempt = storage.with_write_tx(|| -> Result<(), StorageError> {
            panic!("observation must never invoke a writer")
        });
        assert!(matches!(attempt, Err(TransactionError::Storage(_))));
        Ok(())
    });
    // Then: the outer scope fails and a later observation can start normally.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(
        storage
            .with_read_tx(|| storage.project_get_by_id())
            .unwrap(),
        1
    );
}

#[test]
fn caught_sql_failure_restores_metadata_and_the_fts_schema() {
    use crate::{
        domain::{ElementKind, KnowledgeId, LineRange, Sha256},
        service::{KnowledgeMetadata, NodeMetadata},
    };
    use std::error::Error;
    // Given: the production cache schema and an independent observer.
    let root = tempfile::TempDir::new().unwrap();
    let databases = crate::storage::open_test_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let cache = &databases.cache_storage;
    let observer = Connection::open(root.path().join("cache.db")).unwrap();
    let id = KnowledgeId::parse("a.md").unwrap();
    let node = NodeMetadata::Knowledge(KnowledgeMetadata {
        id: id.clone(),
        uri: id.clone(),
        kind: ElementKind::Vertex,
        hash: Sha256::digest(b"alpha"),
        created_at: 7,
        source_snapshot: None,
        lines: LineRange::new(1, 1).unwrap(),
        search_hash: Sha256::digest(b"alpha"),
    });
    // When: the callback catches a real SQL failure after changing metadata and DDL.
    let result = cache.with_write_tx(|| -> Result<(), StorageError> {
        cache.nodes_upsert_by_id(&node).unwrap();
        cache
            .get_test_writer()
            .execute_batch("drop table text_index")
            .unwrap();
        let error = cache
            .replace_text(&id, "unique-payload Authorization:Bearer-secret")
            .unwrap_err();
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert!(error.source().is_some());
        for sentinel in ["unique-payload", "Authorization", "text_index"] {
            assert!(!error.to_string().contains(sentinel));
            assert!(!error.get_message().contains(sentinel));
        }
        Ok(())
    });
    // Then: the rollback restores the schema, metadata and publication revision.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    for query in [
        "select count(*) from nodes",
        "select count(*) from text_index",
        "select revision from project",
    ] {
        assert_eq!(
            observer
                .query_row(query, [], |row| row.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}

#[test]
fn publication_revision_is_provisional_until_the_outer_scope_commits() {
    // Given: an unpublished cache and an independent observer.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text)").unwrap();
    let observer = Connection::open(&path).unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: nested services obtain the pending publication revision.
    let revision = storage
        .with_write_tx(|| -> Result<u64, StorageError> {
            assert_eq!(
                observer
                    .query_row("select revision from project", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                0
            );
            assert_eq!(storage.project_get_revision()?, 1);
            storage
                .with_write_tx(|| storage.project_get_revision())
                .map_err(|error| match error {
                    TransactionError::Storage(error) | TransactionError::Operation(error) => error,
                })
        })
        .unwrap();
    // Then: exactly that revision is published once at the outer commit.
    assert_eq!(revision, 1);
    assert_eq!(
        observer
            .query_row("select revision from project", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert!(storage.project_get_revision().is_err());
}

#[test]
fn joined_read_inside_write_observes_uncommitted_revision() {
    // Given: a cache writer with an attached vector identity database.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); attach database ':memory:' as vec; create table vec.meta(id integer primary key,config_identity text)").unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: a nested read observes a change made by the parent write.
    let revision = storage
        .with_write_tx(|| -> Result<u64, StorageError> {
            storage
                .get_write_connection()?
                .execute_batch("update project set revision=7")
                .unwrap();
            storage
                .with_read_tx(|| storage.project_get_by_id())
                .map_err(|error| match error {
                    TransactionError::Storage(error) | TransactionError::Operation(error) => error,
                })
        })
        .unwrap();
    // Then: the nested scope reads through the writer's transaction.
    assert_eq!(revision, 7);
}

#[test]
fn read_scope_pins_vector_identity_before_callback() {
    // Given: cache and vector databases on a joined read connection.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let vector_path = root.path().join("vectors.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,1); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text)").unwrap();
    let first = crate::domain::Sha256::digest(b"first");
    let second = crate::domain::Sha256::digest(b"second");
    let vector_writer = Connection::open(&vector_path).unwrap();
    vector_writer.execute_batch("pragma journal_mode=wal; create table meta(id integer primary key,config_identity text)").unwrap();
    vector_writer
        .execute("insert into meta values(1,?1)", [first.to_hex()])
        .unwrap();
    let reader = Connection::open(&path).unwrap();
    reader
        .execute("attach database ?1 as vec", [vector_path.to_str().unwrap()])
        .unwrap();
    let storage = CacheStorage::new(reader, writer);
    // When: another connection changes identity before the closure's first vector query.
    let observed = storage
        .with_read_tx(|| -> Result<_, StorageError> {
            vector_writer
                .execute("update meta set config_identity=?1", [second.to_hex()])
                .unwrap();
            storage.meta_get_by_id()
        })
        .unwrap();
    // Then: the scope observes the identity captured before entering the closure.
    assert_eq!(observed, Some(first));
}

#[test]
fn caught_database_operation_error_prevents_commit() {
    // Given: a cache whose missing files table will cause a real SQL error.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); create table sample(value integer)").unwrap();
    let observer = Connection::open(&path).unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: a service catches a database-operation error and returns success.
    let result = storage.with_write_tx(|| -> Result<(), StorageError> {
        storage
            .get_write_connection()?
            .execute_batch("insert into sample values(7)")
            .unwrap();
        let path = crate::domain::RepoPath::parse("missing.md").unwrap();
        assert!(storage.files_delete_by_path(&path).is_err());
        Ok(())
    });
    // Then: the earlier write and the publication revision both roll back.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(
        observer
            .query_row("select count(*) from sample", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        observer
            .query_row("select revision from project", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn read_scope_pins_revision_while_another_connection_publishes() {
    // Given: a WAL cache whose revision can change through an independent writer.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text)").unwrap();
    let publisher = Connection::open(&path).unwrap();
    let reader = Connection::open(&path).unwrap();
    reader.execute_batch("attach database ':memory:' as vec; create table vec.meta(id integer primary key,config_identity text)").unwrap();
    let storage = CacheStorage::new(reader, writer);
    // When: a second publisher commits between two joined read scopes.
    let revision = storage
        .with_read_tx(|| -> Result<u64, StorageError> {
            assert_eq!(storage.project_get_by_id()?, 0);
            publisher
                .execute_batch("update project set revision=1")
                .unwrap();
            storage
                .with_read_tx(|| storage.project_get_by_id())
                .map_err(|error| match error {
                    TransactionError::Storage(error) | TransactionError::Operation(error) => error,
                })
        })
        .unwrap();
    // Then: both reads use the original view, and the next scope sees the publication.
    assert_eq!(revision, 0);
    assert_eq!(
        storage
            .with_read_tx(|| storage.project_get_by_id())
            .unwrap(),
        1
    );
    assert!(storage.project_get_by_id().is_err());
}

#[test]
fn caught_nested_panic_rolls_back_and_storage_can_be_reused() {
    // Given: a shared cache with an independent observer of committed writes.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); create table sample(value integer)").unwrap();
    let observer = Connection::open(&path).unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: the parent catches a panic after a nested service has written.
    let result = storage.with_write_tx(|| -> Result<(), StorageError> {
        let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = storage.with_write_tx(|| -> Result<(), StorageError> {
                storage
                    .get_write_connection()?
                    .execute_batch("insert into sample values(7)")
                    .unwrap();
                panic!("inner failure");
            });
        }));
        assert!(panic.is_err());
        Ok(())
    });
    // Then: the outer transaction cannot commit and a subsequent scope is independent.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(
        observer
            .query_row("select count(*) from sample", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    storage
        .with_write_tx(|| -> Result<(), StorageError> { Ok(()) })
        .unwrap();
    assert_eq!(
        observer
            .query_row("select revision from project", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn standalone_scope_commits_once() {
    // Given: a cache and an independent connection observing its committed state.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); create table sample(value integer)").unwrap();
    let observer = Connection::open(&path).unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: a service performs one standalone write scope.
    let result = storage.with_write_tx(|| -> Result<(), StorageError> {
        storage
            .get_write_connection()?
            .execute_batch("insert into sample values(7)")
            .unwrap();
        Ok(())
    });
    // Then: its changes and one publication revision are visible together.
    assert!(result.is_ok());
    assert_eq!(
        observer
            .query_row("select value from sample", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        7
    );
    assert_eq!(
        observer
            .query_row("select revision from project", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn joined_scope_does_not_commit_and_caught_error_prevents_commit() {
    // Given: two services share a real cache connection and an independent observer.
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("cache.db");
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("pragma journal_mode=wal; create table project(id integer primary key,revision integer); insert into project values(1,0); create table nodes(id integer primary key, uri text); create table edges(source_id integer, target_id integer, type text); create table unresolved_edges(source_id integer, target_uri text, type text); create table sample(value integer)").unwrap();
    let observer = Connection::open(&path).unwrap();
    let storage = CacheStorage::new(Connection::open(&path).unwrap(), writer);
    // When: B joins A, writes, then fails and A catches that error.
    let result = storage.with_write_tx(|| -> Result<(), StorageError> {
        let nested = storage.with_write_tx(|| -> Result<(), StorageError> {
            storage
                .get_write_connection()?
                .execute_batch("insert into sample values(7)")
                .unwrap();
            assert_eq!(
                observer
                    .query_row("select count(*) from sample", [], |r| r.get::<_, i64>(0))
                    .unwrap(),
                0
            );
            Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "inner failure",
            ))
        });
        assert!(matches!(nested, Err(TransactionError::Operation(_))));
        Ok(())
    });
    // Then: even a successful parent cannot commit the failed nested operation.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(
        observer
            .query_row("select count(*) from sample", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        observer
            .query_row("select revision from project", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

mod cancellation_tests {
    use crate::service::StorageError;
    use crate::service::TransactionError;

    use crate::storage::TestDatabases;
    use crate::{
        cli::ResponseWriter,
        domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256},
        process::SigintListener,
        service::{
            CacheDatabase, CommandError, CommandErrorKind, FileRow, KnowledgeMetadata,
            NodeMetadata, StorageRead, StoredNode,
        },
    };
    use rusqlite::Connection;
    use serde_json::Value;
    use std::{
        ffi::{c_int, c_void},
        fs::{self, File},
        io::{BufRead, BufReader, Read, Write},
        os::{
            fd::AsRawFd,
            unix::net::{UnixListener, UnixStream},
        },
        path::{Path, PathBuf},
        process::{Child, Command, ExitStatus, Stdio},
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    const WAIT: Duration = Duration::from_secs(20);
    const BEFORE_CHILD: &str = "storage::cache::tests::cancellation_tests::before_commit_child";
    const AFTER_CHILD: &str = "storage::cache::tests::cancellation_tests::after_commit_child";
    const SQL_CHILD: &str = "storage::cache::tests::cancellation_tests::sqlite_step_child";
    const CHILD_CACHE: &str = "LETOPIS_CANCELLATION_CACHE";
    const CHILD_VECTORS: &str = "LETOPIS_CANCELLATION_VECTORS";
    const CHILD_SOCKET: &str = "LETOPIS_CANCELLATION_SOCKET";
    const CHILD_OUTPUT: &str = "LETOPIS_CANCELLATION_OUTPUT";
    const CANCELLED_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";

    #[test]
    fn cancels_inside_sqlite_step() {
        // Given: a real staged cache unit and a progress callback that blocks inside SQL.
        let fixture = Fixture::new();
        publish(&fixture.paths, "a.md", "OldToken");
        let mut child = fixture.spawn(SQL_CHILD);
        let mut control = fixture.accept();
        assert_eq!(get_event(&mut control), "staged");
        assert_eq!(get_event(&mut control), "inside-sql");
        // When
        child.send_sigint();
        let status = child.wait_exit();
        // Then: the process died from handled cancellation before the transaction committed.
        assert_handled_exit(&status);
        assert_cancelled_file(&fixture.paths.output);
        assert_old_publication(&fixture.paths);
        publish(&fixture.paths, "later.md", "LaterToken");
        assert_later_publication(&fixture.paths, 2);
    }

    #[test]
    fn termination_before_commit_keeps_old_publication() {
        // Given: a real cache unit staged but held before commit until process death.
        let fixture = Fixture::new();
        publish(&fixture.paths, "a.md", "OldToken");
        let mut child = fixture.spawn(BEFORE_CHILD);
        let mut control = fixture.accept();
        assert_eq!(get_event(&mut control), "staged");
        // When
        child.send_sigint();
        let status = child.wait_exit();
        // Then: the old publication survives and a later publication still succeeds.
        assert_handled_exit(&status);
        assert_cancelled_file(&fixture.paths.output);
        assert_old_publication(&fixture.paths);
        publish(&fixture.paths, "later.md", "LaterToken");
        assert_later_publication(&fixture.paths, 2);
    }

    #[test]
    fn termination_after_commit_keeps_new_publication() {
        // Given: the child acknowledges only after its cache commit returned.
        let fixture = Fixture::new();
        publish(&fixture.paths, "a.md", "OldToken");
        let mut child = fixture.spawn(AFTER_CHILD);
        let mut control = fixture.accept();
        let acknowledgement = get_event(&mut control);
        assert_eq!(acknowledgement, "committed 2");
        // When
        child.send_sigint();
        let status = child.wait_exit();
        // Then: cancellation never rolls back the committed publication.
        assert_handled_exit(&status);
        assert_cancelled_file(&fixture.paths.output);
        assert_new_publication(&fixture.paths);
        publish(&fixture.paths, "later.md", "LaterToken");
        assert_later_publication(&fixture.paths, 3);
    }

    #[test]
    #[ignore = "private SQL-step child, invoked only by cancels_inside_sqlite_step"]
    fn sqlite_step_child() {
        let Some(paths) = Paths::from_environment() else {
            return;
        };
        // Given: a parent-owned publication and cancellation output.
        let storage = open_storage(&paths);
        let _listener = install_listener(&paths.output);
        let control = UnixStream::connect(&paths.socket).expect("control connection");
        write_event(control.try_clone().expect("barrier clone"), "staged");
        let barrier = register_sql_barrier(storage.cache_storage.writer.as_ref().unwrap(), control);
        // When: stage real changes at the selected cancellation boundary.
        let unit_result = storage
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
                stage(unit, "b.md", "NewToken");
                // Then: the armed callback holds the SQL step until process termination.
                barrier.arm();

                unit.project_get_revision()
            })
            .map_err(TransactionError::into_inner);
        let _ = unit_result;
    }

    #[test]
    #[ignore = "private before-commit child, invoked only by termination_before_commit_keeps_old_publication"]
    fn before_commit_child() {
        let Some(paths) = Paths::from_environment() else {
            return;
        };
        // Given: a parent-owned publication and cancellation output.
        let storage = open_storage(&paths);
        let _listener = install_listener(&paths.output);
        let mut control = UnixStream::connect(&paths.socket).expect("control connection");
        // When: stage real changes at the selected cancellation boundary.
        assert!(matches!(
            storage.cache_storage.with_write_tx(|| {
                let unit = &storage.cache_storage;
                stage(unit, "b.md", "NewToken");
                // Then: keep the transaction uncommitted until process termination.
                writeln!(control, "staged").expect("staged acknowledgement");
                hold_until_parent_closes(control);

                Err::<(), _>(())
            }),
            Err(TransactionError::Operation(()))
        ));
    }

    #[test]
    #[ignore = "private after-commit child, invoked only by termination_after_commit_keeps_new_publication"]
    fn after_commit_child() {
        let Some(paths) = Paths::from_environment() else {
            return;
        };
        // Given: a parent-owned publication and cancellation output.
        let storage = open_storage(&paths);
        let _listener = install_listener(&paths.output);
        let mut control = UnixStream::connect(&paths.socket).expect("control connection");
        // When: stage real changes at the selected cancellation boundary.
        let unit_result = storage
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
                stage(unit, "b.md", "NewToken");

                unit.project_get_revision()
            })
            .map_err(TransactionError::into_inner);
        let revision = unit_result.expect("committed publication");
        // Then: acknowledge only the completed commit before awaiting cancellation.
        writeln!(control, "committed {revision}").expect("commit acknowledgement");
        hold_until_parent_closes(control);
    }

    fn publish(paths: &Paths, id: &str, text: &str) -> u64 {
        let storage = open_storage(paths);
        let unit_result = storage
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
                stage(unit, id, text);

                unit.project_get_revision()
            })
            .map_err(TransactionError::into_inner);
        unit_result.expect("publication commit")
    }

    fn stage<C: CacheDatabase>(unit: &C, id: &str, text: &str) {
        let knowledge = KnowledgeId::parse(id).expect("knowledge id");
        let hash = Sha256::digest(text.as_bytes());
        unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
            uri: knowledge.clone(),
            id: knowledge.clone(),
            kind: ElementKind::Vertex,
            hash,
            created_at: 1,
            source_snapshot: None,
            lines: LineRange::new(1, 1).expect("line range"),
            search_hash: hash,
        }))
        .expect("node");
        unit.replace_text(&knowledge, text).expect("text");
        unit.files_upsert_by_path(&FileRow {
            path: RepoPath::parse(id).expect("file path"),
            size: u64::try_from(text.len()).expect("text size"),
            mtime_ns: 11,
            inode: 13,
            hash,
            checked_at_ns: 19,
            valid: true,
            error: None,
        })
        .expect("file row");
    }

    // The safe `rusqlite` progress handler is compiled only with its `hooks`
    // feature, which this product dependency does not enable. This private child
    // therefore registers the pinned FFI directly and leaks its context until the
    // process exits, so every callback sees live state, including the blocking one.
    fn register_sql_barrier(connection: &Connection, control: UnixStream) -> ProgressBarrier {
        let state = Box::into_raw(Box::new(ProgressState {
            armed: AtomicBool::new(false),
            control_mutex: Mutex::new(Some(control)),
        }));
        // SAFETY: `connection` is live and owns the handler registration. The boxed
        // context is intentionally leaked for this private child process, so its
        // address stays valid for every callback invocation, including the blocking
        // one that ends only when this process exits.
        unsafe {
            rusqlite::ffi::sqlite3_progress_handler(
                connection.handle(),
                1,
                Some(sqlite_progress),
                state.cast(),
            );
        }
        ProgressBarrier { state }
    }

    struct ProgressState {
        armed: AtomicBool,
        control_mutex: Mutex<Option<UnixStream>>,
    }

    struct ProgressBarrier {
        state: *mut ProgressState,
    }

    impl ProgressBarrier {
        fn arm(&self) {
            // SAFETY: the pointer is the leaked context registered above for this process.
            unsafe { (*self.state).armed.store(true, Ordering::SeqCst) };
        }
    }

    unsafe extern "C" fn sqlite_progress(context: *mut c_void) -> c_int {
        // SAFETY: SQLite passes the exact leaked context registered above; it lives
        // until this private child process exits even when the callback blocks.
        let state = unsafe { &*context.cast::<ProgressState>() };
        if !state.armed.load(Ordering::SeqCst) {
            return 0;
        }
        let Ok(mut control) = state.control_mutex.lock() else {
            return 0;
        };
        let Some(mut stream) = control.take() else {
            return 0;
        };
        let _ = writeln!(stream, "inside-sql");
        let mut byte = [0];
        let _ = stream.read(&mut byte);
        0
    }

    fn hold_until_parent_closes(mut control: UnixStream) {
        let mut byte = [0];
        let _ = control.read(&mut byte);
    }

    fn install_listener(output: &Path) -> SigintListener {
        let mut output = File::create(output).expect("cancellation output");
        let writer = ResponseWriter::default();
        SigintListener::signal_listener_install(Box::new(move || {
            let response = Err(CommandError::new(
                CommandErrorKind::Cancelled,
                "command cancelled",
            ));
            let _ = writer.response_write(&mut output, response);
        }))
        .expect("listener")
    }

    struct Publication {
        revision: u64,
        nodes: Vec<String>,
        files: Vec<String>,
        old: Vec<String>,
        new: Vec<String>,
        later: Vec<String>,
    }

    fn assert_old_publication(paths: &Paths) {
        let published = get_publication(paths);
        assert_eq!(published.revision, 1);
        assert_eq!(published.nodes, ["a.md"]);
        assert_eq!(published.files, ["a.md"]);
        assert_eq!(published.old, ["a.md"]);
        assert!(published.new.is_empty());
        assert!(published.later.is_empty());
    }

    fn assert_new_publication(paths: &Paths) {
        let published = get_publication(paths);
        assert_eq!(published.revision, 2);
        assert_eq!(published.nodes, ["a.md", "b.md"]);
        assert_eq!(published.files, ["a.md", "b.md"]);
        assert_eq!(published.old, ["a.md"]);
        assert_eq!(published.new, ["b.md"]);
        assert!(published.later.is_empty());
    }

    fn assert_later_publication(paths: &Paths, revision: u64) {
        let published = get_publication(paths);
        assert_eq!(published.revision, revision);
        assert!(published.nodes.iter().any(|id| id == "later.md"));
        assert_eq!(published.later, ["later.md"]);
    }

    fn get_publication(paths: &Paths) -> Publication {
        let storage = open_storage(paths);
        assert_integrity(&storage);
        storage
            .cache_storage
            .with_read_tx(|| {
                Ok::<_, StorageError>({
                    let read = &storage.cache_storage;
                    let revision = read.project_get_by_id().unwrap();
                    let nodes = read
                        .nodes_get()
                        .expect("nodes")
                        .iter()
                        .map(get_node_id)
                        .collect();
                    let files = read
                        .files_get()
                        .expect("files")
                        .iter()
                        .map(|file| file.path.as_str().to_owned())
                        .collect();
                    Publication {
                        revision,
                        nodes,
                        files,
                        old: get_text_matches_by_expression(read, "OldToken"),
                        new: get_text_matches_by_expression(read, "NewToken"),
                        later: get_text_matches_by_expression(read, "LaterToken"),
                    }
                })
            })
            .map_err(TransactionError::into_inner)
            .expect("read transaction")
    }

    fn get_node_id(node: &StoredNode) -> String {
        match &node.metadata {
            NodeMetadata::Knowledge(knowledge) => knowledge.id.as_str().to_owned(),
            NodeMetadata::Source(source) => source.uri.as_str().to_owned(),
        }
    }

    fn get_text_matches_by_expression(read: &dyn StorageRead, expression: &str) -> Vec<String> {
        read.text_index_get_by_expression(
            expression,
            std::num::NonZeroUsize::new(100).unwrap(),
        )
            .expect("FTS expression")
            .into_iter()
            .map(|found| found.id.as_str().to_owned())
            .collect()
    }

    fn assert_integrity(storage: &TestDatabases) {
        for connection in [
            storage.cache_storage.writer.as_ref().unwrap(),
            &storage.vector_storage.connection,
        ] {
            let result: String = connection
                .query_row("pragma quick_check", [], |row| row.get(0))
                .expect("quick_check");
            assert_eq!(result, "ok");
        }
    }

    fn open_storage(paths: &Paths) -> TestDatabases {
        crate::storage::open_test_storage(&paths.cache, &paths.vectors).expect("storage")
    }

    fn assert_handled_exit(status: &ExitStatus) {
        assert_eq!(status.code(), Some(1), "unhandled signal exit: {status:?}");
    }

    fn assert_cancelled_file(output: &Path) {
        let bytes = fs::read(output).expect("cancellation output");
        assert_eq!(bytes, CANCELLED_ENVELOPE);
        let response: Value = serde_json::from_slice(&bytes).expect("one JSON value");
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "cancelled");
    }

    struct Paths {
        cache: PathBuf,
        vectors: PathBuf,
        socket: PathBuf,
        output: PathBuf,
    }

    impl Paths {
        fn from_environment() -> Option<Self> {
            Some(Self {
                cache: PathBuf::from(std::env::var_os(CHILD_CACHE)?),
                vectors: PathBuf::from(std::env::var_os(CHILD_VECTORS)?),
                socket: PathBuf::from(std::env::var_os(CHILD_SOCKET)?),
                output: PathBuf::from(std::env::var_os(CHILD_OUTPUT)?),
            })
        }
    }

    struct Fixture {
        _root: TempDir,
        _socket: TempDir,
        listener: UnixListener,
        paths: Paths,
    }

    impl Fixture {
        fn new() -> Self {
            let root = tempfile::tempdir().expect("storage root");
            let socket = tempfile::Builder::new()
                .prefix("tk29-storage-")
                .tempdir_in("/tmp")
                .expect("socket directory");
            let address = socket.path().join("control.sock");
            let listener = UnixListener::bind(&address).expect("control listener");
            Self {
                paths: Paths {
                    cache: root.path().join("cache.db"),
                    vectors: root.path().join("vectors.db"),
                    socket: address,
                    output: root.path().join("cancelled.json"),
                },
                _root: root,
                _socket: socket,
                listener,
            }
        }

        fn spawn(&self, child: &str) -> OwnedChild {
            let process = Command::new(std::env::current_exe().expect("test executable"))
                .args(["--exact", child, "--ignored", "--nocapture"])
                .env(CHILD_CACHE, &self.paths.cache)
                .env(CHILD_VECTORS, &self.paths.vectors)
                .env(CHILD_SOCKET, &self.paths.socket)
                .env(CHILD_OUTPUT, &self.paths.output)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("child spawn");
            OwnedChild(Some(process))
        }

        fn accept(&self) -> BufReader<UnixStream> {
            wait_readable(self.listener.as_raw_fd());
            let (stream, _) = self.listener.accept().expect("control connection");
            stream
                .set_read_timeout(Some(WAIT))
                .expect("control read timeout");
            stream
                .set_write_timeout(Some(WAIT))
                .expect("control write timeout");
            BufReader::new(stream)
        }
    }

    fn get_event(control: &mut BufReader<UnixStream>) -> String {
        let mut line = String::new();
        assert!(
            control.read_line(&mut line).expect("control event") > 0,
            "child closed before acknowledgement"
        );
        line.trim().to_owned()
    }

    fn write_event(mut control: UnixStream, event: &str) {
        writeln!(control, "{event}").expect("control event");
    }

    fn wait_readable(fd: i32) {
        let mut poll = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `poll` points to one initialized entry for the live listener.
        let ready = unsafe {
            libc::poll(
                &raw mut poll,
                1,
                i32::try_from(WAIT.as_millis()).expect("poll timeout"),
            )
        };
        assert_eq!(ready, 1, "child did not connect");
    }

    struct OwnedChild(Option<Child>);

    impl OwnedChild {
        fn send_sigint(&mut self) {
            let child = self.0.as_mut().expect("live child");
            assert!(
                child.try_wait().expect("child status").is_none(),
                "child already exited"
            );
            let pid = i32::try_from(child.id()).expect("child pid");
            // SAFETY: the unreaped child is owned here, so its PID cannot be reused
            // before collection; signal only that process, not its group.
            assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
        }

        fn wait_exit(&mut self) -> ExitStatus {
            let child = self.0.as_mut().expect("live child");
            let deadline = Instant::now() + WAIT;
            loop {
                if let Some(status) = child.try_wait().expect("child status") {
                    return status;
                }
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    panic!("child did not exit within the watchdog");
                }
                thread::sleep(Duration::from_millis(5));
            }
        }
    }

    impl Drop for OwnedChild {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = child.wait();
            }
        }
    }
}

mod graph_tests {
    use super::super::decode_node;
    use crate::service::StorageErrorKind;
    use rusqlite::{Connection, params};

    #[test]
    fn rejects_each_inconsistent_persisted_source_field() {
        let connection = Connection::open_in_memory().unwrap();
        for (id, snapshot, search) in [
            ("different.rs", None, None),
            ("code.rs", Some("unexpected snapshot"), None),
            ("code.rs", None, Some("unexpected search hash")),
        ] {
            // Given: each row violates exactly one source-specific invariant.
            let mut statement = connection
                .prepare("select 1, ?1, 'source', 'code.rs', null, 0, ?2, null, null, ?3")
                .unwrap();
            let mut rows = statement.query(params![id, snapshot, search]).unwrap();
            let row = rows.next().unwrap().unwrap();
            // When: persisted source metadata is decoded.
            let error = decode_node(row).unwrap_err();
            // Then: each inconsistent source field is rejected.
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        }
    }
}

mod file_tests {
    use super::super::{CacheStorage, transaction};
    use crate::service::StorageErrorKind;
    use crate::service::StorageRead;
    use rusqlite::Connection;

    #[test]
    fn rejects_an_out_of_range_persisted_valid_flag() {
        // Given: malformed persisted data can pair valid=2 with an error.
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "create table files(path, size, mtime_ns, inode, hash, checked_at_ns, valid, error);
             insert into files values('a.md', 1, 0, 1,
               '0000000000000000000000000000000000000000000000000000000000000000',
               0, 2, 'invalid file');",
        ).unwrap();
        let storage = CacheStorage::new_observed(Some(connection));
        // When: malformed observations are retrieved in an active read scope.
        let error = transaction::run_read(storage.get_test_reader(), &storage.state, || {
            storage.files_get()
        })
        .unwrap_err()
        .into_inner();
        // Then: an invalid boolean representation cannot become a valid row.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    }
}

mod neighbor_tests {
    use super::super::{
        CacheStorage, edges_get_by_row_id_and_type, nodes_get_by_source_snapshot, transaction,
    };
    use crate::service::StorageRead;
    use crate::{
        domain::{EdgeType, TreeOid},
        service::{
            ElementAddress, GraphNodeKey, NeighborCursor, NeighborDirection, NeighborPageRequest,
            StorageErrorKind, TraversalMode,
        },
        storage::{query::build_query, schema::bootstrap_cache},
    };
    use rusqlite::{Connection, params_from_iter};
    use sea_query::SelectStatement;
    use std::{collections::BTreeSet, num::NonZeroUsize};

    #[derive(Debug)]
    struct Instruction {
        address: i64,
        opcode: String,
        p1: i64,
        p2: i64,
        p4: Option<String>,
    }

    #[test]
    fn pages_retain_limit_bounded_top_k_for_every_query_variant() {
        // Given: 4096 neighbors in reverse numeric/URI order and unresolved declarations.
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        create_high_degree_graph(&db);
        let filters = [
            BTreeSet::new(),
            BTreeSet::from([EdgeType::parse("supports").unwrap()]),
            BTreeSet::from([
                EdgeType::parse("includes").unwrap(),
                EdgeType::parse("supports").unwrap(),
            ]),
        ];
        let cases = [
            (
                TraversalMode::Relations,
                [Some("refutes"), Some("supports"), Some("supports")],
            ),
            (
                TraversalMode::Containment,
                [Some("includes"), None, Some("includes")],
            ),
            (
                TraversalMode::Both,
                [Some("includes"), Some("supports"), Some("includes")],
            ),
        ];
        // When / Then: all 72 directional/mode/filter/cursor/limit variants use bounded native bytecode and URI pages.
        println!(
            "bundled SQLite {}: high_degree=4096, variants=72",
            rusqlite::version()
        );
        for direction in [NeighborDirection::Outgoing, NeighborDirection::Incoming] {
            for (mode, first_types) in cases {
                for (edge_types, first_type) in filters.iter().zip(first_types) {
                    assert_bounded_query_variants(&db, direction, mode, edge_types, first_type);
                }
            }
        }
    }

    #[test]
    fn narrow_basis_query_deduplicates_non_null_roots_and_rejects_bad_oids() {
        // Given: duplicate and null snapshot roots share the current cache.
        let mut db = Connection::open_in_memory().unwrap();
        bootstrap_cache(&mut db).unwrap();
        db.execute_batch("insert into nodes(uri,kind,hash,created_at,source_snapshot,line_start,line_end,search_hash) values
            ('a.md','vertex','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused'),
            ('b.md','vertex','unused',1,null,1,1,'unused'),
            ('c.md','vertex','unused',1,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa',1,1,'unused'),
            ('d.md','vertex','unused',1,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb',1,1,'unused');").unwrap();
        let storage = CacheStorage::new_observed(Some(db));
        let db = storage.get_test_reader();
        // When: distinct snapshot roots are retrieved in an active read scope.
        let roots = transaction::run_read(db, &storage.state, || {
            storage.nodes_get_by_source_snapshot()
        })
        .unwrap();
        // Then: only the two distinct non-null roots are returned in sorted order.
        assert_eq!(
            roots,
            BTreeSet::from([
                TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap(),
                TreeOid::parse("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb").unwrap()
            ])
        );
        let plan = explain_query(db, &nodes_get_by_source_snapshot());
        println!("basis: {plan:?}");
        assert!(
            plan.iter()
                .any(|line| line.contains("nodes_source_snapshot")),
            "{plan:?}"
        );
        assert!(
            !plan.iter().any(|line| line.contains("TEMP B-TREE")),
            "{plan:?}"
        );
        // Given: introduce a malformed snapshot identity.
        db.execute(
            "update nodes set source_snapshot='malformed' where uri='b.md'",
            [],
        )
        .unwrap();
        // When / Then: malformed roots are rejected on the next read.
        assert_eq!(
            transaction::run_read(db, &storage.state, || storage
                .nodes_get_by_source_snapshot())
            .unwrap_err()
            .into_inner()
            .get_kind(),
            StorageErrorKind::InternalError
        );
    }

    fn create_high_degree_graph(db: &Connection) {
        db.execute_batch("insert into nodes(id,uri,kind,hash,created_at,line_start,line_end,search_hash)
            values(10000,'root.md','vertex','unused',1,1,1,'unused');
            with recursive ordinal(n) as (values(0) union all select n+1 from ordinal where n<4095)
            insert into nodes(id,uri,kind,hash,created_at,line_start,line_end,search_hash)
            select 4096-n,printf('n%04d.md',n),'vertex','unused',1,1,1,'unused' from ordinal;
            insert into edges select 10000,id,'supports' from nodes where id<>10000;
            insert into edges select 10000,id,'refutes' from nodes where id<>10000;
            insert into edges select 10000,id,'includes' from nodes where id<>10000;
            insert into edges select id,10000,'supports' from nodes where id<>10000;
            insert into edges select id,10000,'refutes' from nodes where id<>10000;
            insert into edges select id,10000,'includes' from nodes where id<>10000;
            insert into unresolved_edges values(10000,'a-missing.md','includes'),(10000,'a-missing.md','supports');").unwrap();
    }

    fn assert_bounded_query_variants(
        db: &Connection,
        direction: NeighborDirection,
        mode: TraversalMode,
        edge_types: &BTreeSet<EdgeType>,
        first_type: Option<&str>,
    ) {
        let cursor = NeighborCursor {
            edge_type: EdgeType::parse("supports").unwrap(),
            neighbor: ElementAddress::parse("n2047.md").unwrap(),
        };
        for after in [None, Some(&cursor)] {
            for limit in [1, 32] {
                let request = NeighborPageRequest {
                    node: GraphNodeKey::new(10000),
                    direction,
                    mode,
                    edge_types,
                    after,
                    limit: NonZeroUsize::new(limit).unwrap(),
                };
                let query = edges_get_by_row_id_and_type(&request);
                let plan = explain_query(db, &query);
                assert_directional_index(&plan, direction);
                assert_bounded_vm(db, &query, limit, edge_types.len());
                let expected_type = match (after, mode) {
                    (Some(_), TraversalMode::Containment) => None,
                    (Some(_), _) => Some("supports"),
                    (None, _) => first_type,
                };
                let first = if after.is_some() { 2048 } else { 0 };
                let expected: Vec<_> = expected_type
                    .into_iter()
                    .flat_map(|kind| {
                        (first..first + limit)
                            .map(move |n| (kind.to_owned(), format!("n{n:04}.md")))
                    })
                    .collect();
                let actual: Vec<_> = get_neighbor_page(db, &request)
                    .into_iter()
                    .map(|row| {
                        (
                            row.edge.get_edge_type().as_str().to_owned(),
                            row.node.element.as_str().to_owned(),
                        )
                    })
                    .collect();
                assert_eq!(actual, expected);
                println!(
                    "{direction:?}/{mode:?}/{edge_types:?}/cursor={}/limit={limit}: {plan:?}; top-K <= {limit}",
                    after.is_some()
                );
            }
        }
    }

    fn assert_directional_index(plan: &[String], direction: NeighborDirection) {
        let index = match direction {
            NeighborDirection::Outgoing => "edges_outgoing",
            NeighborDirection::Incoming => "edges_incoming",
        };
        assert!(
            plan.iter()
                .any(|line| line.contains("SEARCH edges USING") && line.contains(index)),
            "{plan:?}"
        );
        assert!(
            !plan
                .iter()
                .any(|line| line.contains("AUTOMATIC") || line.contains("MATERIALIZE")),
            "{plan:?}"
        );
    }

    fn assert_bounded_vm(
        db: &Connection,
        query: &SelectStatement,
        limit: usize,
        filter_count: usize,
    ) {
        let program = get_query_program(db, query);
        assert!(
            !program.iter().any(|op| op.opcode.starts_with("Sorter")
                || matches!(
                    op.opcode.as_str(),
                    "OpenAutoindex"
                        | "OffsetLimit"
                        | "RowSetAdd"
                        | "Insert"
                        | "OpenDup"
                        | "OpenPseudo"
                )),
            "{program:#?}"
        );
        for open in program.iter().filter(|op| op.opcode == "OpenEphemeral") {
            assert!(open.p4.as_deref().is_some_and(|key| key.starts_with("k(")));
            let inserts: Vec<_> = program
                .iter()
                .filter(|op| op.opcode == "IdxInsert" && op.p1 == open.p1)
                .collect();
            if program
                .iter()
                .any(|op| op.opcode == "Sequence" && op.p1 == open.p1)
            {
                assert_eq!(inserts.len(), 1, "{program:#?}");
                assert_top_k_control_flow(&program, inserts[0], limit);
            } else {
                assert_constant_filter(&program, open, &inserts, filter_count);
            }
        }
        assert!(
            program.iter().any(|op| op.opcode == "IfNotZero"),
            "{program:#?}"
        );
        if limit == 32 && filter_count == 0 {
            println!(
                "native VM: {:?}",
                program
                    .iter()
                    .map(|op| (
                        op.address,
                        op.opcode.as_str(),
                        op.p1,
                        op.p2,
                        op.p4.as_deref()
                    ))
                    .collect::<Vec<_>>()
            );
        }
    }

    fn get_query_program(db: &Connection, query: &SelectStatement) -> Vec<Instruction> {
        let (sql, values) = build_query(query).unwrap();
        db.prepare(&format!("explain {sql}"))
            .unwrap()
            .query_map(params_from_iter(&values), |row| {
                Ok(Instruction {
                    address: row.get(0)?,
                    opcode: row.get(1)?,
                    p1: row.get(2)?,
                    p2: row.get(3)?,
                    p4: row.get(5)?,
                })
            })
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn assert_top_k_control_flow(program: &[Instruction], insert: &Instruction, limit: usize) {
        let gate = program
            .iter()
            .rfind(|op| op.opcode == "IfNotZero" && op.address < insert.address)
            .unwrap_or_else(|| panic!("{program:#?}"));
        let start = usize::try_from(gate.address).unwrap();
        let sequence = &program[start..=start + 3];
        // Pinned SQLite pushOntoSorter: consume LIMIT slots, then discard or delete the largest before inserting.
        assert_eq!(
            sequence
                .iter()
                .map(|op| op.opcode.as_str())
                .collect::<Vec<_>>(),
            ["IfNotZero", "Last", "IdxLE", "Delete"]
        );
        assert_eq!(gate.p2, gate.address + 4);
        for op in &sequence[1..=3] {
            assert_eq!(op.p1, insert.p1);
        }
        assert!(sequence[2].p2 > insert.address);
        assert!(
            program[start + 4..usize::try_from(insert.address).unwrap()]
                .iter()
                .all(|op| matches!(
                    op.opcode.as_str(),
                    "Column" | "Rowid" | "IdxRowid" | "MakeRecord"
                )),
            "{program:#?}"
        );
        let limit_initialization = program
            .iter()
            .find(|op| op.opcode == "Integer" && op.p2 == gate.p1)
            .unwrap_or_else(|| panic!("{program:#?}"));
        assert_eq!(limit_initialization.p1, i64::try_from(limit).unwrap());
    }

    fn assert_constant_filter(
        program: &[Instruction],
        open: &Instruction,
        inserts: &[&Instruction],
        filter_count: usize,
    ) {
        assert_eq!(inserts.len(), filter_count);
        assert!(filter_count <= 2);
        assert_eq!(open.p2, 1);
        let once = &program[usize::try_from(open.address - 1).unwrap()];
        assert_eq!(once.opcode, "Once");
        assert!(inserts.iter().all(|op| op.address < once.p2));
        let end = usize::try_from(inserts.last().unwrap().address).unwrap();
        assert!(
            program[usize::try_from(open.address).unwrap()..=end]
                .iter()
                .all(|op| matches!(
                    op.opcode.as_str(),
                    "OpenEphemeral" | "Variable" | "MakeRecord" | "IdxInsert"
                ))
        );
        for insert in inserts {
            let index = usize::try_from(insert.address).unwrap();
            assert_eq!(program[index - 2].opcode, "Variable");
            assert_eq!(program[index - 1].opcode, "MakeRecord");
        }
    }

    fn get_neighbor_page(
        db: &Connection,
        request: &NeighborPageRequest<'_>,
    ) -> Vec<super::super::GraphNeighbor> {
        let (sql, values) = build_query(&edges_get_by_row_id_and_type(request)).unwrap();
        let mut statement = db.prepare(&sql).unwrap();
        let mut rows = statement.query(params_from_iter(values)).unwrap();
        let mut neighbors = Vec::new();
        while let Some(row) = rows.next().unwrap() {
            neighbors.push(super::super::decode_neighbor(row).unwrap());
        }
        neighbors
    }

    fn explain_query(db: &Connection, query: &SelectStatement) -> Vec<String> {
        let (sql, values) = build_query(query).unwrap();
        assert!(!sql.contains("OFFSET"));
        assert!(!sql.contains("\"hash\""));
        db.prepare(&format!("explain query plan {sql}"))
            .unwrap()
            .query_map(params_from_iter(values), |row| row.get(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    }
}
