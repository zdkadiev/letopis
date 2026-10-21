use crate::{
    CheckResult, Config, StoreOutcome, attached, capture_config, identity_matches, open, pending,
    record_failure, reserve, store,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, path::Path};
fn live(root: &Path, id: &str) -> CheckResult<String> {
    let s = fs::read_to_string(root.join(id))?;
    let (line, text) = s.split_once('\n').ok_or("invalid fixture")?;
    let meta: serde_json::Value = serde_json::from_str(
        line.strip_prefix("<!-- ")
            .and_then(|x| x.strip_suffix(" -->"))
            .ok_or("invalid fixture")?,
    )?;
    if !meta["links"].is_array() {
        return Err("invalid fixture".into());
    }
    Ok(text.into())
}
fn cleanup(root: &Path, config: &Config) -> CheckResult<()> {
    let c = attached(root)?;
    let referenced = c
        .prepare("SELECT DISTINCT search_hash FROM nodes WHERE search_hash IS NOT NULL")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<BTreeSet<_>>>()?;
    drop(c);
    let mut c = open(&root.join(".letopis/vectors.db"))?;
    let tx = c.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    if !identity_matches(&tx, config, "main")? {
        return Ok(());
    }
    let keys = tx
        .prepare("SELECT row_id,search_hash FROM vector_keys")?
        .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (row, hash) in keys {
        if !referenced.contains(&hash) {
            tx.execute("DELETE FROM vector_index WHERE rowid=?1", [row])?;
            tx.execute("DELETE FROM vector_keys WHERE row_id=?1", [row])?;
        }
    }
    let failures = tx
        .prepare("SELECT search_hash FROM embed_failures")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for hash in failures {
        if !referenced.contains(&hash) {
            tx.execute("DELETE FROM embed_failures WHERE search_hash=?1", [hash])?;
        }
    }
    tx.commit()?;
    Ok(())
}
pub fn drain(
    root: &Path,
    provider: &mut dyn FnMut(&str, &str) -> CheckResult<Vec<f32>>,
    checkpoint: &mut dyn FnMut(&str) -> CheckResult<()>,
) -> CheckResult<()> {
    let config = capture_config(root)?;
    let mut skipped = BTreeSet::new();
    loop {
        let c = attached(root)?;
        if !identity_matches(&c, &config, "vec")? {
            return Ok(());
        }
        let next = pending(&c, &config, &skipped)?.into_iter().next();
        drop(c);
        let Some(work) = next else {
            return cleanup(root, &config);
        };
        checkpoint("selected")?;
        let text = match live(root, &work.id) {
            Ok(text) => text,
            Err(error) => {
                if !reserve(root, &config, &work.search_hash)? {
                    return Ok(());
                }
                if !record_failure(root, &config, &work.search_hash, &error.to_string())? {
                    return Ok(());
                }
                continue;
            }
        };
        let actual = format!("{:x}", Sha256::digest(text.as_bytes()));
        if actual != work.search_hash {
            skipped.insert(work.search_hash);
            continue;
        }
        if !reserve(root, &config, &work.search_hash)? {
            return Ok(());
        }
        checkpoint("reserved")?;
        checkpoint("before_request")?;
        let result = provider(&config.model, &text);
        checkpoint("after_response")?;
        let error = match result {
            Ok(vector) => match store(root, &config, &work.search_hash, &vector, checkpoint) {
                Ok(StoreOutcome::Stored) => continue,
                Ok(StoreOutcome::ConfigChanged) => return Ok(()),
                Err(e) => e.to_string(),
            },
            Err(e) => e.to_string(),
        };
        if !record_failure(root, &config, &work.search_hash, &error)? {
            return Ok(());
        }
    }
}
