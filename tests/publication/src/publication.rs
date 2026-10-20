use crate::{
    CheckResult, cache,
    corpus::{detect, hash, parse},
};
use rusqlite::{OptionalExtension, params};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::Path,
};

fn lock(root: &Path, checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>) -> CheckResult<File> {
    let mut file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(root.join(".letopis/index.lock"))?;
    // SAFETY: file owns a valid descriptor throughout both flock calls.
    let result = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        if error.kind() != std::io::ErrorKind::WouldBlock {
            return Err(error.into());
        }
        checkpoint("waiting")?;
        loop {
            // SAFETY: the descriptor is still owned by file; kernel releases it on close/death.
            if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
                break;
            }
            let error = std::io::Error::last_os_error();
            if error.kind() != std::io::ErrorKind::Interrupted {
                return Err(error.into());
            }
        }
    }
    file.set_len(0)?;
    writeln!(file, "{}", std::process::id())?;
    Ok(file)
}

pub fn publish(
    root: &Path,
    reindex: bool,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<()> {
    let mut conn = cache(root)?;
    if !reindex && detect(root, &conn, false)?.is_empty() {
        return Ok(());
    }
    checkpoint("detected")?;
    let _lock = lock(root, checkpoint)?;
    checkpoint("locked")?;
    let delta = detect(root, &conn, reindex)?;
    checkpoint("redetected")?;
    if delta.is_empty() && !reindex {
        return Ok(());
    }
    let parsed: std::collections::BTreeMap<_, _> = delta
        .iter()
        .map(|(path, state)| (path.clone(), state.as_ref().map(|f| parse(path, &f.bytes))))
        .collect();
    let tx = conn.transaction()?;
    checkpoint("begun")?;
    // Preserve invalid files' old nodes, edges and FTS. A full reindex visits every
    // current file, but cannot regenerate an invalid file's last valid bytes.
    for (path, state) in &delta {
        if parsed[path].as_ref().is_some_and(Result::is_err) {
            continue;
        }
        if let Some(id) = tx
            .query_row("SELECT row_id FROM nodes WHERE id=?1", [path], |r| {
                r.get::<_, i64>(0)
            })
            .optional()?
        {
            tx.execute("DELETE FROM edges WHERE source_id=?1", [path])?;
            tx.execute("DELETE FROM text_index WHERE rowid=?1", [id])?;
        }
        if state.is_none() {
            tx.execute("DELETE FROM nodes WHERE id=?1", [path])?;
        }
    }
    checkpoint("cleared")?;
    for (path, state) in &delta {
        if let (Some(f), Some(Ok((meta, text)))) = (state, &parsed[path]) {
            tx.execute("INSERT INTO nodes(id,kind,uri,hash,created_at,line_start,line_end,search_hash) VALUES(?1,'vertex',?1,?2,0,1,1,?3) ON CONFLICT(id) DO UPDATE SET hash=excluded.hash,search_hash=excluded.search_hash",params![path,f.hash,hash(text.as_bytes())])?;
            for (target, kind) in &meta.links {
                tx.execute(
                    "INSERT OR IGNORE INTO edges VALUES(?1,?2,?3)",
                    params![path, target, kind],
                )?;
            }
        }
    }
    checkpoint("projections")?;
    for path in delta.keys() {
        if let Some(Ok((_, text))) = &parsed[path] {
            tx.execute(
                "INSERT INTO text_index(rowid,text) SELECT row_id,?2 FROM nodes WHERE id=?1",
                params![path, text],
            )?;
        }
    }
    checkpoint("fts")?;
    for (path, state) in &delta {
        match state {
            None => {
                tx.execute("DELETE FROM files WHERE path=?1", [path])?;
            }
            Some(f) => {
                let error = parsed[path].as_ref().and_then(|p| p.as_ref().err());
                tx.execute("INSERT INTO files VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(path) DO UPDATE SET size=excluded.size,mtime_ns=excluded.mtime_ns,inode=excluded.inode,hash=excluded.hash,checked_at_ns=excluded.checked_at_ns,valid=excluded.valid,error=excluded.error",params![path,f.size,f.mtime,f.inode,f.hash,f.checked,error.is_none(),error])?;
            }
        }
    }
    tx.execute("UPDATE project SET revision=revision+1", [])?;
    checkpoint("fingerprints")?;
    checkpoint("before_commit")?;
    tx.commit()?;
    checkpoint("after_commit")?;
    Ok(())
}
