mod bindings {
    use super::super::LinkObservations;
    use crate::service::StorageError;
    use crate::{
        domain::{
            Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri,
            TreeOid,
        },
        filesystem::LocalFileSystem,
        markdown::MarkdownParser,
        service::{
            DocumentError, DocumentErrorKind, DocumentResult, ElementAddress, GraphNeighbor,
            GraphNodeKey, GraphNodeRef, GraphRead, KnowledgeMetadata, NeighborPageRequest,
            ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest, SourceService, StorageResult,
        },
    };
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

    const EXPECTED_BINDINGS: [&str; 70] = [
        "s000.rs", "s001.rs", "s002.rs", "s003.rs", "s004.rs", "s005.rs", "s006.rs", "s007.rs",
        "s008.rs", "s009.rs", "s010.rs", "s011.rs", "s012.rs", "s013.rs", "s014.rs", "s015.rs",
        "s016.rs", "s017.rs", "s018.rs", "s019.rs", "s020.rs", "s021.rs", "s022.rs", "s023.rs",
        "s024.rs", "s025.rs", "s026.rs", "s027.rs", "s028.rs", "s029.rs", "s030.rs", "s031.rs",
        "s032.rs", "s033.rs", "s034.rs", "s035.rs", "s036.rs", "s037.rs", "s038.rs", "s039.rs",
        "s040.rs", "s041.rs", "s042.rs", "s043.rs", "s044.rs", "s045.rs", "s046.rs", "s047.rs",
        "s048.rs", "s049.rs", "s050.rs", "s051.rs", "s052.rs", "s053.rs", "s054.rs", "s055.rs",
        "s056.rs", "s057.rs", "s058.rs", "s059.rs", "s060.rs", "s061.rs", "s062.rs", "s063.rs",
        "s064.rs", "s065.rs", "s066.rs", "s067.rs", "s068.rs", "s069.rs",
    ];

    #[test]
    fn observes_large_source_bindings_pagewise() {
        // Given: source bindings and provenance observations have recorded call order.
        let events = Rc::new(RefCell::new(Vec::new()));
        let read = BindingRead {
            events: events.clone(),
            count: 70,
            fail_at: None,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: None,
            observed: RefCell::new(Vec::new()),
        };
        let node = build_knowledge();
        let root = tempfile::TempDir::new().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let sources = SourceService::new(
            Rc::new(crate::storage::CacheStorage::new_observed(None)),
            LocalFileSystem::new(root.path()).unwrap(),
        );
        let observations = LinkObservations {
            storage_read: &read,
            filesystem: &filesystem,
            markdown: &MarkdownParser,
            sources: &sources,
            provenance: &provenance,
        };
        // When: the selected knowledge node observes its complete source bindings.
        let flags = observations.observe_bindings(&node).unwrap();
        // Then: bounded observations preserve flags or report the expected failure.
        assert_eq!(
            *events.borrow(),
            [
                "resolve",
                "page:0",
                "observe:32",
                "page:32",
                "observe:32",
                "page:64",
                "observe:6",
                "page:70"
            ]
        );
        assert_eq!(
            flags,
            BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
        );
        assert_eq!(
            provenance
                .observed
                .borrow()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            EXPECTED_BINDINGS,
            "binding identities and multiplicity"
        );
    }

    #[test]
    fn observes_empty_bindings_once_to_preserve_basis_validation() {
        // Given: source bindings and provenance observations have recorded call order.
        let events = Rc::new(RefCell::new(Vec::new()));
        let read = BindingRead {
            events: events.clone(),
            count: 0,
            fail_at: None,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: None,
            observed: RefCell::new(Vec::new()),
        };
        let node = build_knowledge();
        let root = tempfile::TempDir::new().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let sources = SourceService::new(
            Rc::new(crate::storage::CacheStorage::new_observed(None)),
            LocalFileSystem::new(root.path()).unwrap(),
        );
        let observations = LinkObservations {
            storage_read: &read,
            filesystem: &filesystem,
            markdown: &MarkdownParser,
            sources: &sources,
            provenance: &provenance,
        };
        // When: the selected knowledge node observes its complete source bindings.
        let flags = observations.observe_bindings(&node).unwrap();
        // Then: bounded observations preserve flags or report the expected failure.
        assert_eq!(*events.borrow(), ["resolve", "page:0", "observe:0"]);
        assert_eq!(flags, BTreeSet::from([SearchFlag::SourceChanged]));
    }

