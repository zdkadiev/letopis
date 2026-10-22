use super::connection::map_sqlite;
use crate::{
    domain::Sha256,
    service::{StorageError, StorageErrorKind, StorageRead, StorageResult},
};
use rusqlite::{Connection, OptionalExtension, Transaction};

pub struct SqliteRead<'a> {
    _transaction: Transaction<'a>,
    revision: u64,
    config_identity: Option<Sha256>,
}
impl<'a> SqliteRead<'a> {
    pub(super) fn begin(connection: &'a mut Connection) -> StorageResult<Self> {
        let transaction = connection.transaction().map_err(map_sqlite)?;
        let revision: i64 = transaction
            .query_row("SELECT revision FROM project WHERE id=1", [], |row| {
                row.get(0)
            })
            .map_err(map_sqlite)?;
        let revision = u64::try_from(revision).map_err(|error| {
            StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "invalid storage revision",
                error,
            )
        })?;
        let identity: Option<String> = transaction
            .query_row(
                "SELECT config_identity FROM vec.meta WHERE id=1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(map_sqlite)?;
        let config_identity = identity
            .map(|raw| Sha256::parse(&raw))
            .transpose()
            .map_err(|error| {
                StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "invalid stored configuration identity",
                    error,
                )
            })?;
        Ok(Self {
            _transaction: transaction,
            revision,
            config_identity,
        })
    }
}
impl StorageRead for SqliteRead<'_> {
    fn get_revision(&self) -> u64 {
        self.revision
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.config_identity
    }
}
