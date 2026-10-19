// Isolated capacity experiment; not Engram implementation.
use rusqlite::{Connection, LoadExtensionGuard, OpenFlags, params};
use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    path::Path,
    thread,
    time::{Duration, Instant},
};
type R<T> = Result<T, Box<dyn Error>>;
const N: usize = 100_000;
const DIM: usize = 768;
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9e3779b97f4a7c15);
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
fn id(i: usize, domain: u64) -> String {
    let a = mix(i as u64 ^ domain);
    let b = mix(a);
    format!(
        "{:08x}-{:04x}-4{:03x}-{:04x}-{:012x}",
        (a >> 32) as u32,
        (a >> 16) as u16,
        a & 0xfff,
        0x8000 | ((b >> 48) & 0x3fff),
        b & 0xffffffffffff
    )
}
fn vector(i: usize) -> Vec<u8> {
    let mut state = mix(i as u64 + 9801);
    let mut v = [0f32; DIM];
    let mut norm = 0f32;
    for x in &mut v {
        state = mix(state);
        *x = ((state >> 40) as f32 / (1u32 << 24) as f32) * 2. - 1.;
        norm += *x * *x;
    }
    let norm = norm.sqrt();
    v.iter().flat_map(|x| (x / norm).to_ne_bytes()).collect()
}
fn text(i: usize) -> String {
    const WORDS: &[&str] = &[
        "agent",
        "memory",
        "knowledge",
        "revision",
        "snapshot",
        "constraint",
        "document",
        "graph",
        "vertex",
        "link",
        "index",
        "search",
        "transaction",
        "validation",
        "atomic",
        "history",
        "evidence",
        "project",
        "function",
        "module",
        "interface",
        "contract",
        "requirement",
        "implementation",
        "error",
        "recovery",
        "queue",
        "embedding",
        "model",
        "local",
        "markdown",
        "repository",
        "context",
        "relation",
        "result",
        "schema",
        "operation",
        "semantic",
        "reference",
        "persistent",
        "storage",
        "architecture",
        "assumption",
        "code",
        "service",
        "event",
        "domain",
        "metadata",
        "version",
        "paragraph",
        "identifier",
        "boundary",
        "system",
        "analysis",
        "task",
        "session",
        "rule",
        "source",
    ];
    let mut s = format!("graph memory needle{i:06} document knowledge\n");
    let mut state = mix(i as u64 + 4201);
    while s.len() < 2000 {
        state = mix(state);
        s.push_str(WORDS[(state as usize) % WORDS.len()]);
        s.push(' ');
    }
    s.truncate(2000);
    assert_eq!(s.len(), 2000);
    s
}
fn bytes(p: &str) -> u64 {
    fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}
