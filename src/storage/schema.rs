use super::{
    connection::{map_sqlite, wait_for_initialization},
    native::register_vec1,
};
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, Transaction, TransactionBehavior};

const CACHE_SCHEMA_VERSION: i64 = 2;
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

pub(super) fn bootstrap_cache(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, CACHE_SCHEMA, CACHE_SCHEMA_VERSION)
}

pub(super) fn bootstrap_vectors(connection: &mut Connection) -> StorageResult<()> {
    bootstrap_schema(connection, VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)
}

pub(super) fn validate_cache(connection: &Connection) -> StorageResult<()> {
    if !has_schema_in(connection, "main", CACHE_SCHEMA, CACHE_SCHEMA_VERSION)? {
        return Err(fail_incompatible_schema());
    }
    Ok(())
}

pub(super) fn validate_vector_database(connection: &Connection) -> StorageResult<()> {
    if !has_schema_in(connection, "main", VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)? {
        return Err(fail_incompatible_schema());
    }
    Ok(())
}

pub(super) fn validate_vectors(connection: &Connection) -> StorageResult<()> {
    if !has_schema_in(connection, "vec", VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION)? {
        return Err(fail_incompatible_schema());
    }
    Ok(())
}

pub(super) fn attach_empty_vectors(connection: &Connection) -> StorageResult<()> {
    connection
        .execute("attach database ?1 as vec", ["file::memory:?mode=memory"])
        .map_err(map_sqlite)?;
    // Only the ephemeral attachment receives the unchanged vector schema.
    let schema = VECTOR_SCHEMA
        .replace("create table ", "create table vec.")
        .replace("create virtual table ", "create virtual table vec.")
        .replace("insert into vector_index", "insert into vec.vector_index");
    connection.execute_batch(&schema).map_err(map_sqlite)
}

