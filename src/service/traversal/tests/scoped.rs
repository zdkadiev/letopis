use super::{
    super::TraversalService,
    traced_storage::{Events, TracedStorage},
};
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid,
    },
    filesystem::LocalFileSystem,
    markdown::MarkdownParser,
    service::{
        CacheWrite, CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind,
        DocumentResult, DrainOutcome, ElementAddress, Embedder, EmbeddingErrorKind,
        EmbeddingResult, GraphService, KnowledgeMetadata, NodeMetadata, ProvenanceObservation,
        ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, Refresher,
        SearchFlag, SnapshotError, SourceMetadata, SourceObservation, SourceProvenance,
        SourceReader, SourceRevisionComparison, SourceRevisionRequest, SourceService, Storage,
        StorageRead, TraversalDirection, TraversalMode, TraversalRequest, Traverser,
    },
    storage::SqliteStorage,
};
use std::{
    cell::RefCell, collections::BTreeSet, error::Error, num::NonZeroUsize, path::Path, rc::Rc,
};
use tempfile::TempDir;

const S1: &str = "1111111111111111111111111111111111111111";
const S2: &str = "2222222222222222222222222222222222222222";
struct Fixture {
    root: TempDir,
    events: Events,
}
#[derive(Clone, Copy)]
enum Failure {
    None,
    Page,
    Source,
    Provenance,
}
struct Readiness {
    events: Events,
    state: ConfigState,
}
struct Refresh {
    events: Events,
}
struct Provenance {
    events: Events,
    failure: Failure,
}
struct Sources {
    inner: SourceService<LocalFileSystem>,
    events: Events,
    failure: Failure,
}

#[test]
fn retains_original_graph_bindings_and_revision_across_pages() {
    // Arrange: publish replacement membership and basis only after revision 1 is pinned.
    let fixture = Fixture::new();
    let root = fixture.root.path().to_owned();
    let mut service = fixture.build_service(
        Failure::None,
        Some(Box::new(move || publish_replacement(&root))),
    );
    let mut request = build_request();
    request.depth = 2;
    request.edge_limit = NonZeroUsize::new(10);
    // Act.
    let output = service.traverse(&request).unwrap();
    // Assert pinned graph/coordinates/basis separately from live observations.
    assert_eq!(
        output
            .elements
            .iter()
            .map(|x| x.element_id.as_str())
            .collect::<Vec<_>>(),
        ["b.md", "old.rs"]
    );
    assert!(output.elements.iter().all(|x| x.revision == 1));
    assert_eq!(
        output.elements[0].lines,
        Some(LineRange::new(4, 8).unwrap())
    );
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::Stale, SearchFlag::SourceChanged])
    );
    assert!(output.elements[1].flags.is_empty());
    let edges = output.edges.unwrap();
    assert_eq!(
        edges
            .iter()
            .map(|e| (
                e.get_source().as_str(),
                e.get_target().as_str(),
                e.get_edge_type().as_str()
            ))
            .collect::<Vec<_>>(),
        [("a.md", "b.md", "includes"), ("b.md", "old.rs", "external")]
    );
    let events = fixture.events.borrow();
    assert!(
        events
            .iter()
            .any(|x| x == &format!("provenance:{S1}:old.rs"))
    );
    assert!(events.iter().any(|x| x == "source:old.rs"));
    assert!(
        !events
            .iter()
            .any(|x| x.contains(S2) || x.contains("new.rs") || x.contains("c.md"))
    );
    assert_eq!(
        events.iter().filter(|x| x.as_str() == "begin_read").count(),
        1
    );
    fixture.assert_released();
}

#[test]
fn synchronizes_readiness_before_one_pinned_read_without_query_inference() {
    for state in [
        ConfigState::Missing,
        ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
    ] {
        let fixture = Fixture::new();
        let mut service = fixture.build_service(Failure::None, None);
        service.embeddings = Box::new(Readiness {
            events: fixture.events.clone(),
            state,
        });
        let output = service.traverse(&build_request()).unwrap();
        assert_eq!(output.elements.len(), 1);
        let events = fixture.events.borrow();
        assert_eq!(&events[..3], ["refresh", "readiness", "begin_read"]);
        assert_eq!(
            events.iter().filter(|x| x.as_str() == "begin_read").count(),
            1
        );
    }
}

