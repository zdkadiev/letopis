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
    pub(super) static ON_INITIALIZATION_CONTENTION: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
}

const INITIALIZATION_WAIT: Duration = Duration::from_secs(5);
const INITIALIZATION_RETRY: Duration = Duration::from_millis(1);
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

pub(crate) fn map_sqlite(error: rusqlite::Error) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        "storage database operation failed",
        error,
    )
}

// Only schema creation and WAL initialization/readiness use this bounded lock wait.
// The five-second budget applies to each call, not the entire open or command.
// Cache/read connections retain zero busy timeout; VectorStorage separately
// configures a bounded wait on its independent vectors writer.
pub(super) fn wait_for_initialization<T>(
    mut operation: impl FnMut() -> StorageResult<T>,
) -> StorageResult<T> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) if can_retry_initialization(&error, started.elapsed()) => {
                #[cfg(test)]
                if let Some(hook) = ON_INITIALIZATION_CONTENTION.take() {
                    hook();
                }
                thread::sleep(INITIALIZATION_RETRY);
            }
            Err(error) => return Err(error),
        }
    }
}

fn can_retry_initialization(error: &StorageError, elapsed: Duration) -> bool {
    is_initialization_contention(error) && elapsed < INITIALIZATION_WAIT
}

fn is_initialization_contention(error: &StorageError) -> bool {
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

/// Open a writable database without following the final path component.
///
/// # Errors
/// Returns native registration and `SQLite` opening failures; callers verify paths before configuration.
pub(crate) fn open_confined_database(path: &Path) -> StorageResult<Connection> {
    open_raw_database(path, DATABASE_OPEN_FLAGS | OpenFlags::SQLITE_OPEN_NOFOLLOW)
}

/// Open an existing database read-only, without creating or configuring persistent state.
///
/// # Errors
/// Returns native registration, `SQLite` opening or timeout configuration failures.
pub(crate) fn open_observed_database(path: &Path) -> StorageResult<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY
        | OpenFlags::SQLITE_OPEN_NO_MUTEX
        | OpenFlags::SQLITE_OPEN_URI
        | OpenFlags::SQLITE_OPEN_NOFOLLOW;
    let connection = open_raw_database(path, flags)?;
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(map_sqlite)?;
    Ok(connection)
}

/// Configure the writable vector attachment after composition has verified its path.
///
/// # Errors
/// Reports unsupported `SQLite` policy or database contention and I/O.
pub(crate) fn configure_vectors(connection: &Connection) -> StorageResult<()> {
    configure_database(connection, VECTOR_SCHEMA)
}

fn open_raw_database(path: &Path, flags: OpenFlags) -> StorageResult<Connection> {
    register_vec1()?;
    Connection::open_with_flags(path, flags).map_err(map_sqlite)
}

pub(crate) fn configure_connection(connection: &Connection) -> StorageResult<()> {
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(map_sqlite)?;
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
    connection
        .execute("attach database ?1 as vec", [filename])
        .map_err(map_sqlite)?;
    Ok(())
}

pub(crate) fn attach_cache(connection: &Connection, path: &Path) -> StorageResult<()> {
    let uri = encode_read_only_uri(path)?;
    connection
        .execute("attach database ?1 as cache", [uri])
        .map_err(map_sqlite)?;
    Ok(())
}

