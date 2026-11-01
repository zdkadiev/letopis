use super::ReadTransaction;
use super::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, ElementLocation, Embedder,
    FileSystem, GraphRead, GraphTraversal, KnowledgeMetadata, MarkdownCodec, NEIGHBOR_PAGE_QUANTUM,
    NeighborCursor, NeighborDirection, NeighborPageRequest, NodeMetadata, ProvenanceRequest,
    Refresher, SearchFlag, SourceAvailability, SourceProvenance, SourceReader, Storage,
    StorageRead, Traversal, TraversalLink, TraversalMode, TraversalRequest, Traverser,
    utils::is_current_knowledge,
};
use crate::domain::{EdgeTarget, EdgeType, Graph, RepoPath, SourceUri};
use std::{collections::BTreeSet, error::Error, fmt, num::NonZeroUsize};

/// Refresh, pin, traverse and observe selected links under one scoped storage read.
///
/// Collaborators are independently owned; readiness cannot write through the pinned read view.
pub(crate) struct TraversalService<F> {
    /// Exclusive owner of the scoped graph/metadata read capability.
    pub(crate) storage: Box<dyn Storage>,
    /// Confined live knowledge observations.
    pub(crate) filesystem: F,
    /// Canonical publication before readiness and pinning.
    pub(crate) refresher: Box<dyn Refresher>,
    /// Independent configuration readiness/reset owner; no inference is requested.
    pub(crate) embeddings: Box<dyn Embedder>,
    /// Shared whole-file knowledge parser.
    pub(crate) markdown: Box<dyn MarkdownCodec>,
    /// Live source observations constrained by the same read view.
    pub(crate) sources: Box<dyn SourceReader>,
    /// Read-only pagewise source comparisons and basis validation.
    pub(crate) provenance: Box<dyn SourceProvenance>,
    /// Injected incremental graph algorithm without complete metadata access.
    pub(crate) graph: Box<dyn GraphTraversal>,
}
struct LinkObservations<'a, F> {
    filesystem: &'a F,
    markdown: &'a dyn MarkdownCodec,
    sources: &'a dyn SourceReader,
    provenance: &'a dyn SourceProvenance,
}
impl<F: FileSystem> Traverser for TraversalService<F> {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal> {
        self.refresher.refresh().map_err(map_traversal_error)?;
        self.embeddings
            .get_configuration()
            .map_err(map_traversal_error)?;
        let mut read = self.storage.begin_read_tx().map_err(map_traversal_error)?;
        let graph = self.graph.traverse(read.as_mut(), request)?;
        let observations = LinkObservations {
            filesystem: &self.filesystem,
            markdown: self.markdown.as_ref(),
            sources: self.sources.as_ref(),
            provenance: self.provenance.as_ref(),
        };
        observations.get_links(read.as_mut(), graph)
    }
}
impl<F: FileSystem> LinkObservations<'_, F> {
    fn get_links(&self, read: &mut dyn ReadTransaction, graph: Graph) -> CommandResult<Traversal> {
        let (elements, edges) = graph.into_parts();
        let elements = elements
            .into_iter()
            .map(|element| self.get_link(read, element))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(Traversal { elements, edges })
    }

    fn get_link(
        &self,
        read: &mut dyn ReadTransaction,
        element: EdgeTarget,
    ) -> CommandResult<TraversalLink> {
        match element {
            EdgeTarget::Knowledge(_) => self.get_knowledge_link(read, &element),
            EdgeTarget::Source(uri) => self.get_source_link(read, &uri),
        }
    }

    fn get_knowledge_link(
        &self,
        read: &mut dyn ReadTransaction,
        element: &EdgeTarget,
    ) -> CommandResult<TraversalLink> {
        let stored = read
            .get_node_by_id(element)
            .map_err(map_traversal_error)?
            .ok_or_else(fail_invalid_traversal_data)?;
        let NodeMetadata::Knowledge(node) = stored.metadata else {
            return Err(fail_invalid_traversal_data());
        };
        if node.id.as_str() != element.as_str() || node.uri != node.id {
            return Err(fail_invalid_traversal_data());
        }
        let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
            .map_err(map_traversal_error)?;
        let mut flags = self.observe_bindings(read, &node)?;
        let file = self
            .filesystem
            .get_file(&path)
            .map_err(map_traversal_error)?;
        let parsed = file
            .as_ref()
            .and_then(|file| self.markdown.parse_file(&path, &file.bytes).ok().flatten());
        if !is_current_knowledge(&node, parsed.as_ref()) {
            flags.insert(SearchFlag::Stale);
        }
        Ok(TraversalLink {
            element_id: ElementAddress::parse(node.id.as_str())?,
            path: ElementLocation::Local(path),
            lines: Some(node.lines),
            revision: read.get_revision(),
            flags,
        })
    }

    fn get_source_link(
        &self,
        read: &mut dyn StorageRead,
        uri: &SourceUri,
    ) -> CommandResult<TraversalLink> {
        let observation = self
            .sources
            .get_source(read, uri)
            .map_err(map_traversal_error)?
            .ok_or_else(fail_invalid_traversal_data)?;
        if observation.metadata.uri != *uri {
            return Err(fail_invalid_traversal_data());
        }
        let (path, lines) = match uri.get_local() {
            Some(local) => (
                ElementLocation::Local(local.get_path().clone()),
                local.get_lines(),
            ),
            None => (ElementLocation::Remote(uri.clone()), None),
        };
        let flags = if observation.availability == SourceAvailability::SourceNotFound {
            BTreeSet::from([SearchFlag::SourceNotFound])
        } else {
            BTreeSet::new()
        };
        Ok(TraversalLink {
            element_id: ElementAddress::parse(uri.as_str())?,
            path,
            lines,
            revision: read.get_revision(),
            flags,
        })
    }
    /// Observe each outgoing source binding once in bounded pages, regardless of traversal filters.
    fn observe_bindings(
        &self,
        read: &mut dyn GraphRead,
        node: &KnowledgeMetadata,
    ) -> CommandResult<BTreeSet<SearchFlag>> {
        let id = ElementAddress::parse(node.id.as_str())?;
        let handle = read
            .get_node_ref_by_id(&id)
            .map_err(map_traversal_error)?
            .ok_or_else(fail_invalid_traversal_data)?;
        let types = BTreeSet::from([EdgeType::parse("external").map_err(map_traversal_error)?]);
        let mut after = None;
        let mut flags = BTreeSet::new();
        loop {
            let page = read
                .get_neighbor_page(&NeighborPageRequest {
                    node: handle.key,
                    direction: NeighborDirection::Outgoing,
                    mode: TraversalMode::Relations,
                    edge_types: &types,
                    after: after.as_ref(),
                    limit: get_page_limit(),
                })
                .map_err(map_traversal_error)?;
            if page.is_empty() {
                if after.is_none() {
                    flags.extend(self.observe_source_page(node, &BTreeSet::new())?);
                }
                return Ok(flags);
            }
            after = page
                .last()
                .map(|last| {
                    ElementAddress::parse(last.node.element.as_str()).map(|neighbor| {
                        NeighborCursor {
                            edge_type: last.edge.get_edge_type().clone(),
                            neighbor,
                        }
                    })
                })
                .transpose()?;
            let sources = page
                .into_iter()
                .map(|item| match item.node.element {
                    EdgeTarget::Source(source) => Ok(source),
                    EdgeTarget::Knowledge(_) => Err(fail_invalid_traversal_data()),
                })
                .collect::<CommandResult<BTreeSet<_>>>()?;
            flags.extend(self.observe_source_page(node, &sources)?);
        }
    }
    fn observe_source_page(
        &self,
        node: &KnowledgeMetadata,
        sources: &BTreeSet<SourceUri>,
    ) -> CommandResult<BTreeSet<SearchFlag>> {
        Ok(self
            .provenance
            .observe_sources(ProvenanceRequest {
                source_snapshot: node.source_snapshot.as_ref(),
                sources,
            })
            .map_err(map_traversal_error)?
            .flags)
    }
}

