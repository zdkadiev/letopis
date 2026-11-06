use super::{connection::wait_for_storage_contention, native::register_vec1};
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, Transaction, TransactionBehavior};

const CACHE_SCHEMA_VERSION: i64 = 4;
const VECTOR_SCHEMA_VERSION: i64 = 1;
const USER_VERSION_PRAGMA: &str = "user_version";
const CACHE_SCHEMA: &str = include_str!("cache.sql");
const VECTOR_SCHEMA: &str = include_str!("vectors.sql");

#[derive(Debug, PartialEq, Eq)]
struct SchemaDefinition {
    object_type: String,
    name: String,
    sql: String,
}

#[derive(Clone, Copy)]
enum SqlText {
    Syntax,
    Quoted(char),
    LineComment,
    BlockComment,
}

pub(crate) fn bootstrap_cache(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, CACHE_SCHEMA, CACHE_SCHEMA_VERSION)
}

pub(crate) fn bootstrap_vectors(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)
}

pub(crate) fn validate_cache(connection: &Connection) -> StorageResult<()> {
    if !is_schema_present(connection, "main", CACHE_SCHEMA, CACHE_SCHEMA_VERSION)? {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "incompatible storage database schema",
        ));
    }
    Ok(())
}

pub(crate) fn validate_vector_database(connection: &Connection) -> StorageResult<()> {
    if !is_schema_present(connection, "main", VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)? {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "incompatible storage database schema",
        ));
    }
    Ok(())
}

pub(crate) fn validate_vectors(connection: &Connection) -> StorageResult<()> {
    if !is_schema_present(connection, "vec", VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)? {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "incompatible storage database schema",
        ));
    }
    Ok(())
}

pub(crate) fn attach_empty_vectors(connection: &Connection) -> StorageResult<()> {
    connection.execute("attach database ?1 as vec", ["file::memory:?mode=memory"])?;
    // Only the ephemeral attachment receives the unchanged vector schema.
    let schema = VECTOR_SCHEMA
        .replace("create table ", "create table vec.")
        .replace("create virtual table ", "create virtual table vec.")
        .replace("insert into vector_index", "insert into vec.vector_index");
    connection
        .execute_batch(&schema)
        .map_err(StorageError::from)
}

fn bootstrap_schema(
    connection: &mut Connection,
    sql: &str,
    expected_version: i64,
) -> StorageResult<()> {
    // Pin version and definitions to one read view without requesting a writer.
    let initialized = wait_for_storage_contention(|| {
        let read = connection.transaction()?;
        let initialized = is_schema_present(&read, "main", sql, expected_version)?;
        read.commit()?;
        Ok(initialized)
    })?;
    if initialized {
        return Ok(());
    }
    initialize_schema(connection, sql, expected_version)
}

fn initialize_schema(
    connection: &Connection,
    sql: &str,
    expected_version: i64,
) -> StorageResult<()> {
    // The whole attempt is repeatable: a failed commit rolls back, and another
    // initializer may have published the schema before the next attempt.
    wait_for_storage_contention(|| {
        let transaction = Transaction::new_unchecked(connection, TransactionBehavior::Immediate)?;
        // A competing initializer may have committed while this connection waited.
        if !is_schema_present(&transaction, "main", sql, expected_version)? {
            transaction.execute_batch(sql)?;
            transaction.pragma_update(None, USER_VERSION_PRAGMA, expected_version)?;
        }
        transaction.commit().map_err(StorageError::from)
    })
}

fn is_schema_present(
    connection: &Connection,
    schema: &str,
    sql: &str,
    expected_version: i64,
) -> StorageResult<bool> {
    let version: i64 =
        connection.query_row(&format!("pragma {schema}.user_version"), [], |row| {
            row.get(0)
        })?;
    let definitions = sqlite_schema_get_by_schema(connection, schema)?;
    match version {
        0 if definitions.is_empty() => Ok(false),
        version if version == expected_version => {
            register_vec1()?;
            let reference = Connection::open_in_memory()?;
            reference.execute_batch(sql)?;
            if definitions != sqlite_schema_get_by_schema(&reference, "main")? {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "incompatible storage database schema",
                ));
            }
            Ok(true)
        }
        _ => Err(StorageError::new(
            StorageErrorKind::InternalError,
            "incompatible storage database schema",
        )),
    }
}

fn sqlite_schema_get_by_schema(
    connection: &Connection,
    schema: &str,
) -> StorageResult<Vec<SchemaDefinition>> {
    // Schema names are private literals main/vec; values remain bound.
    let sql = format!(
        "select type,name,sql from {schema}.sqlite_schema
        where sql is not null and name not glob 'sqlite_*'
        and name not in (select name from pragma_table_list where schema=?1 and type='shadow')
        order by type,name"
    );
    let mut statement = connection.prepare(&sql)?;
    statement
        .query_map([schema], |row| {
            let sql: String = row.get(2)?;
            Ok(SchemaDefinition {
                object_type: row.get(0)?,
                name: row.get(1)?,
                sql: normalize_keyword_case(&sql),
            })
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(StorageError::from)
}

// SQLite preserves keyword spelling in sqlite_schema; literals must remain exact.

fn normalize_keyword_case(sql: &str) -> String {
    let mut state = SqlText::Syntax;
    let mut chars = sql.chars().peekable();
    let mut result = String::with_capacity(sql.len());
    while let Some(ch) = chars.next() {
        match state {
            SqlText::Syntax => match ch {
                '\'' | '"' | '`' | '[' => {
                    state = SqlText::Quoted(if ch == '[' { ']' } else { ch });
                    result.push(ch);
                }
                '-' if chars.peek() == Some(&'-') => {
                    state = SqlText::LineComment;
                    result.push_str("--");
                    chars.next();
                }
                '/' if chars.peek() == Some(&'*') => {
                    state = SqlText::BlockComment;
                    result.push_str("/*");
                    chars.next();
                }
                _ => result.push(ch.to_ascii_lowercase()),
            },
            SqlText::Quoted(end) => {
                result.push(ch);
                if ch == end && end != ']' && chars.peek() == Some(&end) {
                    result.push(end);
                    chars.next();
                } else if ch == end {
                    state = SqlText::Syntax;
                }
            }
            SqlText::LineComment => {
                result.push(ch);
                if ch == '\n' {
                    state = SqlText::Syntax;
                }
            }
            SqlText::BlockComment => {
                result.push(ch);
                if ch == '*' && chars.peek() == Some(&'/') {
                    result.push('/');
                    chars.next();
                    state = SqlText::Syntax;
                }
            }
        }
    }
    result
}

#[cfg(test)]
#[path = "schema_test.rs"]
mod tests;
