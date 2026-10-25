use super::{build_config, seed_dimension};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    filesystem::LocalFileSystem,
    markdown::MarkdownParser,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DocumentResult, DrainOutcome, Embedder,
        EmbeddingConfig, EmbeddingResult, KnowledgeMetadata, NodeMetadata, ProvenanceObservation,
        ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, Refresher,
        SearchFlag, SearchRequest, Searcher, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead, StorageResult,
        VectorOutcome,
        search::{SearchDependencies, SearchService},
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

struct NoRefresh;
struct Query {
    config: EmbeddingConfig,
    calls: Rc<Cell<usize>>,
    hook: Option<Box<dyn FnOnce()>>,
}
type RecordedSources = Vec<(Option<TreeOid>, BTreeSet<SourceUri>)>;
struct Observations(Rc<RefCell<RecordedSources>>);
struct ReadOnlyStorage {
    inner: SqliteStorage,
    reads: Rc<Cell<usize>>,
}

#[test]
fn metadata_failure_after_query_releases_pinned_view() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open_storage(root.path());
    publish_candidate(&mut storage, "a", "old.rs");
    let config = build_config(2);
    seed_dimension(&mut storage, &config, 2);
    let connection = rusqlite::Connection::open(root.path().join("cache.db")).unwrap();
    connection
        .execute("UPDATE nodes SET created_at = -1 WHERE id = ?1", ["a.md"])
        .unwrap();
    let calls = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let mut search = SearchService::new(SearchDependencies {
        refresher: NoRefresh,
        storage: ReadOnlyStorage {
            inner: storage,
            reads: reads.clone(),
        },
        embeddings: Query {
            config,
            calls: calls.clone(),
            hook: None,
        },
        filesystem: LocalFileSystem::open(root.path()).unwrap(),
        markdown: MarkdownParser::new(),
        provenance: Observations(observed.clone()),
    });

    let error = search.search(&build_request()).unwrap_err();

    assert_eq!(calls.get(), 1);
    assert_eq!(reads.get(), 1);
    assert!(observed.borrow().is_empty());
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.get_message(), "cannot search knowledge");
    assert!(std::error::Error::source(&error).is_some());
    assert!(!format!("{error:?} {error}").contains("created_at"));
    // Keep the service alive: returning the error itself must release its reader.
    assert_can_checkpoint(root.path());
    connection
        .execute("UPDATE nodes SET created_at = 1 WHERE id = ?1", ["a.md"])
        .unwrap();
    publish_candidate(&mut open_storage(root.path()), "b", "new.rs");
    assert_eq!(
        open_storage(root.path())
            .begin_read()
            .unwrap()
            .get_revision(),
        2
    );
    assert_can_checkpoint(root.path());
}

#[test]
fn search_observes_metadata_and_bindings_from_original_pinned_view() {
    let root = tempfile::tempdir().unwrap();
    let mut storage = open_storage(root.path());
    publish_candidate(&mut storage, "a", "old.rs");
    let config = build_config(2);
    seed_dimension(&mut storage, &config, 2);
    let mut writer = open_storage(root.path());
    let calls = Rc::new(Cell::new(0));
    let reads = Rc::new(Cell::new(0));
    let observed = Rc::new(RefCell::new(Vec::new()));
    let mut search = SearchService::new(SearchDependencies {
        refresher: NoRefresh,
        storage: ReadOnlyStorage {
            inner: storage,
            reads: reads.clone(),
        },
        embeddings: Query {
            config,
            calls: calls.clone(),
            hook: Some(Box::new(move || {
                publish_candidate(&mut writer, "b", "new.rs");
            })),
        },
        filesystem: LocalFileSystem::open(root.path()).unwrap(),
        markdown: MarkdownParser::new(),
        provenance: Observations(observed.clone()),
    });

    let links = search.search(&build_request()).unwrap();

    assert_eq!(calls.get(), 1);
    assert_eq!(reads.get(), 1);
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].revision, 1);
    assert_eq!(
        links[0].flags,
        BTreeSet::from([SearchFlag::Stale, SearchFlag::SourceChanged])
    );
    assert_eq!(
        *observed.borrow(),
        vec![(
            Some(TreeOid::parse(&"a".repeat(40)).unwrap()),
            BTreeSet::from([SourceUri::parse("old.rs").unwrap()])
        )]
    );
    assert_eq!(
        open_storage(root.path())
            .begin_read()
            .unwrap()
            .get_revision(),
        2
    );
    assert_can_checkpoint(root.path());
}

impl Refresher for NoRefresh {
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        Ok(RefreshOutcome {
            revision: 1,
            published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    }
}
impl Embedder for Query {
    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        Ok(ConfigState::Ready(self.config.clone()))
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
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        self.0
            .borrow_mut()
            .push((request.source_snapshot.cloned(), request.sources.clone()));
        Ok(ProvenanceObservation {
            flags: BTreeSet::from([SearchFlag::SourceChanged]),
            issues: BTreeSet::new(),
        })
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("search cannot compare revisions")
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
    fn reset_vectors(&mut self, _: &Sha256) -> StorageResult<bool> {
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
fn build_request() -> SearchRequest {
    SearchRequest {
        query: "query".into(),
        limit: NonZeroUsize::new(10).unwrap(),
    }
}
fn assert_can_checkpoint(root: &Path) {
    for name in ["cache.db", "vectors.db"] {
        let connection = rusqlite::Connection::open(root.join(name)).unwrap();
        let busy: i64 = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 0, "leaked read prevented checkpoint of {name}");
    }
}
