use letopis_storage_build::{CheckResult, open_checked, require_sqlite_version};
use rusqlite::{Connection, params};
use std::{
    env,
    path::{Path, PathBuf},
};

fn extension() -> PathBuf {
    env::var_os("LETOPIS_VEC1_EXTENSION")
        .map(PathBuf::from)
        .expect("LETOPIS_VEC1_EXTENSION is required; run python3 run.py")
}

fn open(path: &Path) -> CheckResult<Connection> {
    // SAFETY: run.py supplies its freshly built, checksum-verified vec1 library.
    unsafe { open_checked(path, &extension()) }
}

#[test]
fn rejects_sqlite_below_the_supported_floor() {
    // Given / When
    let error = require_sqlite_version(3_051_002).unwrap_err();
    // Then
    assert!(error.to_string().contains("SQLite 3.51.3+ required"));
    require_sqlite_version(3_051_003).unwrap();
    require_sqlite_version(3_053_002).unwrap();
}

#[test]
fn missing_extension_is_an_error() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    // When
    let error =
        unsafe { open_checked(&dir.path().join("db"), &dir.path().join("absent")) }.unwrap_err();
    // Then
    assert!(error.to_string().contains("vec1 extension"), "{error}");
    Ok(())
}

#[test]
fn checked_connection_enforces_the_storage_policy() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    // When
    let conn = open(&dir.path().join("db"))?;
    let journal: String = conn.pragma_query_value(None, "journal_mode", |row| row.get(0))?;
    // Then
    assert_eq!(journal, "wal");
    for (pragma, expected) in [("synchronous", 2), ("foreign_keys", 1)] {
        let value: i64 = conn.pragma_query_value(None, pragma, |row| row.get(0))?;
        assert_eq!(value, expected, "{pragma}");
    }
    #[cfg(target_os = "macos")]
    assert_eq!(
        conn.pragma_query_value(None, "fullfsync", |row| row.get::<_, i64>(0))?,
        1
    );
    let vec1: String = conn.query_row("SELECT vec1_info()", [], |row| row.get(0))?;
    assert!(vec1.starts_with("version 0.8"), "{vec1}");
    let error = conn
        .query_row("SELECT load_extension('absent')", [], |_| Ok(()))
        .unwrap_err();
    assert!(error.to_string().contains("not authorized"), "{error}");
    Ok(())
}

fn create_fts(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE ft USING fts5(text, content='', contentless_delete=1);
         INSERT INTO ft(rowid,text) VALUES(1,'persistent memory'),(2,'agent graph');",
    )
}

fn matches(conn: &Connection, query: &str) -> rusqlite::Result<Vec<i64>> {
    conn.prepare("SELECT rowid FROM ft WHERE ft MATCH ?1 ORDER BY rowid")?
        .query_map([query], |row| row.get(0))?
        .collect()
}

fn create_vectors(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE v USING vec1(vector);
         INSERT INTO v(rowid,vector) VALUES
             (1,vec1_from_json('[10,0]')),
             (2,vec1_from_json('[1,1]')),
             (3,vec1_from_json('[-1,0]'));
         INSERT INTO v(cmd,arg) VALUES('rebuild','{\"index\":\"none\",\"distance\":\"cos\"}');",
    )
}

fn vector(conn: &Connection, rowid: i64) -> rusqlite::Result<Vec<u8>> {
    conn.query_row("SELECT vector FROM v WHERE rowid=?1", [rowid], |row| {
        row.get(0)
    })
}

fn nearest(conn: &Connection) -> rusqlite::Result<Vec<i64>> {
    let query: Vec<u8> = [1.0_f32, 0.0]
        .into_iter()
        .flat_map(f32::to_ne_bytes)
        .collect();
    conn.prepare("SELECT rowid FROM v(?1,'{\"K\":3}')")?
        .query_map([query], |row| row.get(0))?
        .collect()
}

#[test]
fn fts_contentless_delete_and_committed_writes_survive_reopen() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    {
        // When
        let mut conn = open(&path)?;
        let tx = conn.transaction()?;
        create_fts(&tx)?;
        tx.commit()?;
        // Then
        assert_eq!(matches(&conn, "memory")?, [1]);
        let text: Option<String> =
            conn.query_row("SELECT text FROM ft WHERE rowid=1", [], |row| row.get(0))?;
        assert_eq!(text, None);
        conn.execute("DELETE FROM ft WHERE rowid=1", [])?;
        assert!(matches(&conn, "memory")?.is_empty());
        assert_eq!(matches(&conn, "graph")?, [2]);
    }
    let conn = open(&path)?;
    assert!(matches(&conn, "memory")?.is_empty());
    assert_eq!(matches(&conn, "graph")?, [2]);
    Ok(())
}

#[test]
fn fts_insert_update_delete_roll_back_explicitly_and_on_drop() -> CheckResult<()> {
    // Given
    for explicit in [true, false] {
        let dir = tempfile::tempdir()?;
        let mut conn = open(&dir.path().join("db"))?;
        create_fts(&conn)?;
        {
            // When
            let tx = conn.transaction()?;
            tx.execute_batch(
                "UPDATE ft SET text='replacement' WHERE rowid=1;
                              DELETE FROM ft WHERE rowid=2;
                              INSERT INTO ft(rowid,text) VALUES(3,'temporary');",
            )?;
            // Then
            assert_eq!(matches(&tx, "replacement")?, [1]);
            assert!(matches(&tx, "graph")?.is_empty());
            assert_eq!(matches(&tx, "temporary")?, [3]);
            if explicit {
                tx.rollback()?;
            }
        }
        assert_eq!(matches(&conn, "memory")?, [1]);
        assert_eq!(matches(&conn, "graph")?, [2]);
        assert!(matches(&conn, "replacement OR temporary")?.is_empty());
    }
    Ok(())
}

