use super::native::register_vec1;
use crate::service::{StorageError, StorageErrorKind, StorageResult};
use rusqlite::{Connection, ErrorCode, OpenFlags};
use rustix::{
    fd::{AsFd, BorrowedFd},
    fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, open, statat},
    io::Errno,
};
#[cfg(test)]
use std::cell::RefCell;
use std::{
    error::Error,
    fmt::Write,
    fs,
    path::{Path, PathBuf},
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
pub(super) const CACHE_DATABASE: &str = "cache.db";
pub(super) const VECTOR_DATABASE: &str = "vectors.db";
const DATABASE_OPEN_FLAGS: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_CREATE)
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX)
    .union(OpenFlags::SQLITE_OPEN_URI);

pub(super) struct ConfinedDirectory<'a> {
    path: &'a Path,
    pinned: BorrowedFd<'a>,
    #[cfg(test)]
    before_open: RefCell<Option<Box<dyn FnOnce()>>>,
}
impl<'a> ConfinedDirectory<'a> {
    pub(super) fn new(path: &'a Path, pinned: &'a impl AsFd) -> Self {
        Self {
            path,
            pinned: pinned.as_fd(),
            #[cfg(test)]
            before_open: RefCell::new(None),
        }
    }

    /// Open a named database only while its path matches the pinned service directory.
    ///
    /// # Errors
    /// Rejects unsafe aliases, replacement/symlinks, native capability or `SQLite` failures.
    pub(super) fn open_database(&self, name: &str) -> StorageResult<Connection> {
        self.verify()?;
        #[cfg(test)]
        if let Some(hook) = self.before_open.take() {
            hook();
        }
        // Keep the supplied path: canonicalization would hide the symlink from SQLite.
        let connection = open_raw_database(
            &self.path.join(name),
            DATABASE_OPEN_FLAGS | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        self.verify()?;
        configure_connection(&connection)?;
        Ok(connection)
    }

    /// Attach the confined vectors database under the connection's no-follow policy.
    ///
    /// # Errors
    /// Rejects replaced/unsafe paths or `SQLite` attachment/configuration failures.
    pub(super) fn attach_vectors(&self, connection: &Connection) -> StorageResult<()> {
        self.verify()?;
        #[cfg(test)]
        if let Some(hook) = self.before_open.take() {
            hook();
        }
        // ATTACH inherits SQLITE_OPEN_NOFOLLOW from this directory's main connection.
        attach_unconfigured_vectors(connection, &self.path.join(VECTOR_DATABASE))?;
        self.verify()?;
        configure_database(connection, "vec")
    }

    /// Attach the confined cache read-only for hash-reference checks during vector writes.
    ///
    /// # Errors
    /// Rejects changed directory/leaf paths and failures attaching the read-only database.
    pub(super) fn attach_cache(&self, connection: &Connection) -> StorageResult<()> {
        self.verify()?;
        #[cfg(test)]
        if let Some(hook) = self.before_open.take() {
            hook();
        }
        attach_cache(connection, &self.path.join(CACHE_DATABASE))?;
        self.verify()
    }

    /// Verify the retained directory and require distinct regular database leaves.
    ///
    /// Missing leaves are allowed for initial creation; existing aliases are rejected.
    /// # Errors
    /// Reports unsafe paths, replacement or failed descriptor/path observations.
    pub(super) fn verify(&self) -> StorageResult<()> {
        let directory = open(
            self.path,
            OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(map_path_error)?;
        let current = fstat(&directory).map_err(map_path_error)?;
        let pinned = fstat(self.pinned).map_err(map_path_error)?;
        if !has_same_identity(&current, &pinned) {
            return Err(fail_unsafe_path());
        }
        let cache = self.read_regular_stat(CACHE_DATABASE)?;
        let vectors = self.read_regular_stat(VECTOR_DATABASE)?;
        if let (Some(cache), Some(vectors)) = (cache, vectors)
            && has_same_identity(&cache, &vectors)
        {
            return Err(fail_unsafe_path());
        }
        Ok(())
    }

    fn read_regular_stat(&self, name: &str) -> StorageResult<Option<Stat>> {
        match statat(self.pinned, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) if FileType::from_raw_mode(stat.st_mode) == FileType::RegularFile => {
                Ok(Some(stat))
            }
            Ok(_) => Err(fail_unsafe_path()),
            Err(Errno::NOENT) => Ok(None),
            Err(error) => Err(map_path_error(error)),
        }
    }
}

fn has_same_identity(left: &Stat, right: &Stat) -> bool {
    left.st_dev == right.st_dev && left.st_ino == right.st_ino
}

fn fail_unsafe_path() -> StorageError {
    StorageError::new(
        StorageErrorKind::InvalidRequest,
        "unsafe document storage path",
    )
}

fn map_path_error(error: Errno) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InvalidRequest,
        "cannot validate document storage path",
        error,
    )
}