pub(crate) fn attach_read_only_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    let uri = encode_read_only_uri(path)?;
    connection
        .execute("attach database ?1 as vec", [uri])
        .map_err(map_sqlite)?;
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
    connection
        .pragma_update(None, FOREIGN_KEYS_PRAGMA, "ON")
        .map_err(map_sqlite)?;
    let journal = initialize_journal(connection, schema)?;
    connection
        .execute_batch(&format!("pragma {schema}.synchronous=full;"))
        .map_err(map_sqlite)?;
    #[cfg(target_os = "macos")]
    connection
        .execute_batch(&format!("pragma {schema}.fullfsync=on;"))
        .map_err(map_sqlite)?;
    let sync: i64 = connection
        .query_row(&format!("pragma {schema}.synchronous"), [], |row| {
            row.get(0)
        })
        .map_err(map_sqlite)?;
    let foreign: i64 = connection
        .pragma_query_value(None, FOREIGN_KEYS_PRAGMA, |row| row.get(0))
        .map_err(map_sqlite)?;
    #[cfg(target_os = "macos")]
    {
        let full: i64 = connection
            .query_row(&format!("pragma {schema}.fullfsync"), [], |row| row.get(0))
            .map_err(map_sqlite)?;
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
    wait_for_initialization(|| {
        let journal = get_journal(connection, schema).map_err(map_sqlite)?;
        if journal == WAL_MODE {
            return Ok(journal);
        }
        connection
            .query_row(&format!("pragma {schema}.journal_mode=wal"), [], |row| {
                row.get(0)
            })
            .map_err(map_sqlite)
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
    let version: String = connection
        .query_row("select vec1_info()", [], |row| row.get(0))
        .map_err(map_sqlite)?;
    if !version.starts_with(VEC1_VERSION_PREFIX) {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "unsupported native vector version",
        ));
    }
    connection.execute_batch("create virtual table temp.letopis_fts_probe using fts5(text,content='',contentless_delete=1); drop table temp.letopis_fts_probe;").map_err(map_sqlite)
}

