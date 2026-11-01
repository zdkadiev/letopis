use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        ElementAddress, EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary,
        EmbeddingWork, FileRow, GraphNeighbor, GraphNodeRef, GraphRead, NeighborPageRequest,
        QueryError, Storage, StorageError, StorageErrorKind, StorageRead, StorageResult,
        StoredNode, TextMatch, VectorMatch, VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{cell::RefCell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};

pub(super) type Events = Rc<RefCell<Vec<String>>>;
pub(super) struct TracedStorage {
    pub(super) inner: SqliteStorage,
    pub(super) events: Events,
    pub(super) after_pin: Option<Box<dyn FnOnce()>>,
    pub(super) fail_page: Option<usize>,
}
pub(super) struct TracedRead<R> {
    inner: R,
    events: Events,
    fail_page: Option<usize>,
    pages: usize,
    after_pin: Option<Box<dyn FnOnce()>>,
}
impl Storage for TracedStorage {
    type Read<'a> = TracedRead<<SqliteStorage as Storage>::Read<'a>>;
    type CacheWrite<'a> = <SqliteStorage as Storage>::CacheWrite<'a>;
    type VectorWrite<'a> = <SqliteStorage as Storage>::VectorWrite<'a>;
    fn begin_read_tx(&mut self) -> StorageResult<Self::Read<'_>> {
        self.events.borrow_mut().push("begin_read_tx".into());
        let read = self.inner.begin_read_tx()?;
        Ok(TracedRead {
            inner: read,
            events: self.events.clone(),
            fail_page: self.fail_page,
            pages: 0,
            after_pin: self.after_pin.take(),
        })
    }
    fn begin_cache_write_tx(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write_tx()
    }
    fn begin_vector_write_tx(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write_tx(expected)
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        identity: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, identity)
    }
}
impl<R: GraphRead> GraphRead for TracedRead<R> {
    fn get_node_ref_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        self.events
            .borrow_mut()
            .push(format!("resolve:{}", id.as_str()));
        self.inner.get_node_ref_by_id(id)
    }
    fn get_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        assert!(request.limit.get() <= 32);
        self.pages += 1;
        self.events
            .borrow_mut()
            .push(format!("page:{}", self.pages));
        if self.fail_page == Some(self.pages) {
            return Err(StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "page unavailable",
                std::io::Error::other("PRIVATE_PAGE"),
            ));
        }
        let page = self.inner.get_neighbor_page(request)?;
        if let Some(hook) = self.after_pin.take() {
            hook();
        }
        Ok(page)
    }
}
impl<R: StorageRead> StorageRead for TracedRead<R> {
    fn get_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        self.inner.get_embedding_status(policy)
    }

    fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        self.events
            .borrow_mut()
            .push(format!("metadata:{}", id.as_str()));
        self.inner.get_node_by_id(id)
    }
    fn get_revision(&self) -> u64 {
        self.inner.get_revision()
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.inner.get_config_identity()
    }
    fn get_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        panic!("whole nodes")
    }
    fn get_edges_by_source_id(&mut self, _: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        panic!("whole edges")
    }
    fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        self.inner.get_source_snapshots()
    }
    fn get_file_by_path(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        self.inner.get_file_by_path(path)
    }
    fn get_files(&mut self) -> StorageResult<Vec<FileRow>> {
        self.inner.get_files()
    }
    fn get_text_matches_by_expression(&mut self, _: &str) -> StorageResult<Vec<TextMatch>> {
        panic!("query text")
    }
    fn get_next_embedding_work_by_id_and_max_attempts(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        after: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        self.inner
            .get_next_embedding_work_by_id_and_max_attempts(expected, max_attempts, after)
    }
    fn get_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        self.inner.get_vector_dimensions(expected)
    }
    fn get_vector_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        self.inner.get_vector_by_search_hash(expected, search_hash)
    }
    fn get_nearest_vectors(
        &mut self,
        _: &Sha256,
        _: &[f32],
        _: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        panic!("query vectors")
    }
    fn get_failure_by_search_hash(
        &mut self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        self.inner.get_failure_by_search_hash(expected, search_hash)
    }
    fn get_query_error(&mut self) -> StorageResult<Option<QueryError>> {
        self.inner.get_query_error()
    }
}
