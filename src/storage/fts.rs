use super::{
    connection::map_sqlite,
    graph::{SOURCE_KIND, map_invalid_value, map_mutation},
    query::{Nodes, TextIndex, build_query},
};
use crate::{
    domain::KnowledgeId,
    service::{StorageError, StorageErrorKind, StorageResult, TextMatch},
};
use rusqlite::{Connection, OptionalExtension, params, params_from_iter};
use sea_query::{Expr, ExprTrait, Query};
pub(super) fn replace_text(db: &Connection, id: &KnowledgeId, text: &str) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::select()
            .column(Nodes::RowId)
            .from(Nodes::Table)
            .and_where(Expr::col(Nodes::Id).eq(id.as_str()))
            .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND)),
    )?;
    let row: Option<i64> = db
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(map_sqlite)?;
    let row = row.ok_or_else(|| {
        StorageError::new(StorageErrorKind::NotFound, "knowledge metadata is absent")
    })?;
    let (sql, values) = build_query(
        Query::delete()
            .from_table(TextIndex::Table)
            .and_where(Expr::col(TextIndex::Rowid).eq(row)),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    let (sql, values) = build_query(
        Query::insert()
            .into_table(TextIndex::Table)
            .columns([TextIndex::Rowid, TextIndex::Text])
            .values([Expr::val(row), Expr::val(text)])
            .map_err(map_invalid_value)?,
    )?;
    db.execute(&sql, params_from_iter(values))
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
            "SELECT nodes.id, nodes.row_id, bm25(text_index) AS score
             FROM text_index
             JOIN nodes ON nodes.row_id = text_index.rowid
             WHERE text_index MATCH ?1 AND nodes.kind <> ?2
             ORDER BY score, nodes.id",
        )
        .map_err(map_sqlite)?;
    let mut rows = stmt
        .query(params![expression, SOURCE_KIND])
        .map_err(map_match)?;
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
