//! TK-2 scratch publication harness, not the production storage/Markdown implementation.
pub use letopis_storage_build::CheckResult;
mod config;
mod corpus;
mod graph;
mod publication;
pub use config::reset_config;
pub use graph::{diagnostics, traverse};
pub use publication::publish;
use rusqlite::Connection;
use std::{env, fs, path::Path, time::Duration};

pub fn open(path: &Path) -> CheckResult<Connection> {
    let extension = env::var_os("LETOPIS_VEC1_EXTENSION").ok_or("run tests/publication/run.py")?;
    // SAFETY: the runner supplies its checksum-verified, freshly compiled vec1 library.
    let conn = unsafe { letopis_storage_build::open_checked(path, Path::new(&extension))? };
    conn.busy_timeout(Duration::from_secs(10))?;
    Ok(conn)
}

pub fn cache(root: &Path) -> CheckResult<Connection> {
    use std::os::unix::fs::DirBuilderExt;
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(root.join(".letopis"))?;
    let conn = open(&root.join(".letopis/cache.db"))?;
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='project')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        conn.execute_batch("BEGIN IMMEDIATE")?;
        let exists: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='project')",
            [],
            |r| r.get(0),
        )?;
        if !exists {
            conn.execute_batch(include_str!("../schema.sql"))?;
            conn.execute("INSERT INTO project(id,revision) VALUES(1,0)", [])?;
        }
        conn.execute_batch("COMMIT")?;
    }
    Ok(conn)
}