#[cfg(test)]
mod policy_tests {
    use super::{
        attach_cache, attach_vectors, can_retry_initialization, configure_database, get_journal,
        map_sqlite, open_database, wait_for_initialization,
    };
    use crate::service::{StorageError, StorageErrorKind};
    use crate::with_verified_database_paths;
    use rusqlite::{Connection, ErrorCode, ffi};
    use std::ffi::{CStr, c_char, c_int, c_void};
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
        process::Command,
        time::Duration,
    };
    use tempfile::TempDir;

    use crate::process_test_support as process_fixture;

    struct IgnoredPolicy<'a>(&'a Connection);

    #[test]
    fn initialization_retry_stops_at_the_exact_budget_without_retrying_other_errors() {
        // Given: exercise the real SQLite contention classification without a clock race.
        let busy = map_sqlite(rusqlite::Error::SqliteFailure(
            ffi::Error::new(ffi::SQLITE_BUSY),
            None,
        ));
        let ordinary = StorageError::new(StorageErrorKind::InternalError, "not contention");
        for (seconds, expected) in [(0, true), (4, true), (5, false), (6, false)] {
            // When / Then: the five-second deadline is exclusive for another retry.
            let elapsed = Duration::from_secs(seconds);
            assert_eq!(can_retry_initialization(&busy, elapsed), expected);
            assert!(!can_retry_initialization(&ordinary, elapsed));
        }
    }

    #[test]
    fn rejects_a_service_symlink_back_to_the_pinned_directory() {
        // Given: identity equality after following a link is not confinement.
        let root = TempDir::new().unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let path = base.join(".letopis");
        fs::create_dir(&path).unwrap();
        let pinned = File::open(&path).unwrap();

        let original = base.join("original");
        fs::rename(&path, &original).unwrap();
        symlink(&original, &path).unwrap();
        // When: the filesystem revalidates the retained directory and database leaves.
        let error = with_verified_database_paths(&path, &pinned, |_, _| Ok(())).unwrap_err();
        // Then: the unsafe replacement is rejected without creating database files.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(fs::read_dir(original).unwrap().next().is_none());
    }

    #[test]
    fn rejects_service_directory_replacement_with_a_fifo_without_blocking() {
        const CHILD: &str = "LETOPIS_STORAGE_DIRECTORY_FIFO_CHILD";
        if std::env::var_os(CHILD).is_none() {
            process_fixture::supervise_test(
                "storage::connection::policy_tests::rejects_service_directory_replacement_with_a_fifo_without_blocking",
                &[(CHILD, "1")],
            );
            return;
        }
        // Given: the pin was valid before an independent path replacement.
        let root = TempDir::new().unwrap();
        let base = fs::canonicalize(root.path()).unwrap();
        let path = base.join(".letopis");
        fs::create_dir(&path).unwrap();
        let pinned = File::open(&path).unwrap();

        fs::rename(&path, base.join("original")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        // When: the filesystem revalidates the retained directory and database leaves.
        let error = with_verified_database_paths(&path, &pinned, |_, _| Ok(())).unwrap_err();
        // Then: the parent's owned-child watchdog detects a blocking open.
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(
            fs::read_dir(base.join("original"))
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn rejects_non_regular_database_leaves_before_sqlite_opens_them() {
        for name in ["cache.db", "vectors.db"] {
            // Given: a pinned service directory contains a nonregular database leaf.
            let root = TempDir::new().unwrap();
            let path = fs::canonicalize(root.path()).unwrap();
            let pinned = File::open(&path).unwrap();

            fs::create_dir(path.join(name)).unwrap();
            // When: the filesystem revalidates the retained directory and database leaves.
            let error = with_verified_database_paths(&path, &pinned, |_, _| Ok(())).unwrap_err();
            // Then: the unsafe replacement is rejected without creating database files.
            assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
            assert!(fs::read_dir(path.join(name)).unwrap().next().is_none());
        }
    }

    #[test]
    fn does_not_retry_non_contention_storage_errors() {
        // Given: a finite second result exposes an accidental retry without timing.
        let mut attempts = 0;
        // When
        let error = wait_for_initialization(|| {
            attempts += 1;
            if attempts == 1 {
                return Err(StorageError::new(
                    StorageErrorKind::InternalError,
                    "nonretryable",
                ));
            }
            Ok(42)
        })
        .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert_eq!(attempts, 1);
    }

    #[test]
    fn rejects_a_database_that_cannot_enable_wal() {
        // Given: SQLite keeps an in-memory database in memory journal mode.
        let connection = Connection::open_in_memory().unwrap();
        // When
        let error = configure_database(&connection, "main").unwrap_err();
        // Then: successful PRAGMA execution alone does not establish durability.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        let journal: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal, "memory");
    }

    #[test]
    fn reads_the_existing_database_journal_mode() {
        // Given
        let root = TempDir::new().unwrap();
        let connection = Connection::open(root.path().join("database")).unwrap();
        for expected in ["wal", "delete"] {
            connection
                .pragma_update(None, "journal_mode", expected)
                .unwrap();
            // When
            let journal = get_journal(&connection, "main").unwrap();
            // Then: readiness checks must observe the mode before changing it.
            assert_eq!(journal, expected);
        }
    }

    #[test]
    fn rejects_each_policy_setting_that_sqlite_does_not_apply() {
        for policy in [c"foreign_keys", c"synchronous"] {
            // Given: ignore only the selected setter while allowing all observations.
            let root = TempDir::new().unwrap();
            let connection = Connection::open(root.path().join("database")).unwrap();
            let name = policy.to_str().unwrap();
            connection.pragma_update(None, name, 0).unwrap();
            let ignored = IgnoredPolicy::new(&connection, policy);
            // When
            let error = configure_database(&connection, "main").unwrap_err();
            drop(ignored);
            // Then: WAL and the other setting work, but one bad setting must fail closed.
            assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
            assert_eq!(get_journal(&connection, "main").unwrap(), "wal");
            for (setting, expected) in [("foreign_keys", 1), ("synchronous", 2)] {
                let observed: i64 = connection
                    .pragma_query_value(None, setting, |row| row.get(0))
                    .unwrap();
                assert_eq!(observed, if setting == name { 0 } else { expected });
            }
            // Cleanup is scoped: the same setter works after the authorizer is removed.
            connection.pragma_update(None, name, 1).unwrap();
            let restored: i64 = connection
                .pragma_query_value(None, name, |row| row.get(0))
                .unwrap();
            assert_eq!(restored, 1);
        }
    }

    #[test]
    fn both_database_connections_enforce_policy() {
        // Given
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache.db");
        let vectors = root.path().join("vectors.db");
        for path in [&cache, &vectors] {
            // When
            let connection = open_database(path).unwrap();
            // Then
            assert_policy(&connection, "main");
            let info: String = connection
                .query_row("select vec1_info()", [], |r| r.get(0))
                .unwrap();
            assert!(info.starts_with("version 0.8"));
            let error = connection
                .query_row("select load_extension('missing')", [], |r| {
                    r.get::<_, String>(0)
                })
                .unwrap_err();
            assert!(error.to_string().contains("not authorized"));
        }
        // When / Then: the attached vectors database follows the same policy.
        let connection = open_database(&cache).unwrap();
        attach_vectors(&connection, &vectors).unwrap();
        assert_policy(&connection, "vec");
    }

    #[test]
    fn attached_cache_is_read_only_with_encoded_path_delimiters() {
        // Given: delimiters, quotes and UTF-8 must name the literal file.
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache ?#%é' name.db");
        let writer = open_database(&cache).unwrap();
        writer.pragma_update(None, "user_version", 7).unwrap();
        let vectors = open_database(&root.path().join("vectors")).unwrap();
        // When
        attach_cache(&vectors, &cache).unwrap();
        let version: i64 = vectors
            .query_row("pragma cache.user_version", [], |r| r.get(0))
            .unwrap();
        let error = vectors
            .execute_batch("pragma cache.user_version=8;")
            .unwrap_err();
        // Then: the attached URI uses the intended database and rejects writes.
        assert_eq!(version, 7);
        assert_eq!(error.sqlite_error_code(), Some(ErrorCode::ReadOnly));
        assert_eq!(
            writer
                .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            7
        );
    }

    fn assert_policy(connection: &rusqlite::Connection, schema: &str) {
        let journal: String = connection
            .query_row(&format!("pragma {schema}.journal_mode"), [], |r| r.get(0))
            .unwrap();
        assert_eq!(journal, "wal");
        for (pragma, expected) in [("synchronous", 2), ("foreign_keys", 1), ("busy_timeout", 0)] {
            let actual: i64 = connection
                .query_row(&format!("pragma {schema}.{pragma}"), [], |r| r.get(0))
                .unwrap();
            assert_eq!(actual, expected, "{schema}.{pragma}");
        }
        #[cfg(target_os = "macos")]
        assert_eq!(
            connection
                .query_row(&format!("pragma {schema}.fullfsync"), [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
    }

    impl<'a> IgnoredPolicy<'a> {
        fn new(connection: &'a Connection, policy: &'static CStr) -> Self {
            // SAFETY: the owned connection outlives this guard, the callback never
            // unwinds, and its read-only C-string context has static lifetime.
            let result = unsafe {
                ffi::sqlite3_set_authorizer(
                    connection.handle(),
                    Some(accept_policy_setting),
                    policy.as_ptr().cast_mut().cast(),
                )
            };
            assert_eq!(result, ffi::SQLITE_OK);
            Self(connection)
        }
    }

    impl Drop for IgnoredPolicy<'_> {
        fn drop(&mut self) {
            // SAFETY: the borrowed connection is still live; removing a callback
            // cannot access its context after this guard returns.
            unsafe {
                ffi::sqlite3_set_authorizer(self.0.handle(), None, std::ptr::null_mut());
            }
        }
    }

    unsafe extern "C" fn accept_policy_setting(
        context: *mut c_void,
        action: c_int,
        name: *const c_char,
        value: *const c_char,
        _database: *const c_char,
        _trigger: *const c_char,
    ) -> c_int {
        if action != ffi::SQLITE_PRAGMA || context.is_null() || name.is_null() || value.is_null() {
            return ffi::SQLITE_OK;
        }
        // SAFETY: SQLite supplies a live NUL-terminated PRAGMA name for this action;
        // the checked context is the static C string installed by IgnoredPolicy.
        // Comparing bytes neither allocates, invokes SQLite, nor can panic/unwind.
        let selected = unsafe { CStr::from_ptr(name) == CStr::from_ptr(context.cast()) };
        if selected {
            ffi::SQLITE_IGNORE
        } else {
            ffi::SQLITE_OK
        }
    }
}