fn bootstrap_schema(
    connection: &mut Connection,
    sql: &str,
    expected_version: i64,
) -> StorageResult<()> {
    // Pin version and definitions to one read view without requesting a writer.
    let initialized = wait_for_initialization(|| {
        let read = connection.transaction().map_err(map_sqlite)?;
        let initialized = has_schema(&read, sql, expected_version)?;
        read.commit().map_err(map_sqlite)?;
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
    let transaction = wait_for_initialization(|| {
        Transaction::new_unchecked(connection, TransactionBehavior::Immediate).map_err(map_sqlite)
    })?;
    // A competing initializer may have committed while this connection waited.
    if !has_schema(&transaction, sql, expected_version)? {
        transaction.execute_batch(sql).map_err(map_sqlite)?;
        transaction
            .pragma_update(None, USER_VERSION_PRAGMA, expected_version)
            .map_err(map_sqlite)?;
    }
    transaction.commit().map_err(map_sqlite)
}

fn has_schema(connection: &Connection, sql: &str, expected_version: i64) -> StorageResult<bool> {
    has_schema_in(connection, "main", sql, expected_version)
}

fn has_schema_in(
    connection: &Connection,
    schema: &str,
    sql: &str,
    expected_version: i64,
) -> StorageResult<bool> {
    let version: i64 = connection
        .query_row(&format!("pragma {schema}.user_version"), [], |row| {
            row.get(0)
        })
        .map_err(map_sqlite)?;
    let definitions = get_definitions_in(connection, schema)?;
    match version {
        0 if definitions.is_empty() => Ok(false),
        version if version == expected_version => {
            register_vec1()?;
            let reference = Connection::open_in_memory().map_err(map_sqlite)?;
            reference.execute_batch(sql).map_err(map_sqlite)?;
            if definitions != get_definitions(&reference)? {
                return Err(fail_incompatible_schema());
            }
            Ok(true)
        }
        _ => Err(fail_incompatible_schema()),
    }
}

fn get_definitions(connection: &Connection) -> StorageResult<Vec<SchemaDefinition>> {
    get_definitions_in(connection, "main")
}

fn get_definitions_in(
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
    let mut statement = connection.prepare(&sql).map_err(map_sqlite)?;
    statement
        .query_map([schema], |row| {
            let sql: String = row.get(2)?;
            Ok(SchemaDefinition {
                object_type: row.get(0)?,
                name: row.get(1)?,
                sql: normalize_keyword_case(&sql),
            })
        })
        .map_err(map_sqlite)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(map_sqlite)
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

fn fail_incompatible_schema() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "incompatible storage database schema",
    )
}

#[cfg(test)]
mod tests {
    use super::{
        CACHE_SCHEMA, CACHE_SCHEMA_VERSION, VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION, bootstrap_cache,
        bootstrap_schema, bootstrap_vectors,
    };
    use crate::storage::connection::{ON_INITIALIZATION_CONTENTION, open_database};
    use crate::{
        domain::Sha256,
        process_test_support::supervise_test,
        service::{Storage, StorageResult, VectorOutcome},
        storage::SqliteStorage,
    };
    use rusqlite::{Connection, TransactionBehavior};
    use std::{
        fs,
        os::unix::fs::MetadataExt,
        path::Path,
        sync::mpsc,
        thread,
        time::{Duration, Instant},
    };
    use tempfile::TempDir;

    #[test]
    fn keyword_normalization_preserves_quotes_and_comments() {
        // Given: quoted names, escaped delimiters and quotes inside comments.
        let cases = [
            "select 'A''B', \"Case\", `Case``Name`, [Case]",
            "-- 'Unclosed quote\nselect 'Exact'",
            "/* 'Unclosed quote */ select 'Exact'",
        ];
        for sql in cases {
            // When: existing lowercase SQL is normalized for comparison.
            let actual = super::normalize_keyword_case(sql);
            // Then: every quoted value, name and comment remains byte-exact.
            assert_eq!(actual, sql);
        }
    }

    #[test]
    fn accepts_legacy_keyword_case_without_ignoring_literal_case() {
        // Given: the previously published cache schema with uppercase keywords.
        let mut connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch(include_str!(
                "../../tests/fixtures/storage/legacy-cache.sql"
            ))
            .unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        // When / Then: the lowercase source describes the same stored schema.
        assert!(bootstrap_cache(&mut connection).is_ok());

        // Given: an uppercase vector schema with its native extension registered.
        super::register_vec1().unwrap();
        let mut vectors = Connection::open_in_memory().unwrap();
        vectors
            .execute_batch(include_str!(
                "../../tests/fixtures/storage/legacy-vectors.sql"
            ))
            .unwrap();
        vectors.pragma_update(None, "user_version", 1).unwrap();
        // When / Then: vector validation accepts the prior spelling too.
        assert!(bootstrap_vectors(&mut vectors).is_ok());

        // Given: two schemas whose string constraints differ in case.
        let mut distinct = Connection::open_in_memory().unwrap();
        distinct
            .execute_batch("create table t(value text check(value='A')); pragma user_version=1")
            .unwrap();
        // When / Then: changing a literal remains an incompatible schema.
        assert!(
            super::bootstrap_schema(
                &mut distinct,
                "create table t(value text check(value='a')); pragma user_version=1",
                1
            )
            .is_err()
        );
    }

    #[test]
    fn cache_and_vector_versions_are_independent() {
        // Given / When / Then: initialize each schema and check its version.
        for (bootstrap, version) in [
            (
                bootstrap_cache as fn(&mut Connection) -> StorageResult<()>,
                2,
            ),
            (bootstrap_vectors, 1),
        ] {
            let mut connection = Connection::open_in_memory().unwrap();
            bootstrap(&mut connection).unwrap();
            assert_eq!(
                connection
                    .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                    .unwrap(),
                version
            );
        }
    }

    #[test]
    fn existing_schemas_validate_on_query_only_connections() {
        for bootstrap in [bootstrap_cache, bootstrap_vectors] {
            // Given a recognized schema on a connection that forbids writes.
            let root = TempDir::new().unwrap();
            let mut connection = open_database(&root.path().join("database")).unwrap();
            bootstrap(&mut connection).unwrap();
            connection.pragma_update(None, "query_only", true).unwrap();
            // When / Then: validation needs no SQLite write transaction.
            bootstrap(&mut connection).unwrap();
        }
    }

    #[test]
    fn waiting_initializer_rechecks_committed_schema_for_both_databases() {
        for (sql, version) in [
            (CACHE_SCHEMA, CACHE_SCHEMA_VERSION),
            (VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION),
        ] {
            // Given a first initializer holding an uncommitted real schema.
            let root = TempDir::new().unwrap();
            let path = root.path().join("database");
            let mut owner = open_database(&path).unwrap();
            let mut waiter = open_database(&path).unwrap();
            let transaction = owner
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .unwrap();
            transaction.execute_batch(sql).unwrap();
            transaction
                .pragma_update(None, "user_version", version)
                .unwrap();
            let (contended, observed) = mpsc::channel();
            let (release, released) = mpsc::channel();
            let worker = thread::spawn(move || {
                ON_INITIALIZATION_CONTENTION.set(Some(Box::new(move || {
                    contended.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(20)).unwrap();
                })));
                bootstrap_schema(&mut waiter, sql, version)
            });
            // When: the waiter has observed no schema, then failed to take ownership.
            observed.recv_timeout(Duration::from_secs(20)).unwrap();
            transaction.commit().unwrap();
            release.send(()).unwrap();
            // Then: rechecking accepts the owner's schema instead of replaying DDL.
            worker.join().unwrap().unwrap();
            assert_eq!(
                owner
                    .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                    .unwrap(),
                version
            );
        }
    }

    #[test]
    fn waiting_initializer_rejects_incompatible_schema_committed_by_owner() {
        // Given a virgin schema while another owner holds the initialization write.
        let root = TempDir::new().unwrap();
        let path = root.path().join("database");
        let mut owner = open_database(&path).unwrap();
        let mut waiter = open_database(&path).unwrap();
        let transaction = owner
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        transaction
            .execute_batch("create table incompatible(value text)")
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
        // When: the competing initializer commits an unsupported schema.
        observed.recv_timeout(Duration::from_secs(20)).unwrap();
        transaction.commit().unwrap();
        release.send(()).unwrap();
        // Then rejection, with no partial cache creation or version rewrite.
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
                    "select count(*) from sqlite_schema where name='project'",
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
            supervise_test(
                "storage::schema::tests::initialization_contention_expires_and_can_be_retried_safely",
                &[(CHILD, "1")],
            );
            return;
        }
        // Given a missing schema whose write ownership cannot be acquired yet.
        let root = TempDir::new().unwrap();
        let path = root.path().join("database");
        let mut owner = open_database(&path).unwrap();
        let mut waiter = open_database(&path).unwrap();
        let transaction = owner
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .unwrap();
        // When: hold ownership throughout the bounded initialization wait.
        let started = Instant::now();
        let error = bootstrap_cache(&mut waiter).unwrap_err();
        // Then a safe contention error, no partial schema, and a successful retry.
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
    #[test]
    fn old_cache_rejection_preserves_populated_vectors_config_and_locks() {
        // Given: closed populated databases and independent configuration/lock identities.
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        let identity = Sha256::digest(b"model");
        let hash = Sha256::digest(b"text");
        {
            let mut storage = SqliteStorage::open(&cache, &vectors).unwrap();
            storage.reset_vectors(None, &identity).unwrap();
            let VectorOutcome::Current(mut write) =
                storage.begin_vector_write_tx(&identity).unwrap()
            else {
                panic!()
            };
            write.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
            write.reserve_attempt(&hash, 3).unwrap();
            write.record_failure(&hash, "failure sentinel").unwrap();
            write.commit().unwrap();
        }
        {
            let connection = Connection::open(&cache).unwrap();
            connection
                .execute_batch(
                    "drop index edges_outgoing;
                drop index edges_incoming;
                drop index nodes_source_snapshot;
                create index edges_target on edges(target_id);
                pragma user_version=1;",
                )
                .unwrap();
        }
        let files = [
            "cache.db",
            "vectors.db",
            "config.json",
            "index.lock",
            "embed.lock",
        ];
        for name in &files[2..] {
            fs::write(root.path().join(name), name).unwrap();
        }
        let before: Vec<_> = files
            .iter()
            .map(|name| capture_file(&root.path().join(name)))
            .collect();
        // When: cache incompatibility is detected before the vector database is opened.
        let error = SqliteStorage::open(&cache, &vectors).unwrap_err();
        // Then: exact content and inode identity, including populated native vector state.
        assert_eq!(error.get_message(), "incompatible storage database schema");
        for (name, expected) in files.iter().zip(before) {
            assert_eq!(capture_file(&root.path().join(name)), expected, "{name}");
        }
    }

    fn capture_file(path: &Path) -> (u64, Vec<u8>) {
        (fs::metadata(path).unwrap().ino(), fs::read(path).unwrap())
    }
}