#[test]
fn vectors_use_cosine_distance_and_persist_committed_changes() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    {
        // When
        let mut conn = open(&path)?;
        let tx = conn.transaction()?;
        create_vectors(&tx)?;
        tx.commit()?;
        // Cosine chooses [10,0]; Euclidean would incorrectly choose [1,1].
        // Then
        assert_eq!(nearest(&conn)?, [1, 2, 3]);
        let expected: Vec<u8> = [10.0_f32, 0.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        assert_eq!(vector(&conn, 1)?, expected);
        conn.execute(
            "UPDATE v SET vector=vec1_from_json('[-1,-1]') WHERE rowid=1",
            [],
        )?;
        conn.execute("DELETE FROM v WHERE rowid=3", [])?;
        assert_eq!(nearest(&conn)?, [2, 1]);
    }
    let conn = open(&path)?;
    assert_eq!(nearest(&conn)?, [2, 1]);
    let expected: Vec<u8> = [-1.0_f32, -1.0]
        .into_iter()
        .flat_map(f32::to_ne_bytes)
        .collect();
    assert_eq!(vector(&conn, 1)?, expected);
    Ok(())
}

#[test]
fn vector_insert_update_delete_roll_back_explicitly_and_on_drop() -> CheckResult<()> {
    // Given
    for explicit in [true, false] {
        let dir = tempfile::tempdir()?;
        let mut conn = open(&dir.path().join("db"))?;
        create_vectors(&conn)?;
        {
            // When
            let tx = conn.transaction()?;
            tx.execute_batch(
                "UPDATE v SET vector=vec1_from_json('[-1,-1]') WHERE rowid=1;
                              DELETE FROM v WHERE rowid=3;
                              INSERT INTO v(rowid,vector) VALUES(4,vec1_from_json('[0.5,0]'));",
            )?;
            // Then
            assert_eq!(nearest(&tx)?, [4, 2, 1]);
            if explicit {
                tx.rollback()?;
            }
        }
        assert_eq!(nearest(&conn)?, [1, 2, 3]);
        let expected: Vec<u8> = [10.0_f32, 0.0]
            .into_iter()
            .flat_map(f32::to_ne_bytes)
            .collect();
        assert_eq!(vector(&conn, 1)?, expected);
        let count: i64 =
            conn.query_row("SELECT count(*) FROM v WHERE rowid=4", [], |row| row.get(0))?;
        assert_eq!(count, 0);
    }
    Ok(())
}

#[test]
fn fts_and_vectors_commit_and_roll_back_together() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let path = dir.path().join("db");
    let mut conn = open(&path)?;
    create_fts(&conn)?;
    create_vectors(&conn)?;
    // When
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO ft(rowid,text) VALUES(4,'atomic')", [])?;
    tx.execute(
        "INSERT INTO v(rowid,vector) VALUES(4,vec1_from_json('[0,1]'))",
        [],
    )?;
    tx.rollback()?;
    // Then
    assert!(matches(&conn, "atomic")?.is_empty());
    assert_eq!(nearest(&conn)?, [1, 2, 3]);
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO ft(rowid,text) VALUES(4,'atomic')", [])?;
    tx.execute(
        "INSERT INTO v(rowid,vector) VALUES(4,vec1_from_json('[0,1]'))",
        [],
    )?;
    tx.commit()?;
    drop(conn);
    let conn = open(&path)?;
    assert_eq!(matches(&conn, "atomic")?, [4]);
    let expected: Vec<u8> = [0.0_f32, 1.0]
        .into_iter()
        .flat_map(f32::to_ne_bytes)
        .collect();
    assert_eq!(vector(&conn, 4)?, expected);
    Ok(())
}

#[test]
fn vector_dimension_mismatch_is_rejected_without_losing_existing_rows() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let conn = open(&dir.path().join("db"))?;
    create_vectors(&conn)?;
    // When
    assert!(
        conn.execute(
            "INSERT INTO v(rowid,vector) VALUES(?1,vec1_from_json(?2))",
            params![4, "[1,0,0]"]
        )
        .is_err()
    );
    // Then
    assert_eq!(nearest(&conn)?, [1, 2, 3]);
    Ok(())
}

#[test]
fn checked_connection_enforces_foreign_keys() -> CheckResult<()> {
    // Given
    let dir = tempfile::tempdir()?;
    let conn = open(&dir.path().join("db"))?;
    conn.execute_batch(
        "CREATE TABLE parent(id INTEGER PRIMARY KEY);
                        CREATE TABLE child(parent_id INTEGER REFERENCES parent(id));",
    )?;
    // When
    let error = conn.execute("INSERT INTO child VALUES(1)", []).unwrap_err();
    // Then
    assert_eq!(
        error.sqlite_error_code(),
        Some(rusqlite::ErrorCode::ConstraintViolation)
    );
    Ok(())
}
