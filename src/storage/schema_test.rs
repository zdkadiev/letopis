use crate::service::StorageError;

use crate::storage::get_current;

use super::{
    CACHE_SCHEMA, CACHE_SCHEMA_VERSION, VECTOR_SCHEMA, VECTOR_SCHEMA_VERSION, bootstrap_cache,
    bootstrap_schema, bootstrap_vectors,
};
use crate::storage::connection::{ON_STORAGE_CONTENTION, open_database};
use crate::{
    domain::Sha256,
    process_test_support::supervise_test,
    service::{StorageResult, VectorDatabase},
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
fn accepts_keyword_case_without_ignoring_literal_case() {
    // Given: a small schema using uppercase keywords and exact string literals.
    let mut connection = Connection::open_in_memory().unwrap();
    connection
        .execute_batch("CREATE TABLE t(value TEXT CHECK(value='A')); PRAGMA user_version=1")
        .unwrap();
    // When / Then: lowercase keywords describe exactly the same schema.
    bootstrap_schema(
        &mut connection,
        "create table t(value text check(value='A'))",
        1,
    )
    .unwrap();

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
            4,
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
            ON_STORAGE_CONTENTION.set(Some(Box::new(move || {
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
        ON_STORAGE_CONTENTION.set(Some(Box::new(move || {
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
fn schema_commit_contention_is_retried_until_the_reader_releases() {
    // Given: a rollback-journal database and a reader holding a shared lock.
    let root = TempDir::new().unwrap();
    let path = root.path().join("database");
    let mut initializer = Connection::open(&path).unwrap();
    initializer.busy_timeout(Duration::ZERO).unwrap();
    let reader = Connection::open(&path).unwrap();
    reader
        .execute_batch("begin; select count(*) from sqlite_schema;")
        .unwrap();
    let (contended, observed) = mpsc::channel();
    let worker = thread::spawn(move || {
        ON_STORAGE_CONTENTION.set(Some(Box::new(move || {
            contended.send(()).unwrap();
        })));
        bootstrap_cache(&mut initializer)
    });
    // When: the blocked commit is retried after the reader releases its lock.
    observed.recv_timeout(Duration::from_secs(10)).unwrap();
    reader.execute_batch("commit;").unwrap();
    // Then: the retried attempt publishes the complete schema.
    worker.join().unwrap().unwrap();
    assert_eq!(
        reader
            .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        CACHE_SCHEMA_VERSION
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
        let storage = crate::storage::open_test_storage(&cache, &vectors).unwrap();
        storage
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        storage
            .vector_storage
            .with_write_tx(&identity, || {
                let write = &storage.vector_storage;
                write.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
                write
                    .embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                    .unwrap();
                write
                    .embed_failures_update_by_search_hash(&hash, "failure sentinel")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
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
    let error = crate::storage::open_test_storage(&cache, &vectors).unwrap_err();
    // Then: exact content and inode identity, including populated native vector state.
    assert_eq!(error.get_message(), "incompatible storage database schema");
    for (name, expected) in files.iter().zip(before) {
        assert_eq!(capture_file(&root.path().join(name)), expected, "{name}");
    }
}

fn capture_file(path: &Path) -> (u64, Vec<u8>) {
    (fs::metadata(path).unwrap().ino(), fs::read(path).unwrap())
}
