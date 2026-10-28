use super::{
    connection::map_sqlite,
    graph::{SOURCE_KIND, map_invalid_value},
    query::{EmbedFailures, Nodes, Schemas, VectorIndex, VectorKeys, build_query},
    vectors::fail_invalid_stored_vector,
};
use crate::{
    domain::Sha256,
    service::{
        EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary, StorageError,
        StorageErrorKind, StorageResult,
    },
};
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use sea_query::{Expr, ExprTrait, JoinType, Order, Query, SelectStatement};

#[derive(Clone, Copy)]
enum HashCoverage {
    All,
    Stored,
}

pub(super) fn read_embedding_status(
    connection: &Connection,
    identity: Option<Sha256>,
    policy: &EmbeddingStatusPolicy,
) -> StorageResult<EmbeddingStatusSummary> {
    if matches!(policy, EmbeddingStatusPolicy::Disabled) {
        return Ok(EmbeddingStatusSummary {
            backlog: 0,
            failed: 0,
            exhausted: vec![],
        });
    }
    let total = count_hashes(connection, HashCoverage::All)?;
    let EmbeddingStatusPolicy::Ready {
        identity: expected,
        max_attempts,
    } = policy
    else {
        return Ok(EmbeddingStatusSummary {
            backlog: total,
            failed: 0,
            exhausted: vec![],
        });
    };
    if *max_attempts == 0 || i64::try_from(*max_attempts).is_err() {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid status attempt budget",
        ));
    }
    if identity != Some(*expected) {
        return Ok(EmbeddingStatusSummary {
            backlog: total,
            failed: 0,
            exhausted: vec![],
        });
    }
    verify_current_vectors(connection)?;
    let covered = count_hashes(connection, HashCoverage::Stored)?;
    let exhausted = read_current_failures(connection, *max_attempts)?;
    let failed = u64::try_from(exhausted.len()).map_err(map_invalid_value)?;
    let backlog = total
        .checked_sub(covered)
        .and_then(|missing| missing.checked_sub(failed))
        .ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InternalError,
                "invalid embedding status counts",
            )
        })?;
    Ok(EmbeddingStatusSummary {
        backlog,
        failed,
        exhausted,
    })
}

fn verify_current_vectors(connection: &Connection) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::select()
            .column((VectorKeys::Table, VectorKeys::RowId))
            .from((Schemas::Vec, VectorKeys::Table))
            .join(
                JoinType::LeftJoin,
                (Schemas::Vec, VectorIndex::Table),
                Expr::col((VectorIndex::Table, VectorIndex::Rowid))
                    .equals((VectorKeys::Table, VectorKeys::RowId)),
            )
            .and_where(Expr::col((VectorIndex::Table, VectorIndex::Rowid)).is_null())
            .and_where(Expr::exists(
                Query::select()
                    .expr(Expr::val(1))
                    .from(Nodes::Table)
                    .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
                    .and_where(
                        Expr::col((Nodes::Table, Nodes::SearchHash))
                            .equals((VectorKeys::Table, VectorKeys::SearchHash)),
                    )
                    .to_owned(),
            ))
            .limit(1),
    )?;
    let invalid: Option<i64> = connection
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .optional()
        .map_err(map_sqlite)?;
    if invalid.is_some() {
        return Err(fail_invalid_stored_vector());
    }
    Ok(())
}

fn count_hashes(connection: &Connection, coverage: HashCoverage) -> StorageResult<u64> {
    let mut query = Query::select();
    query
        .expr(Expr::col(Nodes::SearchHash).count_distinct())
        .from(Nodes::Table)
        .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND));
    if matches!(coverage, HashCoverage::Stored) {
        query.and_where(Expr::exists(select_stored_hash()));
    }
    let (sql, values) = build_query(&query)?;
    let count: i64 = connection
        .query_row(&sql, params_from_iter(values), |row| row.get(0))
        .map_err(map_sqlite)?;
    u64::try_from(count).map_err(map_invalid_value)
}

fn read_current_failures(
    connection: &Connection,
    max_attempts: u64,
) -> StorageResult<Vec<EmbeddingFailure>> {
    let (sql, values) = build_query(
        Query::select()
            .distinct()
            .columns([
                EmbedFailures::SearchHash,
                EmbedFailures::Attempts,
                EmbedFailures::Error,
            ])
            .from((Schemas::Vec, EmbedFailures::Table))
            .and_where(Expr::exists(
                Query::select()
                    .expr(Expr::val(1))
                    .from(Nodes::Table)
                    .and_where(Expr::col(Nodes::Kind).ne(SOURCE_KIND))
                    .and_where(
                        Expr::col((Nodes::Table, Nodes::SearchHash))
                            .equals((EmbedFailures::Table, EmbedFailures::SearchHash)),
                    )
                    .and_where(Expr::exists(select_stored_hash()).not())
                    .to_owned(),
            ))
            .order_by(EmbedFailures::SearchHash, Order::Asc),
    )?;
    let mut statement = connection.prepare(&sql).map_err(map_sqlite)?;
    let rows = statement
        .query_map(params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, Option<String>>(2)?,
            ))
        })
        .map_err(map_sqlite)?;
    let mut exhausted = Vec::new();
    for row in rows {
        let (hash, attempts, error) = row.map_err(map_sqlite)?;
        let attempts = u64::try_from(attempts).map_err(map_invalid_value)?;
        if attempts == 0 {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "invalid stored embedding attempts",
            ));
        }
        let search_hash = Sha256::parse(&hash).map_err(map_invalid_value)?;
        if attempts >= max_attempts {
            exhausted.push(EmbeddingFailure {
                search_hash,
                attempts,
                error,
            });
        }
    }
    Ok(exhausted)
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
