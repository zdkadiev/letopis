use super::{
    STORAGE_FAILURE, attach_cache, attach_vectors, configure_database, get_journal,
    is_storage_contention_retryable, open_database, wait_for_storage_contention,
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
    let busy = StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        STORAGE_FAILURE,
        rusqlite::Error::SqliteFailure(ffi::Error::new(ffi::SQLITE_BUSY), None),
    );
    let ordinary = StorageError::new(StorageErrorKind::InternalError, "not contention");
    for (seconds, expected) in [(0, true), (4, true), (5, false), (6, false)] {
        // When / Then: the five-second deadline is exclusive for another retry.
        let elapsed = Duration::from_secs(seconds);
        assert_eq!(is_storage_contention_retryable(&busy, elapsed), expected);
        assert!(!is_storage_contention_retryable(&ordinary, elapsed));
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
            "storage::connection::tests::rejects_service_directory_replacement_with_a_fifo_without_blocking",
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
    let error = wait_for_storage_contention(|| {
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