    #[test]
    fn propagates_late_provenance_failure_without_partial_flags_or_further_pages() {
        // Given: source bindings and provenance observations have recorded call order.
        let events = Rc::new(RefCell::new(Vec::new()));
        let read = BindingRead {
            events: events.clone(),
            count: 70,
            fail_at: None,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: Some("s032.rs"),
            observed: RefCell::new(Vec::new()),
        };
        let node = build_knowledge();
        let root = tempfile::TempDir::new().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let sources = SourceService::new(
            Rc::new(crate::storage::CacheStorage::new_observed(None)),
            LocalFileSystem::new(root.path()).unwrap(),
        );
        let observations = LinkObservations {
            storage_read: &read,
            filesystem: &filesystem,
            markdown: &MarkdownParser,
            sources: &sources,
            provenance: &provenance,
        };
        // When: the selected knowledge node observes its complete source bindings.
        let error = observations.observe_bindings(&node).unwrap_err();
        // Then: bounded observations preserve flags or report the expected failure.
        assert_eq!(
            error.get_kind(),
            crate::service::CommandErrorKind::InternalError
        );
        assert_eq!(
            *events.borrow(),
            ["resolve", "page:0", "observe:32", "page:32", "observe:32"]
        );
    }

    #[test]
    fn reports_late_binding_page_failure_after_completed_observations() {
        // Given: source bindings and provenance observations have recorded call order.
        let events = Rc::new(RefCell::new(Vec::new()));
        let read = BindingRead {
            events: events.clone(),
            count: 70,
            fail_at: Some(64),
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: None,
            observed: RefCell::new(Vec::new()),
        };
        let root = tempfile::TempDir::new().unwrap();
        let filesystem = LocalFileSystem::new(root.path()).unwrap();
        let sources = SourceService::new(
            Rc::new(crate::storage::CacheStorage::new_observed(None)),
            LocalFileSystem::new(root.path()).unwrap(),
        );
        let observations = LinkObservations {
            storage_read: &read,
            filesystem: &filesystem,
            markdown: &MarkdownParser,
            sources: &sources,
            provenance: &provenance,
        };
        // When: the selected knowledge node observes its complete source bindings.
        let error = observations
            .observe_bindings(&build_knowledge())
            .unwrap_err();
        // Then: bounded observations preserve flags or report the expected failure.
        assert_eq!(
            error.get_kind(),
            crate::service::CommandErrorKind::InternalError
        );
        assert_eq!(
            *events.borrow(),
            [
                "resolve",
                "page:0",
                "observe:32",
                "page:32",
                "observe:32",
                "page:64"
            ]
        );
    }

    struct BindingRead {
        events: Rc<RefCell<Vec<String>>>,
        count: usize,
        fail_at: Option<usize>,
    }
    struct BindingProvenance {
        events: Rc<RefCell<Vec<String>>>,
        fail_on: Option<&'static str>,
        observed: RefCell<Vec<String>>,
    }

    impl GraphRead for BindingRead {
        fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
            assert_eq!(uri.as_str(), "b.md");
            self.events.borrow_mut().push("resolve".into());
            Ok(Some(GraphNodeRef {
                key: GraphNodeKey::new(1),
                element: EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
            }))
        }

