use super::super::utils::set_rollback_denial;
use super::{State, run};
use crate::service::{StorageError, TransactionError};
use rusqlite::Connection;
use std::{cell::Cell, thread, time::Duration};
use tempfile::TempDir;

#[test]
fn write_scope_waits_for_a_concurrent_writer_to_release() {
    // Given: a WAL database whose only write lock is held by another connection.
    let root = TempDir::new().unwrap();
    let path = root.path().join("database");
    let owner = Connection::open(&path).unwrap();
    let journal: String = owner
        .query_row("pragma journal_mode=wal", [], |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "wal");
    owner.busy_timeout(Duration::ZERO).unwrap();
    owner.execute_batch("begin immediate").unwrap();
    let waiter = Connection::open(&path).unwrap();
    waiter.busy_timeout(Duration::ZERO).unwrap();
    let state = Cell::new(State::Idle);
    let releaser = thread::spawn(move || {
        thread::sleep(Duration::from_millis(100));
        owner.execute_batch("commit").unwrap();
    });
    // When: a write scope starts while the lock is still held.
    let result = run(&waiter, &state, || Ok::<_, StorageError>(7), || Ok(()));
    // Then: acquisition retries and the operation completes after the release.
    assert_eq!(result.unwrap(), 7);
    releaser.join().unwrap();
    assert_eq!(state.get(), State::Idle);
}

#[test]
fn failed_commit_rolls_back_and_allows_reuse() {
    // Given: a deferred foreign key checked by SQLite at commit.
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("pragma foreign_keys=on; create table parent(id integer primary key); create table child(parent_id integer references parent(id) deferrable initially deferred)").unwrap();
    let state = Cell::new(State::Idle);
    // When: the callback succeeds but the real commit fails.
    let result = run(
        &connection,
        &state,
        || -> Result<(), StorageError> {
            connection
                .execute("insert into child values(7)", [])
                .unwrap();
            Ok(())
        },
        || Ok(()),
    );
    // Then: no row persists and the connection can serve another transaction.
    assert!(matches!(result, Err(TransactionError::Storage(_))));
    assert_eq!(state.get(), State::Idle);
    assert_eq!(
        connection
            .query_row("select count(*) from child", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    run(
        &connection,
        &state,
        || -> Result<(), StorageError> { Ok(()) },
        || Ok(()),
    )
    .unwrap();
}

#[test]
fn failed_rollback_disables_the_connection() {
    // Given: SQLite itself refuses rollback through its authorizer.
    let connection = Connection::open_in_memory().unwrap();
    set_rollback_denial(&connection, true);
    let state = Cell::new(State::Idle);
    // When: an operation fails and neither explicit nor guard cleanup can roll back.
    let result = run(
        &connection,
        &state,
        || Err::<(), _>("original error"),
        || Ok(()),
    );
    // Then: the original error survives and later callbacks cannot execute.
    assert!(matches!(
        result,
        Err(TransactionError::Operation("original error"))
    ));
    assert_eq!(state.get(), State::Broken);
    let retry = run(
        &connection,
        &state,
        || -> Result<(), StorageError> { panic!("broken connection must not execute callbacks") },
        || Ok(()),
    );
    assert!(matches!(retry, Err(TransactionError::Storage(_))));
    set_rollback_denial(&connection, false);
    connection.execute_batch("rollback").unwrap();
}
