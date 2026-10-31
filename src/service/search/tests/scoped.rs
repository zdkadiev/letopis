mod revisions;

use super::{build_config, build_noop_refresher, seed_dimension};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    filesystem::LocalFileSystem,
    markdown::MarkdownParser,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind,
        DocumentResult, DrainOutcome, Embedder, EmbeddingConfig, EmbeddingErrorKind,
        EmbeddingResult, KnowledgeMetadata, MockRefresher, NodeMetadata, ProvenanceObservation,
        ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RecheckFilter, SearchFilters,
        SearchFlag, SearchRequest, Searcher, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead, StorageResult,
        VectorOutcome, search::SearchService,
    },
    storage::SqliteStorage,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    num::NonZeroUsize,
    path::Path,
    rc::Rc,
};

struct Query {
    config: EmbeddingConfig,
    state: Option<ConfigState>,
    calls: Rc<Cell<usize>>,
    hook: Option<Box<dyn FnOnce()>>,
}
type RecordedSources = Vec<(Option<TreeOid>, BTreeSet<SourceUri>)>;
#[derive(Clone, Copy)]
enum ProvenanceFailure {
    Observation,
    Comparison,
}
struct Observations {
    events: RefCell<Vec<String>>,
    observed: Rc<RefCell<RecordedSources>>,
    failure: Option<ProvenanceFailure>,
}
struct ReadOnlyStorage {
    inner: SqliteStorage,
    reads: Rc<Cell<usize>>,
}

type ScopedSearch = SearchService<
    MockRefresher,
    ReadOnlyStorage,
    Query,
    LocalFileSystem,
    MarkdownParser,
    Observations,
>;
struct ScopedFixture {
    root: tempfile::TempDir,
    config: EmbeddingConfig,
    calls: Rc<Cell<usize>>,
    reads: Rc<Cell<usize>>,
    observed: Rc<RefCell<RecordedSources>>,
}

#[test]
fn non_ready_configuration_preserves_fts_and_one_pinned_consumer_view() {
    for state in [
        ConfigState::Missing,
        ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
        ConfigState::Changed,
    ] {
        // Given: vectors exist, but readiness must still disable query inference.
        let fixture = ScopedFixture::new();
        let mut search = fixture.build_search(None, None);
        search.embeddings.state = Some(state);
        // When
        let links = search.search(&build_request()).unwrap();
        // Then
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].element_id.as_str(), "a.md");
        assert_eq!(fixture.calls.get(), 0);
        assert_eq!(fixture.reads.get(), 1);
        assert_can_checkpoint(fixture.root.path());
    }
}

#[test]
fn metadata_failure_after_query_releases_pinned_view() {
    // Given
    let fixture = ScopedFixture::new();
    let root = fixture.root.path();
    let connection = rusqlite::Connection::open(root.join("cache.db")).unwrap();
    connection
        .execute("update nodes set created_at = -1 where id = ?1", ["a.md"])
        .unwrap();
    let mut search = fixture.build_search(None, None);

    // When
    let error = search.search(&build_request()).unwrap_err();

    // Then
    assert_eq!(fixture.calls.get(), 1);
    assert_eq!(fixture.reads.get(), 1);
    assert!(fixture.observed.borrow().is_empty());
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.get_message(), "cannot search knowledge");
    assert!(std::error::Error::source(&error).is_some());
    assert!(!format!("{error:?} {error}").contains("created_at"));
    // Keep the service alive: returning the error itself must release its reader.
    assert_can_checkpoint(root);
    connection
        .execute("update nodes set created_at = 1 where id = ?1", ["a.md"])
        .unwrap();
    publish_candidate(&mut open_storage(root), "b", "new.rs");
    assert_eq!(open_storage(root).begin_read().unwrap().get_revision(), 2);
    assert_can_checkpoint(root);
    drop(search);
}

