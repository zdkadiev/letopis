use super::native::register_vec1;
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, ErrorCode, OpenFlags};
#[cfg(test)]
use std::cell::RefCell;
use std::{
    error::Error,
    fmt::Write,
    path::Path,
    thread,
    time::{Duration, Instant},
};

#[cfg(test)]
thread_local! {
    pub(super) static ON_STORAGE_CONTENTION: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

const STORAGE_CONTENTION_WAIT: Duration = Duration::from_secs(5);
const STORAGE_CONTENTION_RETRY: Duration = Duration::from_millis(1);
const MIN_SQLITE_VERSION: i32 = 3_051_003;
const VEC1_VERSION_PREFIX: &str = "version 0.8";
const MAIN_SCHEMA: &str = "main";
const VECTOR_SCHEMA: &str = "vec";
const FOREIGN_KEYS_PRAGMA: &str = "foreign_keys";
const WAL_MODE: &str = "wal";
const DATABASE_OPEN_FLAGS: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_CREATE)
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_URI);

pub(crate) const STORAGE_FAILURE: &str = "storage database operation failed";

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> Self {
        Self::new_with_cause(StorageErrorKind::InternalError, STORAGE_FAILURE, error)
    }
}

// Schema creation, WAL initialization/readiness and write-transaction acquisition
// use this bounded lock wait. The five-second budget applies to each call, not the
// entire open or command. Cache/read connections retain zero busy timeout;
// VectorStorage separately configures a bounded wait on its independent writer.

pub(super) fn wait_for_storage_contention<T>(
    mut operation: impl FnMut() -> StorageResult<T>,
) -> StorageResult<T> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if is_storage_contention_retryable(&error, started.elapsed()) => {
                #[cfg(test)]
                if let Some(hook) = ON_STORAGE_CONTENTION.take() {
                    hook();
                }
                thread::sleep(STORAGE_CONTENTION_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn is_storage_contention_retryable(error: &StorageError, elapsed: Duration) -> bool {
    is_storage_contention(error) && elapsed < STORAGE_CONTENTION_WAIT
}

fn is_storage_contention(error: &StorageError) -> bool {
    let cause = error
        .source()
        .and_then(|cause| cause.downcast_ref::<rusqlite::Error>());
    matches!(
        cause.and_then(rusqlite::Error::sqlite_error_code),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    )
}

pub(crate) fn open_database(path: &Path) -> StorageResult<Connection> {
    let connection = open_raw_database(path, DATABASE_OPEN_FLAGS)?;
    configure_connection(&connection)?;
    Ok(connection)
}

/// Opens a writable database without following the final path component.
///
/// # Arguments
///
/// * `path` — the database path to open; its final component must not be a symbolic link.
///
/// # Errors
/// Returns native registration and `SQLite` opening failures; callers verify paths before
/// configuration.
pub(crate) fn open_confined_database(path: &Path) -> StorageResult<Connection> {
    open_raw_database(path, DATABASE_OPEN_FLAGS | OpenFlags::SQLITE_OPEN_NOFOLLOW)
}

/// Opens an existing database read-only, without creating or configuring persistent state.
///
/// # Arguments
///
/// * `path` — the existing database path to open.
///
/// # Errors
/// Returns native registration, `SQLite` opening or timeout configuration failures.
pub(crate) fn open_observed_database(path: &Path) -> StorageResult<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = open_raw_database(path, flags)?;
    connection.busy_timeout(Duration::ZERO)?;
    Ok(connection)
}

/// Configures the writable vector attachment after composition has verified its path.
///
/// # Arguments
///
/// * `connection` — the connection whose `vec` attachment receives the durability policy.
///
/// # Errors
/// Reports unsupported `SQLite` policy or database contention and I/O.
pub(crate) fn configure_vectors(connection: &Connection) -> StorageResult<()> {
    configure_database(connection, VECTOR_SCHEMA)
}

fn open_raw_database(path: &Path, flags: OpenFlags) -> StorageResult<Connection> {
    register_vec1()?;
    Connection::open_with_flags(path, flags).map_err(StorageError::from)
}

pub(crate) fn configure_connection(connection: &Connection) -> StorageResult<()> {
    connection.busy_timeout(Duration::ZERO)?;
    configure_database(connection, MAIN_SCHEMA)?;
    verify_capabilities(connection)
}

pub(crate) fn attach_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    attach_unconfigured_vectors(connection, path)?;
    configure_database(connection, VECTOR_SCHEMA)
}