        fn edges_get_by_row_id_and_type(
            &self,
            request: &NeighborPageRequest<'_>,
        ) -> StorageResult<Vec<GraphNeighbor>> {
            assert_eq!(request.limit.get(), 32);
            assert_eq!(
                request
                    .edge_types
                    .iter()
                    .map(EdgeType::as_str)
                    .collect::<Vec<_>>(),
                ["external"]
            );
            let start = request.after.map_or(0, |cursor| {
                cursor.neighbor.as_str()[1..4].parse::<usize>().unwrap() + 1
            });
            self.events.borrow_mut().push(format!("page:{start}"));
            if self.fail_at == Some(start) {
                return Err(StorageError::new(
                    crate::service::StorageErrorKind::InternalError,
                    "late binding page unavailable",
                ));
            }
            Ok((start..self.count)
                .take(32)
                .map(|index| {
                    let target =
                        EdgeTarget::Source(SourceUri::parse(&format!("s{index:03}.rs")).unwrap());
                    let edge = Edge::new(
                        KnowledgeId::parse("b.md").unwrap(),
                        target.clone(),
                        EdgeType::parse("external").unwrap(),
                    )
                    .unwrap();
                    GraphNeighbor {
                        node: GraphNodeRef {
                            key: GraphNodeKey::new(i64::try_from(index + 2).unwrap()),
                            element: target,
                        },
                        edge,
                    }
                })
                .collect())
        }
    }

    impl SourceProvenance for BindingProvenance {
        fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            panic!("unexpected revision validation")
        }

        fn observe_sources(
            &self,
            request: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            assert!(request.sources.len() <= 32);
            self.events
                .borrow_mut()
                .push(format!("observe:{}", request.sources.len()));
            self.observed.borrow_mut().extend(
                request
                    .sources
                    .iter()
                    .map(|source| source.as_str().to_owned()),
            );
            let first = request.sources.first().map(SourceUri::as_str);
            if self.fail_on.is_some() && first == self.fail_on {
                return Err(DocumentError::new(
                    DocumentErrorKind::InternalError,
                    "private provenance failure",
                ));
            }
            let flag = if first == Some("s032.rs") {
                SearchFlag::SourceNotFound
            } else {
                SearchFlag::SourceChanged
            };
            Ok(ProvenanceObservation {
                flags: BTreeSet::from([flag]),
                issues: BTreeSet::new(),
            })
        }

        fn compare_revision(
            &self,
            _: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            panic!("unexpected revision comparison")
        }
    }
    fn build_knowledge() -> KnowledgeMetadata {
        KnowledgeMetadata {
            id: KnowledgeId::parse("b.md").unwrap(),
            uri: KnowledgeId::parse("b.md").unwrap(),
            kind: ElementKind::Vertex,
            hash: Sha256::digest(b"fixture"),
            created_at: 3,
            source_snapshot: None,
            lines: LineRange::new(1, 2).unwrap(),
            search_hash: Sha256::digest(b"search"),
        }
    }
}
mod scoped {
    use crate::domain::RepoPath;
    use crate::service::TransactionError;

    use super::{
        super::TraversalService,
        traced_storage::{Events, TracedStorage},
    };
    use crate::{
        domain::{
            Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri,
            TreeOid,
        },
        filesystem::LocalFileSystem,
        markdown::MarkdownParser,
        service::{
            CacheDatabase, CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind,
            DocumentResult, DrainOutcome, ElementAddress, Embedder, EmbeddingErrorKind,
            EmbeddingResult, GraphService, KnowledgeMetadata, NodeMetadata, ProvenanceObservation,
            ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome, Refresher,
            SearchFlag, SnapshotError, SourceMetadata, SourceObservation, SourceProvenance,
            SourceReader, SourceRevisionComparison, SourceRevisionRequest, SourceService,
            StorageRead, TraversalDirection, TraversalMode, TraversalRequest, Traverser,
        },
        storage::TestDatabases,
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
        inner: SourceService<LocalFileSystem, TracedStorage>,
        events: Events,
        failure: Failure,
    }

