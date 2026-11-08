use crate::storage::{
    bootstrap_vectors,
    connection::{ON_STORAGE_CONTENTION, open_database},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior};
use std::{path::Path, sync::mpsc, thread, time::Duration};
use tempfile::TempDir;

#[test]
fn exact_flat_bootstrap_preserves_populated_state_and_is_idempotent() {
    // Given: a populated recognized none/cos database with signed-zero vector payload and application state.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    let before = get_vector_state(&connection);
    // When: writable bootstrap prepares the exact flat representation twice.
    bootstrap_vectors(&mut connection).unwrap();
    let total_changes = connection.total_changes();
    bootstrap_vectors(&mut connection).unwrap();
    // Then: the native mode changes once while original bytes, rowids, identity, dimensions and errors remain exact.
    assert_eq!(get_native_mode(&connection), "flat");
    assert_eq!(get_vector_state(&connection), before);
    assert_eq!(connection.total_changes(), total_changes);
    assert_eq!(
        connection
            .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn exact_flat_upgrade_preserves_a_separate_pinned_reader() {
    // Given: a separate reader has pinned the old valid none representation with no live cursor.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let mut writer = build_legacy_vectors(&path);
    let reader = open_database(&path).unwrap();
    reader.execute_batch("begin").unwrap();
    let before = get_vector_state(&reader);
    assert_eq!(get_native_mode(&reader), "none");
    // When: another writable connection publishes the physical index upgrade.
    bootstrap_vectors(&mut writer).unwrap();
    // Then: the pinned read still sees its original bytes and mode; a new view sees flat and the same bytes.
    assert_eq!(get_vector_state(&reader), before);
    assert_eq!(get_native_mode(&reader), "none");
    reader.execute_batch("commit").unwrap();
    assert_eq!(get_native_mode(&reader), "flat");
    assert_eq!(get_vector_state(&reader), before);
}

#[test]
fn exact_flat_bootstrap_does_not_repair_nonfinite_or_malformed_old_vectors() {
    // Given: old none vectors are independently corrupted at the native shadow boundary.
    for bytes in [
        vec![0],
        [f32::NAN.to_ne_bytes(), 0.0_f32.to_ne_bytes()].concat(),
    ] {
        let root = TempDir::new().unwrap();
        let mut connection = build_legacy_vectors(&root.path().join("vectors"));
        connection
            .execute(
                "update vector_index_base set vector=?1 where id=7",
                [&bytes],
            )
            .unwrap();
        let model: Vec<u8> = connection
            .query_row("select val from vector_index_model", [], |row| row.get(0))
            .unwrap();
        // When: writable bootstrap encounters malformed data before any conversion.
        let result = bootstrap_vectors(&mut connection);
        // Then: corruption stays an error, original model and corrupt bytes remain untouched, and the transaction closes.
        assert!(result.is_err());
        assert_eq!(get_native_mode(&connection), "none");
        assert_eq!(
            connection
                .query_row("select val from vector_index_model", [], |row| row
                    .get::<_, Vec<u8>>(0))
                .unwrap(),
            model
        );
        assert_eq!(
            connection
                .query_row(
                    "select vector from vector_index_base where id=7",
                    [],
                    |row| row.get::<_, Vec<u8>>(0)
                )
                .unwrap(),
            bytes
        );
        assert!(connection.is_autocommit());
    }
}

#[test]
fn exact_flat_query_only_bootstrap_preserves_legacy_observation() {
    // Given: a recognized legacy none database is explicitly query-only.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    connection.pragma_update(None, "query_only", true).unwrap();
    // When: bootstrap validates this observation-only handle.
    bootstrap_vectors(&mut connection).unwrap();
    // Then: existing query-only compatibility is retained without a hidden migration writer.
    assert_eq!(get_native_mode(&connection), "none");
}

#[test]
fn exact_flat_failed_native_rebuild_rolls_back_model_and_retries_cleanly() {
    // Given: native index insertion will fail after rebuild has written a new model.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let mut connection = build_legacy_vectors(&path);
    let before = get_vector_state(&connection);
    connection.execute_batch("create trigger fail_rebuild before insert on vector_index_idx begin select raise(abort,'injected native failure'); end").unwrap();
    // When: private migration encounters the injected native failure, then the fault is removed.
    assert!(super::vector_index_prepare(&mut connection).is_err());
    // Then: rollback unloads the changed native model and preserves the old logical contents.
    assert!(connection.is_autocommit());
    assert_eq!(get_native_mode(&connection), "none");
    assert_eq!(get_vector_state(&connection), before);
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
    assert_eq!(get_vector_state(&reopened), before);
}

#[test]
fn exact_flat_competing_opener_rechecks_the_committed_model() {
    // Given: an owner holds the writer while a second legacy opener reaches migration.
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
fn exact_flat_fresh_query_only_and_empty_retained_dimensions_are_compatible() {
    // Given: a fresh database and a legacy database retaining dimensions after deletion.
    let root = TempDir::new().unwrap();
    let mut fresh = open_database(&root.path().join("fresh")).unwrap();
    bootstrap_vectors(&mut fresh).unwrap();
    let mut legacy = build_legacy_vectors(&root.path().join("legacy"));
    legacy.execute("delete from vector_index", []).unwrap();
    legacy.execute("delete from vector_keys", []).unwrap();
    // When: fresh flat is reopened query-only and the empty legacy index is upgraded.
    fresh.pragma_update(None, "query_only", true).unwrap();
    bootstrap_vectors(&mut fresh).unwrap();
    bootstrap_vectors(&mut legacy).unwrap();
    // Then: both models are flat, and deleting all old rows did not erase native dimensions.
    assert_eq!(get_native_mode(&fresh), "flat");
    assert_eq!(get_native_mode(&legacy), "flat");
    assert_eq!(
        legacy
            .query_row(
                "select val from vector_index_config where id=2",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        2
    );
}

#[test]
fn exact_flat_bootstrap_rejects_an_unrecognized_metric() {
    // Given: a structurally compatible index has an explicit L2 physical model.
    let root = TempDir::new().unwrap();
    let mut connection = build_legacy_vectors(&root.path().join("vectors"));
    connection.execute("insert into vector_index(cmd,arg) values('rebuild','{index:\"none\",distance:\"l2\"}')", []).unwrap();
    let before = get_vector_state(&connection);
    // When: bootstrap checks this unrecognized migration source.
    let result = bootstrap_vectors(&mut connection);
    // Then: migration refuses rather than silently changing metric or rewriting state.
    assert!(result.is_err());
    assert_eq!(get_vector_state(&connection), before);
    assert_eq!(get_native_mode(&connection), "none");
}

#[test]
fn exact_flat_bootstrap_rejects_a_corrupt_distance_hidden_by_native_catalog() {
    // Given: the pinned native header's big-endian distance field is changed from2 to unknown3.
    let root = TempDir::new().unwrap();
    let path = root.path().join("vectors");
    let connection = build_legacy_vectors(&path);
    let mut model: Vec<u8> = connection
        .query_row("select val from vector_index_model", [], |row| row.get(0))
        .unwrap();
    model[20..24].copy_from_slice(&3_u32.to_be_bytes());
    connection
        .execute("update vector_index_model set val=?1", [&model])
        .unwrap();
    drop(connection);
    let mut reopened = open_database(&path).unwrap();
    // When: catalog output deceptively says none/cos, and bootstrap considers migration.
    assert_eq!(get_native_mode(&reopened), "none");
    let result = bootstrap_vectors(&mut reopened);
    // Then: the unknown exact model is rejected without repairing its corrupt original header.
    assert!(result.is_err());
    assert_eq!(
        reopened
            .query_row("select val from vector_index_model", [], |row| row
                .get::<_, Vec<u8>>(0))
            .unwrap(),
        model
    );
    assert!(reopened.is_autocommit());
}

#[test]
fn exact_flat_upgrade_does_not_repair_a_corrupt_native_model_version() {
    // Given: recognized legacy model bytes have a missing, invalidly typed, nonpositive or overflowing native version.
    for statement in [
        "delete from vector_index_config where id=0",
        "update vector_index_config set val=0 where id=0",
        "update vector_index_config set val=-1 where id=0",
        "update vector_index_config set val=1.5 where id=0",
        "update vector_index_config set val=9223372036854775807 where id=0",
    ] {
        let root = TempDir::new().unwrap();
        let mut connection = build_legacy_vectors(&root.path().join("vectors"));
        connection.execute(statement, []).unwrap();
        let before = get_model_version_value(&connection);
        let model: Vec<u8> = connection
            .query_row("select val from vector_index_model where id=1", [], |row| {
                row.get(0)
            })
            .unwrap();
        // When: physical index preparation considers this corrupt source configuration.
        let result = bootstrap_vectors(&mut connection);
        // Then: corruption is rejected without recreating version state or replacing model bytes.
        assert!(result.is_err(), "{statement}");
        assert_eq!(get_model_version_value(&connection), before);
        assert_eq!(
            connection
                .query_row("select val from vector_index_model where id=1", [], |row| {
                    row.get::<_, Vec<u8>>(0)
                })
                .unwrap(),
            model
        );
        assert!(connection.is_autocommit());
    }
}

fn get_model_version_value(connection: &Connection) -> Option<String> {
    connection
        .query_row(
            "select typeof(val)||':'||quote(val) from vector_index_config where id=0",
            [],
            |row| row.get(0),
        )
        .optional()
        .unwrap()
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
