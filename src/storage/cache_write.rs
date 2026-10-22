use super::{connection::map_sqlite, files, fts, graph};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath},
    service::{CacheWrite, FileRow, NodeMetadata, StorageError, StorageErrorKind, StorageResult},
};
use rusqlite::{Connection, Transaction, TransactionBehavior};

pub struct SqliteCacheWrite<'a> {
    transaction: Transaction<'a>,
    failed: bool,
}
impl<'a> SqliteCacheWrite<'a> {
    pub(super) fn begin(db: &'a mut Connection) -> StorageResult<Self> {
        Ok(Self {
            transaction: db
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(map_sqlite)?,
            failed: false,
        })
    }
    fn record<T>(&mut self, result: StorageResult<T>) -> StorageResult<T> {
        if result.is_err() {
            self.failed = true;
        }
        result
    }
}
impl CacheWrite for SqliteCacheWrite<'_> {
    fn upsert_node(&mut self, node: &NodeMetadata) -> StorageResult<i64> {
        let result = graph::upsert_node(&self.transaction, node);
        self.record(result)
    }
    fn insert_edge(&mut self, edge: &Edge) -> StorageResult<()> {
        let result = graph::insert_edge(&self.transaction, edge);
        self.record(result)
    }
    fn delete_outgoing(&mut self, source: &KnowledgeId) -> StorageResult<()> {
        let result = graph::delete_outgoing(&self.transaction, source);
        self.record(result)
    }
    fn delete_node(&mut self, id: &EdgeTarget) -> StorageResult<()> {
        let result = graph::delete_node(&self.transaction, id);
        self.record(result)
    }
    fn replace_text(&mut self, id: &KnowledgeId, text: &str) -> StorageResult<()> {
        let result = fts::replace_text(&self.transaction, id, text);
        self.record(result)
    }
    fn upsert_file(&mut self, file: &FileRow) -> StorageResult<()> {
        let result = files::upsert_file(&self.transaction, file);
        self.record(result)
    }
    fn delete_file(&mut self, path: &RepoPath) -> StorageResult<()> {
        let result = files::delete_file(&self.transaction, path);
        self.record(result)
    }
    fn commit(self) -> StorageResult<u64> {
        if self.failed {
            return Err(StorageError::new(
                StorageErrorKind::InvalidRequest,
                "cache unit has failed",
            ));
        }
        let revision: i64 = self
            .transaction
            .query_row("SELECT revision FROM project WHERE id=1", [], |r| r.get(0))
            .map_err(map_sqlite)?;
        let next = revision
            .checked_add(1)
            .filter(|_| revision >= 0)
            .ok_or_else(|| {
                StorageError::new(StorageErrorKind::InternalError, "storage revision overflow")
            })?;
        self.transaction
            .execute("UPDATE project SET revision=?1 WHERE id=1", [next])
            .map_err(map_sqlite)?;
        self.transaction.commit().map_err(map_sqlite)?;
        graph::read_unsigned(next)
    }
}

#[cfg(test)]
mod tests {
    use super::SqliteCacheWrite;
    use crate::{
        domain::{EdgeTarget, ElementKind, KnowledgeId, LineRange, Sha256},
        service::{CacheWrite, KnowledgeMetadata, NodeMetadata, StorageErrorKind},
        storage::{connection::open_database, graph, schema::bootstrap_cache},
    };
    use std::error::Error;
    use tempfile::TempDir;

    #[test]
    fn caught_sql_failure_poisoning_restores_metadata_and_index() {
        // Arrange: a real schema and caller-owned transaction.
        let root = TempDir::new().unwrap();
        let mut db = open_database(&root.path().join("cache.db")).unwrap();
        bootstrap_cache(&mut db).unwrap();
        let id = KnowledgeId::parse("a.md").unwrap();
        let node = NodeMetadata::Knowledge(KnowledgeMetadata {
            id: id.clone(),
            uri: id.clone(),
            kind: ElementKind::Vertex,
            hash: Sha256::digest(b"alpha"),
            created_at: 7,
            source_snapshot: None,
            lines: LineRange::new(1, 1).unwrap(),
            search_hash: Sha256::digest(b"alpha"),
        });
        let mut write = SqliteCacheWrite::begin(&mut db).unwrap();
        write.upsert_node(&node).unwrap();
        write
            .transaction
            .execute_batch("DROP TABLE text_index")
            .unwrap();
        // Act: catch an actual SQL failure rather than an input rejection.
        let error = write
            .replace_text(&id, "unique-payload Authorization:Bearer-secret")
            .unwrap_err();
        assert!(write.commit().is_err());
        // Assert: neither metadata nor schema destruction was published.
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert!(error.source().is_some());
        for sentinel in ["unique-payload", "Authorization", "text_index"] {
            assert!(!error.to_string().contains(sentinel));
            assert!(!error.get_message().contains(sentinel));
        }
        assert!(
            graph::read_node(&db, &EdgeTarget::Knowledge(id))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            db.query_row("SELECT count(*) FROM text_index", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            db.query_row("SELECT revision FROM project", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            0
        );
    }
}
