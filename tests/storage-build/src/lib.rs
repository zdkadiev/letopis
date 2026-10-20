//! Connection policy for TK-1 integration checks; not the production storage adapter.

use rusqlite::{Connection, LoadExtensionGuard};
use std::{error::Error, path::Path};

pub type CheckResult<T> = Result<T, Box<dyn Error>>;

pub fn require_sqlite_version(version: i32) -> CheckResult<()> {
    if version < 3_051_003 {
        return Err(format!("SQLite 3.51.3+ required; found version number {version}").into());
    }
    Ok(())
}

/// Open a scratch database with the storage capabilities required by TK-1.
///
/// # Safety
/// `extension` must name the trusted vec1 library built by the checksum-verifying runner.
/// Loading arbitrary native libraries can execute code with unrestricted memory access.
pub unsafe fn open_checked(path: &Path, extension: &Path) -> CheckResult<Connection> {
    require_sqlite_version(rusqlite::version_number())?;
    let conn = Connection::open(path)?;
    conn.execute_batch(
        "CREATE VIRTUAL TABLE temp.tk1_fts USING fts5(text, content='', contentless_delete=1);
         DROP TABLE temp.tk1_fts;",
    )
    .map_err(|error| format!("FTS5 contentless-delete is required: {error}"))?;

    // SAFETY: the runner compiles this library from checksum-verified vec1 C source.
    // Callers must supply that trusted build; arbitrary extension paths execute native code.
    unsafe {
        let _guard = LoadExtensionGuard::new(&conn)?;
        conn.load_extension(extension, Some("sqlite3_extension_init"))
            .map_err(|error| format!("vec1 extension could not be loaded: {error}"))?;
    }
    let info: String = conn
        .query_row("SELECT vec1_info()", [], |row| row.get(0))
        .map_err(|error| format!("vec1 extension is required: {error}"))?;
    if !info.starts_with("version 0.8 ") {
        return Err(format!("vec1 extension 0.8 required; found {info}").into());
    }
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    conn.pragma_update(None, "foreign_keys", true)?;
    #[cfg(target_os = "macos")]
    conn.pragma_update(None, "fullfsync", true)?;
    Ok(conn)
}
