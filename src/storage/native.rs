use crate::service::{StorageError, StorageErrorKind, StorageResult};
use libsqlite3_sys::{SQLITE_OK, sqlite3, sqlite3_api_routines, sqlite3_auto_extension};
use std::{
    ffi::{c_char, c_int},
    sync::OnceLock,
};

// SAFETY: this matches the C definition compiled by build.rs, which implements
// SQLite's extension entry-point ABI against the same bundled headers.
unsafe extern "C" {
    fn letopis_vec1_init(
        database: *mut sqlite3,
        error: *mut *mut c_char,
        api: *const sqlite3_api_routines,
    ) -> c_int;
}

pub(super) fn register_vec1() -> StorageResult<()> {
    static RESULT: OnceLock<c_int> = OnceLock::new();
    let result = RESULT.get_or_init(|| {
        // SAFETY: the statically linked C function has SQLite's three-argument
        // extension ABI, using the same bundled headers as libsqlite3-sys. Its
        // code lives for the entire process. Registration retains no Rust data.
        unsafe { sqlite3_auto_extension(Some(letopis_vec1_init)) }
    });
    if *result != SQLITE_OK {
        return Err(StorageError::new(
            StorageErrorKind::InternalError,
            "native vector registration failed",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[path = "native_test.rs"]
mod tests;
