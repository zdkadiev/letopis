use letopis::service::StorageError;
use letopis::service::{CacheDatabase, VectorDatabase};
use support::{Databases, open_storage};
#[path = "support/storage.rs"]
mod support;
use letopis::{domain::Sha256, open_status_storage, service::StorageRead};
use rusqlite::Connection;
use std::{
    fs::{self, File},
    path::Path,
};
use tempfile::TempDir;

#[test]
fn observes_distinct_hashes_coverage_and_filtered_failures() {
    // Given: shared current text, one vector and one exhausted uncovered reservation.
    let root = TempDir::new().unwrap();
    let writer = open_writer(root.path());
    let identity = Sha256::digest(b"model");
    writer.vectors.reset_vectors(None, &identity).unwrap();
    publish(&writer, &[("a.md", "A"), ("shared.md", "A"), ("b.md", "B")]);
    writer
        .vectors
        .with_write_tx(&identity, || {
            writer
                .vectors
                .store_vector(&Sha256::digest(b"A"), &[1.0, 0.0, 0.0], 3)?;
            writer
                .vectors
                .embed_failures_upsert_by_search_hash_and_attempts(&Sha256::digest(b"B"), 1)?;
            Ok::<_, StorageError>(())
        })
        .unwrap();
    let pinned = File::open(root.path()).unwrap();
    let status = open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
    // When: independent observations run in one pinned status scope.
    let observed = status
        .with_status_read_tx(|| {
            Ok::<_, StorageError>((
                status.nodes_get_by_kind()?,
                status.nodes_get_coverage_by_kind()?,
                status.embed_failures_get_by_kind_and_search_hash_and_attempts(1)?,
                status.embed_failures_get_by_kind_and_search_hash_and_attempts(2)?,
            ))
        })
        .unwrap()
        .unwrap();
    // Then: counts remain SQL-distinct and only threshold-matching rows are returned.
    assert_eq!((observed.0, observed.1), (2, 1));
    assert_eq!(observed.2.len(), 1);
    assert_eq!(observed.2[0].search_hash, Sha256::digest(b"B"));
    assert_eq!(observed.2[0].attempts, 1);
    assert!(observed.3.is_empty());
}

#[test]
fn status_scope_skips_missing_publication_without_creating_databases() {
    // Given: a retained directory without either database.
    let root = TempDir::new().unwrap();
    let path = root.path().canonicalize().unwrap();
    let pinned = File::open(&path).unwrap();
    let storage = open_status_storage(&path, &pinned).unwrap();
    // When: status asks for a publication that does not exist.
    let result = storage
        .with_status_read_tx(|| -> letopis::service::StorageResult<()> {
            panic!("absent publication callback executed")
        })
        .unwrap();
    // Then: the callback was skipped and neither durable file exists.
    assert_eq!(result, None);
    assert!(!path.join("cache.db").exists());
    assert!(!path.join("vectors.db").exists());
}

#[test]
fn rejects_existing_invalid_vector_schema_without_cache() {
    // Given
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("vectors.db"), b"not sqlite").unwrap();
    let pinned = File::open(root.path()).unwrap();
    // When / Then
    assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    assert!(!root.path().join("cache.db").exists());
}

#[test]
fn rejects_unsafe_paths_and_existing_invalid_schemas() {
    // Given
    use std::os::unix::fs::symlink;
    // When / Then
    for leaf in ["cache.db", "vectors.db"] {
        let root = TempDir::new().unwrap();
        let pinned = File::open(root.path()).unwrap();
        symlink("outside", root.path().join(leaf)).unwrap();
        // When / Then
        assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    }
    for leaf in ["cache.db", "vectors.db"] {
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        publish(&writer, &[("a.md", "A")]);
        drop(writer);
        let connection = Connection::open(root.path().join(leaf)).unwrap();
        connection.execute_batch("pragma user_version=99").unwrap();
        let pinned = File::open(root.path()).unwrap();
        // When / Then
        assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    }
    let root = TempDir::new().unwrap();
    let pinned = File::open(root.path()).unwrap();
    let different = TempDir::new().unwrap();
    assert!(open_status_storage(&different.path().canonicalize().unwrap(), &pinned).is_err());
}

fn open_writer(root: &Path) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
    open_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
}
fn publish<C: CacheDatabase, V: VectorDatabase>(writer: &Databases<C, V>, rows: &[(&str, &str)]) {
    writer
        .cache
        .with_write_tx(|| {
            let unit = &writer.cache;
            for (id, text) in rows {
                unit.nodes_upsert_by_id(&support::build_knowledge(id, text))
                    .unwrap();
            }

            unit.project_get_revision()
        })
        .unwrap();
}
