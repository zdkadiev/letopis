#![allow(dead_code)]
use letopis_embedding_checks::{
    CheckResult, Config, cache, capture_config, open, publish, reset_config,
};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
pub fn hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}
pub fn fixture(root: &Path, name: &str, text: &str) -> CheckResult<()> {
    fs::write(
        root.join(name),
        format!("<!-- {{\"links\":[]}} -->\n{text}"),
    )?;
    Ok(())
}
pub fn config(root: &Path, model: &str, retries: u32) -> CheckResult<Config> {
    cache(root)?;
    fs::write(
        root.join(".letopis/config.json"),
        serde_json::to_vec(
            &serde_json::json!({"provider":"fixture","model":model,"parameters":{},"max_retries":retries}),
        )?,
    )?;
    reset_config(root, &mut |_| Ok(()))?;
    capture_config(root)
}
pub fn setup(root: &Path) -> CheckResult<Config> {
    fixture(root, "a.md", "alpha")?;
    publish(root, false, &mut |_| Ok(()))?;
    config(root, "three", 2)
}
pub fn blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_ne_bytes()).collect()
}
pub fn seed(root: &Path, text: &str, v: &[f32]) -> CheckResult<()> {
    let mut c = open(&root.join(".letopis/vectors.db"))?;
    let tx = c.transaction()?;
    tx.execute(
        "INSERT INTO vector_keys(search_hash) VALUES(?1)",
        [hash(text)],
    )?;
    let id = tx.last_insert_rowid();
    tx.execute(
        "INSERT INTO vector_index(rowid,vector) VALUES(?1,?2)",
        rusqlite::params![id, blob(v)],
    )?;
    tx.commit()?;
    Ok(())
}
pub fn state(c: &Connection) -> CheckResult<serde_json::Value> {
    Ok(
        serde_json::json!({"identity": c.query_row("SELECT config_identity FROM vec.meta",[],|r|r.get::<_,String>(0))?,
 "keys": c.prepare("SELECT row_id,search_hash FROM vec.vector_keys ORDER BY row_id")?.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?,
 "vectors":c.prepare("SELECT rowid,vector FROM vec.vector_index ORDER BY rowid")?.query_map([],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?,
 "failures":c.prepare("SELECT search_hash,attempts,error FROM vec.embed_failures ORDER BY search_hash")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,Option<String>>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?}),
    )
}
pub fn alpha(c: &Connection) -> CheckResult<Vec<String>> {
    Ok(c.prepare("SELECT id FROM nodes JOIN text_index ON nodes.row_id=text_index.rowid WHERE text_index MATCH 'alpha'")?.query_map([],|r|r.get(0))?.collect::<rusqlite::Result<_>>()?)
}

pub struct Process {
    child: std::process::Child,
    input: std::process::ChildStdin,
    lines: std::sync::mpsc::Receiver<String>,
    errors: fs::File,
}
impl Process {
    pub fn start(root: &Path, mode: &str, cuts: &str) -> CheckResult<Self> {
        use std::{
            io::BufRead,
            process::{Command, Stdio},
        };
        let errors = tempfile::tempfile()?;
        let mut child = Command::new(env!("CARGO_BIN_EXE_embedder"))
            .args([root.as_os_str(), mode.as_ref(), cuts.as_ref()])
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
            Ok(s) => s,
            Err(e) => {
                self.errors.seek(SeekFrom::Start(0)).unwrap();
                let mut err = String::new();
                self.errors.read_to_string(&mut err).unwrap();
                panic!("checkpoint EOF/timeout: {e}; {err}")
            }
        }
    }
    pub fn at(&mut self, name: &str) {
        assert_eq!(self.line(), format!("checkpoint:{name}"))
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
            if let Some(s) = self.child.try_wait()? {
                assert!(s.success(), "child exit {s}");
                break;
            }
            assert!(std::time::Instant::now() < deadline, "child exit timeout");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        Ok(())
    }
    pub fn kill(mut self) -> CheckResult<()> {
        use std::os::unix::process::ExitStatusExt;
        self.child.kill()?;
        assert_eq!(self.child.wait()?.signal(), Some(libc::SIGKILL));
        Ok(())
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
pub fn requests(root: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(root.join(".letopis/requests.jsonl"))
        .unwrap_or_default()
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect()
}
pub fn attempts(root: &Path, text: &str) -> CheckResult<i64> {
    let c = open(&root.join(".letopis/vectors.db"))?;
    Ok(c.query_row(
        "SELECT COALESCE((SELECT attempts FROM embed_failures WHERE search_hash=?1),0)",
        [hash(text)],
        |r| r.get(0),
    )?)
}
pub fn work(root: &Path, cfg: &Config) -> CheckResult<Vec<letopis_embedding_checks::WorkItem>> {
    letopis_embedding_checks::pending(
        &letopis_embedding_checks::attached(root)?,
        cfg,
        &Default::default(),
    )
}

pub fn stored(c: &Connection) -> CheckResult<std::collections::BTreeMap<String, Vec<u8>>> {
    Ok(c.prepare("SELECT k.search_hash,v.vector FROM vec.vector_keys k JOIN vec.vector_index v ON v.rowid=k.row_id ORDER BY k.search_hash")?.query_map([],|r|Ok((r.get(0)?,r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
}