pub(super) fn map_sqlite(error: rusqlite::Error) -> StorageError {
    StorageError::new_with_cause(
        StorageErrorKind::InternalError,
        "storage database operation failed",
        error,
    )
}

// Only schema creation and WAL initialization/readiness use this bounded lock wait.
// The five-second budget applies to each call, not the entire open or command.
// Ordinary cache/vector writes retain the connection's zero busy timeout.
pub(super) fn wait_for_initialization<T>(
    mut operation: impl FnMut() -> StorageResult<T>,
) -> StorageResult<T> {
    let started = Instant::now();
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error)
                if is_initialization_contention(&error)
                    && started.elapsed() < INITIALIZATION_WAIT =>
            {
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

fn is_initialization_contention(error: &StorageError) -> bool {
    let cause = error
        .source()
        .and_then(|cause| cause.downcast_ref::<rusqlite::Error>());
    matches!(
        cause.and_then(rusqlite::Error::sqlite_error_code),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
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
    let connection = open_raw_database(path, DATABASE_OPEN_FLAGS)?;
    configure_connection(&connection)?;
    Ok(connection)
}

fn open_raw_database(path: &Path, flags: OpenFlags) -> StorageResult<Connection> {
    register_vec1()?;
    Connection::open_with_flags(path, flags).map_err(map_sqlite)
}

fn configure_connection(connection: &Connection) -> StorageResult<()> {
    connection
        .busy_timeout(Duration::ZERO)
        .map_err(map_sqlite)?;
    configure_database(connection, "main")?;
    verify_capabilities(connection)
}

pub(super) fn attach_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    attach_unconfigured_vectors(connection, path)?;
    configure_database(connection, "vec")
}

fn attach_unconfigured_vectors(connection: &Connection, path: &Path) -> StorageResult<()> {
    let filename = path.to_str().ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid vector database path",
        )
    })?;
    connection
        .execute("ATTACH DATABASE ?1 AS vec", [filename])
        .map_err(map_sqlite)?;
    Ok(())
}