    #[test]
    fn retains_original_graph_bindings_and_revision_across_pages() {
        // Given: publish replacement membership and basis only after revision 1 is pinned.
        let fixture = Fixture::new();
        let root = fixture.root.path().to_owned();
        let mut service = fixture.build_service(
            Failure::None,
            Some(Box::new(move || publish_replacement(&root))),
        );
        let mut request = build_request();
        request.depth = 2;
        request.edge_limit = NonZeroUsize::new(10);
        // When: traversal reads successive pages.
        let output = service.traverse(&request).unwrap();
        // Then: pinned graph/coordinates/basis separately from live observations.
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
            events.iter().filter(|x| x.as_str() == "read_scope").count(),
            1
        );
        fixture.assert_released();
    }

    #[test]
    fn synchronizes_readiness_before_one_pinned_read_without_query_inference() {
        // Given: readiness states and recorded traversal events.
        for state in [
            ConfigState::Missing,
            ConfigState::Changed,
            ConfigState::Invalid(EmbeddingErrorKind::InvalidConfig),
        ] {
            let fixture = Fixture::new();
            let mut service = fixture.build_service(Failure::None, None);
            service.embeddings = Box::new(Readiness {
                events: fixture.events.clone(),
                state,
            });
            // When: traversal runs under each readiness state.
            let output = service.traverse(&build_request()).unwrap();
            // Then: readiness precedes one pinned read without query inference.
            assert_eq!(output.elements.len(), 1);
            let events = fixture.events.borrow();
            assert_eq!(&events[..3], ["refresh", "readiness", "read_scope"]);
            assert_eq!(
                events.iter().filter(|x| x.as_str() == "read_scope").count(),
                1
            );
        }
    }

    #[test]
    fn releases_read_after_late_failure_while_service_lives() {
        // Given: late page, source and provenance failures.
        for failure in [Failure::Page, Failure::Source, Failure::Provenance] {
            let fixture = Fixture::new();
            if matches!(failure, Failure::Provenance) {
                let storage = open_storage(fixture.root.path());
                storage
                    .cache_storage
                    .with_write_tx(|| {
                        let write = &storage.cache_storage;
                        write
                            .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                                "b.md", "c.md", "includes",
                            ))
                            .unwrap();

                        write.project_get_revision()
                    })
                    .unwrap();
            }
            let mut service = fixture.build_service(failure, None);
            let mut request = build_request();
            request.depth = 3;
            // When: each failure follows at least one successfully selected graph record.
            let error = service.traverse(&request).unwrap_err();
            // Then: no partial success, safe public context, retained private error chain.
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
                service
                    .cache
                    .with_read_tx(|| service.cache.project_get_by_id())
                    .unwrap(),
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
        // Given: normal and depth-zero traversal requests.
        for depth in [0, 1] {
            let fixture = Fixture::new();
            let mut service = fixture.build_service(Failure::None, None);
            let mut request = build_request();
            request.depth = depth;
            // When: traversal returns its elements.
            let output = service.traverse(&request).unwrap();
            // Then: the scoped read is released for both result shapes.
            assert_eq!(output.elements.len(), usize::try_from(depth).unwrap());
            fixture.assert_released();
            publish_replacement(fixture.root.path());
            assert_eq!(
                service
                    .cache
                    .with_read_tx(|| service.cache.project_get_by_id())
                    .unwrap(),
                3
            );
        }
    }

    impl Fixture {
        fn new() -> Self {
            let root = TempDir::new().unwrap();
            std::fs::write(root.path().join("old.rs"), b"live source").unwrap();
            let storage = open_storage(root.path());
            let write_result = storage
                .cache_storage
                .with_write_tx(|| {
                    let write = &storage.cache_storage;
                    for id in ["a.md", "b.md", "c.md"] {
                        write.nodes_upsert_by_id(&build_knowledge(id, S1)).unwrap();
                    }
                    write.nodes_upsert_by_id(&build_source("old.rs")).unwrap();
                    write
                        .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                            "a.md", "b.md", "includes",
                        ))
                        .unwrap();
                    write
                        .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                            "b.md", "old.rs", "external",
                        ))
                        .unwrap();

                    write.project_get_revision()
                })
                .map_err(TransactionError::into_inner);
            assert_eq!(write_result.unwrap(), 1);
            Self {
                root,
                events: Rc::new(RefCell::new(Vec::new())),
            }
        }

        fn build_service(
            &self,
            failure: Failure,
            after_pin: Option<Box<dyn FnOnce()>>,
        ) -> TraversalService<LocalFileSystem, TracedStorage> {
            let storage = Rc::new(TracedStorage {
                active: std::cell::Cell::new(false),
                inner: open_storage(self.root.path()).cache_storage,
                events: self.events.clone(),
                after_pin: RefCell::new(after_pin),
                pages: std::cell::Cell::new(0),
                fail_page: matches!(failure, Failure::Page).then_some(2),
            });
            TraversalService {
                cache: Rc::clone(&storage),
                filesystem: LocalFileSystem::new(self.root.path()).unwrap(),
                refresher: Box::new(Refresh {
                    events: self.events.clone(),
                }),
                embeddings: Box::new(Readiness {
                    events: self.events.clone(),
                    state: ConfigState::Missing,
                }),
                markdown: Box::new(MarkdownParser::new()),
                sources: Box::new(Sources {
                    inner: SourceService::new(
                        Rc::clone(&storage),
                        LocalFileSystem::new(self.root.path()).unwrap(),
                    ),
                    events: self.events.clone(),
                    failure,
                }),
                provenance: Box::new(Provenance {
                    events: self.events.clone(),
                    failure,
                }),
                graph: Box::new(GraphService::new(storage)),
            }
        }

        fn assert_released(&self) {
            let connection = rusqlite::Connection::open(self.root.path().join("cache.db")).unwrap();
            connection
                .execute("update project set revision=revision+1", [])
                .unwrap();
            let busy: i64 = connection
                .query_row("pragma wal_checkpoint(truncate)", [], |row| row.get(0))
                .unwrap();
            assert_eq!(busy, 0, "reader retained by live service");
        }
    }

    impl Refresher for Refresh {
        fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            panic!("routine reads must not request a full audit")
        }

        fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            panic!("this scenario must not trigger a full reindex")
        }

        fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            self.events.borrow_mut().push("refresh".into());
            Ok(RefreshOutcome {
                revision: 1,
                is_published: false,
                files_updated: 0,
                files_removed: 0,
                invalid_files_updated: 0,
            })
        }
    }

    impl Embedder for Readiness {
        fn get_configuration(&mut self) -> EmbeddingResult<ConfigState> {
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
        fn get_source(&self, uri: &SourceUri) -> DocumentResult<Option<SourceObservation>> {
            self.events
                .borrow_mut()
                .push(format!("source:{}", uri.as_str()));
            if matches!(self.failure, Failure::Source) {
                return Err(DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "observation unavailable",
                    std::io::Error::other("PRIVATE_OBSERVATION"),
                ));
            }
            self.inner.get_source(uri)
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
                return Err(DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "observation unavailable",
                    std::io::Error::other("PRIVATE_OBSERVATION"),
                ));
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
        let storage = open_storage(root);
        storage
            .cache_storage
            .with_write_tx(|| {
                let write = &storage.cache_storage;
                write
                    .edges_delete_by_source_id(&KnowledgeId::parse("a.md").unwrap())
                    .unwrap();
                write
                    .edges_delete_by_source_id(&KnowledgeId::parse("b.md").unwrap())
                    .unwrap();
                let NodeMetadata::Knowledge(mut replacement) = build_knowledge("b.md", S2) else {
                    panic!("expected knowledge fixture")
                };
                replacement.lines = LineRange::new(20, 30).unwrap();
                write
                    .nodes_upsert_by_id(&NodeMetadata::Knowledge(replacement))
                    .unwrap();
                write.nodes_upsert_by_id(&build_source("new.rs")).unwrap();
                write
                    .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                        "a.md", "c.md", "includes",
                    ))
                    .unwrap();
                write
                    .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                        "b.md", "new.rs", "external",
                    ))
                    .unwrap();
                write
                    .delete_node(&EdgeTarget::Source(SourceUri::parse("old.rs").unwrap()))
                    .unwrap();

                write.project_get_revision()
            })
            .unwrap();
    }
    fn open_storage(root: &Path) -> TestDatabases {
        crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
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
}
mod traced_storage {
    use crate::service::TransactionError;
    use crate::{
        domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
        service::{
            ElementAddress, EmbeddingFailure, EmbeddingWork, FileRow, GraphNeighbor, GraphNodeRef,
            GraphRead, NeighborPageRequest, QueryError, StorageError, StorageErrorKind,
            StorageRead, StorageResult, StoredNode, TextMatch, VectorMatch, VectorOutcome,
        },
    };
    use std::{cell::RefCell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};

