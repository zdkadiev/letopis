//! `SQLite` failure injection shared by storage regression tests.
use crate::service::VectorOutcome;
use rusqlite::{Connection, ffi};
use std::ffi::{CStr, c_char, c_int, c_void};

/// Make `SQLite` reject rollback statements for one exclusively owned test connection.
///
/// # Panics
/// Panics if `SQLite` cannot register or clear the native authorizer.
pub(super) fn set_rollback_denial(connection: &Connection, enabled: bool) {
    let callback: Option<
        unsafe extern "C" fn(
            *mut c_void,
            c_int,
            *const c_char,
            *const c_char,
            *const c_char,
            *const c_char,
        ) -> c_int,
    > = if enabled { Some(deny_rollback) } else { None };
    // SAFETY: the connection outlives registration; the callback retains no pointers,
    // never unwinds, and this test exclusively owns the synchronous connection.
    let result =
        unsafe { ffi::sqlite3_set_authorizer(connection.handle(), callback, std::ptr::null_mut()) };
    assert_eq!(result, ffi::SQLITE_OK);
}

unsafe extern "C" fn deny_rollback(
    _: *mut c_void,
    action: c_int,
    operation: *const c_char,
    _: *const c_char,
    _: *const c_char,
    _: *const c_char,
) -> c_int {
    if action == ffi::SQLITE_TRANSACTION && !operation.is_null() {
        // SAFETY: SQLite supplies a NUL-terminated operation name for this action,
        // valid for the duration of the callback; it is never retained.
        if unsafe { CStr::from_ptr(operation) }.to_bytes() == b"ROLLBACK" {
            return ffi::SQLITE_DENY;
        }
    }
    ffi::SQLITE_OK
}

/// Concrete database owners used by library fixtures; vectors close before cache.
#[derive(Debug)]
pub(crate) struct TestDatabases {
    pub(crate) vectors: super::VectorStorage,
    pub(crate) cache: super::CacheStorage,
}

/// Open the real composition path for an independently owned test database pair.
///
/// # Errors
/// Returns the same path, schema and `SQLite` failures as production composition.
pub(crate) fn open_test_storage(
    cache: &std::path::Path,
    vectors: &std::path::Path,
) -> crate::service::StorageResult<TestDatabases> {
    crate::open_sqlite_databases(cache, vectors)
        .map(|(vectors, cache)| TestDatabases { vectors, cache })
}

/// Open the real confined composition path for library fixtures.
///
/// # Errors
/// Rejects unsafe paths, observed replacement and `SQLite` initialization failures.
pub(crate) fn open_confined_test_storage(
    directory: &std::path::Path,
    pinned: &impl rustix::fd::AsFd,
) -> crate::service::StorageResult<TestDatabases> {
    crate::open_confined_databases(directory, pinned)
        .map(|(vectors, cache)| TestDatabases { vectors, cache })
}

/// Get a successful fixture outcome, panicking if its configuration changed.
#[cfg(test)]
#[track_caller]
pub(crate) fn get_current<T>(outcome: VectorOutcome<T>) -> T {
    match outcome {
        VectorOutcome::Current(value) => value,
        VectorOutcome::ConfigChanged => panic!("fixture configuration changed"),
    }
}
