use super::{
    connection::map_sqlite,
    graph::{map_invalid_value, map_mutation},
};
use crate::{
    domain::KnowledgeId,
    service::{StorageError, StorageErrorKind, StorageResult, TextMatch},
};
use rusqlite::{Connection, OptionalExtension, params};
pub(super) fn replace_text(db: &Connection, id: &KnowledgeId, text: &str) -> StorageResult<()> {
    let row: Option<i64> = db
        .query_row(
            "SELECT row_id FROM nodes WHERE id=?1 AND kind<>'source'",
            [id.as_str()],
            |r| r.get(0),
        )
        .optional()
        .map_err(map_sqlite)?;
    let row = row.ok_or_else(|| {
        StorageError::new(StorageErrorKind::NotFound, "knowledge metadata is absent")
    })?;
    db.execute("DELETE FROM text_index WHERE rowid=?1", [row])
        .map_err(map_mutation)?;
    db.execute(
        "INSERT INTO text_index(rowid,text) VALUES(?1,?2)",
        params![row, text],
    )
    .map_err(map_mutation)?;
    Ok(())
}
fn map_match(error: rusqlite::Error) -> StorageError {
    if matches!(&error,rusqlite::Error::SqliteFailure(e,_) if e.code==rusqlite::ErrorCode::Unknown)
    {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "invalid full-text expression",
            error,
        )
    } else {
        map_sqlite(error)
    }
}
pub(super) fn match_text(db: &Connection, expression: &str) -> StorageResult<Vec<TextMatch>> {
    let mut stmt = db
        .prepare(
            "SELECT nodes.id,nodes.row_id,bm25(text_index) AS score
         FROM text_index JOIN nodes ON nodes.row_id=text_index.rowid
         WHERE text_index MATCH ?1 AND nodes.kind<>'source' ORDER BY score,nodes.id",
        )
        .map_err(map_sqlite)?;
    let mut rows = stmt.query([expression]).map_err(map_match)?;
    let mut found = Vec::new();
    while let Some(row) = rows.next().map_err(map_match)? {
        let id: String = row.get(0).map_err(map_sqlite)?;
        found.push(TextMatch {
            id: KnowledgeId::parse(&id).map_err(map_invalid_value)?,
            row_id: row.get(1).map_err(map_sqlite)?,
            bm25: row.get(2).map_err(map_sqlite)?,
        });
    }
    Ok(found)
}
