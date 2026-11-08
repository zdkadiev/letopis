use super::connection::wait_for_storage_contention;
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, OptionalExtension, Transaction, TransactionBehavior};

/// Prepare the exact flat cosine representation for writable databases.
///
/// Rebuilds the index in one immediate transaction when the stored model differs from the flat
/// model this module builds. A matching model is left untouched, and read-only and query-only
/// handles are left unchanged.
///
/// # Arguments
///
/// * `connection` — the database connection whose vector index is prepared.
///
/// # Errors
/// Returns a storage error when the bounded wait for storage contention expires or the atomic
/// rebuild is unsuccessful.
pub(super) fn vector_index_prepare(connection: &mut Connection) -> StorageResult<()> {
    let is_flat = wait_for_storage_contention(|| {
        let read = connection.transaction()?;
        let is_flat = is_flat_index(&read)?;
        read.commit()?;
        Ok(is_flat)
    })?;
    let is_query_only: bool =
        connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    if is_flat || is_query_only || connection.is_readonly("main")? {
        return Ok(());
    }
    wait_for_storage_contention(|| {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        // Recheck after acquiring the writer; another opener may already have rebuilt it.
        if !is_flat_index(&transaction)? {
            transaction.execute(
                "insert into vector_index(cmd,arg) values('rebuild','{index:\"flat\",distance:\"cos\"}')",
                [],
            )?;
            if !is_flat_index(&transaction)? {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector model",
                ));
            }
        }
        transaction.commit().map_err(StorageError::from)
    })
}

/// Returns whether the stored native model equals the flat model built by this pinned native
/// module.
///
/// Reads the stored model and changes nothing.
///
/// # Arguments
///
/// * `connection` — the database connection whose stored vector model is read.
///
/// # Errors
/// Returns a storage error when the reference schema or either model query fails.
fn is_flat_index(connection: &Connection) -> StorageResult<bool> {
    // Creating the schema in memory makes the native module write its own model header, which
    // yields the exact bytes to compare against.
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(include_str!("vectors.sql"))?;
    let reference_model: Vec<u8> =
        reference.query_row("select val from vector_index_model where id=1", [], |row| {
            row.get(0)
        })?;
    let stored_model: Option<Vec<u8>> = connection
        .query_row("select val from vector_index_model where id=1", [], |row| {
            row.get(0)
        })
        .optional()?;
    Ok(stored_model.as_deref() == Some(reference_model.as_slice()))
}

#[cfg(test)]
#[path = "vector_index_test.rs"]
mod tests;
