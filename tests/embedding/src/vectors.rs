use crate::{CheckResult, cache};
use rusqlite::Connection;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};
#[derive(Clone, Debug)]
pub struct Config {
    pub identity: String,
    pub model: String,
    pub max_retries: u32,
}
#[derive(Debug, PartialEq)]
pub enum ReadOutcome {
    Vectors(Vec<String>),
    FtsOnly,
}
pub fn capture_config(root: &Path) -> CheckResult<Config> {
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".letopis/config.json"))?)?;
    Ok(Config {
        identity: format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)),
        model: value["model"].as_str().ok_or("model")?.into(),
        max_retries: value["max_retries"]
            .as_u64()
            .ok_or("max_retries")?
            .try_into()?,
    })
}
pub fn attached(root: &Path) -> CheckResult<Connection> {
    let conn = cache(root)?;
    conn.execute(
        "ATTACH DATABASE ?1 AS vec",
        [root.join(".letopis/vectors.db").to_str().ok_or("path")?],
    )?;
    conn.execute_batch("PRAGMA vec.journal_mode=WAL; PRAGMA vec.synchronous=FULL;")?;
    #[cfg(target_os = "macos")]
    conn.execute_batch("PRAGMA vec.fullfsync=ON;")?;
    Ok(conn)
}
#[derive(Clone, Debug, PartialEq)]
pub struct WorkItem {
    pub id: String,
    pub search_hash: String,
}
#[derive(Debug, PartialEq)]
pub enum StoreOutcome {
    Stored,
    ConfigChanged,
}
pub fn bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_ne_bytes()).collect()
}
fn valid(v: &[f32]) -> CheckResult<()> {
    if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
        return Err("invalid vector".into());
    }
    Ok(())
}
pub fn identity_matches(conn: &Connection, config: &Config, schema: &str) -> CheckResult<bool> {
    let identity: String = conn.query_row(
        &format!("SELECT config_identity FROM {schema}.meta WHERE id=1"),
        [],
        |r| r.get(0),
    )?;
    Ok(identity == config.identity)
}
pub fn search_vectors(
    conn: &Connection,
    config: &Config,
    query: &[f32],
) -> CheckResult<ReadOutcome> {
    // Keep the identity check and virtual-table query in the same vec snapshot.
    let own = if conn.is_autocommit() {
        Some(conn.unchecked_transaction()?)
    } else {
        None
    };
    let result = (|| {
        if !identity_matches(conn, config, "vec")? {
            return Ok(ReadOutcome::FtsOnly);
        }
        valid(query)?;
        let count: i64 =
            conn.query_row("SELECT count(*) FROM vec.vector_keys", [], |r| r.get(0))?;
        if count == 0 {
            return Ok(ReadOutcome::Vectors(Vec::new()));
        }
        let options = format!("{{K:{count}}}");
        let result=conn.prepare("SELECT n.id FROM vec.vector_index(?1,?2) AS v JOIN vec.vector_keys k ON k.row_id=v.rowid JOIN nodes n ON n.search_hash=k.search_hash ORDER BY n.id")?
 .query_map(rusqlite::params![bytes(query),options],|r|r.get(0))?.collect::<rusqlite::Result<Vec<String>>>()?;
        Ok(ReadOutcome::Vectors(result))
    })();
    drop(own);
    result
}
pub fn pending(
    conn: &Connection,
    config: &Config,
    skipped: &std::collections::BTreeSet<String>,
) -> CheckResult<Vec<WorkItem>> {
    if !identity_matches(conn, config, "vec")? {
        return Ok(Vec::new());
    }
    let rows=conn.prepare("SELECT MIN(n.id),n.search_hash FROM nodes n WHERE n.kind<>'source' AND n.search_hash IS NOT NULL AND NOT EXISTS(SELECT 1 FROM vec.vector_keys k WHERE k.search_hash=n.search_hash) AND NOT EXISTS(SELECT 1 FROM vec.embed_failures f WHERE f.search_hash=n.search_hash AND f.attempts>=?1) GROUP BY n.search_hash ORDER BY MIN(n.id)")?
 .query_map([1+i64::from(config.max_retries)],|r|Ok(WorkItem{id:r.get(0)?,search_hash:r.get(1)?}))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter(|x| !skipped.contains(&x.search_hash))
        .collect())
}
pub fn reserve(root: &Path, config: &Config, hash: &str) -> CheckResult<bool> {
    let mut c = crate::open(&root.join(".letopis/vectors.db"))?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !identity_matches(&tx, config, "main")? {
        return Ok(false);
    }
    let changed=tx.execute("INSERT INTO embed_failures(search_hash,attempts,error) VALUES(?1,1,NULL) ON CONFLICT(search_hash) DO UPDATE SET attempts=attempts+1 WHERE attempts<?2",rusqlite::params![hash,1+i64::from(config.max_retries)])?;
    tx.commit()?;
    Ok(changed == 1)
}
pub fn record_failure(root: &Path, config: &Config, hash: &str, error: &str) -> CheckResult<bool> {
    let mut c = crate::open(&root.join(".letopis/vectors.db"))?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !identity_matches(&tx, config, "main")? {
        return Ok(false);
    }
    tx.execute(
        "UPDATE embed_failures SET error=?1 WHERE search_hash=?2",
        rusqlite::params![error, hash],
    )?;
    tx.commit()?;
    Ok(true)
}
pub fn store(
    root: &Path,
    config: &Config,
    hash: &str,
    vector: &[f32],
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<StoreOutcome> {
    valid(vector)?;
    let dimensions = match config.model.as_str() {
        "three" => 3,
        "four" => 4,
        _ => return Err("unknown fixture model".into()),
    };
    if vector.len() != dimensions {
        return Err("wrong model dimension".into());
    }
    let mut c = crate::open(&root.join(".letopis/vectors.db"))?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !identity_matches(&tx, config, "main")? {
        return Ok(StoreOutcome::ConfigChanged);
    }
    let inserted = tx.execute(
        "INSERT OR IGNORE INTO vector_keys(search_hash) VALUES(?1)",
        [hash],
    )?;
    let row: i64 = tx.query_row(
        "SELECT row_id FROM vector_keys WHERE search_hash=?1",
        [hash],
        |r| r.get(0),
    )?;
    checkpoint("store_key")?;
    if inserted == 1 {
        tx.execute(
            "INSERT INTO vector_index(rowid,vector) VALUES(?1,?2)",
            rusqlite::params![row, bytes(vector)],
        )?;
    }
    checkpoint("store_vector")?;
    tx.execute("DELETE FROM embed_failures WHERE search_hash=?1", [hash])?;
    checkpoint("before_store_commit")?;
    tx.commit()?;
    checkpoint("after_store_commit")?;
    Ok(StoreOutcome::Stored)
}