fn emit(c: &Connection, path: &str, phase: &str, t: Instant) -> R<()> {
    let used = unsafe { rusqlite::ffi::sqlite3_memory_used() };
    let high = unsafe { rusqlite::ffi::sqlite3_memory_highwater(0) };
    let pages: i64 = c.pragma_query_value(None, "page_count", |r| r.get(0))?;
    println!(
        "{{\"event\":\"phase\",\"phase\":\"{phase}\",\"pid\":{},\"elapsed_s\":{},\"db_bytes\":{},\"wal_bytes\":{},\"shm_bytes\":{},\"sqlite_heap_bytes\":{used},\"sqlite_heap_highwater_bytes\":{high},\"page_count\":{pages}}}",
        std::process::id(),
        t.elapsed().as_secs_f64(),
        bytes(path),
        bytes(&format!("{path}-wal")),
        bytes(&format!("{path}-shm"))
    );
    io::stdout().flush()?;
    thread::sleep(Duration::from_millis(300));
    Ok(())
}
fn open(path: &str, ext: &str, read_only: bool) -> R<Connection> {
    let flags = if read_only {
        OpenFlags::SQLITE_OPEN_READ_ONLY
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let c = Connection::open_with_flags(path, flags)?;
    unsafe {
        let _guard = LoadExtensionGuard::new(&c)?;
        c.load_extension(ext, Some("sqlite3_extension_init"))?;
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
fn config(c: &Connection) -> R<()> {
    let version: String = c.query_row("SELECT sqlite_version()", [], |r| r.get(0))?;
    let vec: String = c.query_row("SELECT vec1_info()", [], |r| r.get(0))?;
    assert!(rusqlite::version_number() >= 3_051_003);
    println!(
        "{{\"event\":\"config\",\"sqlite\":{version:?},\"vec1\":{vec:?},\"pid\":{}}}",
        std::process::id()
    );
    for (p, want) in [("synchronous", 2), ("fullfsync", 1), ("foreign_keys", 1)] {
        let got: i64 = c.pragma_query_value(None, p, |r| r.get(0))?;
        assert_eq!(got, want);
        println!("{{\"event\":\"pragma\",\"name\":\"{p}\",\"value\":{got}}}");
    }
    for p in ["cache_size", "page_size", "mmap_size", "wal_autocheckpoint"] {
        let got: i64 = c.pragma_query_value(None, p, |r| r.get(0))?;
        println!("{{\"event\":\"pragma\",\"name\":\"{p}\",\"value\":{got}}}");
    }
    Ok(())
}
fn verify(c: &Connection) -> R<()> {
    for (table, want) in [
        ("nodes", 200000),
        ("edges", 500000),
        ("text_index", 100000),
        ("vector_index", 100000),
    ] {
        let got: i64 = c.query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
        assert_eq!(got, want);
        println!("{{\"event\":\"count\",\"table\":\"{table}\",\"rows\":{got}}}");
    }
    let count: i64 = c.query_row("SELECT count(*) FROM nodes WHERE kind='source'", [], |r| {
        r.get(0)
    })?;
    assert_eq!(count, 100000);
    let count: i64 = c.query_row(
        "SELECT count(*) FROM text_index WHERE text IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);
    let count: i64 = c.query_row(
        "SELECT count(*) FROM sqlite_master WHERE name='text_index_content'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(count, 0);
    let mut st = c.prepare("PRAGMA foreign_key_check")?;
    assert!(st.query([])?.next()?.is_none());
    let integrity: String = c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    assert_eq!(integrity, "ok");
    println!(
        "{{\"event\":\"verify\",\"result\":\"PASS\",\"contentless_text_null\":true,\"foreign_keys\":true}}"
    );
    Ok(())
}
fn load(path: &str, ext: &str) -> R<()> {
    if Path::new(path).exists() {
        return Err("new DB path must not exist".into());
    }
    let t = Instant::now();
    let mut c = open(path, ext, false)?;
    config(&c)?;
    emit(&c, path, "load_open", t)?;
    c.execute_batch(include_str!("../schema.sql"))?;
    let ids: Vec<String> = (1..=2 * N).map(|i| id(i, 0x5eeda123)).collect();
    for base in (0..2 * N).step_by(5000) {
        let tx = c.transaction()?;
        {
            let mut ins=tx.prepare("INSERT INTO nodes(id,kind,uri,hash,line_start,line_end,created_at,source_snapshot) VALUES(?1,?2,?3,?4,?5,?6,1792320000,?7)")?;
            let mut ft = tx.prepare("INSERT INTO text_index(rowid,text) VALUES(?1,?2)")?;
            for i in base..(base + 5000).min(2 * N) {
                let knowledge = i < N;
                let kind = if !knowledge {
                    "source"
                } else if (i + 1) % 20 == 0 {
                    "metavertex"
                } else {
                    "vertex"
                };
                let uri = if knowledge {
                    format!("docs/knowledge-{:04}.md#{}", i / 30, ids[i])
                } else {
                    format!(
                        "src/module_{:05}/component_{:06}.rs#L10-L30",
                        (i - N) / 50,
                        i - N
                    )
                };
                let hash = format!("{:064x}", mix(i as u64));
                let snapshot = if knowledge {
                    Some("0123456789abcdef0123456789abcdef01234567")
                } else {
                    None
                };
                ins.execute(params![
                    &ids[i],
                    kind,
                    uri,
                    hash,
                    (if knowledge { 1 + (i % 30) * 40 } else { 10 }) as i64,
                    (if knowledge { 40 + (i % 30) * 40 } else { 30 }) as i64,
                    snapshot
                ])?;
                if knowledge {
                    ft.execute(params![(i + 1) as i64, text(i)])?;
                }
            }
        }
        tx.commit()?;
        if base % 25000 == 0 {
            emit(&c, path, "loading_nodes_fts", t)?;
        }
    }
    emit(&c, path, "nodes_fts_loaded", t)?;
    for base in (0..N).step_by(5000) {
        let tx = c.transaction()?;
        {
            let mut ins=tx.prepare("INSERT INTO edges(id,source_id,target_id,type,hash,created_at) VALUES(?1,?2,?3,?4,?5,1792320000)")?;
            for i in base..(base + 5000).min(N) {
                for j in 0..5 {
                    let target = if j == 4 { N + i } else { (i + 1 + j * 7) % N };
                    let kind = if j == 4 {
                        "external"
                    } else if j == 0 && (i + 1) % 20 == 0 {
                        "includes"
                    } else {
                        "related"
                    };
                    let edge_i = i * 5 + j;
                    ins.execute(params![
                        id(edge_i, 0xbedda901),
                        &ids[i],
                        &ids[target],
                        kind,
                        format!("{:064x}", mix(edge_i as u64))
                    ])?;
                }
            }
        }
        tx.commit()?;
        if base % 25000 == 0 {
            emit(&c, path, "loading_edges", t)?;
        }
    }
    drop(ids);
    emit(&c, path, "graph_loaded", t)?;
    for base in (0..N).step_by(5000) {
        let tx = c.transaction()?;
        {
            let mut ins = tx.prepare("INSERT INTO vector_index(rowid,vector) VALUES(?1,?2)")?;
            for i in base..(base + 5000).min(N) {
                ins.execute(params![(i + 1) as i64, vector(i)])?;
            }
        }
        tx.commit()?;
        if base % 25000 == 0 {
            emit(&c, path, "loading_vectors", t)?;
        }
    }
    emit(&c, path, "vectors_unindexed", t)?;
    let start = Instant::now();
    c.execute("INSERT INTO vector_index(cmd,arg) VALUES('rebuild','{\"index\":\"flat\",\"distance\":\"cos\"}')",[])?;
    println!(
        "{{\"event\":\"rebuild\",\"ms\":{}}}",
        start.elapsed().as_secs_f64() * 1000.
    );
    emit(&c, path, "flat_rebuilt", t)?;
    verify(&c)?;
    emit(&c, path, "load_verified", t)?;
    let r: (i64, i64, i64) = c.query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |r| {
        Ok((r.get(0)?, r.get(1)?, r.get(2)?))
    })?;
    assert_eq!(r.0, 0);
    emit(&c, path, "checkpoint_done", t)?;
    println!(
        "{{\"event\":\"complete\",\"mode\":\"load\",\"seconds\":{}}}",
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
fn timed(
    c: &Connection,
    label: &str,
    count: usize,
    mut f: impl FnMut(&Connection, usize) -> R<usize>,
) -> R<()> {
    for i in 0..count {
        let t = Instant::now();
        let rows = f(c, i)?;
        println!(
            "{{\"event\":\"query\",\"kind\":\"{label}\",\"i\":{i},\"ms\":{},\"rows\":{rows}}}",
            t.elapsed().as_secs_f64() * 1000.
        );
    }
    io::stdout().flush()?;
    Ok(())
}
fn read(path: &str, ext: &str) -> R<()> {
    let t = Instant::now();
    let c = open(path, ext, true)?;
    config(&c)?;
    emit(&c, path, "read_open", t)?;
    timed(&c, "fts_rare", 30, |c, i| {
        let id = (i * 1901) % N;
        let mut s = c.prepare(
            "SELECT rowid FROM text_index WHERE text_index MATCH ?1 ORDER BY rank LIMIT 10",
        )?;
        let a = s
            .query_map([format!("needle{id:06}")], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(a, vec![(id + 1) as i64]);
        Ok(a.len())
    })?;
    emit(&c, path, "fts_rare_done", t)?;
    timed(&c, "fts_common", 30, |c, _| {
        let mut s=c.prepare("SELECT rowid FROM text_index WHERE text_index MATCH 'graph AND memory' ORDER BY rank LIMIT 10")?;
        let a = s
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(a.len(), 10);
        Ok(a.len())
    })?;
    emit(&c, path, "fts_common_done", t)?;
    timed(&c, "vector_k10", 30, |c, i| {
        let known = 1 + ((i * 1901) % N) as i64;
        let q: Vec<u8> = c.query_row(
            "SELECT vector FROM vector_index WHERE rowid=?1",
            [known],
            |r| r.get(0),
        )?;
        let mut s = c.prepare("SELECT rowid FROM vector_index(?1,'{\"K\":10}')")?;
        let a = s
            .query_map([&q], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        assert_eq!(a.len(), 10);
        assert_eq!(a[0], known);
        Ok(a.len())
    })?;
    emit(&c, path, "vector_done", t)?;
    for (label, column) in [("traverse_out", "source_id"), ("traverse_in", "target_id")] {
        timed(&c, label, 30, |c, i| {
            let mut s=c.prepare(&format!("WITH RECURSIVE walk(id,depth) AS (SELECT ?1,0 UNION ALL SELECT e.{} ,w.depth+1 FROM walk w JOIN edges e ON e.{column}=w.id WHERE w.depth<3) SELECT DISTINCT id FROM walk WHERE depth>0 AND id<>?1 LIMIT 100",if column=="source_id"{"target_id"}else{"source_id"}))?;
            let a = s
                .query_map([id(1 + (i * 1901) % N, 0x5eeda123)], |r| {
                    r.get::<_, String>(0)
                })?
                .collect::<Result<Vec<_>, _>>()?;
            assert!(!a.is_empty());
            Ok(a.len())
        })?;
    }
    emit(&c, path, "traverse_done", t)?;
    verify(&c)?;
    emit(&c, path, "read_verified", t)?;
    println!(
        "{{\"event\":\"complete\",\"mode\":\"read\",\"seconds\":{}}}",
        t.elapsed().as_secs_f64()
    );
    Ok(())
}
fn main() -> R<()> {
    let a: Vec<String> = env::args().collect();
    if a.len() != 4 {
        return Err("usage: PROBE load|read DB EXTENSION".into());
    }
    match a[1].as_str() {
        "load" => load(&a[2], &a[3]),
        "read" => read(&a[2], &a[3]),
        _ => Err("unknown mode".into()),
    }
}
