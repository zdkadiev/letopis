use super::connection::{map_sqlite, wait_for_storage_contention};
use crate::service::{
    StorageError, StorageErrorKind, StorageResult, TransactionError, TransactionRequirement,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use std::{
    cell::Cell,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum State {
    Idle,
    Reading,
    Writing,
    Failed,
    Broken,
}

/// Enforce a use case's entry requirement and poison an incompatible active scope.
///
/// # Errors
/// Rejects incompatible, failed or disabled transaction state.
pub(super) fn verify_scope(
    state: &Cell<State>,
    requirement: TransactionRequirement,
) -> StorageResult<()> {
    match (state.get(), requirement) {
        (State::Idle, _) | (State::Reading, TransactionRequirement::ReadOnly) => Ok(()),
        (State::Reading | State::Writing, _) => {
            state.set(State::Failed);
            Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "incompatible active storage scope",
            ))
        }
        (State::Failed | State::Broken, _) => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "storage transaction is unavailable",
        )),
    }
}

pub(super) fn run<T, E, F, C>(
    connection: &Connection,
    state: &Cell<State>,
    operation: F,
    before_commit: C,
) -> Result<T, TransactionError<E>>
where
    F: FnOnce() -> Result<T, E>,
    C: FnOnce() -> Result<(), StorageError>,
{
    run_with_behavior(
        connection,
        state,
        operation,
        before_commit,
        TransactionBehavior::Immediate,
    )
}

pub(super) fn run_read<T, E, F>(
    connection: &Connection,
    state: &Cell<State>,
    operation: F,
) -> Result<T, TransactionError<E>>
where
    F: FnOnce() -> Result<T, E>,
{
    run_with_behavior(
        connection,
        state,
        operation,
        || Ok(()),
        TransactionBehavior::Deferred,
    )
}

fn run_with_behavior<T, E, F, C>(
    connection: &Connection,
    state: &Cell<State>,
    operation: F,
    before_commit: C,
    behavior: TransactionBehavior,
) -> Result<T, TransactionError<E>>
where
    F: FnOnce() -> Result<T, E>,
    C: FnOnce() -> Result<(), StorageError>,
{
    match state.get() {
        State::Idle => {}
        State::Reading if matches!(behavior, TransactionBehavior::Deferred) => {
            return run_joined(state, operation);
        }
        State::Reading => {
            state.set(State::Failed);
            return Err(TransactionError::Storage(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cannot upgrade a read transaction",
            )));
        }
        State::Writing => return run_joined(state, operation),
        State::Failed | State::Broken => {
            return Err(TransactionError::Storage(StorageError::new(
                StorageErrorKind::InternalError,
                "storage transaction is unavailable",
            )));
        }
    }
    let transaction = wait_for_storage_contention(|| {
        Transaction::new_unchecked(connection, behavior).map_err(map_sqlite)
    })
    .map_err(TransactionError::Storage)?;
    state.set(if matches!(behavior, TransactionBehavior::Deferred) {
        State::Reading
    } else {
        State::Writing
    });
    let result = catch_unwind(AssertUnwindSafe(operation));
    let failed =
        !matches!(state.get(), State::Reading | State::Writing) || !matches!(&result, Ok(Ok(_)));
    let finalize = finish(transaction, failed, before_commit);
    state.set(if connection.is_autocommit() {
        State::Idle
    } else {
        State::Broken
    });
    match result {
        Err(panic) => resume_unwind(panic),
        Ok(Err(error)) => Err(TransactionError::Operation(error)),
        Ok(Ok(value)) => {
            finalize.map_err(TransactionError::Storage)?;
            if failed {
                return Err(TransactionError::Storage(StorageError::new(
                    StorageErrorKind::InternalError,
                    "storage transaction failed",
                )));
            }
            Ok(value)
        }
    }
}

fn run_joined<T, E, F>(state: &Cell<State>, operation: F) -> Result<T, TransactionError<E>>
where
    F: FnOnce() -> Result<T, E>,
{
    let result = catch_unwind(AssertUnwindSafe(operation));
    if !matches!(&result, Ok(Ok(_))) {
        state.set(State::Failed);
    }
    match result {
        Ok(result) => result.map_err(TransactionError::Operation),
        Err(panic) => resume_unwind(panic),
    }
}

fn finish<C>(
    transaction: Transaction<'_>,
    failed: bool,
    before_commit: C,
) -> Result<(), StorageError>
where
    C: FnOnce() -> Result<(), StorageError>,
{
    if failed {
        return transaction.rollback().map_err(map_sqlite);
    }
    if let Err(error) = before_commit() {
        let _ = transaction.rollback();
        return Err(error);
    }
    transaction.commit().map_err(map_sqlite)
}

#[cfg(test)]
mod tests {
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
            || -> Result<(), StorageError> {
                panic!("broken connection must not execute callbacks")
            },
            || Ok(()),
        );
        assert!(matches!(retry, Err(TransactionError::Storage(_))));
        set_rollback_denial(&connection, false);
        connection.execute_batch("rollback").unwrap();
    }
}
