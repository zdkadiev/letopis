use super::connection::map_sqlite;
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{StorageError, StorageErrorKind, StorageResult, StoredVectorKey, VectorMatch},
};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

const RESET_INDEX: &str = "DROP TABLE vector_index; CREATE VIRTUAL TABLE vector_index USING vec1(vector); INSERT INTO vector_index(cmd,arg) VALUES('rebuild','{index:\"none\",distance:\"cos\"}'); DELETE FROM vector_keys; DELETE FROM embed_failures;";

pub(super) fn matches_identity(connection: &Connection, expected: &Sha256) -> StorageResult<bool> {
    let identity: Option<String> = connection
        .query_row("SELECT config_identity FROM meta WHERE id=1", [], |r| {
            r.get(0)
        })
        .optional()
        .map_err(map_sqlite)?;
    Ok(identity.as_deref() == Some(expected.to_hex().as_str()))
}
pub(super) fn reset_vectors(connection: &mut Connection, identity: &Sha256) -> StorageResult<bool> {
    let tx = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(map_sqlite)?;
    if matches_identity(&tx, identity)? {
        return Ok(false);
    }
    tx.execute_batch(RESET_INDEX).map_err(map_sqlite)?;
    tx.execute("INSERT INTO meta(id,config_identity) VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET config_identity=excluded.config_identity", [identity.to_hex()]).map_err(map_sqlite)?;
    tx.commit().map_err(map_sqlite)?;
    Ok(true)
}
pub(super) fn encode_vector(vector: &[f32], dimensions: usize) -> StorageResult<Vec<u8>> {
    if vector.is_empty() || vector.len() != dimensions || vector.iter().any(|v| !v.is_finite()) {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector values or dimensions",
        ));
    }
    Ok(vector.iter().flat_map(|v| v.to_ne_bytes()).collect())
}
pub(super) fn store_vector(
    connection: &Connection,
    hash: &Sha256,
    vector: &[f32],
    dimensions: usize,
) -> StorageResult<StoredVectorKey> {
    let bytes = encode_vector(vector, dimensions)?;
    let inserted = connection
        .execute(
            "INSERT OR IGNORE INTO vector_keys(search_hash) VALUES(?1)",
            [hash.to_hex()],
        )
        .map_err(map_sqlite)?
        == 1;
    let row_id = connection
        .query_row(
            "SELECT row_id FROM vector_keys WHERE search_hash=?1",
            [hash.to_hex()],
            |r| r.get::<_, i64>(0),
        )
        .map_err(map_sqlite)?;
    if inserted {
        connection
            .execute(
                "INSERT INTO vector_index(rowid,vector) VALUES(?1,?2)",
                params![row_id, bytes],
            )
            .map_err(map_sqlite)?;
    } else {
        let exists = connection
            .query_row(
                "SELECT 1 FROM vector_index WHERE rowid=?1",
                [row_id],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_sqlite)?;
        if exists.is_none() {
            return Err(invalid_stored_vector());
        }
    }
    connection
        .execute(
            "DELETE FROM embed_failures WHERE search_hash=?1",
            [hash.to_hex()],
        )
        .map_err(map_sqlite)?;
    Ok(StoredVectorKey { row_id, inserted })
}
pub(super) fn read_vector(
    connection: &Connection,
    hash: &Sha256,
) -> StorageResult<Option<Vec<f32>>> {
    let value = connection.query_row("SELECT v.vector FROM vec.vector_keys k LEFT JOIN vec.vector_index v ON v.rowid=k.row_id WHERE k.search_hash=?1", [hash.to_hex()], |r| r.get::<_,Option<Vec<u8>>>(0)).optional().map_err(map_sqlite)?;
    value
        .map(|bytes| decode_vector(&bytes.ok_or_else(invalid_stored_vector)?))
        .transpose()
}
fn decode_vector(bytes: &[u8]) -> StorageResult<Vec<f32>> {
    if bytes.is_empty() || !bytes.len().is_multiple_of(4) {
        return Err(invalid_stored_vector());
    }
    let values: Vec<_> = bytes
        .chunks_exact(4)
        .map(|c| f32::from_ne_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if values.iter().any(|v| !v.is_finite()) {
        return Err(invalid_stored_vector());
    }
    Ok(values)
}
fn invalid_stored_vector() -> StorageError {
    StorageError::new(
        StorageErrorKind::InternalError,
        "invalid stored vector data",
    )
}
pub(super) fn find_nearest_vectors(
    connection: &Connection,
    query: &[f32],
    dimensions: usize,
) -> StorageResult<Vec<VectorMatch>> {
    let bytes = encode_vector(query, dimensions)?;
    let count: i64 = connection
        .query_row("SELECT count(*) FROM vec.vector_keys", [], |r| r.get(0))
        .map_err(map_sqlite)?;
    if count == 0 {
        return Ok(Vec::new());
    }
    let mut statement = connection.prepare("SELECT n.id,k.search_hash,v.distance FROM vec.vector_index(?1,?2) v JOIN vec.vector_keys k ON k.row_id=v.rowid JOIN nodes n ON n.search_hash=k.search_hash WHERE n.kind<>'source' ORDER BY v.distance,n.id").map_err(map_sqlite)?;
    let rows = statement
        .query_map(params![bytes, format!("{{K:{count}}}")], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, f64>(2)?,
            ))
        })
        .map_err(map_sqlite)?;
    rows.map(|row| {
        let (id, hash, distance) = row.map_err(map_sqlite)?;
        Ok(VectorMatch {
            id: KnowledgeId::parse(&id).map_err(|_| invalid_stored_vector())?,
            search_hash: Sha256::parse(&hash).map_err(|_| invalid_stored_vector())?,
            distance,
        })
    })
    .collect()
}
