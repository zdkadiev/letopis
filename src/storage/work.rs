use super::{
    connection::map_sqlite,
    graph::{SOURCE_KIND, map_invalid_value},
    query::{EmbedFailures, Nodes, Schemas, VectorIndex, VectorKeys, build_query},
};
use crate::{
    domain::{KnowledgeId, Sha256},
    service::{EmbeddingWork, StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use sea_query::{Expr, ExprTrait, Order, Query, SelectStatement};

pub(super) fn read_next_embedding_work(
    connection: &Connection,
    max_attempts: u64,
    after: Option<&KnowledgeId>,
) -> StorageResult<Option<EmbeddingWork>> {
    if max_attempts == 0 {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "attempt budget must be positive",
        ));
    }
    let mut query = Query::select();
    query
        .columns([Nodes::Id, Nodes::SearchHash])
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
        .and_where(Expr::exists(select_stored_hash()).not())
        .and_where(Expr::exists(select_exhausted_hash(max_attempts)).not())
        .order_by(Nodes::Id, Order::Asc)
        .limit(1);
    if let Some(after) = after {
        query.and_where(Expr::col(Nodes::Id).gt(after.as_str()));
    }
    let (sql, values) = build_query(&query)?;
    let row = connection
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .optional()
        .map_err(map_sqlite)?;
    row.map(|(id, hash)| {
        Ok(EmbeddingWork {
            id: KnowledgeId::parse(&id).map_err(map_invalid_value)?,
            search_hash: Sha256::parse(&hash).map_err(map_invalid_value)?,
        })
    })
    .transpose()
}

fn select_stored_hash() -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Vec, VectorKeys::Table))
        .and_where(
            Expr::col((VectorKeys::Table, VectorKeys::SearchHash))
                .equals((Nodes::Table, Nodes::SearchHash)),
        )
        .to_owned()
}

fn select_exhausted_hash(max_attempts: u64) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Vec, EmbedFailures::Table))
        .and_where(
            Expr::col((EmbedFailures::Table, EmbedFailures::SearchHash))
                .equals((Nodes::Table, Nodes::SearchHash)),
        )
        .and_where(Expr::col(EmbedFailures::Attempts).gte(max_attempts))
        .to_owned()
}

pub(super) fn prune_unreferenced(connection: &Connection) -> StorageResult<()> {
    let orphan_keys = Query::select()
        .column(VectorKeys::RowId)
        .from(VectorKeys::Table)
        .and_where(
            Expr::exists(select_live_hash(Expr::col((
                VectorKeys::Table,
                VectorKeys::SearchHash,
            ))))
            .not(),
        )
        .to_owned();
    let (sql, values) = build_query(
        Query::delete()
            .from_table(VectorIndex::Table)
            .and_where(Expr::col(VectorIndex::Rowid).in_subquery(orphan_keys)),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    let (sql, values) = build_query(
        Query::delete().from_table(VectorKeys::Table).and_where(
            Expr::exists(select_live_hash(Expr::col((
                VectorKeys::Table,
                VectorKeys::SearchHash,
            ))))
            .not(),
        ),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    let (sql, values) = build_query(
        Query::delete().from_table(EmbedFailures::Table).and_where(
            Expr::exists(select_live_hash(Expr::col((
                EmbedFailures::Table,
                EmbedFailures::SearchHash,
            ))))
            .not(),
        ),
    )?;
    connection
        .execute(&sql, params_from_iter(values))
        .map_err(map_sqlite)?;
    Ok(())
}

fn select_live_hash(hash: Expr) -> SelectStatement {
    Query::select()
        .expr(Expr::val(1))
        .from((Schemas::Cache, Nodes::Table))
        .and_where(Expr::col((Nodes::Table, Nodes::Kind)).ne(SOURCE_KIND))
        .and_where(Expr::col((Nodes::Table, Nodes::SearchHash)).eq(hash))
        .to_owned()
}
