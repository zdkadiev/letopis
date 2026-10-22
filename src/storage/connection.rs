use super::native::register_vec1;
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, OpenFlags};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const MIN_SQLITE_VERSION: i32 = 3_051_003;
const VEC1_VERSION_PREFIX: &str = "version 0.8";

pub(super) fn map_sqlite(error: rusqlite::Error) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        "storage database operation failed",
        error,
    )
}

pub(super) fn prepare_paths(cache: &Path, vectors: &Path) -> StorageResult<(PathBuf, PathBuf)> {
    let cache = resolve_path(cache)?;
    let vectors = resolve_path(vectors)?;
    if cache == vectors || has_same_file(&cache, &vectors)? {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "storage databases must be distinct files",
        ));
    }
    Ok((cache, vectors))
}

fn resolve_path(path: &Path) -> StorageResult<PathBuf> {
    let raw = path
        .to_str()
        .filter(|raw| !raw.is_empty() && !raw.contains('\0'))
        .ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            )
        })?;
    let path = Path::new(raw);
    let resolved = if path.exists() {
        fs::canonicalize(path)
    } else {
        let name = path.file_name().ok_or_else(|| {
            StorageError::new(
                StorageErrorKind::InvalidRequest,
                "invalid storage database path",
            )
        })?;
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or(Path::new("."));
        fs::canonicalize(parent).map(|parent| parent.join(name))
    }
    .map_err(|error| {
        StorageError::new_with_cause(
            StorageErrorKind::InvalidRequest,
            "storage parent or file is unavailable",
            error,
        )
    })?;
    if resolved.to_str().is_none()
        || resolved.is_dir()
        || (!path.exists() && fs::symlink_metadata(path).is_ok())
    {
        return Err(StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage database path",
        ));
    }
    Ok(resolved)
}

fn has_same_file(cache: &Path, vectors: &Path) -> StorageResult<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let read = |path| match fs::metadata(path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(StorageError::new_with_cause(
                StorageErrorKind::InvalidRequest,
                "storage file identity unavailable",
                error,
            )),
        };
        if let (Some(cache), Some(vectors)) = (read(cache)?, read(vectors)?) {
            return Ok(cache.dev() == vectors.dev() && cache.ino() == vectors.ino());
        }
    }
    Ok(false)
}

pub(super) fn open_database(path: &Path) -> StorageResult<Connection> {
    register_vec1()?;
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags).map_err(map_sqlite)?;
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(map_sqlite)?;
    configure_database(&connection, "main")?;
    verify_capabilities(&connection)?;
    Ok(connection)
}

pub(super) fn attach_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    let filename = path.to_str().ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector database path",
        )
    })?;
    connection
        .execute("ATTACH DATABASE ?1 AS vec", [filename])
        .map_err(map_sqlite)?;
    configure_database(connection, "vec")
}

fn configure_database(connection: &Connection, schema: &str) -> StorageResult<()> {
    // `schema` is private and only the literal main/vec names reach this helper.
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite)?;
    let journal: String = connection
        .query_row(&format!("PRAGMA {schema}.journal_mode=WAL"), [], |row| {
            row.get(0)
        })
        .map_err(map_sqlite)?;
    connection
        .execute_batch(&format!("PRAGMA {schema}.synchronous=FULL;"))
        .map_err(map_sqlite)?;
    #[cfg(target_os = "macos")]
    connection
        .execute_batch(&format!("PRAGMA {schema}.fullfsync=ON;"))
        .map_err(map_sqlite)?;
    let sync: i64 = connection
        .query_row(&format!("PRAGMA {schema}.synchronous"), [], |row| {
            row.get(0)
        })
        .map_err(map_sqlite)?;
    let foreign: i64 = connection
        .pragma_query_value(None, "foreign_keys", |row| row.get(0))
        .map_err(map_sqlite)?;
    #[cfg(target_os = "macos")]
    {
        let full: i64 = connection
            .query_row(&format!("PRAGMA {schema}.fullfsync"), [], |row| row.get(0))
            .map_err(map_sqlite)?;
        if full != 1 {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "storage durability policy unavailable",
            ));
        }
    }
    if journal != "wal" || sync != 2 || foreign != 1 {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "storage connection policy unavailable",
        ));
    }
    Ok(())
}

fn verify_capabilities(connection: &Connection) -> StorageResult<()> {
    if rusqlite::version_number() < MIN_SQLITE_VERSION {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported SQLite version",
        ));
    }
    let version: String = connection
        .query_row("SELECT vec1_info()", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if !version.starts_with(VEC1_VERSION_PREFIX) {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported native vector version",
        ));
    }
    connection.execute_batch("CREATE VIRTUAL TABLE temp.letopis_fts_probe USING fts5(text,content='',contentless_delete=1); DROP TABLE temp.letopis_fts_probe;").map_err(map_sqlite)
}

#[cfg(test)]
mod tests {
    use super::{attach_vectors, open_database};
    use tempfile::TempDir;

    #[test]
    fn both_database_connections_enforce_policy() {
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        for path in [&cache, &vectors] {
            let connection = open_database(path).unwrap();
            assert_policy(&connection, "main");
            let info: String = connection
                .query_row("SELECT vec1_info()", [], |r| r.get(0))
                .unwrap();
            assert!(info.starts_with("version 0.8"));
            let error = connection
                .query_row("SELECT load_extension('missing')", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap_err();
            assert!(error.to_string().contains("not authorized"));
        }
        let connection = open_database(&cache).unwrap();
        attach_vectors(&connection, &vectors).unwrap();
        assert_policy(&connection, "vec");
    }

    fn assert_policy(connection: &rusqlite::Connection, schema: &str) {
        let journal: String = connection
            .query_row(&format!("PRAGMA {schema}.journal_mode"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        for (pragma, expected) in [("synchronous", 2), ("foreign_keys", 1), ("busy_timeout", 0)] {
            let actual: i64 = connection
                .query_row(&format!("PRAGMA {schema}.{pragma}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{schema}.{pragma}");
        }
        #[cfg(target_os = "macos")]
        assert_eq!(
            connection
                .query_row(&format!("PRAGMA {schema}.fullfsync"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }
}
