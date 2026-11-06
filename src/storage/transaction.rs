use super::connection::wait_for_storage_contention;
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

/// Verifies a use case's entry requirement and marks an incompatible active scope as failed.
///
/// # Arguments
///
/// * `state` — the current transaction state, which is marked failed when the requirement is
///   incompatible.
/// * `requirement` — the access level the use case needs from the active scope.
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
        Transaction::new_unchecked(connection, behavior).map_err(StorageError::from)
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
    is_failed: bool,
    before_commit: C,
) -> Result<(), StorageError>
where
    C: FnOnce() -> Result<(), StorageError>,
{
    if is_failed {
        return transaction.rollback().map_err(StorageError::from);
    }
    if let Err(error) = before_commit() {
        let _ = transaction.rollback();
        return Err(error);
    }
    transaction.commit().map_err(StorageError::from)
}

#[cfg(test)]
#[path = "transaction_test.rs"]
mod tests;
