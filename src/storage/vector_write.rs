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
    /// Begin an atomic vector write when the stored identity matches `expected`.
    /// A different identity returns `ConfigChanged` without changing data.
    /// Dropping an uncommitted write rolls back its changes.
    ///
    /// # Errors
    /// Reports writer contention or database failures.
    pub(super) fn begin_tx(
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
    fn reserve_attempt(&mut self, search_hash: &Sha256, max_attempts: u64) -> StorageResult<bool> {
        let result = vectors::reserve_attempt(&self.transaction, search_hash, max_attempts);
        self.failed |= result.is_err();
        result
    }
    fn record_failure(&mut self, search_hash: &Sha256, error: &str) -> StorageResult<bool> {
        let result = vectors::record_failure(&self.transaction, search_hash, error);
        self.failed |= result.is_err();
        result
    }
    fn set_query_error(&mut self, at: u64, error: &str) -> StorageResult<()> {
        let result = vectors::set_query_error(&self.transaction, at, error);
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
        search_hash: &Sha256,
        vector: &[f32],
        dimensions: usize,
    ) -> StorageResult<StoredVectorKey> {
        let result = vectors::store_vector(&self.transaction, search_hash, vector, dimensions);
        if result.is_err() {
            self.failed = true;
        }
        result
    }
    fn commit(self: Box<Self>) -> StorageResult<()> {
        if self.failed {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "vector unit has failed",
            ));
        }
        self.transaction.commit().map_err(map_sqlite)
    }
}
