use super::build_query;
use crate::service::StorageErrorKind;
use rusqlite::{Connection, params_from_iter, types::Value as SqliteValue};
use sea_query::{Enum, Expr, OptionEnum, Query, Value};

#[test]
fn quoted_unicode_text_remains_bound() {
    // Given
    let text = "mémoire '); DROP TABLE nodes; -- ?1";
    // When
    let (sql, result) = get_values(vec![Value::from(text)]);
    // Then
    assert!(!sql.contains(text));
    assert!(sql.contains('?'));
    assert_eq!(result, vec![SqliteValue::Text(text.into())]);
}

#[test]
fn nullable_and_blob_values_preserve_sqlite_types() {
    // Given
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
    // When
    let (_, result) = get_values(values);
    // Then
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
    // Given
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
    // When
    let (_, result) = get_values(numbers);
    let error = build_query(Query::select().expr(Expr::val(u64::MAX))).unwrap_err();
    // Then
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
    // Given
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
    // When
    let (_, result) = get_values(values);
    // Then
    assert_eq!(result, vec![SqliteValue::Null; count]);
}

fn get_values(values: Vec<Value>) -> (String, Vec<SqliteValue>) {
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
