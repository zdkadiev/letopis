use super::{
    connection::{map_sqlite, wait_for_initialization},
    native::register_vec1,
};
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, Transaction, TransactionBehavior};

const SCHEMA_VERSION: i64 = 1;
const USER_VERSION_PRAGMA: &str = "user_version";
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
    // Pin version and definitions to one read view without requesting a writer.
    let initialized = wait_for_initialization(|| {
        let read = connection.transaction().map_err(map_sqlite)?;
        let initialized = has_schema(&read, sql)?;
        read.commit().map_err(map_sqlite)?;
        Ok(initialized)
    })?;
    if initialized {
        return Ok(());
    }
    initialize_schema(connection, sql)
}

fn initialize_schema(connection: &Connection, sql: &str) -> StorageResult<()> {
    let transaction = wait_for_initialization(|| {
        Transaction::new_unchecked(connection, TransactionBehavior::Immediate).map_err(map_sqlite)
    })?;
    // A competing initializer may have committed while this connection waited.
    if !has_schema(&transaction, sql)? {
        transaction.execute_batch(sql).map_err(map_sqlite)?;
        transaction
            .pragma_update(None, USER_VERSION_PRAGMA, SCHEMA_VERSION)
            .map_err(map_sqlite)?;
    }
    transaction.commit().map_err(map_sqlite)
}

fn has_schema(connection: &Connection, sql: &str) -> StorageResult<bool> {
    let version: i64 = connection
        .pragma_query_value(None, USER_VERSION_PRAGMA, |row| row.get(0))
        .map_err(map_sqlite)?;
    let definitions = read_definitions(connection)?;
    match version {
        0 if definitions.is_empty() => Ok(false),
        SCHEMA_VERSION => {
            register_vec1()?;
            let reference = Connection::open_in_memory().map_err(map_sqlite)?;
            reference.execute_batch(sql).map_err(map_sqlite)?;
            if definitions != read_definitions(&reference)? {
                return Err(incompatible_schema());
            }
            Ok(true)
        }
        _ => Err(incompatible_schema()),
    }
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

#[cfg(test)]
mod tests {
    use super::{
        CACHE_SCHEMA, VECTOR_SCHEMA, bootstrap_cache, bootstrap_schema, bootstrap_vectors,
    };
    use crate::storage::connection::{ON_INITIALIZATION_CONTENTION, open_database};
    use rusqlite::TransactionBehavior;
    use std::{
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    #[test]
    fn existing_schemas_validate_on_query_only_connections() {
        for bootstrap in [bootstrap_cache, bootstrap_vectors] {
            // Arrange a recognized schema on a connection that forbids writes.
            let root = TempDir::new().unwrap();
            let mut connection = open_database(&root.path().join("database")).unwrap();
            bootstrap(&mut connection).unwrap();
            connection.pragma_update(None, "query_only", true).unwrap();
            // Act and assert: validation needs no SQLite write transaction.
            bootstrap(&mut connection).unwrap();
        }
    }
    #[test]
    fn waiting_initializer_rechecks_committed_schema_for_both_databases() {
        for sql in [CACHE_SCHEMA, VECTOR_SCHEMA] {
            // Arrange a first initializer holding an uncommitted real schema.
            let root = TempDir::new().unwrap();
            let path = root.path().join("database");
            let mut owner = open_database(&path).unwrap();
            let mut waiter = open_database(&path).unwrap();
            let transaction = owner
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            transaction.execute_batch(sql).unwrap();
            transaction.pragma_update(None, "user_version", 1).unwrap();
            let (contended, observed) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let worker = thread::spawn(move || {
                ON_INITIALIZATION_CONTENTION.set(Some(Box::new(move || {
                    contended.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(20)).unwrap();
                })));
                bootstrap_schema(&mut waiter, sql)
            });
            // Act: the waiter has observed no schema, then failed to take ownership.
            observed.recv_timeout(Duration::from_secs(20)).unwrap();
            transaction.commit().unwrap();
            release.send(()).unwrap();
            // Assert: rechecking accepts the owner's schema instead of replaying DDL.
            worker.join().unwrap().unwrap();
            assert_eq!(
                owner
                    .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                1
            );
        }
    }

    #[test]
    fn waiting_initializer_rejects_incompatible_schema_committed_by_owner() {
        // Arrange a virgin schema while another owner holds the initialization write.
        let root = TempDir::new().unwrap();
        let path = root.path().join("database");
        let mut owner = open_database(&path).unwrap();
        let mut waiter = open_database(&path).unwrap();
        let transaction = owner
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction
            .execute_batch("CREATE TABLE incompatible(value TEXT)")
            .unwrap();
        let (contended, observed) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let worker = thread::spawn(move || {
            ON_INITIALIZATION_CONTENTION.set(Some(Box::new(move || {
                contended.send(()).unwrap();
                released.recv_timeout(Duration::from_secs(20)).unwrap();
            })));
            bootstrap_cache(&mut waiter)
        });
        // Act: the competing initializer commits an unsupported schema.
        observed.recv_timeout(Duration::from_secs(20)).unwrap();
        transaction.commit().unwrap();
        release.send(()).unwrap();
        // Assert rejection, with no partial cache creation or version rewrite.
        assert!(worker.join().unwrap().is_err());
        assert_eq!(
            owner
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            owner
                .query_row(
                    "SELECT count(*) FROM sqlite_schema WHERE name='project'",
                    [],
                    |r| r.get::<_, i64>(0)
                )
                .unwrap(),
            0
        );
    }
    #[test]
    fn initialization_contention_expires_and_can_be_retried_safely() {
        const CHILD: &str = "LETOPIS_INITIALIZATION_EXPIRATION_CHILD";
        if std::env::var_os(CHILD).is_none() {
            crate::process_test_support::supervise_test(
                "storage::schema::tests::initialization_contention_expires_and_can_be_retried_safely",
                &[(CHILD, "1")],
            );
            return;
        }
        // Arrange a missing schema whose write ownership cannot be acquired yet.
        let root = TempDir::new().unwrap();
        let path = root.path().join("database");
        let mut owner = open_database(&path).unwrap();
        let mut waiter = open_database(&path).unwrap();
        let transaction = owner
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        // Act: hold ownership throughout the bounded initialization wait.
        let started = Instant::now();
        let error = bootstrap_cache(&mut waiter).unwrap_err();
        // Assert a safe contention error, no partial schema, and a successful retry.
        assert!(started.elapsed() >= Duration::from_secs(5));
        assert_eq!(
            std::error::Error::source(&error)
                .unwrap()
                .downcast_ref::<rusqlite::Error>()
                .unwrap()
                .sqlite_error_code(),
            Some(rusqlite::ErrorCode::DatabaseBusy)
        );
        assert_eq!(
            waiter
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
        drop(transaction);
        bootstrap_cache(&mut waiter).unwrap();
        assert_eq!(
            waiter
                .pragma_query_value(None, "busy_timeout", |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
