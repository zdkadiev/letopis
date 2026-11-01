use super::{CountingMarkdown, TestFiles, TestProvenance, build_candidates, build_service};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        BasisRetainer, CacheWrite, DocumentRefreshService, DocumentResult, ElementAddress,
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
    // Given: a warm projection with declared basis roots.
    let (_root, mut initial) = build_service();
    initial.refresh().unwrap();
    let mut node = initial
        .storage
        .begin_read_tx()
        .unwrap()
        .get_nodes()
        .unwrap()
        .remove(0)
        .metadata;
    let NodeMetadata::Knowledge(ref mut knowledge) = node else {
        panic!()
    };
    knowledge.source_snapshot = Some(TreeOid::parse(&"a".repeat(40)).unwrap());
    let mut write = initial.storage.begin_cache_write_tx().unwrap();
    write.upsert_node(&node).unwrap();
    write.commit().unwrap();
    let reads = Rc::new(Cell::new(0));
    let mut service = DocumentRefreshService {
        git: build_candidates(),
        filesystem: TestFiles::new(),
        markdown: CountingMarkdown(Cell::new(0)),
        storage: ReadingStorage {
            inner: initial.storage,
            reads: Rc::clone(&reads),
        },
        provenance: RetainingProvenance,
    };
    // When: refresh checks the unchanged corpus.
    let result = service.refresh().unwrap();
    // Then: basis roots are observed without loading node metadata.
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
    fn begin_read_tx(&mut self) -> StorageResult<Self::Read<'_>> {
        Ok(ReadingView {
            inner: self.inner.begin_read_tx()?,
            reads: Rc::clone(&self.reads),
        })
    }
    fn begin_cache_write_tx(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write_tx()
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        identity: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, identity)
    }
    fn begin_vector_write_tx(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write_tx(expected)
    }
}
impl<R: GraphRead> GraphRead for ReadingView<R> {
    fn get_node_ref_by_id(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        self.inner.get_node_ref_by_id(id)
    }
    fn get_neighbor_page(
        &mut self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        self.inner.get_neighbor_page(request)
    }
}
impl<R: StorageRead> StorageRead for ReadingView<R> {
    fn get_embedding_status(
        &mut self,
        policy: &EmbeddingStatusPolicy,
    ) -> StorageResult<EmbeddingStatusSummary> {
        self.inner.get_embedding_status(policy)
    }

    fn get_nodes(&mut self) -> StorageResult<Vec<StoredNode>> {
        panic!("warm refresh loaded node metadata")
    }
    fn get_source_snapshots(&mut self) -> StorageResult<BTreeSet<TreeOid>> {
        self.reads.set(self.reads.get() + 1);
        self.inner.get_source_snapshots()
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
    fn get_node_by_id(&mut self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        self.inner.get_node_by_id(id)
    }
    fn get_edges_by_source_id(&mut self, source: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        self.inner.get_edges_by_source_id(source)
    }
    fn get_file_by_path(&mut self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        self.inner.get_file_by_path(path)
    }
    fn get_files(&mut self) -> StorageResult<Vec<FileRow>> {
        self.inner.get_files()
    }
    fn get_text_matches_by_expression(
        &mut self,
        expression: &str,
    ) -> StorageResult<Vec<TextMatch>> {
        self.inner.get_text_matches_by_expression(expression)
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
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        self.inner.get_nearest_vectors(expected, query, dimensions)
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
    fn verify_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert_eq!(
            snapshots,
            &BTreeSet::from([TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()])
        );
        Ok(())
    }
    fn retain_snapshots(&self, snapshots: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        assert_eq!(
            snapshots,
            &BTreeSet::from([TreeOid::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap()])
        );
        Ok(())
    }
}
