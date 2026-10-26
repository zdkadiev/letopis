use super::{ConfinedDirectory, attach_cache, attach_vectors, open_database};
use rusqlite::ErrorCode;
use std::{
    fs::{self, File},
    os::unix::fs::symlink,
};
use tempfile::TempDir;

#[test]
fn reject_cache_attachment_symlink_installed_after_validation() {
    // Given: pin the service directory and open the vector connection.
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
    // When / Then: the read-only ATTACH inherits NOFOLLOW, including a URI.
    assert!(directory.attach_cache(&connection).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"");
    assert!(!outside.path().join("outside.db-wal").exists());
}

#[test]
fn reject_database_leaf_symlink_installed_after_directory_validation() {
    for name in ["cache.db", "vectors.db"] {
        // Given
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
        // When / Then
        assert!(directory.open_database(name).is_err());
        assert_eq!(fs::read(&target).unwrap(), b"");
    }
}

#[test]
fn reject_vector_attachment_symlink_installed_after_validation() {
    // Given
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
    // When / Then
    assert!(directory.attach_vectors(&connection).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"");
}

#[test]
fn reject_replaced_service_directory_before_initialization() {
    for use_symlink in [false, true] {
        // Given
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
        // When / Then
        assert!(directory.open_database("cache.db").is_err());
        assert!(!path.join("cache.db").exists());
        assert!(!outside.path().join("cache.db").exists());
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
        .query_row("PRAGMA cache.user_version", [], |r| r.get(0))
        .unwrap();
    let error = vectors
        .execute_batch("PRAGMA cache.user_version=8;")
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
