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
    RowId,
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

fn convert_value(value: Value) -> StorageResult<SqliteValue> {
    let converted = match value {
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
mod tests {
    use super::build_query;
    use crate::service::StorageErrorKind;
    use rusqlite::{Connection, params_from_iter, types::Value as SqliteValue};
    use sea_query::{Enum, Expr, OptionEnum, Query, Value};

    fn read_values(values: Vec<Value>) -> (String, Vec<SqliteValue>) {
        let db = Connection::open_in_memory().unwrap();
        let mut query = Query::select();
        for value in values {
            query.expr(Expr::val(value));
        }
        let (sql, parameters) = build_query(&query).unwrap();
        let mut statement = db.prepare(&sql).unwrap();
        let count = statement.column_count();
        let result = statement
            .query_row(params_from_iter(parameters), |row| {
                (0..count).map(|index| row.get(index)).collect()
            })
            .unwrap();
        (sql, result)
    }

    #[test]
    fn quoted_unicode_text_remains_bound() {
        // Arrange
        let text = "mémoire '); DROP TABLE nodes; -- ?1";
        // Act
        let (sql, result) = read_values(vec![Value::from(text)]);
        // Assert
        assert!(!sql.contains(text));
        assert!(sql.contains('?'));
        assert_eq!(result, vec![SqliteValue::Text(text.into())]);
    }

    #[test]
    fn nullable_and_blob_values_preserve_sqlite_types() {
        // Arrange
        let values = vec![
            Value::String(None),
            Value::Bytes(None),
            Value::Bytes(Some(vec![])),
            Value::Bytes(Some(vec![0, 255])),
            Value::Bool(Some(false)),
            Value::Bool(Some(true)),
            Value::Char(Some('é')),
            Value::Enum(OptionEnum::Some(Box::new(Enum {
                type_name: "kind".into(),
                value: "vertex".into(),
            }))),
            Value::Enum(OptionEnum::None("kind".into())),
        ];
        // Act
        let (_, result) = read_values(values);
        // Assert
        assert_eq!(
            result,
            vec![
                SqliteValue::Null,
                SqliteValue::Null,
                SqliteValue::Blob(vec![]),
                SqliteValue::Blob(vec![0, 255]),
                SqliteValue::Integer(0),
                SqliteValue::Integer(1),
                SqliteValue::Text("é".into()),
                SqliteValue::Text("vertex".into()),
                SqliteValue::Null
            ]
        );
    }

    #[test]
    fn numeric_values_preserve_bounds_and_reject_overflow() {
        // Arrange
        let numbers = vec![
            Value::TinyInt(Some(i8::MIN)),
            Value::SmallInt(Some(i16::MIN)),
            Value::Int(Some(i32::MIN)),
            Value::BigInt(Some(i64::MIN)),
            Value::BigInt(Some(i64::MAX)),
            Value::TinyUnsigned(Some(u8::MAX)),
            Value::SmallUnsigned(Some(u16::MAX)),
            Value::Unsigned(Some(u32::MAX)),
            Value::BigUnsigned(Some(7)),
            Value::Float(Some(1.25)),
            Value::Double(Some(-2.5)),
        ];
        // Act
        let (_, result) = read_values(numbers);
        let error = build_query(Query::select().expr(Expr::val(u64::MAX))).unwrap_err();
        // Assert
        assert_eq!(
            result,
            vec![
                SqliteValue::Integer(i64::from(i8::MIN)),
                SqliteValue::Integer(i64::from(i16::MIN)),
                SqliteValue::Integer(i64::from(i32::MIN)),
                SqliteValue::Integer(i64::MIN),
                SqliteValue::Integer(i64::MAX),
                SqliteValue::Integer(i64::from(u8::MAX)),
                SqliteValue::Integer(i64::from(u16::MAX)),
                SqliteValue::Integer(i64::from(u32::MAX)),
                SqliteValue::Integer(7),
                SqliteValue::Real(1.25),
                SqliteValue::Real(-2.5)
            ]
        );
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(!error.to_string().contains(&u64::MAX.to_string()));
    }

    #[test]
    fn every_primitive_null_remains_null() {
        // Arrange
        let values = vec![
            Value::Bool(None),
            Value::TinyInt(None),
            Value::SmallInt(None),
            Value::Int(None),
            Value::BigInt(None),
            Value::TinyUnsigned(None),
            Value::SmallUnsigned(None),
            Value::Unsigned(None),
            Value::BigUnsigned(None),
            Value::Float(None),
            Value::Double(None),
            Value::String(None),
            Value::Char(None),
            Value::Bytes(None),
        ];
        let count = values.len();
        // Act
        let (_, result) = read_values(values);
        // Assert
        assert_eq!(result, vec![SqliteValue::Null; count]);
    }
}
