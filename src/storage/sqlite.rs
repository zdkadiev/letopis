use super::{
    cache_write::SqliteCacheWrite,
    connection::{
        CACHE_DATABASE, ConfinedDirectory, VECTOR_DATABASE, attach_cache, attach_vectors,
        map_sqlite, open_database, prepare_paths,
    },
    read::SqliteRead,
    schema::{bootstrap_cache, bootstrap_vectors},
    vector_write::SqliteVectorWrite,
    vectors,
};
use crate::service::{CacheWrite, ReadTransaction, VectorWrite};
use crate::{
    domain::Sha256,
    service::{Storage, StorageResult, VectorOutcome},
};
use rusqlite::Connection;
use rustix::fd::AsFd;
use std::{path::Path, time::Duration};

// Drain and query diagnostics share this writer; cache/read connections stay fail-fast.
const VECTOR_WRITE_WAIT: Duration = Duration::from_secs(5);

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
    /// Vector writes wait up to five seconds for a competing `SQLite` writer; cache
    /// writes and read connections retain their zero busy timeout.
    pub(crate) fn open(cache: &Path, vectors: &Path) -> StorageResult<Self> {
        let (cache, vectors) = prepare_paths(cache, vectors)?;
        let mut cache_writer = open_database(&cache)?;
        bootstrap_cache(&mut cache_writer)?;
        let mut vector_writer = open_database(&vectors)?;
        bootstrap_vectors(&mut vector_writer)?;
        vector_writer
            .busy_timeout(VECTOR_WRITE_WAIT)
            .map_err(map_sqlite)?;
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
        vector_writer
            .busy_timeout(VECTOR_WRITE_WAIT)
            .map_err(map_sqlite)?;
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
    fn begin_vector_write_tx(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>> {
        match SqliteVectorWrite::begin_tx(&mut self.vector_writer, expected)? {
            VectorOutcome::Current(writer) => Ok(VectorOutcome::Current(Box::new(writer))),
            VectorOutcome::ConfigChanged => Ok(VectorOutcome::ConfigChanged),
        }
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        desired: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        vectors::reset_vectors(&mut self.vector_writer, observed, desired)
    }
    fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>> {
        Ok(Box::new(SqliteCacheWrite::begin_tx(
            &mut self.cache_writer,
        )?))
    }
    fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>> {
        Ok(Box::new(SqliteRead::begin_tx(&mut self.read)?))
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteStorage;
    use crate::{
        domain::Sha256,
        service::{Storage, VectorOutcome},
    };
    use std::fs::File;
    use tempfile::TempDir;

    #[test]
    fn vector_writer_wait_is_bounded_without_changing_cache_or_read_policy() {
        for confined in [false, true] {
            // Given: exercise both supported open paths against independent databases.
            let root = TempDir::new().unwrap();
            let path = root.path().canonicalize().unwrap();
            let pinned = File::open(&path).unwrap();
            let storage = if confined {
                SqliteStorage::open_confined(&path, &pinned).unwrap()
            } else {
                SqliteStorage::open(&path.join("cache.db"), &path.join("vectors.db")).unwrap()
            };
            // When / Then: only vectors writers have the five-second contention budget.
            for (connection, expected) in [
                (&storage.vector_writer, 5000),
                (&storage.cache_writer, 0),
                (&storage.read, 0),
            ] {
                let timeout: i64 = connection
                    .pragma_query_value(None, "busy_timeout", |r| r.get(0))
                    .unwrap();
                assert_eq!(timeout, expected);
            }
        }
    }

    #[test]
    fn confined_vector_pruning_uses_read_only_cache_and_closes_cleanly() {
        // Given: only the confined production opener is under test.
        let root = TempDir::new().unwrap();
        let path = root.path().canonicalize().unwrap();
        let pinned = File::open(&path).unwrap();
        let mut storage = SqliteStorage::open_confined(&path, &pinned).unwrap();
        let identity = Sha256::digest(b"confined");
        seed_vector(&mut storage, &identity);
        // When: pruning requires the read-only cache attachment used for hash-reference checks.
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        unit.prune_unreferenced().unwrap();
        unit.commit().unwrap();
        // Then: both native deletion and lifetime/permission policy.
        assert_eq!(
            unwrap_current(
                storage
                    .begin_read_tx()
                    .unwrap()
                    .get_vector_by_search_hash(&identity, &identity)
                    .unwrap()
            ),
            None
        );
        assert!(
            storage
                .vector_writer
                .execute_batch("delete from cache.nodes")
                .is_err()
        );
        drop(storage);
        assert!(!path.join("cache.db-wal").exists());
        assert!(!path.join("cache.db-shm").exists());
    }

    #[test]
    fn reset_late_meta_failure_preserves_identity_key_vector_and_query_error() {
        // Given: a populated native table, then fail after DROP/recreation/deletion.
        let root = TempDir::new().unwrap();
        let cache = root.path().join("cache");
        let vectors = root.path().join("vectors");
        let mut storage = SqliteStorage::open(&cache, &vectors).unwrap();
        let m1 = Sha256::digest(b"M1");
        let m2 = Sha256::digest(b"M2");
        let key = seed_vector(&mut storage, &m1);
        storage.vector_writer.execute_batch("insert into embed_failures values('failed-text',1,'safe failure'); insert into query_error values(1,7,'safe failure'); create trigger deny_meta_update before update on meta begin select raise(abort, 'blocked meta write'); end;").unwrap();
        // When: the metadata update fails after native schema changes.
        assert!(storage.reset_vectors(Some(&m1), &m2).is_err());
        assert_eq!(count_failures(&storage), 1);
        // Then: the same and a newly opened read both retain exact M1 bytes.
        assert_eq!(
            storage
                .vector_writer
                .query_row("select row_id from vector_keys", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            key
        );
        let mut read = storage.begin_read_tx().unwrap();
        assert_eq!(read.get_config_identity(), Some(m1));
        assert_eq!(
            unwrap_current(read.get_vector_by_search_hash(&m1, &m1).unwrap()),
            Some(vec![1.0, 0.0, 0.0])
        );
        drop(read);
        storage
            .vector_writer
            .execute_batch("drop trigger deny_meta_update")
            .unwrap();
        drop(storage);
        let mut reopened = SqliteStorage::open(&cache, &vectors).unwrap();
        let mut read = reopened.begin_read_tx().unwrap();
        assert_eq!(read.get_config_identity(), Some(m1));
        assert_eq!(
            unwrap_current(read.get_vector_by_search_hash(&m1, &m1).unwrap()),
            Some(vec![1.0, 0.0, 0.0])
        );
        drop(read);
        assert_eq!(
            reopened.reset_vectors(Some(&m1), &m2).unwrap(),
            VectorOutcome::Current(true)
        );
        assert_eq!(count_failures(&reopened), 0);
        assert_eq!(
            reopened
                .vector_writer
                .query_row("select error from query_error", [], |r| r
                    .get::<_, String>(0))
                .unwrap(),
            "safe failure"
        );
    }
    #[test]
    fn success_deletes_failure_atomically_and_corrupt_key_poisoning_rolls_back() {
        // Catches lost cleanup, uncommitted cleanup, and acceptance of orphan keys.
        // Given: reserved failures and a vector key that can be corrupted.
        let root = TempDir::new().unwrap();
        let mut storage =
            SqliteStorage::open(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        let identity = Sha256::digest(b"M1");
        let hash = Sha256::digest(b"text");
        storage.reset_vectors(None, &identity).unwrap();
        storage
            .vector_writer
            .execute(
                "insert into embed_failures values(?1,1,'safe failure')",
                [hash.to_hex()],
            )
            .unwrap();
        // When: a successful vector write is abandoned before commit.
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        drop(unit);
        // Then: rollback retains the previously recorded failure.
        assert_eq!(
            storage
                .vector_writer
                .query_row("select count(*) from embed_failures", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            1
        );
        // When: repeat the vector write and commit it.
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        unit.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        unit.commit().unwrap();
        // Then: committing the successful vector write removes its failure.
        assert_eq!(
            storage
                .vector_writer
                .query_row("select count(*) from embed_failures", [], |r| r
                    .get::<_, i64>(0))
                .unwrap(),
            0
        );
        // Given: introduce a key without its native vector.
        storage
            .vector_writer
            .execute(
                "insert into vector_keys(search_hash) values(?1)",
                [identity.to_hex()],
            )
            .unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
        // When / Then: a key without its native vector poisons the write and prevents commit.
        assert!(unit.store_vector(&identity, &[1.0, 0.0, 0.0], 3).is_err());
        assert!(unit.commit().is_err());
    }

    fn seed_vector(storage: &mut SqliteStorage, identity: &Sha256) -> i64 {
        storage.reset_vectors(None, identity).unwrap();
        let mut unit = unwrap_current(storage.begin_vector_write_tx(identity).unwrap());
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
            .query_row("select count(*) from embed_failures", [], |r| r.get(0))
            .unwrap()
    }
}

#[cfg(test)]
#[path = "cancellation_tests.rs"]
mod cancellation_tests;
