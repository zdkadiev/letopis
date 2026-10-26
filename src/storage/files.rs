use super::{
    connection::map_sqlite,
    graph::{check_unsigned, map_invalid_value, map_mutation, read_unsigned},
    query::{Files, build_query},
};
use crate::{
    domain::{RepoPath, Sha256},
    service::{FileRow, StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, OptionalExtension, Row, params_from_iter};
use sea_query::{Expr, ExprTrait, OnConflict, Order, Query, Value};

const FILE_COLUMNS: [Files; 8] = [
    Files::Path,
    Files::Size,
    Files::MtimeNs,
    Files::Inode,
    Files::Hash,
    Files::CheckedAtNs,
    Files::Valid,
    Files::Error,
];
pub(super) fn upsert_file(db: &Connection, file: &FileRow) -> StorageResult<()> {
    let values: [Value; 8] = [
        file.path.as_str().into(),
        check_unsigned(file.size)?.into(),
        file.mtime_ns.into(),
        check_unsigned(file.inode)?.into(),
        file.hash.to_hex().into(),
        file.checked_at_ns.into(),
        file.valid.into(),
        file.error.as_deref().into(),
    ];
    let (sql, values) = build_query(
        Query::insert()
            .into_table(Files::Table)
            .columns(FILE_COLUMNS)
            .values(values.map(Expr::val))
            .map_err(map_invalid_value)?
            .on_conflict(
                OnConflict::column(Files::Path)
                    .update_columns(FILE_COLUMNS.into_iter().skip(1))
                    .to_owned(),
            ),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}
pub(super) fn delete_file(db: &Connection, path: &RepoPath) -> StorageResult<()> {
    let (sql, values) = build_query(
        Query::delete()
            .from_table(Files::Table)
            .and_where(Expr::col(Files::Path).eq(path.as_str())),
    )?;
    db.execute(&sql, params_from_iter(values))
        .map_err(map_mutation)?;
    Ok(())
}
pub(super) fn read_file(db: &Connection, path: &RepoPath) -> StorageResult<Option<FileRow>> {
    let (sql, values) = build_query(
        Query::select()
            .columns(FILE_COLUMNS)
            .from(Files::Table)
            .and_where(Expr::col(Files::Path).eq(path.as_str())),
    )?;
    db.query_row(&sql, params_from_iter(values), |row| Ok(decode_file(row)))
        .optional()
        .map_err(map_sqlite)?
        .transpose()
}
pub(super) fn list_files(db: &Connection) -> StorageResult<Vec<FileRow>> {
    let (sql, values) = build_query(
        Query::select()
            .columns(FILE_COLUMNS)
            .from(Files::Table)
            .order_by(Files::Path, Order::Asc),
    )?;
    let mut statement = db.prepare(&sql).map_err(map_sqlite)?;
    let mut rows = statement
        .query(params_from_iter(values))
        .map_err(map_sqlite)?;
    let mut files = Vec::new();
    while let Some(row) = rows.next().map_err(map_sqlite)? {
        files.push(decode_file(row)?);
    }
    Ok(files)
}
fn decode_file(row: &Row<'_>) -> StorageResult<FileRow> {
    let path: String = row.get(0).map_err(map_sqlite)?;
    let size = read_unsigned(row.get(1).map_err(map_sqlite)?)?;
    let mtime_ns = row.get(2).map_err(map_sqlite)?;
    let inode = read_unsigned(row.get(3).map_err(map_sqlite)?)?;
    let hash: String = row.get(4).map_err(map_sqlite)?;
    let checked_at_ns = row.get(5).map_err(map_sqlite)?;
    let valid: i64 = row.get(6).map_err(map_sqlite)?;
    let error: Option<String> = row.get(7).map_err(map_sqlite)?;
    if ![0, 1].contains(&valid) || (valid == 1) != error.is_none() {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "invalid persisted file state",
        ));
    }
    Ok(FileRow {
        path: RepoPath::parse(&path).map_err(map_invalid_value)?,
        size,
        mtime_ns,
        inode,
        hash: Sha256::parse(&hash).map_err(map_invalid_value)?,
        checked_at_ns,
        valid: valid == 1,
        error,
    })
}

#[cfg(test)]
mod tests {
    use super::list_files;
    use crate::service::StorageErrorKind;
    use rusqlite::Connection;

    #[test]
    fn rejects_an_out_of_range_persisted_valid_flag() {
        // Given: malformed persisted data can pair valid=2 with an error.
        let connection = Connection::open_in_memory().unwrap();
        connection.execute_batch(
            "CREATE TABLE files(path, size, mtime_ns, inode, hash, checked_at_ns, valid, error);
             INSERT INTO files VALUES('a.md', 1, 0, 1,
               '0000000000000000000000000000000000000000000000000000000000000000',
               0, 2, 'invalid file');",
        ).unwrap();
        // When
        let error = list_files(&connection).unwrap_err();
        // Then: an invalid boolean representation cannot become a valid row.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    }
}
