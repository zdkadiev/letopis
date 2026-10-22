use super::{
    connection::map_sqlite,
    graph::{check_unsigned, map_invalid_value, map_mutation, read_unsigned},
};
use crate::{
    domain::{RepoPath, Sha256},
    service::{FileRow, StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, OptionalExtension, params};
pub(super) fn upsert_file(db: &Connection, file: &FileRow) -> StorageResult<()> {
    db.execute(
        "INSERT INTO files(path,size,mtime_ns,inode,hash,checked_at_ns,valid,error)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8) ON CONFLICT(path) DO UPDATE SET
         size=excluded.size,mtime_ns=excluded.mtime_ns,inode=excluded.inode,
         hash=excluded.hash,checked_at_ns=excluded.checked_at_ns,valid=excluded.valid,error=excluded.error",
        params![file.path.as_str(),check_unsigned(file.size)?,file.mtime_ns,
            check_unsigned(file.inode)?,file.hash.to_hex(),file.checked_at_ns,file.valid,file.error],
    ).map_err(map_mutation)?;
    Ok(())
}
pub(super) fn delete_file(db: &Connection, path: &RepoPath) -> StorageResult<()> {
    db.execute("DELETE FROM files WHERE path=?1", [path.as_str()])
        .map_err(map_mutation)?;
    Ok(())
}
pub(super) fn read_file(db: &Connection, path: &RepoPath) -> StorageResult<Option<FileRow>> {
    let raw = db.query_row(
        "SELECT path,size,mtime_ns,inode,hash,checked_at_ns,valid,error FROM files WHERE path=?1",
        [path.as_str()],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,i64>(1)?,row.get(2)?,
            row.get::<_,i64>(3)?,row.get::<_,String>(4)?,row.get(5)?,
            row.get::<_,i64>(6)?,row.get::<_,Option<String>>(7)?)),
    ).optional().map_err(map_sqlite)?;
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