pub(super) fn attach_cache(connection: &Connection, path: &Path) -> StorageResult<()> {
    let filename = path.to_str().ok_or_else(|| {
        StorageError::new(
            StorageErrorKind::InvalidRequest,
            "invalid cache database path",
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
    connection
        .execute("ATTACH DATABASE ?1 AS cache", [uri])
        .map_err(map_sqlite)?;
    Ok(())
}

fn configure_database(connection: &Connection, schema: &str) -> StorageResult<()> {
    // `schema` is private and only the literal main/vec names reach this helper.
    connection
        .pragma_update(None, "foreign_keys", "ON")
        .map_err(map_sqlite)?;
    let journal = initialize_journal(connection, schema)?;
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

fn initialize_journal(connection: &Connection, schema: &str) -> StorageResult<String> {
    // The first read can encounter BUSY_RECOVERY during concurrent WAL startup.
    // Already-ready databases only read their mode; no writer lock is requested.
    wait_for_initialization(|| {
        let journal = read_journal(connection, schema).map_err(map_sqlite)?;
        if journal == "wal" {
            return Ok(journal);
        }
        connection
            .query_row(&format!("PRAGMA {schema}.journal_mode=WAL"), [], |row| {
                row.get(0)
            })
            .map_err(map_sqlite)
    })
}

fn read_journal(connection: &Connection, schema: &str) -> rusqlite::Result<String> {
    connection.query_row(&format!("PRAGMA {schema}.journal_mode"), [], |row| {
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
    use super::{ConfinedDirectory, attach_cache, attach_vectors, open_database};
    use rusqlite::ErrorCode;
    use std::{
        fs::{self, File},
        os::unix::fs::symlink,
    };
    use tempfile::TempDir;

    #[test]
    fn reject_cache_attachment_symlink_installed_after_validation() {
        // Arrange: pin the service directory and open the vector connection.
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = ConfinedDirectory::new(&path, &pinned);
        let connection = directory.open_database("vectors.db").unwrap();
        let link = path.join("cache.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        // Act / Assert: the read-only ATTACH inherits NOFOLLOW, including a URI.
        assert!(directory.attach_cache(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
        assert!(!outside.path().join("outside.db-wal").exists());
    }

    #[test]
    fn reject_database_leaf_symlink_installed_after_directory_validation() {
        for name in ["cache.db", "vectors.db"] {
            let root = TempDir::new().unwrap();
            let path = fs::canonicalize(root.path()).unwrap();
            let outside = TempDir::new().unwrap();
            let target = outside.path().join("outside.db");
            fs::write(&target, []).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = ConfinedDirectory::new(&path, &pinned);
            let link = path.join(name);
            let destination = target.clone();
            directory.before_open.replace(Some(Box::new(move || {
                symlink(destination, link).unwrap();
            })));
            assert!(directory.open_database(name).is_err());
            assert_eq!(fs::read(&target).unwrap(), b"");
        }
    }

    #[test]
    fn reject_vector_attachment_symlink_installed_after_validation() {
        let root = TempDir::new().unwrap();
        let path = fs::canonicalize(root.path()).unwrap();
        let outside = TempDir::new().unwrap();
        let target = outside.path().join("outside.db");
        fs::write(&target, []).unwrap();
        let pinned = File::open(&path).unwrap();
        let directory = ConfinedDirectory::new(&path, &pinned);
        let connection = directory.open_database("cache.db").unwrap();
        let link = path.join("vectors.db");
        let destination = target.clone();
        directory.before_open.replace(Some(Box::new(move || {
            symlink(destination, link).unwrap();
        })));
        assert!(directory.attach_vectors(&connection).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
    }

    #[test]
    fn reject_replaced_service_directory_before_initialization() {
        for use_symlink in [false, true] {
            let root = TempDir::new().unwrap();
            let root_path = fs::canonicalize(root.path()).unwrap();
            let path = root_path.join(".letopis");
            fs::create_dir(&path).unwrap();
            let pinned = File::open(&path).unwrap();
            let directory = ConfinedDirectory::new(&path, &pinned);
            fs::rename(&path, root_path.join("original")).unwrap();
            let outside = TempDir::new().unwrap();
            if use_symlink {
                symlink(outside.path(), &path).unwrap();
            } else {
                fs::create_dir(&path).unwrap();
            }
            assert!(directory.open_database("cache.db").is_err());
            assert!(!path.join("cache.db").exists());
            assert!(!outside.path().join("cache.db").exists());
        }
    }

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

    #[test]
    fn attached_cache_is_read_only_with_encoded_path_delimiters() {
        // Arrange: delimiters, quotes and UTF-8 must name the literal file.
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache ?#%é' name.db");
        let writer = open_database(&cache).unwrap();
        writer.pragma_update(None, "user_version", 7).unwrap();
        let vectors = open_database(&root.path().join("vectors")).unwrap();
        // Act
        attach_cache(&vectors, &cache).unwrap();
        let version: i64 = vectors
            .query_row("PRAGMA cache.user_version", [], |r| r.get(0))
            .unwrap();
        let error = vectors
            .execute_batch("PRAGMA cache.user_version=8;")
            .unwrap_err();
        // Assert: the attached URI uses the intended database and rejects writes.
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
