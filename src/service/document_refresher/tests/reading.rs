use super::{CountingMarkdown, TestFiles, TestProvenance, build_candidates, build_service};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        BasisRetainer, CacheWrite, DocumentRefresher, DocumentResult, ElementAddress,
        EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary, EmbeddingWork, FileRow,
        GraphNeighbor, GraphNodeRef, GraphRead, NeighborPageRequest, NodeMetadata,
        ProvenanceObservation, ProvenanceRequest, QueryError, Refresher, SnapshotError,
        SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead,
        StorageResult, StoredNode, TextMatch, VectorMatch, VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};

struct ReadingStorage {
    inner: SqliteStorage,
    reads: Rc<Cell<usize>>,
}
struct ReadingView<R> {
    inner: R,
    reads: Rc<Cell<usize>>,
}
struct RetainingProvenance;

#[test]
fn warm_refresh_reads_basis_roots_without_loading_nodes() {
    // Given
    let (_root, mut initial) = build_service();
    initial.refresh().unwrap();
    let mut node = initial
        .storage
        .begin_read()
        .unwrap()
        .list_nodes()
        .unwrap()
        .remove(0)
        .metadata;
    let NodeMetadata::Knowledge(ref mut knowledge) = node else {
        panic!()
    };
    knowledge.source_snapshot = Some(TreeOid::parse(&"a".repeat(40)).unwrap());
    let mut write = initial.storage.begin_cache_write().unwrap();
    write.upsert_node(&node).unwrap();
    write.commit().unwrap();
    let reads = Rc::new(Cell::new(0));
    let mut service = DocumentRefresher {
        git: build_candidates(),
        filesystem: TestFiles::new(),
        markdown: CountingMarkdown(Cell::new(0)),
        storage: ReadingStorage {
            inner: initial.storage,
            reads: Rc::clone(&reads),
        },
        provenance: RetainingProvenance,
    };
    // When
    let result = service.refresh().unwrap();
    // Then
    assert!(!result.published);
    assert_eq!(result.revision, 2);
    assert_eq!(reads.get(), 2);
    assert_eq!(service.filesystem.locks.get(), 1);
    assert_eq!(service.filesystem.reads.get(), 0);
    assert_eq!(service.markdown.0.get(), 0);
}

impl Storage for ReadingStorage {
    type Read<'a> = ReadingView<<SqliteStorage as Storage>::Read<'a>>;
    type CacheWrite<'a> = <SqliteStorage as Storage>::CacheWrite<'a>;
    type VectorWrite<'a> = <SqliteStorage as Storage>::VectorWrite<'a>;
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        Ok(ReadingView {
            inner: self.inner.begin_read()?,
            reads: Rc::clone(&self.reads),
        })
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write()
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        identity: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, identity)
    }
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write(expected)
    }
}
impl<R: GraphRead> GraphRead for ReadingView<R> {
    fn resolve_node(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        self.inner.resolve_node(id)
    }
    fn read_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        self.inner.read_neighbor_page(request)
    }
}
impl<R: StorageRead> StorageRead for ReadingView<R> {
    fn read_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        self.inner.read_embedding_status(policy)
    }

    fn list_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        panic!("warm refresh loaded node metadata")
    }
    fn list_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        self.reads.set(self.reads.get() + 1);
        self.inner.list_source_snapshots()
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
    fn read_node(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        self.inner.read_node(id)
    }
    fn list_edges(&mut self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        self.inner.list_edges(source)
    }
    fn read_file(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        self.inner.read_file(path)
    }
    fn list_files(&mut self) -> StorageResult<Vec<FileRow>> {
        self.inner.list_files()
    }
    fn match_text(&mut self, expression: &str) -> StorageResult<Vec<TextMatch>> {
        self.inner.match_text(expression)
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
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        self.inner.find_nearest_vectors(expected, query, dimensions)
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
    fn get_revision(&self) -> u64 {
        self.inner.get_revision()
    }
    fn get_config_identity(&self) -> Option<Sha256> {
        self.inner.get_config_identity()
    }
}
impl SourceProvenance for RetainingProvenance {
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        TestProvenance.validate_revisions(revisions)
    }
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        TestProvenance.observe_sources(request)
    }
    fn compare_revision(
        &self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        TestProvenance.compare_revision(request)
    }
}
impl BasisRetainer for RetainingProvenance {
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert_eq!(
            snapshots,
            &BTreeSet::from([TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()])
        );
        Ok(())
    }
}
