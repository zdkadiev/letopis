//! Private query construction and checked bindings for our synchronous driver.
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::types::Value as SqliteValue;
use sea_query::{Iden, OptionEnum, QueryStatementBuilder, SqliteQueryBuilder, Value};

#[derive(Clone, Copy, Iden)]
pub(super) enum Project {
    Table,
    Id,
    Revision,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum Nodes {
    Table,
    Id,
    Kind,
    Uri,
    Hash,
    CreatedAt,
    SourceSnapshot,
    LineStart,
    LineEnd,
    SearchHash,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum Edges {
    Table,
    SourceId,
    TargetId,
    Type,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum UnresolvedEdges {
    Table,
    SourceId,
    TargetUri,
    Type,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum Files {
    Table,
    Path,
    Size,
    MtimeNs,
    Inode,
    Hash,
    CheckedAtNs,
    Valid,
    Error,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum TextIndex {
    Table,
    Rowid,
    Text,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum Schemas {
    Vec,
    Cache,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum Meta {
    Table,
    Id,
    ConfigIdentity,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum VectorKeys {
    Table,
    RowId,
    SearchHash,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum VectorIndex {
    Table,
    Rowid,
    Vector,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum EmbedFailures {
    Table,
    SearchHash,
    Attempts,
    Error,
}

#[derive(Clone, Copy, Iden)]
pub(super) enum QueryErrorRow {
    #[iden = "query_error"]
    Table,
    Id,
    At,
    Error,
}

pub(super) fn build_query(
    statement: &impl QueryStatementBuilder,
) -> StorageResult<(String, Vec<SqliteValue>)> {
    let (sql, values) = statement.build_any(&SqliteQueryBuilder);
    let parameters = values
        .into_iter()
        .map(convert_value)
        .collect::<StorageResult<_>>()?;
    Ok((sql, parameters))
}

fn convert_value(sea_value: Value) -> StorageResult<SqliteValue> {
    let converted = match sea_value {
        Value::Bool(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::TinyInt(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::SmallInt(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::Int(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::BigInt(v) => v.map(SqliteValue::Integer),
        Value::TinyUnsigned(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::SmallUnsigned(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::Unsigned(v) => v.map(|v| SqliteValue::Integer(i64::from(v))),
        Value::BigUnsigned(v) => v
            .map(i64::try_from)
            .transpose()
            .map_err(|cause| {
                StorageError::new_with_cause(
                    StorageErrorKind::InvalidRequest,
                    "query integer out of range",
                    cause,
                )
            })?
            .map(SqliteValue::Integer),
        Value::Float(v) => v.map(|v| SqliteValue::Real(f64::from(v))),
        Value::Double(v) => v.map(SqliteValue::Real),
        Value::String(v) => v.map(SqliteValue::Text),
        Value::Char(v) => v.map(|v| SqliteValue::Text(v.to_string())),
        Value::Bytes(v) => v.map(SqliteValue::Blob),
        Value::Enum(OptionEnum::Some(v)) => Some(SqliteValue::Text(v.value.into_owned())),
        Value::Enum(OptionEnum::None(_)) => None,
    };
    Ok(converted.unwrap_or(SqliteValue::Null))
}

#[cfg(test)]
#[path = "query_test.rs"]
mod tests;