#[test]
fn search_observes_metadata_and_bindings_from_original_pinned_view() {
    // Given
    let fixture = ScopedFixture::new();
    let root = fixture.root.path();
    let mut writer = open_storage(root);
    let mut search = fixture.build_search(
        Some(Box::new(move || {
            publish_candidate(&mut writer, "b", "new.rs");
        })),
        None,
    );

    // When
    let links = search.search(&build_filtered_request()).unwrap();

    // Then
    assert_eq!(fixture.calls.get(), 1);
    assert_eq!(fixture.reads.get(), 1);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].revision, 1);
    assert_eq!(
        links[0].flags,
        BTreeSet::from([SearchFlag::Stale, SearchFlag::SourceChanged])
    );
    assert_eq!(
        *fixture.observed.borrow(),
        vec![(
            Some(TreeOid::parse(&"a".repeat(40)).unwrap()),
            BTreeSet::from([SourceUri::parse("old.rs").unwrap()])
        )]
    );
    assert_eq!(open_storage(root).begin_read().unwrap().get_revision(), 2);
    assert_can_checkpoint(root);
    drop(search);
}

#[test]
fn late_provenance_failure_releases_pinned_view_while_service_stays_alive() {
    // Given
    for failure in [
        ProvenanceFailure::Observation,
        ProvenanceFailure::Comparison,
    ] {
        let fixture = ScopedFixture::new();
        let root = fixture.root.path();
        let mut search = fixture.build_search(None, Some(failure));

        // When
        let error = search.search(&build_filtered_request()).unwrap_err();

        // Then
        assert_eq!(fixture.calls.get(), 1);
        assert_eq!(fixture.reads.get(), 1);
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(std::error::Error::source(&error).is_some());
        assert!(!format!("{error:?} {error}").contains("PRIVATE"));
        assert_can_checkpoint(root);
        publish_candidate(&mut open_storage(root), "b", "new.rs");
        assert_eq!(open_storage(root).begin_read().unwrap().get_revision(), 2);
        assert_can_checkpoint(root);
        // Ownership must release the read before the still-live service is dropped.
        drop(search);
    }
}

impl ScopedFixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let mut storage = open_storage(root.path());
        publish_candidate(&mut storage, "a", "old.rs");
        let config = build_config(2);
        seed_dimension(&mut storage, &config, 2);
        Self {
            root,
            config,
            calls: Rc::new(Cell::new(0)),
            reads: Rc::new(Cell::new(0)),
            observed: Rc::new(RefCell::new(Vec::new())),
        }
    }
    fn build_search(
        &self,
        hook: Option<Box<dyn FnOnce()>>,
        failure: Option<ProvenanceFailure>,
    ) -> ScopedSearch {
        SearchService {
            refresher: build_noop_refresher(),
            storage: ReadOnlyStorage {
                inner: open_storage(self.root.path()),
                reads: self.reads.clone(),
            },
            embeddings: Query {
                config: self.config.clone(),
                state: None,
                calls: self.calls.clone(),
                hook,
            },
            filesystem: LocalFileSystem::open(self.root.path()).unwrap(),
            markdown: MarkdownParser::new(),
            provenance: Observations {
                events: RefCell::new(Vec::new()),
                observed: self.observed.clone(),
                failure,
            },
        }
    }
}