pub(crate) fn attach_unconfigured_vectors(
    connection: &Connection,
    path: &Path,
) -> StorageResult<()> {
    let filename = path.to_str().ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector database path",
        )
    })?;
    connection.execute("attach database ?1 as vec", [filename])?;
    Ok(())
}

pub(crate) fn attach_cache(connection: &Connection, path: &Path) -> StorageResult<()> {
    let uri = encode_read_only_uri(path)?;
    connection.execute("attach database ?1 as cache", [uri])?;
    Ok(())
}

pub(crate) fn attach_read_only_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    let uri = encode_read_only_uri(path)?;
    connection.execute("attach database ?1 as vec", [uri])?;
    Ok(())
}

fn encode_read_only_uri(path: &Path) -> StorageResult<String> {
    let filename = path.to_str().ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid storage database path",
        )
    })?;
    // Encode URI delimiters and UTF-8 bytes; only the absolute path's separators survive.
    let mut uri = String::from("file:");
    for byte in filename.bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~".contains(&byte) {
            uri.push(char::from(byte));
        } else {
            write!(&mut uri, "%{byte:02X}").map_err(|cause| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "cache URI encoding failed",
                    cause,
                )
            })?;
        }
    }
    uri.push_str("?mode=ro");
    Ok(uri)
}

fn configure_database(connection: &Connection, schema: &str) -> StorageResult<()> {
    // `schema` is private and only the literal main/vec names reach this helper.
    connection.pragma_update(None, FOREIGN_KEYS_PRAGMA, "ON")?;
    let journal = initialize_journal(connection, schema)?;
    connection.execute_batch(&format!("pragma {schema}.synchronous=full;"))?;
    #[cfg(target_os = "macos")]
    connection.execute_batch(&format!("pragma {schema}.fullfsync=on;"))?;
    let sync: i64 = connection.query_row(&format!("pragma {schema}.synchronous"), [], |row| {
        row.get(0)
    })?;
    let foreign: i64 =
        connection.pragma_query_value(None, FOREIGN_KEYS_PRAGMA, |row| row.get(0))?;
    #[cfg(target_os = "macos")]
    {
        let full: i64 =
            connection.query_row(&format!("pragma {schema}.fullfsync"), [], |row| row.get(0))?;
        if full != 1 {
            return Err(StorageError::new(
                StorageErrorKind::InternalError,
                "storage durability policy unavailable",
            ));
        }
    }
    if journal != WAL_MODE || sync != 2 || foreign != 1 {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "storage connection policy unavailable",
        ));
    }
    Ok(())
}

fn initialize_journal(connection: &Connection, schema: &str) -> StorageResult<String> {
    // The first read can encounter BUSY_RECOVERY during concurrent WAL startup.
    // Already-ready databases only read their mode; no writer lock is requested.
    wait_for_storage_contention(|| {
        let journal = get_journal(connection, schema)?;
        if journal == WAL_MODE {
            return Ok(journal);
        }
        connection
            .query_row(&format!("pragma {schema}.journal_mode=wal"), [], |row| {
                row.get(0)
            })
            .map_err(StorageError::from)
    })
}

fn get_journal(connection: &Connection, schema: &str) -> rusqlite::Result<String> {
    connection.query_row(&format!("pragma {schema}.journal_mode"), [], |row| {
        row.get(0)
    })
}

fn verify_capabilities(connection: &Connection) -> StorageResult<()> {
    if rusqlite::version_number() < MIN_SQLITE_VERSION {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported SQLite version",
        ));
    }
    let version: String = connection.query_row("select vec1_info()", [], |row| row.get(0))?;
    if !version.starts_with(VEC1_VERSION_PREFIX) {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported native vector version",
        ));
    }
    connection
        .execute_batch("create virtual table temp.letopis_fts_probe using fts5(text,content='',contentless_delete=1); drop table temp.letopis_fts_probe;")
        .map_err(StorageError::from)
}

#[cfg(test)]
#[path = "connection_test.rs"]
mod tests;
