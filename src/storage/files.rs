use super::{
    connection::map_sqlite,
    graph::{check_unsigned, map_invalid_value, map_mutation, read_unsigned},
    query::{Files, build_query},
};
use crate::{
    domain::{RepoPath, Sha256},
    service::{FileRow, StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, OptionalExtension, params_from_iter};
use sea_query::{Expr, ExprTrait, OnConflict, Query, Value};

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
    let raw = db
        .query_row(&sql, params_from_iter(values), |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, String>(4)?,
                row.get(5)?,
                row.get::<_, i64>(6)?,
                row.get::<_, Option<String>>(7)?,
            ))
        })
        .optional()
        .map_err(map_sqlite)?;
    raw.map(
        |(path, size, mtime_ns, inode, hash, checked_at_ns, valid, error)| {
            if ![0, 1].contains(&valid) || (valid == 1) != error.is_none() {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "invalid persisted file state",
                ));
            }
            Ok(FileRow {
                path: RepoPath::parse(&path).map_err(map_invalid_value)?,
                size: read_unsigned(size)?,
                mtime_ns,
                inode: read_unsigned(inode)?,
                hash: Sha256::parse(&hash).map_err(map_invalid_value)?,
                checked_at_ns,
                valid: valid == 1,
                error,
            })
        },
    )
    .transpose()
}
