// Throwaway integration research; this is not Engram product code.
use rusqlite::{Connection, ErrorCode, LoadExtensionGuard, OpenFlags, params};
use std::{
    env,
    error::Error,
    path::Path,
    thread,
    time::{Duration, Instant},
};

type ProbeResult<T> = Result<T, Box<dyn Error>>;

fn main() -> ProbeResult<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() != 4 {
        return Err("usage: probe NEW_DB GO_DATASET_DB VEC1_EXTENSION".into());
    }
    if Path::new(&args[1]).exists() {
        return Err("new probe database path must not exist".into());
    }
    let mut a = open(&args[1], &args[3], false)?;
    let mut b = open(&args[1], &args[3], false)?;
    for (i, c) in [&a, &b].into_iter().enumerate() {
        let sqlite: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
        let vec1: String = c.query_row("SELECT vec1_info()", [], |r| r.get(0))?;
        println!("config conn={i} sqlite={sqlite} vec1={vec1}");
        assert!(rusqlite::version_number() >= 3_051_003);
        let journal: String = c.pragma_query_value(None, "journal_mode", |r| r.get(0))?;
        assert_eq!(journal, "wal");
        for (name, expected) in [("synchronous", 2), ("fullfsync", 1), ("foreign_keys", 1)] {
            let value: i64 = c.pragma_query_value(None, name, |r| r.get(0))?;
            assert_eq!(value, expected);
            println!("config conn={i} pragma={name} value={value}");
        }
    }
    a.execute_batch(r#"CREATE TABLE nodes(id INTEGER PRIMARY KEY, selected INTEGER);
        INSERT INTO nodes VALUES(1,0),(2,1),(3,0);
        CREATE VIRTUAL TABLE ft USING fts5(text, content='', contentless_delete=1);
        INSERT INTO ft(rowid,text) VALUES(1,'persistent memory');
        CREATE VIRTUAL TABLE v USING vec1(vector);
        INSERT INTO v(rowid,vector) VALUES(1,vec1_from_json('[1,0]')),(2,vec1_from_json('[0,1]')),(3,vec1_from_json('[-1,0]'));
        INSERT INTO v(cmd,arg) VALUES('rebuild','{"index":"flat","distance":"cos"}');"#)?;
    let result: (i64, Option<String>) = a.query_row(
        "SELECT rowid,text FROM ft WHERE ft MATCH 'memory'",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(result, (1, None));
    a.execute("DELETE FROM ft WHERE rowid=1", [])?;
    let count: i64 = a.query_row("SELECT count(*) FROM ft WHERE ft MATCH 'memory'", [], |r| {
        r.get(0)
    })?;
    assert_eq!(count, 0);
    println!("check contentless_fts_delete=PASS");
    let query = blob(&[1.0, 0.0]);
    assert_eq!(nearest(&a, "v", &query)?, 1);
    assert_eq!(nearest(&b, "v", &query)?, 1);
    println!("check extension_two_connections=PASS");
    {
        let tx = b.transaction()?;
        tx.execute(
            "UPDATE v SET vector=vec1_from_json('[0,-1]') WHERE rowid=1",
            [],
        )?;
        // Leaving the scope without commit intentionally tests RAII rollback.
    }
    assert_eq!(nearest(&b, "v", &query)?, 1);
    println!("check vector_raii_rollback=PASS");
    let read = a.transaction()?;
    let before: Vec<u8> = read.query_row("SELECT vector FROM v WHERE rowid=1", [], |r| r.get(0))?;
    b.execute(
        "UPDATE v SET vector=vec1_from_json('[0,-1]') WHERE rowid=1",
        [],
    )?;
    let after: Vec<u8> = read.query_row("SELECT vector FROM v WHERE rowid=1", [], |r| r.get(0))?;
    assert_eq!(before, after);
    assert_eq!(nearest(&read, "v", &query)?, 1);
    assert_eq!(nearest(&b, "v", &query)?, 2);
    read.commit()?;
    println!("check wal_vector_snapshot=PASS");
    b.execute(
        "UPDATE v SET vector=vec1_from_json('[1,0]') WHERE rowid=1",
        [],
    )?;
    for (options, expected) in [(r#"{"K":1}"#, 0), (r#"{"K":1,"streaming":1}"#, 1)] {
        let count: i64 = b.query_row("SELECT count(*) FROM (SELECT vv.rowid FROM v(?1,?2) vv JOIN nodes n ON n.id=vv.rowid WHERE n.selected=1 LIMIT 1)", params![&query, options], |r| r.get(0))?;
        assert_eq!(count, expected);
        println!("filter options={options} count={count}");
    }
    println!("check vector_filters=PASS");
    b.execute("DELETE FROM v WHERE rowid=3", [])?;
    let count: i64 = b.query_row("SELECT count(*) FROM v", [], |r| r.get(0))?;
    assert_eq!(count, 2);
    drop(b);
    let _reopened = open(&args[1], &args[3], false)?;
    println!("check extension_reopened_connection=PASS");
    let dataset = open(&args[2], &args[3], true)?;
    let count: i64 = dataset.query_row("SELECT count(*) FROM big", [], |r| r.get(0))?;
    assert_eq!(count, 50_000);
    let first_vector: Vec<u8> =
        dataset.query_row("SELECT vector FROM big WHERE rowid=1", [], |r| r.get(0))?;
    assert_eq!(first_vector.len(), 384 * 4);
    let started = Instant::now();
    {
        let tx = a.transaction()?;
        tx.execute_batch("CREATE VIRTUAL TABLE big USING vec1(vector)")?;
        let mut insert = tx.prepare("INSERT INTO big(rowid,vector) VALUES(?1,?2)")?;
        let mut select = dataset.prepare("SELECT rowid,vector FROM big")?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let id: i64 = row.get(0)?;
            let vector: Vec<u8> = row.get(1)?;
            insert.execute(params![id, vector])?;
        }
        drop(insert);
        tx.commit()?;
    }
    println!(
        "load from_go_dataset vectors={count} dim=384 elapsed={:?}",
        started.elapsed()
    );
    let started = Instant::now();
    a.execute(
        "INSERT INTO big(cmd,arg) VALUES('rebuild','{\"index\":\"flat\",\"distance\":\"cos\"}')",
        [],
    )?;
    println!("rebuild elapsed={:?}", started.elapsed());
    for i in 0..5 {
        let started = Instant::now();
        let mut statement = a.prepare("SELECT rowid FROM big(?1,'{\"K\":10}')")?;
        let ids = statement
            .query_map([&first_vector], |r| r.get::<_, i64>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        assert_eq!(ids.len(), 10);
        assert_eq!(ids[0], 1);
        println!(
            "search iteration={i} rows={} elapsed={:?}",
            ids.len(),
            started.elapsed()
        );
    }
    interrupted(&a, "core_sql", || {
        a.query_row("WITH RECURSIVE seq(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM seq WHERE x<100000000) SELECT sum(x) FROM seq", [], |r| r.get::<_, i64>(0))
    })?;
    interrupted(&a, "vector_search", || nearest(&a, "big", &first_vector))?;
    interrupted(&a, "flat_rebuild", || {
        a.execute("INSERT INTO big(cmd,arg) VALUES('rebuild','{\"index\":\"flat\",\"distance\":\"cos\"}')", [])
    })?;
    let count: i64 = a.query_row("SELECT count(*) FROM big", [], |r| r.get(0))?;
    assert_eq!(count, 50_000);
    assert_eq!(nearest(&a, "big", &first_vector)?, 1);
    println!("check index_after_cancellation=PASS");
    println!("complete rusqlite_probe=PASS");
    Ok(())
}

fn open(path: &str, extension: &str, read_only: bool) -> ProbeResult<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let c = Connection::open_with_flags(path, flags)?;
    // SAFETY: research uses the previously built, checksum-verified vec1 binary.
    unsafe {
        let _guard = LoadExtensionGuard::new(&c)?;
        c.load_extension(extension, Some("sqlite3_extension_init"))?;
    }
    if !read_only {
        c.pragma_update(None, "journal_mode", "WAL")?;
    }
    c.pragma_update(None, "synchronous", "FULL")?;
    c.pragma_update(None, "fullfsync", true)?;
    c.pragma_update(None, "foreign_keys", true)?;
    c.busy_timeout(Duration::from_secs(5))?;
    Ok(c)
}

fn nearest(c: &Connection, table: &str, query: &[u8]) -> rusqlite::Result<i64> {
    // Table names come only from fixed constants in this probe.
    c.query_row(
        &format!("SELECT rowid FROM {table}(?1,'{{\"K\":1}}')"),
        [query],
        |r| r.get(0),
    )
}

fn interrupted<T>(
    c: &Connection,
    label: &str,
    f: impl FnOnce() -> rusqlite::Result<T>,
) -> ProbeResult<()> {
    let handle = c.get_interrupt_handle();
    let started = Instant::now();
    let outcome = thread::scope(|scope| {
        scope.spawn(move || {
            thread::sleep(Duration::from_millis(1));
            handle.interrupt();
        });
        f()
    });
    let error = outcome.err().ok_or("expected SQLite interruption")?;
    assert_eq!(
        error.sqlite_error_code(),
        Some(ErrorCode::OperationInterrupted)
    );
    println!(
        "cancel label={label} elapsed={:?} error={error}",
        started.elapsed()
    );
    println!("check cancel_{label}=PASS");
    Ok(())
}

fn blob(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
