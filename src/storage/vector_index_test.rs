use crate::storage::{
    bootstrap_vectors,
    connection::{ON_STORAGE_CONTENTION, open_database},
};
use rusqlite::{Connection, TransactionBehavior};
use std::{path::Path, sync::mpsc, thread, time::Duration};
use tempfile::TempDir;

#[test]
fn exact_flat_bootstrap_preserves_populated_state_and_is_idempotent() {
    // Given: a populated non-flat database with signed-zero vector payload and application state.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    let state_before = get_vector_state(&connection);
    // When: writable bootstrap prepares the exact flat representation twice.
    bootstrap_vectors(&mut connection).unwrap();
    let total_changes = connection.total_changes();
    bootstrap_vectors(&mut connection).unwrap();
    // Then: the native mode changes once while original bytes, rowids, identity, dimensions and errors remain exact.
    assert_eq!(get_native_mode(&connection), "flat");
    assert_eq!(get_vector_state(&connection), state_before);
    assert_eq!(connection.total_changes(), total_changes);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn exact_flat_rebuild_preserves_a_separate_pinned_reader() {
    // Given: a separate reader has pinned the old representation with no live cursor.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let mut writer = build_legacy_vectors(&path);
    let reader = open_database(&path).unwrap();
    reader.execute_batch("begin").unwrap();
    let state_before = get_vector_state(&reader);
    assert_eq!(get_native_mode(&reader), "none");
    // When: another writable connection publishes the flat index.
    bootstrap_vectors(&mut writer).unwrap();
    // Then: the pinned read still sees its original bytes and mode; a new view sees flat and the same bytes.
    assert_eq!(get_vector_state(&reader), state_before);
    assert_eq!(get_native_mode(&reader), "none");
    reader.execute_batch("commit").unwrap();
    assert_eq!(get_native_mode(&reader), "flat");
    assert_eq!(get_vector_state(&reader), state_before);
}

#[test]
fn flat_rebuild_replaces_an_unrecognized_native_model() {
    // Given: a structurally valid index with an explicit L2 physical model.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    connection
        .execute(
            "insert into vector_index(cmd,arg) values('rebuild','{index:\"none\",distance:\"l2\"}')",
            [],
        )
        .unwrap();
    // When: writable bootstrap prepares the exact flat cosine representation.
    bootstrap_vectors(&mut connection).unwrap();
    // Then: any non-flat model is rebuilt rather than rejected.
    assert_eq!(get_native_mode(&connection), "flat");
    assert!(connection.is_autocommit());
}

#[test]
fn exact_flat_query_only_bootstrap_preserves_existing_model() {
    // Given: a non-flat database is explicitly query-only.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    connection.pragma_update(None, "query_only", true).unwrap();
    // When: bootstrap prepares this query-only handle.
    bootstrap_vectors(&mut connection).unwrap();
    // Then: the existing model is retained without a hidden rebuild writer.
    assert_eq!(get_native_mode(&connection), "none");
}

#[test]
fn exact_flat_failed_native_rebuild_rolls_back_model_and_retries_cleanly() {
    // Given: native index insertion will fail after rebuild has written a new model.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let mut connection = build_legacy_vectors(&path);
    let state_before = get_vector_state(&connection);
    connection.execute_batch("create trigger fail_rebuild before insert on vector_index_idx begin select raise(abort,'injected native failure'); end").unwrap();
    // When: vector index preparation encounters the injected native failure, then the fault is removed.
    assert!(super::vector_index_prepare(&mut connection).is_err());
    // Then: rollback unloads the changed native model and preserves the old logical contents.
    assert!(connection.is_autocommit());
    assert_eq!(get_native_mode(&connection), "none");
    assert_eq!(get_vector_state(&connection), state_before);
    connection
        .execute_batch("drop trigger fail_rebuild")
        .unwrap();
    // When: bootstrap retries on the same connection, then reopens independently.
    bootstrap_vectors(&mut connection).unwrap();
    let mut reopened = open_database(&path).unwrap();
    bootstrap_vectors(&mut reopened).unwrap();
    // Then: both handles see exact flat bytes without a lingering rolled-back cached model.
    assert_eq!(get_native_mode(&connection), "flat");
    assert_eq!(get_native_mode(&reopened), "flat");
    assert_eq!(get_vector_state(&reopened), state_before);
}

#[test]
fn exact_flat_competing_opener_rechecks_the_committed_model() {
    // Given: an owner holds the writer while a second opener reaches preparation.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let mut owner = build_legacy_vectors(&path);
    let mut waiter = open_database(&path).unwrap();
    let transaction = owner
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    let (contended, observed) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let worker = thread::spawn(move || {
        ON_STORAGE_CONTENTION.set(Some(Box::new(move || {
            contended.send(()).unwrap();
            released.recv_timeout(Duration::from_secs(20)).unwrap();
        })));
        bootstrap_vectors(&mut waiter).unwrap();
        waiter
            .query_row(
                "select val from vector_index_config where id=0",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
    });
    // When: the first owner publishes flat before the waiting opener acquires its writer.
    observed.recv_timeout(Duration::from_secs(20)).unwrap();
    transaction.execute("insert into vector_index(cmd,arg) values('rebuild','{index:\"flat\",distance:\"cos\"}')", []).unwrap();
    let published_version: i64 = transaction
        .query_row(
            "select val from vector_index_config where id=0",
            [],
            |row| row.get(0),
        )
        .unwrap();
    transaction.commit().unwrap();
    release.send(()).unwrap();
    // Then: the waiter recognizes flat and does not increment the published native model version.
    assert_eq!(worker.join().unwrap(), published_version);
    assert_eq!(get_native_mode(&owner), "flat");
}

#[test]
fn exact_flat_fresh_query_only_and_empty_retained_dimensions_are_preserved() {
    // Given: a fresh database and a non-flat database retaining dimensions after deletion.
    let root = TempDir::new().unwrap();
    let mut fresh = open_database(&root.path().join("fresh")).unwrap();
    bootstrap_vectors(&mut fresh).unwrap();
    let mut non_flat = build_legacy_vectors(&root.path().join("non-flat"));
    non_flat.execute("delete from vector_index", []).unwrap();
    non_flat.execute("delete from vector_keys", []).unwrap();
    // When: fresh flat is reopened query-only and the empty index is rebuilt.
    fresh.pragma_update(None, "query_only", true).unwrap();
    bootstrap_vectors(&mut fresh).unwrap();
    bootstrap_vectors(&mut non_flat).unwrap();
    // Then: both models are flat, and deleting all rows did not erase native dimensions.
    assert_eq!(get_native_mode(&fresh), "flat");
    assert_eq!(get_native_mode(&non_flat), "flat");
    assert_eq!(
        non_flat
            .query_row(
                "select val from vector_index_config where id=2",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
}

fn build_legacy_vectors(path: &Path) -> Connection {
    let connection = open_database(path).unwrap();
    connection
        .execute_batch(include_str!(
            "../../tests/fixtures/storage/legacy-vectors.sql"
        ))
        .unwrap();
    connection.pragma_update(None, "user_version", 1).unwrap();
    let bytes = [2.0_f32.to_ne_bytes(), (-0.0_f32).to_ne_bytes()].concat();
    connection
        .execute(
            "insert into vector_index(rowid,vector) values(7,?1)",
            [&bytes],
        )
        .unwrap();
    connection.execute_batch("insert into meta values(1,'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa');
        insert into vector_keys values(7,'bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb');
        insert into embed_failures values('cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc',3,'failure sentinel');
        insert into query_error values(1,17,'query sentinel');").unwrap();
    connection
}

fn get_native_mode(connection: &Connection) -> String {
    // Opening the vector table registers its model, which vec1cat then reports.
    connection
        .prepare("select vector from vector_index limit 0")
        .unwrap();
    let model: String = connection
        .query_row(
            "select model from vec1cat where database='main' and name='vector_index'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    serde_json::from_str::<serde_json::Value>(&model).unwrap()["index"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn get_vector_state(connection: &Connection) -> String {
    let mut result = String::new();
    for sql in [
        "select rowid,hex(vector) from vector_index order by rowid",
        "select * from meta",
        "select * from vector_keys",
        "select * from embed_failures",
        "select * from query_error",
        "select id,val from vector_index_config where id=2",
    ] {
        let mut statement = connection.prepare(sql).unwrap();
        let columns = statement.column_count();
        let mut rows = statement.query([]).unwrap();
        while let Some(row) = rows.next().unwrap() {
            for column in 0..columns {
                result.push_str(&format!("{:?};", row.get_ref(column).unwrap()));
            }
        }
    }
    result
}
