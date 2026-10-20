use crate::{CheckResult, corpus::hash, open};
use rusqlite::{OptionalExtension, TransactionBehavior};
use std::{fs, path::Path};

/// Reset a scratch vector database for the bounded string/integer config fixtures.
/// This is not a production JSON Schema validator or general JCS implementation.
pub fn reset_config(
    root: &Path,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<()> {
    let config: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".letopis/config.json"))?)?;
    if !config.is_object()
        || !config["model"].is_string()
        || !config["provider"].is_string()
        || !config["parameters"].is_object()
        || !config["max_retries"].is_u64()
    {
        return Err("invalid config fixture".into());
    }
    let identity = hash(&serde_json::to_vec(&config)?);
    let mut conn = open(&root.join(".letopis/vectors.db"))?;
    checkpoint("reset_detected")?;
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    checkpoint("reset_begun")?;
    tx.execute_batch("CREATE TABLE IF NOT EXISTS meta(id INTEGER PRIMARY KEY CHECK(id=1),config_identity TEXT NOT NULL)")?;
    let old: Option<String> = tx
        .query_row("SELECT config_identity FROM meta WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()?;
    if old.as_deref() == Some(&identity) {
        return Ok(());
    }
    tx.execute_batch("DROP TABLE IF EXISTS vector_index;
        CREATE VIRTUAL TABLE vector_index USING vec1(vector);
        INSERT INTO vector_index(cmd,arg) VALUES('rebuild','{index:\"none\",distance:\"cos\"}');
        CREATE TABLE IF NOT EXISTS vector_keys(row_id INTEGER PRIMARY KEY,search_hash TEXT NOT NULL UNIQUE);
        CREATE TABLE IF NOT EXISTS embed_failures(search_hash TEXT PRIMARY KEY,attempts INTEGER NOT NULL,error TEXT);
        DELETE FROM vector_keys;DELETE FROM embed_failures;")?;
    checkpoint("reset_cleared")?;
    tx.execute("INSERT INTO meta VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET config_identity=excluded.config_identity",[identity])?;
    checkpoint("reset_identity")?;
    checkpoint("reset_before_commit")?;
    tx.commit()?;
    checkpoint("reset_after_commit")?;
    Ok(())
}
