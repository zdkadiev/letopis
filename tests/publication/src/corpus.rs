//! Bounded fixture grammar, deliberately not the production X-4 parser.
use crate::CheckResult;
use rusqlite::Connection;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

pub const TICK_NS: i64 = 1_000_000_000;

pub fn now() -> CheckResult<i64> {
    Ok(SystemTime::now()
        .duration_since(UNIX_EPOCH)?
        .as_nanos()
        .try_into()?)
}

pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub struct FileState {
    pub size: i64,
    pub mtime: i64,
    pub inode: i64,
    pub hash: String,
    pub checked: i64,
    pub bytes: Vec<u8>,
}

#[derive(Deserialize)]
pub struct Metadata {
    pub links: Vec<(String, String)>,
}

pub fn parse(path: &str, bytes: &[u8]) -> Result<(Metadata, String), String> {
    let bad = || "invalid fixture metadata".to_owned();
    let s = std::str::from_utf8(bytes).map_err(|_| bad())?;
    let (line, text) = s.split_once('\n').ok_or_else(bad)?;
    let meta: Metadata = serde_json::from_str(
        line.strip_prefix("<!-- ")
            .and_then(|x| x.strip_suffix(" -->"))
            .ok_or_else(bad)?,
    )
    .map_err(|_| bad())?;
    if meta
        .links
        .iter()
        .any(|(target, kind)| target == path || !matches!(kind.as_str(), "rel" | "includes"))
    {
        return Err(bad());
    }
    Ok((meta, text.to_owned()))
}

pub fn paths(root: &Path) -> CheckResult<Vec<String>> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) -> CheckResult<()> {
        for e in fs::read_dir(dir)? {
            let e = e?;
            let p = e.path();
            let ty = e.file_type()?;
            if e.file_name() == ".letopis" || ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                walk(root, &p, out)?
            } else if ty.is_file() && p.extension().is_some_and(|e| e == "md" || e == "markdown") {
                out.push(
                    p.strip_prefix(root)?
                        .to_str()
                        .ok_or("non-UTF8 fixture path")?
                        .to_owned(),
                );
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    out.sort();
    Ok(out)
}

pub fn detect(
    root: &Path,
    conn: &Connection,
    all: bool,
) -> CheckResult<BTreeMap<String, Option<FileState>>> {
    let old: BTreeMap<String, (i64, i64, i64, String, i64)> = conn
        .prepare("SELECT path,size,mtime_ns,inode,hash,checked_at_ns FROM files")?
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                (r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?),
            ))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut delta: BTreeMap<_, _> = old.keys().map(|p| (p.clone(), None)).collect();
    for path in paths(root)? {
        let mut f = fs::File::open(root.join(&path))?;
        let m = f.metadata()?;
        let size: i64 = m.len().try_into()?;
        let mtime = m.mtime() * 1_000_000_000 + m.mtime_nsec();
        let inode: i64 = m.ino().try_into()?;
        let prev = old.get(&path);
        let same = prev.is_some_and(|(s, t, i, _, _)| (*s, *t, *i) == (size, mtime, inode));
        let racy = prev.is_some_and(|(_, t, _, _, checked)| *t >= checked - TICK_NS);
        if !all && same && !racy {
            delta.remove(&path);
            continue;
        }
        let mut bytes = Vec::new();
        f.read_to_end(&mut bytes)?;
        let after = f.metadata()?;
        if (m.len(), m.mtime(), m.mtime_nsec(), m.ino())
            != (after.len(), after.mtime(), after.mtime_nsec(), after.ino())
        {
            return Err(format!("fixture changed while being read: {path}").into());
        }
        let hash = hash(&bytes);
        if !all && same && prev.is_some_and(|(_, _, _, h, _)| *h == hash) {
            delta.remove(&path);
            continue;
        }
        delta.insert(
            path,
            Some(FileState {
                size,
                mtime,
                inode,
                hash,
                checked: now()?,
                bytes,
            }),
        );
    }
    Ok(delta)
}
