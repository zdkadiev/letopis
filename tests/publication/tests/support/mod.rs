#![allow(dead_code)]
use letopis_publication_checks::{CheckResult, cache, publish};
use rusqlite::{Connection, types::ValueRef};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

pub fn fixture(root: &Path, name: &str, text: &str, links: &[(&str, &str)]) -> CheckResult<()> {
    let path = root.join(name);
    fs::create_dir_all(path.parent().unwrap())?;
    fs::write(
        path,
        format!("<!-- {} -->\n{text}", json!({"links": links})),
    )?;
    Ok(())
}

pub fn run(root: &Path) -> CheckResult<()> {
    publish(root, false, &mut |_| Ok(()))
}

pub fn rows(conn: &Connection, sql: &str) -> CheckResult<Vec<Vec<Value>>> {
    let mut stmt = conn.prepare(sql)?;
    let count = stmt.column_count();
    Ok(stmt
        .query_map([], |row| {
            (0..count)
                .map(|i| {
                    Ok(match row.get_ref(i)? {
                        ValueRef::Null => Value::Null,
                        ValueRef::Integer(v) => json!(v),
                        ValueRef::Real(v) => json!(v),
                        ValueRef::Text(v) => json!(String::from_utf8_lossy(v)),
                        ValueRef::Blob(v) => json!(v),
                    })
                })
                .collect()
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?)
}

pub fn snapshot(conn: &Connection) -> CheckResult<Value> {
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.words USING fts5vocab(main,text_index,'instance')",
    )?;
    Ok(json!({
        "revision": rows(conn,"SELECT * FROM project ORDER BY id")?,
        "nodes": rows(conn,"SELECT * FROM nodes ORDER BY id")?,
        "edges": rows(conn,"SELECT * FROM edges ORDER BY source_id,target_id,type")?,
        "files": rows(conn,"SELECT * FROM files ORDER BY path")?,
        "fts": rows(conn,"SELECT * FROM words ORDER BY term,doc,col,offset")?,
    }))
}

pub fn corpus(root: &Path) -> CheckResult<BTreeMap<String, Vec<u8>>> {
    fn walk(base: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) -> CheckResult<()> {
        for entry in fs::read_dir(dir)? {
            let path = entry?.path();
            if path.file_name().unwrap() == ".letopis" {
                let config = path.join("config.json");
                if config.is_file() {
                    out.insert(".letopis/config.json".into(), fs::read(config)?);
                }
                continue;
            }
            if path.is_dir() {
                walk(base, &path, out)?;
            } else {
                out.insert(
                    path.strip_prefix(base)?.to_string_lossy().into_owned(),
                    fs::read(path)?,
                );
            }
        }
        Ok(())
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

pub fn revision(conn: &Connection) -> CheckResult<i64> {
    Ok(conn.query_row("SELECT revision FROM project", [], |r| r.get(0))?)
}

pub fn matches(conn: &Connection, text: &str) -> CheckResult<Vec<String>> {
    Ok(conn.prepare("SELECT id FROM nodes JOIN text_index ON nodes.row_id=text_index.rowid WHERE text_index MATCH ?1 ORDER BY id")?
        .query_map([text],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

pub fn assert_nodes(
    conn: &Connection,
    root: &Path,
    expected: &[(&str, &[u8], &str)],
    rev: i64,
) -> CheckResult<()> {
    let actual = rows(
        conn,
        "SELECT id,kind,uri,hash,created_at,source_snapshot,line_start,line_end,search_hash FROM nodes ORDER BY id",
    )?;
    let mut want: Vec<_> = expected
        .iter()
        .map(|(path, bytes, text)| {
            vec![
                json!(path),
                json!("vertex"),
                json!(path),
                json!(format!("{:x}", Sha256::digest(bytes))),
                json!(0),
                Value::Null,
                json!(1),
                json!(1),
                json!(format!("{:x}", Sha256::digest(text.as_bytes()))),
            ]
        })
        .collect();
    want.sort_by_key(|r| r[0].to_string());
    assert_eq!(actual, want);
    assert_eq!(revision(conn)?, rev);
    let mut want_words = Vec::new();
    for (path, _, text) in expected {
        let id: i64 =
            conn.query_row("SELECT row_id FROM nodes WHERE id=?1", [path], |r| r.get(0))?;
        assert!(id > 0);
        // All these hand-checked fixtures contain one lowercase ASCII token.
        assert!(text.bytes().all(|b| b.is_ascii_lowercase()));
        want_words.push(vec![json!(text), json!(id), json!("text"), json!(0)]);
    }
    want_words.sort_by_key(|r| (r[0].to_string(), r[1].as_i64().unwrap()));
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS temp.words USING fts5vocab(main,text_index,'instance')",
    )?;
    assert_eq!(
        rows(conn, "SELECT * FROM words ORDER BY term,doc,col,offset")?,
        want_words
    );
    let file_rows = rows(
        conn,
        "SELECT path,size,mtime_ns,inode,hash,valid,error FROM files ORDER BY path",
    )?;
    let current = corpus(root)?;
    use std::os::unix::fs::MetadataExt;
    let mut want_files = Vec::new();
    for (path, bytes) in current
        .iter()
        .filter(|(p, _)| p.ends_with(".md") || p.ends_with(".markdown"))
    {
        let m = fs::metadata(root.join(path))?;
        let valid = bytes.starts_with(b"<!-- {\"links\":");
        want_files.push(vec![
            json!(path),
            json!(bytes.len()),
            json!(m.mtime() * 1_000_000_000 + m.mtime_nsec()),
            json!(m.ino()),
            json!(format!("{:x}", Sha256::digest(bytes))),
            json!(i64::from(valid)),
            if valid {
                Value::Null
            } else {
                json!("invalid fixture metadata")
            },
        ]);
    }
    assert_eq!(file_rows, want_files);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos() as i64;
    let invalid_clocks: i64 = conn.query_row(
        "SELECT count(*) FROM files WHERE checked_at_ns<=0 OR checked_at_ns>?1",
        [now],
        |r| r.get(0),
    )?;
    assert_eq!(invalid_clocks, 0);
    Ok(())
}

pub fn base(root: &Path) -> CheckResult<()> {
    fixture(root, "a.md", "alpha", &[])?;
    fixture(root, "b.md", "beta", &[])?;
    cache(root)?;
    run(root)
}

pub struct Process {
    child: std::process::Child,
    input: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<String>,
    errors: fs::File,
}

impl Process {
    pub fn start(root: &Path, mode: &str, checkpoints: &str) -> CheckResult<Self> {
        use std::{
            io::BufRead,
            process::{Command, Stdio},
        };
        let errors = tempfile::tempfile()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_publisher"))
            .args([root.as_os_str(), mode.as_ref(), checkpoints.as_ref()])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(errors.try_clone()?)
            .spawn()?;
        let input = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let (tx, lines) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for line in std::io::BufReader::new(stdout).lines() {
                match line {
                    Ok(line) => {
                        if tx.send(line).is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        });
        Ok(Self {
            child,
            input,
            lines,
            errors,
        })
    }
    fn line(&mut self) -> String {
        use std::io::{Read, Seek, SeekFrom};
        match self.lines.recv_timeout(std::time::Duration::from_secs(20)) {
            Ok(line) => line,
            Err(e) => {
                let mut error = String::new();
                self.errors.seek(SeekFrom::Start(0)).unwrap();
                self.errors.read_to_string(&mut error).unwrap();
                panic!("child checkpoint timeout/EOF: {e}; stderr={error}")
            }
        }
    }
    pub fn at(&mut self, name: &str) {
        assert_eq!(self.line(), format!("checkpoint:{name}"));
    }
    pub fn resume(&mut self) -> CheckResult<()> {
        use std::io::Write;
        self.input.write_all(b"continue\n")?;
        self.input.flush()?;
        Ok(())
    }
    pub fn finish(mut self) -> CheckResult<()> {
        assert_eq!(self.line(), "done");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            if let Some(status) = self.child.try_wait()? {
                assert!(status.success(), "child exit: {status}");
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child did not exit after done"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
    pub fn kill(mut self) -> CheckResult<()> {
        use std::os::unix::process::ExitStatusExt;
        self.child.kill()?;
        let status = self.child.wait()?;
        assert_eq!(status.signal(), Some(libc::SIGKILL));
        Ok(())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn change_fixture(root: &Path) -> CheckResult<Vec<u8>> {
    fixture(root, "a.md", "alpha", &[])?;
    fixture(root, "b.md", "beta", &[])?;
    fixture(root, "c.md", "charlie", &[("a.md", "rel")])?;
    run(root)?;
    let a = fs::read(root.join("a.md"))?;
    fs::write(root.join("a.md"), "<!-- {broken -->\ninvalid")?;
    fixture(root, "b.md", "bravo", &[("d.md", "rel")])?;
    fs::remove_file(root.join("c.md"))?;
    fixture(root, "d.md", "delta", &[])?;
    Ok(a)
}

pub fn assert_changed(conn: &Connection, root: &Path, a: &[u8], rev: i64) -> CheckResult<()> {
    assert_nodes(
        conn,
        root,
        &[
            ("a.md", a, "alpha"),
            ("b.md", &fs::read(root.join("b.md"))?, "bravo"),
            ("d.md", &fs::read(root.join("d.md"))?, "delta"),
        ],
        rev,
    )?;
    for (term, name) in [("alpha", "a.md"), ("bravo", "b.md"), ("delta", "d.md")] {
        assert_eq!(matches(conn, term)?, [name]);
    }
    assert!(matches(conn, "beta OR charlie OR invalid")?.is_empty());
    assert_eq!(
        rows(
            conn,
            "SELECT source_id,target_id,type FROM edges ORDER BY source_id,target_id,type"
        )?,
        vec![vec![json!("b.md"), json!("d.md"), json!("rel")]]
    );
    Ok(())
}

pub fn config_fixture(root: &Path, model: &str) -> CheckResult<()> {
    fs::write(
        root.join(".letopis/config.json"),
        format!(
            "{{\"provider\":\"fixture\",\"model\":\"{model}\",\"parameters\":{{}},\"max_retries\":3}}"
        ),
    )?;
    Ok(())
}

pub fn seed_vectors(root: &Path) -> CheckResult<()> {
    let conn = letopis_publication_checks::open(&root.join(".letopis/vectors.db"))?;
    conn.execute_batch("BEGIN;
        CREATE TABLE meta(id INTEGER PRIMARY KEY CHECK(id=1),config_identity TEXT NOT NULL);
        CREATE TABLE vector_keys(row_id INTEGER PRIMARY KEY,search_hash TEXT NOT NULL UNIQUE);
        CREATE TABLE embed_failures(search_hash TEXT PRIMARY KEY,attempts INTEGER NOT NULL,error TEXT);
        CREATE VIRTUAL TABLE vector_index USING vec1(vector);
        INSERT INTO vector_index(rowid,vector) VALUES(1,vec1_from_json('[1,0]'));
        INSERT INTO vector_index(cmd,arg) VALUES('rebuild','{index:\"none\",distance:\"cos\"}');
        INSERT INTO meta VALUES(1,'old-identity');
        INSERT INTO vector_keys VALUES(1,'old-text');
        INSERT INTO embed_failures VALUES('failed',2,'fixture failure');COMMIT;")?;
    Ok(())
}

pub fn vector_snapshot(root: &Path) -> CheckResult<Value> {
    let conn = letopis_publication_checks::open(&root.join(".letopis/vectors.db"))?;
    Ok(json!({
        "meta":rows(&conn,"SELECT * FROM meta ORDER BY id")?,
        "keys":rows(&conn,"SELECT * FROM vector_keys ORDER BY row_id")?,
        "failures":rows(&conn,"SELECT * FROM embed_failures ORDER BY search_hash")?,
        "vectors":rows(&conn,"SELECT rowid,vector FROM vector_index ORDER BY rowid")?,
        "schema":rows(&conn,"SELECT name,type,sql FROM sqlite_master ORDER BY name")?,
    }))
}

pub fn assert_reset(root: &Path) -> CheckResult<()> {
    let actual = vector_snapshot(root)?;
    // Literal canonical bytes for the deliberately string/integer-only config fixture.
    let canonical =
        b"{\"max_retries\":3,\"model\":\"new\",\"parameters\":{},\"provider\":\"fixture\"}";
    assert_eq!(
        actual["meta"],
        json!([[1, format!("{:x}", Sha256::digest(canonical))]])
    );
    for key in ["keys", "failures", "vectors"] {
        assert_eq!(actual[key], json!([]), "{key}");
    }
    let conn = letopis_publication_checks::open(&root.join(".letopis/vectors.db"))?;
    let sql: String = conn.query_row(
        "SELECT sql FROM sqlite_master WHERE name='vector_index'",
        [],
        |r| r.get(0),
    )?;
    assert!(sql.contains("USING vec1(vector)"));
    Ok(())
}
