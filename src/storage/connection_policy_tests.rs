use super::{
    ConfinedDirectory, configure_database, prepare_paths, read_journal, resolve_path,
    wait_for_initialization,
};
use crate::service::{StorageError, StorageErrorKind};
use rusqlite::{Connection, ffi};
use std::ffi::{CStr, c_char, c_int, c_void};
use std::path::Path;
use std::{
    fs::{self, File},
    os::unix::fs::symlink,
    process::Command,
};
use tempfile::TempDir;

use crate::process_test_support as process_fixture;

struct IgnoredPolicy<'a>(&'a Connection);

#[test]
fn rejects_a_service_symlink_back_to_the_pinned_directory() {
    // Arrange: identity equality after following a link is not confinement.
    let root = TempDir::new().unwrap();
    let base = fs::canonicalize(root.path()).unwrap();
    let path = base.join(".letopis");
    fs::create_dir(&path).unwrap();
    let pinned = File::open(&path).unwrap();
    let directory = ConfinedDirectory::new(&path, &pinned);
    let original = base.join("original");
    fs::rename(&path, &original).unwrap();
    symlink(&original, &path).unwrap();
    // Act
    let error = directory.verify().unwrap_err();
    // Assert
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
    // Arrange: the pin was valid before an independent path replacement.
    let root = TempDir::new().unwrap();
    let base = fs::canonicalize(root.path()).unwrap();
    let path = base.join(".letopis");
    fs::create_dir(&path).unwrap();
    let pinned = File::open(&path).unwrap();
    let directory = ConfinedDirectory::new(&path, &pinned);
    fs::rename(&path, base.join("original")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    // Act
    let error = directory.verify().unwrap_err();
    // Assert: the parent's owned-child watchdog detects a blocking open.
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
        // Arrange
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = ConfinedDirectory::new(&path, &pinned);
        fs::create_dir(path.join(name)).unwrap();
        // Act
        let error = directory.verify().unwrap_err();
        // Assert
        assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
        assert!(fs::read_dir(path.join(name)).unwrap().next().is_none());
    }
}

#[test]
fn does_not_retry_non_contention_storage_errors() {
    // Arrange: a finite second result exposes an accidental retry without timing.
    let mut attempts = 0;
    // Act
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
    // Assert
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(attempts, 1);
}

#[test]
fn rejects_database_paths_with_nul_bytes() {
    // Arrange / Act
    let error = resolve_path(Path::new("bad\0database")).unwrap_err();
    // Assert: SQLite must never receive a path that is not a filesystem name.
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
}

#[test]
fn rejects_an_existing_directory_as_a_database_path() {
    // Arrange
    let root = TempDir::new().unwrap();
    // Act
    let error = resolve_path(root.path()).unwrap_err();
    // Assert
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
}

#[test]
fn preserves_non_absence_errors_when_observing_database_identity() {
    // Arrange: a regular parent causes ENOTDIR rather than a missing leaf.
    let root = TempDir::new().unwrap();
    let parent = root.path().join("ordinary-file");
    fs::write(&parent, b"preserved").unwrap();
    // Act
    let error =
        prepare_paths(&parent.join("cache.db"), &root.path().join("vectors.db")).unwrap_err();
    // Assert
    assert_eq!(error.get_kind(), StorageErrorKind::InvalidRequest);
    assert_eq!(fs::read(&parent).unwrap(), b"preserved");
    assert!(!root.path().join("vectors.db").exists());
}

#[test]
fn rejects_a_database_that_cannot_enable_wal() {
    // Arrange: SQLite keeps an in-memory database in memory journal mode.
    let connection = Connection::open_in_memory().unwrap();
    // Act
    let error = configure_database(&connection, "main").unwrap_err();
    // Assert: successful PRAGMA execution alone does not establish durability.
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    let journal: String = connection
        .pragma_query_value(None, "journal_mode", |row| row.get(0))
        .unwrap();
    assert_eq!(journal, "memory");
}

#[test]
fn reads_the_existing_database_journal_mode() {
    // Arrange
    let root = TempDir::new().unwrap();
    let connection = Connection::open(root.path().join("database")).unwrap();
    for expected in ["wal", "delete"] {
        connection
            .pragma_update(None, "journal_mode", expected)
            .unwrap();
        // Act
        let journal = read_journal(&connection, "main").unwrap();
        // Assert: readiness checks must observe the mode before changing it.
        assert_eq!(journal, expected);
    }
}

#[test]
fn rejects_each_policy_setting_that_sqlite_does_not_apply() {
    for policy in [c"foreign_keys", c"synchronous"] {
        // Arrange: ignore only the selected setter while allowing all observations.
        let root = TempDir::new().unwrap();
        let connection = Connection::open(root.path().join("database")).unwrap();
        let name = policy.to_str().unwrap();
        connection.pragma_update(None, name, 0).unwrap();
        let ignored = IgnoredPolicy::new(&connection, policy);
        // Act
        let error = configure_database(&connection, "main").unwrap_err();
        drop(ignored);
        // Assert: WAL and the other setting work, but one bad setting must fail closed.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert_eq!(read_journal(&connection, "main").unwrap(), "wal");
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

impl<'a> IgnoredPolicy<'a> {
    fn new(connection: &'a Connection, policy: &'static CStr) -> Self {
        // SAFETY: the owned connection outlives this guard, the callback never
        // unwinds, and its read-only C-string context has static lifetime.
        let result = unsafe {
            ffi::sqlite3_set_authorizer(
                connection.handle(),
                Some(ignore_policy_setter),
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

unsafe extern "C" fn ignore_policy_setter(
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
