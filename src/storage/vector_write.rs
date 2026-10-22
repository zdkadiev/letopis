use super::{connection::map_sqlite, vectors};
use crate::{
    domain::Sha256,
    service::{
        StorageError, StorageErrorKind, StorageResult, StoredVectorKey, VectorOutcome, VectorWrite,
    },
};
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub struct SqliteVectorWrite<'a> {
    transaction: Transaction<'a>,
    failed: bool,
}
impl<'a> SqliteVectorWrite<'a> {
    pub(super) fn begin(
        connection: &'a mut Connection,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self>> {
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(map_sqlite)?;
        if !vectors::matches_identity(&transaction, expected)? {
            return Ok(VectorOutcome::ConfigChanged);
        }
        Ok(VectorOutcome::Current(Self {
            transaction,
            failed: false,
        }))
    }
}
impl VectorWrite for SqliteVectorWrite<'_> {
    fn store_vector(
        &mut self,
        hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey> {
        let result = vectors::store_vector(&self.transaction, hash, vector, dimensions);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn commit(self) -> StorageResult<()> {
        if self.failed {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "vector unit has failed",
            ));
        }
        self.transaction.commit().map_err(map_sqlite)
    }
}
