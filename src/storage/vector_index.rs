use super::{
    connection::wait_for_storage_contention,
    vector::{VectorSchema, decode_vector, vector_index_config_get_by_id},
};
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, Transaction, TransactionBehavior, types::ValueRef};

// Pinned native MODEL version key; rebuild increments this integer before reloading its model.
const NATIVE_MODEL_VERSION_FIELD: i64 = 0;

/// Prepare only the recognized legacy none/cos physical representation.
/// Read-only/query-only handles preserve their existing compatible observation path.
///
/// # Errors
/// Returns a storage error for unsupported native state, corrupt legacy vectors,
/// contention exceeding the bounded wait, or an unsuccessful atomic rebuild.
pub(super) fn vector_index_prepare(connection: &mut Connection) -> StorageResult<()> {
    let flat = wait_for_storage_contention(|| {
        let read = connection.transaction()?;
        let flat = is_exact_flat_index(&read)?;
        read.commit()?;
        Ok(flat)
    })?;
    let query_only: bool = connection.pragma_query_value(None, "query_only", |row| row.get(0))?;
    if flat || query_only || connection.is_readonly("main")? {
        return Ok(());
    }
    wait_for_storage_contention(|| {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        // Recheck after acquiring the writer; another opener may already have converted it.
        if !is_exact_flat_index(&transaction)? {
            verify_legacy_vectors(&transaction)?;
            transaction.execute(
                "insert into vector_index(cmd,arg) values('rebuild','{index:\"flat\",distance:\"cos\"}')",
                [],
            )?;
            if !is_exact_flat_index(&transaction)? {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid stored vector model",
                ));
            }
        }
        transaction.commit().map_err(StorageError::from)
    })
}

/// Recognize only opaque models created by this pinned native module for the application.
/// Factory equality rejects unknown tags without interpreting or repairing a private header.
fn is_exact_flat_index(connection: &Connection) -> StorageResult<bool> {
    let reference = Connection::open_in_memory()?;
    reference.execute_batch(include_str!("vectors.sql"))?;
    let flat: Vec<u8> =
        reference.query_row("select val from vector_index_model where id=1", [], |row| {
            row.get(0)
        })?;
    reference.execute(
        "insert into vector_index(cmd,arg) values('rebuild','{index:\"none\",distance:\"cos\"}')",
        [],
    )?;
    let none: Vec<u8> =
        reference.query_row("select val from vector_index_model where id=1", [], |row| {
            row.get(0)
        })?;
    let flat: Option<bool> = connection.query_row(
        "select case when val=?1 then 1 when val=?2 then 0 else null end
         from vector_index_model where id=1",
        rusqlite::params![flat, none],
        |row| row.get(0),
    )?;
    flat.ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported stored vector model",
        )
    })
}

fn verify_legacy_vectors(connection: &Connection) -> StorageResult<()> {
    let model_version: i64 = connection.query_row(
        "select val from vector_index_config where id=?1",
        [NATIVE_MODEL_VERSION_FIELD],
        |row| row.get(0),
    )?;
    if model_version <= 0 || model_version == i64::MAX {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid stored vector model version",
        ));
    }
    let dimensions = vector_index_config_get_by_id(connection, VectorSchema::Main)?;
    let mut statement = connection.prepare("select vector from vector_index")?;
    let mut rows = statement.query([])?;
    while let Some(row) = rows.next()? {
        let value = row.get_ref(0)?;
        let ValueRef::Blob(bytes) = value else {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "invalid stored vector data",
            ));
        };
        if dimensions.is_none_or(|size| size.get() * size_of::<f32>() != bytes.len()) {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "invalid stored vector data",
            ));
        }
        decode_vector(bytes)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "vector_index_test.rs"]
mod tests;
