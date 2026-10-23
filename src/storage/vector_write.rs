use super::{connection::map_sqlite, vectors, work::prune_unreferenced};
use crate::{
    domain::Sha256,
    service::{
        StorageError, StorageErrorKind, StorageResult, StoredVectorKey, VectorOutcome, VectorWrite,
    },
};
use rusqlite::{Connection, Transaction, TransactionBehavior};

#[derive(Debug)]
pub(crate) struct SqliteVectorWrite<'a> {
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
    fn prune_unreferenced(&mut self) -> StorageResult<()> {
        let result = prune_unreferenced(&self.transaction);
        self.failed |= result.is_err();
        result
    }
    fn reserve_attempt(&mut self, hash: &Sha256, max_attempts: u64) -> StorageResult<bool> {
        let result = vectors::reserve_attempt(&self.transaction, hash, max_attempts);
        self.failed |= result.is_err();
        result
    }
    fn record_failure(&mut self, hash: &Sha256, safe_error: &str) -> StorageResult<bool> {
        let result = vectors::record_failure(&self.transaction, hash, safe_error);
        self.failed |= result.is_err();
        result
    }
    fn set_query_error(&mut self, at: u64, safe_error: &str) -> StorageResult<()> {
        let result = vectors::set_query_error(&self.transaction, at, safe_error);
        self.failed |= result.is_err();
        result
    }
    fn clear_query_error(&mut self) -> StorageResult<()> {
        let result = vectors::clear_query_error(&self.transaction);
        self.failed |= result.is_err();
        result
    }
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