impl<F> fmt::Debug for TraversalService<F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraversalService").finish_non_exhaustive()
    }
}
fn get_page_limit() -> NonZeroUsize {
    #[expect(
        clippy::expect_used,
        reason = "the internal neighbor page quantum is a positive constant"
    )]
    NonZeroUsize::new(NEIGHBOR_PAGE_QUANTUM).expect("positive internal quantum")
}
fn fail_invalid_traversal_data() -> CommandError {
    CommandError::new(
        CommandErrorKind::InternalError,
        "invalid traversal index data",
    )
}
fn map_traversal_error(cause: impl Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot traverse knowledge",
        cause,
    )
}

#[cfg(test)]
mod tests {
    mod bindings {
        use super::super::LinkObservations;
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
                ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError,
                SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, SourceService,
                StorageResult,
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
            let mut read = BindingRead {
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
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let sources = SourceService::new(LocalFileSystem::open(root.path()).unwrap());
            let observations = LinkObservations {
                filesystem: &filesystem,
                markdown: &MarkdownParser,
                sources: &sources,
                provenance: &provenance,
            };
            // When: the selected knowledge node observes its complete source bindings.
            let flags = observations.observe_bindings(&mut read, &node).unwrap();
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
            let mut read = BindingRead {
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
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let sources = SourceService::new(LocalFileSystem::open(root.path()).unwrap());
            let observations = LinkObservations {
                filesystem: &filesystem,
                markdown: &MarkdownParser,
                sources: &sources,
                provenance: &provenance,
            };
            // When: the selected knowledge node observes its complete source bindings.
            let flags = observations.observe_bindings(&mut read, &node).unwrap();
            // Then: bounded observations preserve flags or report the expected failure.
            assert_eq!(*events.borrow(), ["resolve", "page:0", "observe:0"]);
            assert_eq!(flags, BTreeSet::from([SearchFlag::SourceChanged]));
        }

        #[test]
        fn propagates_late_provenance_failure_without_partial_flags_or_further_pages() {
            // Given: source bindings and provenance observations have recorded call order.
            let events = Rc::new(RefCell::new(Vec::new()));
            let mut read = BindingRead {
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
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let sources = SourceService::new(LocalFileSystem::open(root.path()).unwrap());
            let observations = LinkObservations {
                filesystem: &filesystem,
                markdown: &MarkdownParser,
                sources: &sources,
                provenance: &provenance,
            };
            // When: the selected knowledge node observes its complete source bindings.
            let error = observations.observe_bindings(&mut read, &node).unwrap_err();
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
            let mut read = BindingRead {
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
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let sources = SourceService::new(LocalFileSystem::open(root.path()).unwrap());
            let observations = LinkObservations {
                filesystem: &filesystem,
                markdown: &MarkdownParser,
                sources: &sources,
                provenance: &provenance,
            };
            // When: the selected knowledge node observes its complete source bindings.
            let error = observations
                .observe_bindings(&mut read, &build_knowledge())
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
            fn get_node_ref_by_id(
                &mut self,
                id: &ElementAddress,
            ) -> StorageResult<Option<GraphNodeRef>> {
                assert_eq!(id.as_str(), "b.md");
                self.events.borrow_mut().push("resolve".into());
                Ok(Some(GraphNodeRef {
                    key: GraphNodeKey::new(1),
                    element: EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
                }))
            }
            fn get_neighbor_page(
                &mut self,
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
                    return Err(crate::service::StorageError::new(
                        crate::service::StorageErrorKind::InternalError,
                        "late binding page unavailable",
                    ));
                }
                Ok((start..self.count)
                    .take(32)
                    .map(|index| {
                        let target = EdgeTarget::Source(
                            SourceUri::parse(&format!("s{index:03}.rs")).unwrap(),
                        );
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
                CommandErrorKind, ConfigState, DocumentError, DocumentErrorKind, DocumentResult,
                DrainOutcome, ElementAddress, Embedder, EmbeddingErrorKind, EmbeddingResult,
                GraphService, KnowledgeMetadata, NodeMetadata, ProvenanceObservation,
                ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest, RefreshOutcome,
                Refresher, SearchFlag, SnapshotError, SourceMetadata, SourceObservation,
                SourceProvenance, SourceReader, SourceRevisionComparison, SourceRevisionRequest,
                SourceService, Storage, StorageRead, TraversalDirection, TraversalMode,
                TraversalRequest, Traverser,
            },
            storage::SqliteStorage,
        };
        use std::{
            cell::RefCell, collections::BTreeSet, error::Error, num::NonZeroUsize, path::Path,
            rc::Rc,
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
                events
                    .iter()
                    .filter(|x| x.as_str() == "begin_read_tx")
                    .count(),
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
                assert_eq!(&events[..3], ["refresh", "readiness", "begin_read_tx"]);
                assert_eq!(
                    events
                        .iter()
                        .filter(|x| x.as_str() == "begin_read_tx")
                        .count(),
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
                    let mut storage = open_storage(fixture.root.path());
                    let mut write = storage.begin_cache_write_tx().unwrap();
                    write
                        .insert_edge(&build_edge("b.md", "c.md", "includes"))
                        .unwrap();
                    write.commit().unwrap();
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
                    service.storage.begin_read_tx().unwrap().get_revision(),
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
                assert_eq!(service.storage.begin_read_tx().unwrap().get_revision(), 3);
            }
        }

        impl Fixture {
            fn new() -> Self {
                let root = TempDir::new().unwrap();
                std::fs::write(root.path().join("old.rs"), b"live source").unwrap();
                let mut storage = open_storage(root.path());
                let mut write = storage.begin_cache_write_tx().unwrap();
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
            ) -> TraversalService<LocalFileSystem> {
                TraversalService {
                    storage: Box::new(TracedStorage {
                        inner: open_storage(self.root.path()),
                        events: self.events.clone(),
                        after_pin,
                        fail_page: matches!(failure, Failure::Page).then_some(2),
                    }),
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
                let connection =
                    rusqlite::Connection::open(self.root.path().join("cache.db")).unwrap();
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
            fn refresh_verified(&mut self) -> DocumentResult<RefreshOutcome> {
                panic!("routine reads must not request a full audit")
            }
            fn reindex(&mut self) -> DocumentResult<RefreshOutcome> {
                panic!("this scenario must not trigger a full reindex")
            }

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
            fn get_source(
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
                self.inner.get_source(view, uri)
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
            let mut write = storage.begin_cache_write_tx().unwrap();
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
    }
    mod traced_storage {
        use crate::service::{CacheWrite, ReadTransaction, VectorWrite};
        use crate::{
            domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
            service::{
                ElementAddress, EmbeddingFailure, EmbeddingStatusPolicy, EmbeddingStatusSummary,
                EmbeddingWork, FileRow, GraphNeighbor, GraphNodeRef, GraphRead,
                NeighborPageRequest, QueryError, Storage, StorageError, StorageErrorKind,
                StorageRead, StorageResult, StoredNode, TextMatch, VectorMatch, VectorOutcome,
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
        pub(super) struct TracedRead<'a> {
            inner: Box<dyn ReadTransaction + 'a>,
            events: Events,
            fail_page: Option<usize>,
            pages: usize,
            after_pin: Option<Box<dyn FnOnce()>>,
        }
        impl Storage for TracedStorage {
            fn begin_read_tx(&mut self) -> StorageResult<Box<dyn ReadTransaction + '_>> {
                self.events.borrow_mut().push("begin_read_tx".into());
                let read = self.inner.begin_read_tx()?;
                Ok(Box::new(TracedRead {
                    inner: read,
                    events: self.events.clone(),
                    fail_page: self.fail_page,
                    pages: 0,
                    after_pin: self.after_pin.take(),
                }))
            }
            fn begin_cache_write_tx(&mut self) -> StorageResult<Box<dyn CacheWrite + '_>> {
                self.inner.begin_cache_write_tx()
            }
            fn begin_vector_write_tx(
                &mut self,
                expected: &Sha256,
            ) -> StorageResult<VectorOutcome<Box<dyn VectorWrite + '_>>> {
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
        impl<'a> GraphRead for TracedRead<'a> {
            fn get_node_ref_by_id(
                &mut self,
                id: &ElementAddress,
            ) -> StorageResult<Option<GraphNodeRef>> {
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
        impl<'a> StorageRead for TracedRead<'a> {
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
            fn get_edges_by_source_id(
                &mut self,
                _: Option<&KnowledgeId>,
            ) -> StorageResult<Vec<Edge>> {
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
                self.inner.get_next_embedding_work_by_id_and_max_attempts(
                    expected,
                    max_attempts,
                    after,
                )
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
    }
}
