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
mod tests {
    use super::register_vec1;
    use crate::storage::connection::open_database;

    #[test]
    fn native_capabilities_match_pinned_stack() {
        // Given
        register_vec1().unwrap();
        let root = tempfile::TempDir::new().unwrap();
        let connection = open_database(&root.path().join("native.db")).unwrap();
        // When / Then: probe the native version and required capabilities.
        assert!(rusqlite::version_number() >= 3_051_003);
        let info: String = connection
            .query_row("select vec1_info()", [], |row| row.get(0))
            .unwrap();
        assert!(info.starts_with("version 0.8"), "{info}");
        println!("SQLite={} vec1={info}", rusqlite::version());
        for (pragma, expected) in [
            ("journal_mode", "wal"),
            ("synchronous", "2"),
            ("foreign_keys", "1"),
        ] {
            let value: String = connection
                .query_row(
                    &format!("select cast({pragma} as text) from pragma_{pragma}"),
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(value, expected);
            println!("{pragma}={value}");
        }
        #[cfg(target_os = "macos")]
        {
            let value: i64 = connection
                .pragma_query_value(None, "fullfsync", |row| row.get(0))
                .unwrap();
            assert_eq!(value, 1);
            println!("fullfsync={value}");
        }
        connection.execute_batch("create virtual table f using fts5(text, content='', contentless_delete=1); insert into f(rowid,text) values(1,'secret'); delete from f where rowid=1;").unwrap();
        let count: i64 = connection
            .query_row("select count(*) from f", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
        let error = connection
            .query_row("select load_extension('missing')", [], |row| {
                row.get::<_, String>(0)
            })
            .unwrap_err();
        assert!(error.to_string().contains("not authorized"));
    }
}
