use super::{
    connection::{attach_vectors, open_database, prepare_paths},
    read::SqliteRead,
    schema::{bootstrap_cache, bootstrap_vectors},
};
use crate::service::{Storage, StorageResult};
use rusqlite::Connection;
use std::path::Path;

/// `SQLite` implementation of the application storage contract.
pub struct SqliteStorage {
    read: Connection,
}
impl SqliteStorage {
    /// Open distinct cache/vector files and recognize or initialize their schemas.
    ///
    /// # Errors
    /// Returns an invalid request for invalid/aliased paths and an internal error
    /// for incompatible schemas, native capabilities, contention or database I/O.
    pub fn open(cache: &Path, vectors: &Path) -> StorageResult<Self> {
        let (cache, vectors) = prepare_paths(cache, vectors)?;
        let mut cache_writer = open_database(&cache)?;
        bootstrap_cache(&mut cache_writer)?;
        let mut vector_writer = open_database(&vectors)?;
        bootstrap_vectors(&mut vector_writer)?;
        drop(cache_writer);
        drop(vector_writer);
        let read = open_database(&cache)?;
        attach_vectors(&read, &vectors)?;
        Ok(Self { read })
    }
}
impl Storage for SqliteStorage {
    type Read<'a> = SqliteRead<'a>;
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        SqliteRead::begin(&mut self.read)
    }
}
