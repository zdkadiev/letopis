use super::{connection::map_sqlite, native::register_vec1};
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, TransactionBehavior};

const SCHEMA_VERSION: i64 = 1;
const CACHE_SCHEMA: &str = include_str!("cache.sql");
const VECTOR_SCHEMA: &str = include_str!("vectors.sql");

type Definition = (String, String, String);

pub(super) fn bootstrap_cache(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, CACHE_SCHEMA)
}
pub(super) fn bootstrap_vectors(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, VECTOR_SCHEMA)
}

fn bootstrap_schema(connection: &mut Connection, sql: &str) -> StorageResult<()> {
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sqlite)?;
    let version: i64 = transaction
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(map_sqlite)?;
    let definitions = read_definitions(&transaction)?;
    match version {
        0 if definitions.is_empty() => {
            transaction.execute_batch(sql).map_err(map_sqlite)?;
            transaction
                .pragma_update(None, "user_version", SCHEMA_VERSION)
                .map_err(map_sqlite)?;
        }
        SCHEMA_VERSION => {
            register_vec1()?;
            let reference = Connection::open_in_memory().map_err(map_sqlite)?;
            reference.execute_batch(sql).map_err(map_sqlite)?;
            if definitions != read_definitions(&reference)? {
                return Err(incompatible_schema());
            }
        }
        _ => return Err(incompatible_schema()),
    }
    transaction.commit().map_err(map_sqlite)
}

fn read_definitions(connection: &Connection) -> StorageResult<Vec<Definition>> {
    let mut statement = connection.prepare("SELECT type,name,sql FROM sqlite_schema WHERE sql IS NOT NULL AND name NOT GLOB 'sqlite_*' AND name NOT IN (SELECT name FROM pragma_table_list WHERE schema='main' AND type='shadow') ORDER BY type,name").map_err(map_sqlite)?;
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)
}

fn incompatible_schema() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "incompatible storage database schema",
    )
}
