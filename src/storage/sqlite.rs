use super::{
    cache_write::SqliteCacheWrite,
    connection::{
        CACHE_DATABASE, ConfinedDirectory, VECTOR_DATABASE, attach_cache, attach_vectors,
        open_database, prepare_paths,
    },
    read::SqliteRead,
    schema::{bootstrap_cache, bootstrap_vectors},
    vector_write::SqliteVectorWrite,
    vectors,
};
use crate::{
    domain::Sha256,
    service::{Storage, StorageResult, VectorOutcome},
};
use rusqlite::Connection;
use rustix::fd::AsFd;
use std::path::Path;

/// `SQLite` implementation of the application storage contract.
#[derive(Debug)]
pub(crate) struct SqliteStorage {
    // Drop the read-only cache attachment before its last writable connection,
    // so normal SQLite closure can checkpoint and remove cache WAL sidecars.
    vector_writer: Connection,
    read: Connection,
    cache_writer: Connection,
}
impl SqliteStorage {
    /// Open distinct cache/vector files and recognize or initialize their schemas.
    ///
    /// # Errors
    /// Returns an invalid request for invalid/aliased paths and an internal error
    /// for incompatible schemas, native capabilities, contention or database I/O.
    pub(crate) fn open(cache: &Path, vectors: &Path) -> StorageResult<Self> {
        let (cache, vectors) = prepare_paths(cache, vectors)?;
        let mut cache_writer = open_database(&cache)?;
        bootstrap_cache(&mut cache_writer)?;
        let mut vector_writer = open_database(&vectors)?;
        bootstrap_vectors(&mut vector_writer)?;
        attach_cache(&vector_writer, &cache)?;

        let read = open_database(&cache)?;
        attach_vectors(&read, &vectors)?;
        Ok(Self {
            read,
            cache_writer,
            vector_writer,
        })
    }
    pub(crate) fn open_confined(directory: &Path, pinned: &impl AsFd) -> StorageResult<Self> {
        let paths = ConfinedDirectory::new(directory, pinned);
        let mut cache_writer = paths.open_database(CACHE_DATABASE)?;
        paths.verify()?;
        bootstrap_cache(&mut cache_writer)?;
        let mut vector_writer = paths.open_database(VECTOR_DATABASE)?;
        paths.verify()?;
        bootstrap_vectors(&mut vector_writer)?;
        paths.attach_cache(&vector_writer)?;
        let read = paths.open_database(CACHE_DATABASE)?;
        paths.attach_vectors(&read)?;
        Ok(Self {
            read,
            cache_writer,
            vector_writer,
        })
    }
}
impl Storage for SqliteStorage {
    type Read<'a> = SqliteRead<'a>;
    type VectorWrite<'a> = SqliteVectorWrite<'a>;
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        SqliteVectorWrite::begin(&mut self.vector_writer, expected)
    }
    fn reset_vectors(&mut self, identity: &Sha256) -> StorageResult<bool> {
        vectors::reset_vectors(&mut self.vector_writer, identity)
    }
    type CacheWrite<'a> = SqliteCacheWrite<'a>;
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        SqliteCacheWrite::begin(&mut self.cache_writer)
    }
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        SqliteRead::begin(&mut self.read)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::{
        domain::Sha256,
        service::{Storage, StorageRead, VectorOutcome, VectorWrite},
    };
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn confined_vector_pruning_uses_read_only_cache_and_closes_cleanly() {
        // Arrange: only the confined production opener is under test.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        let mut storage = SqliteStorage::open_confined(&path, &pinned).unwrap();
        let identity = Sha256::digest(b"confined");
        seed_vector(&mut storage, &identity);
        // Act: pruning requires the read-only cache attachment used for hash-reference checks.
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        unit.prune_unreferenced().unwrap();
        unit.commit().unwrap();
        // Assert both native deletion and lifetime/permission policy.
        assert_eq!(
            unwrap_current(
                storage
                    .begin_read()
                    .unwrap()
                    .read_vector(&identity, &identity)
                    .unwrap()
            ),
            None
        );
        assert!(
            storage
                .vector_writer
                .execute_batch("DELETE FROM cache.nodes")
                .is_err()
        );
        drop(storage);
        assert!(!path.join("cache.db-wal").exists());
        assert!(!path.join("cache.db-shm").exists());
    }

    #[test]
    fn reset_late_meta_failure_preserves_identity_key_vector_and_query_error() {
        // Arrange a populated native table, then fail after DROP/recreation/deletion.
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        let vectors = root.path().join("vectors");
        let mut storage = SqliteStorage::open(&cache, &vectors).unwrap();
        let m1 = Sha256::digest(b"M1");
        let m2 = Sha256::digest(b"M2");
        let key = seed_vector(&mut storage, &m1);
        storage.vector_writer.execute_batch("INSERT INTO embed_failures VALUES('failed-text',1,'safe failure'); INSERT INTO query_error VALUES(1,7,'safe failure'); CREATE TRIGGER deny_meta_update BEFORE UPDATE ON meta BEGIN SELECT RAISE(ABORT, 'blocked meta write'); END;").unwrap();
        // Act: the metadata update fails after native schema changes.
        assert!(storage.reset_vectors(&m2).is_err());
        assert_eq!(count_failures(&storage), 1);
        // Assert the same and a newly opened read both retain exact M1 bytes.
        assert_eq!(
            storage
                .vector_writer
                .query_row("SELECT row_id FROM vector_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            key
        );
        let mut read = storage.begin_read().unwrap();
        assert_eq!(read.get_config_identity(), Some(m1));
        assert_eq!(
            unwrap_current(read.read_vector(&m1, &m1).unwrap()),
            Some(vec![1.0, 0.0, 0.0])
        );
        drop(read);
        storage
            .vector_writer
            .execute_batch("DROP TRIGGER deny_meta_update")
            .unwrap();
        drop(storage);
        let mut reopened = SqliteStorage::open(&cache, &vectors).unwrap();
        let mut read = reopened.begin_read().unwrap();
        assert_eq!(read.get_config_identity(), Some(m1));
        assert_eq!(
            unwrap_current(read.read_vector(&m1, &m1).unwrap()),
            Some(vec![1.0, 0.0, 0.0])
        );
        drop(read);
        assert!(reopened.reset_vectors(&m2).unwrap());
        assert_eq!(count_failures(&reopened), 0);
        assert_eq!(
            reopened
                .vector_writer
                .query_row("SELECT error FROM query_error", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "safe failure"
        );
    }
    #[test]
    fn success_deletes_failure_atomically_and_corrupt_key_poisoning_rolls_back() {
        // Catches lost cleanup, uncommitted cleanup, and acceptance of orphan keys.
        let root = TempDir::new().unwrap();
        let mut storage =
            SqliteStorage::open(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let identity = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"text");
        storage.reset_vectors(&identity).unwrap();
        storage
            .vector_writer
            .execute(
                "INSERT INTO embed_failures VALUES(?1,1,'safe failure')",
                [hash.to_hex()],
            )
            .unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        drop(unit);
        assert_eq!(
            storage
                .vector_writer
                .query_row("SELECT count(*) FROM embed_failures", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        unit.commit().unwrap();
        assert_eq!(
            storage
                .vector_writer
                .query_row("SELECT count(*) FROM embed_failures", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        storage
            .vector_writer
            .execute(
                "INSERT INTO vector_keys(search_hash) VALUES(?1)",
                [identity.to_hex()],
            )
            .unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write(&identity).unwrap());
        assert!(unit.store_vector(&identity, &[1.0, 0.0, 0.0], 3).is_err());
        assert!(unit.commit().is_err());
    }

    fn seed_vector(storage: &mut SqliteStorage, identity: &Sha256) -> i64 {
        storage.reset_vectors(identity).unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write(identity).unwrap());
        let row = unit
            .store_vector(identity, &[1.0, 0.0, 0.0], 3)
            .unwrap()
            .row_id;
        unit.commit().unwrap();
        row
    }

    fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
        match outcome {
            VectorOutcome::Current(value) => value,
            VectorOutcome::ConfigChanged => panic!("unexpected identity change"),
        }
    }

    fn count_failures(storage: &SqliteStorage) -> i64 {
        storage
            .vector_writer
            .query_row("SELECT count(*) FROM embed_failures", [], |r| r.get(0))
            .unwrap()
    }
}