#[test]
fn releases_read_after_late_failure_while_service_lives() {
    for failure in [Failure::Page, Failure::Source, Failure::Provenance] {
        let fixture = Fixture::new();
        if matches!(failure, Failure::Provenance) {
            let mut storage = open_storage(fixture.root.path());
            let mut write = storage.begin_cache_write().unwrap();
            write
                .insert_edge(&build_edge("b.md", "c.md", "includes"))
                .unwrap();
            write.commit().unwrap();
        }
        let mut service = fixture.build_service(failure, None);
        let mut request = build_request();
        request.depth = 3;
        // Act: each failure follows at least one successfully selected graph record.
        let error = service.traverse(&request).unwrap_err();
        // Assert no partial success, safe public context, retained private error chain.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(!error.to_string().contains("PRIVATE"));
        assert!(!format!("{error:?}").contains("PRIVATE"));
        let cause = error.source().unwrap().source().unwrap();
        assert!(cause.to_string().contains("PRIVATE"));
        let events = fixture.events.borrow();
        assert!(events.iter().any(|x| x == "page:1"));
        if !matches!(failure, Failure::Page) {
            assert!(events.iter().any(|x| x.starts_with("provenance:")));
        }
        drop(events);
        fixture.assert_released();
        publish_replacement(fixture.root.path());
        assert_eq!(
            service.storage.inner.begin_read().unwrap().get_revision(),
            if matches!(failure, Failure::Provenance) {
                4
            } else {
                3
            }
        );
        drop(service);
    }
}

