use super::{
    connection::{CACHE_DATABASE, ConfinedDirectory, VECTOR_DATABASE},
    read::SqliteRead,
    schema::{attach_empty_vectors, validate_cache, validate_vector_database, validate_vectors},
};
use crate::service::{StatusStorage, StorageResult};
use rusqlite::Connection;
use rustix::fd::AsFd;
use std::path::Path;

/// Confined observation-only storage; absent cache needs no connection.
#[derive(Debug, Default)]
pub(crate) struct SqliteStatusStorage {
    read: Option<Connection>,
}

impl SqliteStatusStorage {
    /// Open existing schemas without bootstrap, repair or vector identity changes.
    /// Missing vectors use only an in-memory copy of their existing schema.
    ///
    /// # Errors
    /// Rejects unsafe paths, incompatible schemas and inaccessible databases.
    pub(crate) fn open_confined(directory: &Path, pinned: &impl AsFd) -> StorageResult<Self> {
        let paths = ConfinedDirectory::new(directory, pinned);
        let Some(read) = paths.open_observed_database(CACHE_DATABASE)? else {
            if let Some(vectors) = paths.open_observed_database(VECTOR_DATABASE)? {
                validate_vector_database(&vectors)?;
            }
            return Ok(Self::default());
        };
        validate_cache(&read)?;
        if paths.attach_observed_vectors(&read)? {
            validate_vectors(&read)?;
        } else {
            attach_empty_vectors(&read)?;
        }
        Ok(Self { read: Some(read) })
    }
}
impl StatusStorage for SqliteStatusStorage {
    type Read<'a> = SqliteRead<'a>;
    fn begin_status_read(&mut self) -> StorageResult<Option<Self::Read<'_>>> {
        self.read
            .as_mut()
            .map(SqliteRead::begin_optional)
            .transpose()
            .map(Option::flatten)
    }
}
