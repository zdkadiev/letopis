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
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        self.events.borrow_mut().push("begin_read".into());
        let read = self.inner.begin_read()?;
        Ok(TracedRead {
            inner: read,
            events: self.events.clone(),
            fail_page: self.fail_page,
            pages: 0,
            after_pin: self.after_pin.take(),
        })
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write()
    }
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write(expected)
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
    fn resolve_node(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        self.events
            .borrow_mut()
            .push(format!("resolve:{}", id.as_str()));
        self.inner.resolve_node(id)
    }
    fn read_neighbor_page(
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
        let page = self.inner.read_neighbor_page(request)?;
        if let Some(hook) = self.after_pin.take() {
            hook();
        }
        Ok(page)
    }
}
impl<R: StorageRead> StorageRead for TracedRead<R> {
    fn read_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        self.inner.read_embedding_status(policy)
    }

    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        self.events
            .borrow_mut()
            .push(format!("metadata:{}", id.as_str()));
        self.inner.read_node(id)
    }
    fn get_revision(&self) -> u64 {
        self.inner.get_revision()
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.inner.get_config_identity()
    }
    fn list_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        panic!("whole nodes")
    }
    fn list_edges(&mut self, _: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        panic!("whole edges")
    }
    fn list_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        self.inner.list_source_snapshots()
    }
    fn read_file(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        self.inner.read_file(path)
    }
    fn list_files(&mut self) -> StorageResult<Vec<FileRow>> {
        self.inner.list_files()
    }
    fn match_text(&mut self, _: &str) -> StorageResult<Vec<TextMatch>> {
        panic!("query text")
    }
    fn read_next_embedding_work(
        &mut self,
        expected: &Sha256,
        max_attempts: u64,
        after: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        self.inner
            .read_next_embedding_work(expected, max_attempts, after)
    }
    fn read_vector_dimensions(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        self.inner.read_vector_dimensions(expected)
    }
    fn read_vector(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        self.inner.read_vector(expected, hash)
    }
    fn find_nearest_vectors(
        &mut self,
        _: &Sha256,
        _: &[f32],
        _: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        panic!("query vectors")
    }
    fn read_failure(
        &mut self,
        expected: &Sha256,
        hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        self.inner.read_failure(expected, hash)
    }
    fn read_query_error(&mut self) -> StorageResult<Option<QueryError>> {
        self.inner.read_query_error()
    }
}