#[test]
fn releases_read_after_normal_and_depth_zero_empty_results() {
    for depth in [0, 1] {
        let fixture = Fixture::new();
        let mut service = fixture.build_service(Failure::None, None);
        let mut request = build_request();
        request.depth = depth;
        let output = service.traverse(&request).unwrap();
        assert_eq!(output.elements.len(), usize::try_from(depth).unwrap());
        fixture.assert_released();
        publish_replacement(fixture.root.path());
        assert_eq!(
            service.storage.inner.begin_read().unwrap().get_revision(),
            3
        );
    }
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        std::fs::write(root.path().join("old.rs"), b"live source").unwrap();
        let mut storage = open_storage(root.path());
        let mut write = storage.begin_cache_write().unwrap();
        for id in ["a.md", "b.md", "c.md"] {
            write.upsert_node(&build_knowledge(id, S1)).unwrap();
        }
        write.upsert_node(&build_source("old.rs")).unwrap();
        write
            .insert_edge(&build_edge("a.md", "b.md", "includes"))
            .unwrap();
        write
            .insert_edge(&build_edge("b.md", "old.rs", "external"))
            .unwrap();
        assert_eq!(write.commit().unwrap(), 1);
        Self {
            root,
            events: Rc::new(RefCell::new(Vec::new())),
        }
    }
    fn build_service(
        &self,
        failure: Failure,
        after_pin: Option<Box<dyn FnOnce()>>,
    ) -> TraversalService<TracedStorage, LocalFileSystem> {
        TraversalService {
            storage: TracedStorage {
                inner: open_storage(self.root.path()),
                events: self.events.clone(),
                after_pin,
                fail_page: matches!(failure, Failure::Page).then_some(2),
            },
            filesystem: LocalFileSystem::open(self.root.path()).unwrap(),
            refresher: Box::new(Refresh {
                events: self.events.clone(),
            }),
            embeddings: Box::new(Readiness {
                events: self.events.clone(),
                state: ConfigState::Missing,
            }),
            markdown: Box::new(MarkdownParser::new()),
            sources: Box::new(Sources {
                inner: SourceService::new(LocalFileSystem::open(self.root.path()).unwrap()),
                events: self.events.clone(),
                failure,
            }),
            provenance: Box::new(Provenance {
                events: self.events.clone(),
                failure,
            }),
            graph: Box::new(GraphService::new()),
        }
    }
    fn assert_released(&self) {
        let connection = rusqlite::Connection::open(self.root.path().join("cache.db")).unwrap();
        connection
            .execute("UPDATE project SET revision=revision+1", [])
            .unwrap();
        let busy: i64 = connection
            .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| row.get(0))
            .unwrap();
        assert_eq!(busy, 0, "reader retained by live service");
    }
}
impl Refresher for Refresh {
    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        self.events.borrow_mut().push("refresh".into());
        Ok(RefreshOutcome {
            revision: 1,
            published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    }
}
impl Embedder for Readiness {
    fn read_configuration(&mut self) -> EmbeddingResult<ConfigState> {
        self.events.borrow_mut().push("readiness".into());
        Ok(std::mem::replace(&mut self.state, ConfigState::Missing))
    }
    fn drain(&mut self) -> EmbeddingResult<DrainOutcome> {
        panic!("unexpected drain")
    }
    fn embed_query(&mut self, _: &str) -> EmbeddingResult<QueryEmbedding> {
        panic!("query inference")
    }
    fn embed_query_with_config(
        &mut self,
        _: QueryEmbeddingRequest<'_>,
    ) -> EmbeddingResult<QueryEmbedding> {
        panic!("query inference")
    }
}
impl SourceReader for Sources {
    fn read_source(
        &self,
        view: &mut dyn StorageRead,
        uri: &SourceUri,
    ) -> DocumentResult<Option<SourceObservation>> {
        self.events
            .borrow_mut()
            .push(format!("source:{}", uri.as_str()));
        if matches!(self.failure, Failure::Source) {
            return Err(build_private_error());
        }
        self.inner.read_source(view, uri)
    }
}
impl SourceProvenance for Provenance {
    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        let sources = request
            .sources
            .iter()
            .map(SourceUri::as_str)
            .collect::<Vec<_>>()
            .join(",");
        self.events.borrow_mut().push(format!(
            "provenance:{}:{sources}",
            request.source_snapshot.unwrap().as_str()
        ));
        if matches!(self.failure, Failure::Provenance) && request.sources.is_empty() {
            return Err(build_private_error());
        }
        Ok(ProvenanceObservation {
            flags: BTreeSet::from([SearchFlag::SourceChanged]),
            issues: BTreeSet::new(),
        })
    }
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("unexpected revision validation")
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("unexpected revision comparison")
    }
}
fn publish_replacement(root: &Path) {
    let mut storage = open_storage(root);
    let mut write = storage.begin_cache_write().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("a.md").unwrap())
        .unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("b.md").unwrap())
        .unwrap();
    let NodeMetadata::Knowledge(mut replacement) = build_knowledge("b.md", S2) else {
        panic!("expected knowledge fixture")
    };
    replacement.lines = LineRange::new(20, 30).unwrap();
    write
        .upsert_node(&NodeMetadata::Knowledge(replacement))
        .unwrap();
    write.upsert_node(&build_source("new.rs")).unwrap();
    write
        .insert_edge(&build_edge("a.md", "c.md", "includes"))
        .unwrap();
    write
        .insert_edge(&build_edge("b.md", "new.rs", "external"))
        .unwrap();
    write
        .delete_node(&EdgeTarget::Source(SourceUri::parse("old.rs").unwrap()))
        .unwrap();
    write.commit().unwrap();
}
fn open_storage(root: &Path) -> SqliteStorage {
    SqliteStorage::open(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
}
fn build_request() -> TraversalRequest {
    TraversalRequest {
        element_ids: BTreeSet::from([ElementAddress::parse("a.md").unwrap()]),
        mode: TraversalMode::Both,
        direction: TraversalDirection::Outgoing,
        depth: 1,
        limit: NonZeroUsize::new(20).unwrap(),
        edge_types: BTreeSet::new(),
        edge_limit: None,
    }
}
fn build_knowledge(id: &str, basis: &str) -> NodeMetadata {
    NodeMetadata::Knowledge(KnowledgeMetadata {
        id: KnowledgeId::parse(id).unwrap(),
        uri: KnowledgeId::parse(id).unwrap(),
        kind: ElementKind::Vertex,
        hash: Sha256::digest(b"pinned knowledge"),
        created_at: 3,
        source_snapshot: Some(TreeOid::parse(basis).unwrap()),
        lines: LineRange::new(4, 8).unwrap(),
        search_hash: Sha256::digest(b"search"),
    })
}
fn build_source(id: &str) -> NodeMetadata {
    NodeMetadata::Source(SourceMetadata {
        uri: SourceUri::parse(id).unwrap(),
        hash: None,
        created_at: 3,
        lines: None,
    })
}
fn build_edge(source: &str, target: &str, kind: &str) -> Edge {
    let target = if kind == "external" {
        EdgeTarget::Source(SourceUri::parse(target).unwrap())
    } else {
        EdgeTarget::Knowledge(KnowledgeId::parse(target).unwrap())
    };
    Edge::new(
        KnowledgeId::parse(source).unwrap(),
        target,
        EdgeType::parse(kind).unwrap(),
    )
    .unwrap()
}
fn build_private_error() -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InternalError,
        "observation unavailable",
        std::io::Error::other("PRIVATE_OBSERVATION"),
    )
}