    pub(super) type Events = Rc<RefCell<Vec<String>>>;
    pub(super) struct TracedStorage {
        pub(super) inner: crate::storage::CacheStorage,
        pub(super) events: Events,
        pub(super) after_pin: RefCell<Option<Box<dyn FnOnce()>>>,
        pub(super) pages: std::cell::Cell<usize>,
        pub(super) active: std::cell::Cell<bool>,
        pub(super) fail_page: Option<usize>,
    }
    use crate::service::cache_test_support::delegate_cache_writes;

    impl crate::service::CacheDatabase for TracedStorage {
        delegate_cache_writes!();

        fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            let nested = self.active.replace(true);
            if !nested {
                self.events.borrow_mut().push("read_scope".into());
                self.pages.set(0);
            }
            let result = self.inner.with_read_tx(operation);
            self.active.set(nested);
            result
        }

        fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_write_tx(operation)
        }

        fn with_status_read_tx<T, E, F>(
            &self,
            operation: F,
        ) -> Result<Option<T>, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.inner.with_status_read_tx(operation)
        }
    }

    impl GraphRead for TracedStorage {
        fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
            self.events
                .borrow_mut()
                .push(format!("resolve:{}", uri.as_str()));
            GraphRead::nodes_get_by_uri(&self.inner, uri)
        }

        fn edges_get_by_row_id_and_type(
            &self,
            request: &NeighborPageRequest<'_>,
        ) -> StorageResult<Vec<GraphNeighbor>> {
            assert!(request.limit.get() <= 32);
            self.pages.set(self.pages.get() + 1);
            self.events
                .borrow_mut()
                .push(format!("page:{}", self.pages.get()));
            if self.fail_page == Some(self.pages.get()) {
                return Err(StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "page unavailable",
                    std::io::Error::other("PRIVATE_PAGE"),
                ));
            }
            let page = self.inner.edges_get_by_row_id_and_type(request)?;
            if let Some(hook) = self.after_pin.take() {
                hook();
            }
            Ok(page)
        }
    }

    impl StorageRead for TracedStorage {
        fn nodes_get_by_kind(&self) -> StorageResult<u64> {
            self.inner.nodes_get_by_kind()
        }

        fn nodes_get_coverage_by_kind(&self) -> StorageResult<u64> {
            self.inner.nodes_get_coverage_by_kind()
        }

        fn embed_failures_get_by_kind_and_search_hash_and_attempts(
            &self,
            max_attempts: u64,
        ) -> StorageResult<Vec<EmbeddingFailure>> {
            self.inner
                .embed_failures_get_by_kind_and_search_hash_and_attempts(max_attempts)
        }

        fn nodes_get_by_uri(&self, uri: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
            self.events
                .borrow_mut()
                .push(format!("metadata:{}", uri.as_str()));
            StorageRead::nodes_get_by_uri(&self.inner, uri)
        }

        fn project_get_by_id(&self) -> StorageResult<u64> {
            self.inner.project_get_by_id()
        }

        fn meta_get_by_id(&self) -> StorageResult<Option<Sha256>> {
            self.inner.meta_get_by_id()
        }

        fn nodes_get(&self) -> StorageResult<Vec<StoredNode>> {
            panic!("whole nodes")
        }

        fn edges_get_by_source_id(&self, _: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
            panic!("whole edges")
        }

        fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>> {
            self.inner.nodes_get_by_source_snapshot()
        }

        fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
            self.inner.files_get_by_path(path)
        }

        fn files_get(&self) -> StorageResult<Vec<FileRow>> {
            self.inner.files_get()
        }

        fn text_index_get_by_expression(
            &self,
            _: &str,
            _limit: NonZeroUsize,
        ) -> StorageResult<Vec<TextMatch>> {
            panic!("query text")
        }

        fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
            &self,
            expected: &Sha256,
            max_attempts: u64,
            after: Option<&KnowledgeId>,
        ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
            self.inner
                .nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
                    expected,
                    max_attempts,
                    after,
                )
        }

        fn vector_index_config_get_by_id(
            &self,
            expected: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
            self.inner.vector_index_config_get_by_id(expected)
        }

        fn vector_keys_get_by_search_hash(
            &self,
            expected: &Sha256,
            search_hash: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
            self.inner
                .vector_keys_get_by_search_hash(expected, search_hash)
        }

        fn vector_index_get_by_vector(
            &self,
            _: &Sha256,
            _: &[f32],
            _: usize,
            _limit: NonZeroUsize,
        ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
            panic!("query vectors")
        }

        fn embed_failures_get_by_search_hash(
            &self,
            expected: &Sha256,
            search_hash: &Sha256,
        ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
            self.inner
                .embed_failures_get_by_search_hash(expected, search_hash)
        }

        fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>> {
            self.inner.query_error_get_by_id()
        }
    }
}