impl Embedder for Query {
    fn get_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        Ok(self
            .state
            .take()
            .unwrap_or_else(|| ConfigState::Ready(self.config.clone())))
    }
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
        panic!("search cannot drain")
    }
    fn embed_query(&mut self, _: &str) -> EmbeddingResult<QueryEmbedding> {
        panic!("use captured configuration")
    }
    fn embed_query_with_config(
        &mut self,
        request: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding> {
        assert_eq!(request.text, "query");
        assert_eq!(request.config.get_identity(), self.config.get_identity());
        self.calls.set(self.calls.get() + 1);
        if let Some(hook) = self.hook.take() {
            hook();
        }
        Ok(QueryEmbedding::Available {
            identity: self.config.get_identity(),
            vector: vec![1.0, 0.0],
        })
    }
}
impl SourceProvenance for Observations {
    fn validate_revisions(&self, revisions: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        self.events.borrow_mut().push("validate".into());
        assert!(revisions.is_empty() || *revisions == BTreeSet::from([build_revision()]));
        Ok(())
    }
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        self.events.borrow_mut().push("observe".into());
        if matches!(self.failure, Some(ProvenanceFailure::Observation)) {
            return Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot observe sources",
                std::io::Error::other("PRIVATE provenance payload"),
            ));
        }
        self.observed
            .borrow_mut()
            .push((request.source_snapshot.cloned(), request.sources.clone()));
        Ok(ProvenanceObservation {
            flags: BTreeSet::from([SearchFlag::SourceChanged]),
            issues: BTreeSet::new(),
        })
    }
    fn compare_revision(
        &self,
        request: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        self.events.borrow_mut().push("compare".into());
        assert_eq!(
            request.source_snapshot,
            Some(&TreeOid::parse(&"a".repeat(40)).unwrap())
        );
        assert_eq!(request.source.as_str(), "old.rs");
        assert_eq!(request.revision, &build_revision());
        if matches!(self.failure, Some(ProvenanceFailure::Comparison)) {
            return Err(SnapshotError::new_with_cause(
                "cannot compare sources",
                std::io::Error::other("PRIVATE comparison payload"),
            ));
        }
        Ok(SourceRevisionComparison::Equal)
    }
}
impl Storage for ReadOnlyStorage {
    type Read<'a>
        = <SqliteStorage as Storage>::Read<'a>
    where
        Self: 'a;
    type CacheWrite<'a>
        = <SqliteStorage as Storage>::CacheWrite<'a>
    where
        Self: 'a;
    type VectorWrite<'a>
        = <SqliteStorage as Storage>::VectorWrite<'a>
    where
        Self: 'a;
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        self.reads.set(self.reads.get() + 1);
        assert_eq!(self.reads.get(), 1, "search opened a second view");
        self.inner.begin_read()
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        panic!("search cannot publish")
    }
    fn begin_vector_write(
        &mut self,
        _: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        panic!("search cannot write vectors")
    }
    fn reset_vectors(
        &mut self,
        _: Option<&Sha256>,
        _: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        panic!("search cannot reset vectors")
    }
}
fn open_storage(root: &Path) -> SqliteStorage {
    SqliteStorage::open(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
}
fn publish_candidate(storage: &mut SqliteStorage, oid: &str, source: &str) {
    let id = KnowledgeId::parse("a.md").unwrap();
    let mut unit = storage.begin_cache_write().unwrap();
    unit.upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
        id: id.clone(),
        uri: id.clone(),
        kind: ElementKind::Vertex,
        hash: Sha256::digest(b"body"),
        created_at: 1,
        source_snapshot: Some(TreeOid::parse(&oid.repeat(40)).unwrap()),
        lines: LineRange::new(1, 2).unwrap(),
        search_hash: Sha256::digest(b"query"),
    }))
    .unwrap();
    unit.replace_text(&id, "query").unwrap();
    unit.delete_outgoing(&id).unwrap();
    unit.insert_edge(
        &Edge::new(
            id,
            EdgeTarget::Source(SourceUri::parse(source).unwrap()),
            EdgeType::parse("external").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    unit.commit().unwrap();
}
fn build_filtered_request() -> SearchRequest {
    SearchRequest {
        filters: SearchFilters {
            needs_recheck: RecheckFilter::Required,
            sources: BTreeSet::from([SourceUri::parse("old.rs").unwrap()]),
            source_revisions: BTreeSet::from([build_revision()]),
        },
        ..build_request()
    }
}
fn build_request() -> SearchRequest {
    SearchRequest {
        filters: Default::default(),
        query: "query".into(),
        limit: NonZeroUsize::new(10).unwrap(),
    }
}
fn build_revision() -> TreeOid {
    TreeOid::parse(&"c".repeat(40)).unwrap()
}
fn assert_can_checkpoint(root: &Path) {
    for name in ["cache.db", "vectors.db"] {
        let connection = rusqlite::Connection::open(root.join(name)).unwrap();
        let busy: i64 = connection
            .query_row("pragma wal_checkpoint(truncate)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 0, "leaked read prevented checkpoint of {name}");
    }
}
