use super::utils::map_document_error;
use super::utils::map_document_file;
use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandResult, ElementAddress, ElementLocation,
    Embedder, FileReader, GraphRead, GraphTraversal, KnowledgeMetadata, MarkdownCodec,
    NEIGHBOR_PAGE_QUANTUM, NeighborCursor, NeighborDirection, NeighborPageRequest, NodeMetadata,
    ProvenanceRequest, Refresher, SearchFlag, SourceAvailability, SourceProvenance, SourceReader,
    StorageRead, Traversal, TraversalLink, TraversalMode, TraversalRequest, Traverser,
    utils::is_current_knowledge,
};
use crate::domain::{EdgeTarget, EdgeType, Graph, RepoPath, SourceUri};
use std::path::Path;
use std::{collections::BTreeSet, error::Error, fmt, num::NonZeroUsize, rc::Rc};

/// Refresh, pin, traverse and observe selected links under one scoped storage read.
///
/// Collaborators are independently owned; readiness cannot write through the pinned read view.
pub(crate) struct TraversalService<F, C> {
    /// Exclusive owner of the scoped graph/metadata read capability.
    pub(crate) storage: Rc<C>,
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
struct LinkObservations<'a, F, C> {
    storage: &'a C,
    filesystem: &'a F,
    markdown: &'a dyn MarkdownCodec,
    sources: &'a dyn SourceReader,
    provenance: &'a dyn SourceProvenance,
}
impl<F: FileReader, C: CacheDatabase> Traverser for TraversalService<F, C> {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal> {
        self.refresher
            .refresh(&[])
            .map_err(|cause| map_document_error(cause, "cannot traverse knowledge"))?;
        self.embeddings
            .get_configuration()
            .map_err(map_traversal_error)?;
        self.storage
            .with_read_tx(|| {
                let graph = self.graph.traverse(request)?;
                let observations = LinkObservations {
                    storage: self.storage.as_ref(),
                    filesystem: &self.filesystem,
                    markdown: self.markdown.as_ref(),
                    sources: self.sources.as_ref(),
                    provenance: self.provenance.as_ref(),
                };
                observations.get_links(graph)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => map_traversal_error(error),
            })
    }
}
impl<F: FileReader, C: StorageRead + GraphRead> LinkObservations<'_, F, C> {
    fn get_links(&self, graph: Graph) -> CommandResult<Traversal> {
        let (elements, edges) = graph.into_parts();
        let elements = elements
            .into_iter()
            .map(|element| self.get_link(element))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(Traversal { elements, edges })
    }

    fn get_link(&self, element: EdgeTarget) -> CommandResult<TraversalLink> {
        match element {
            EdgeTarget::Knowledge(_) => self.get_knowledge_link(&element),
            EdgeTarget::Source(uri) => self.get_source_link(&uri),
        }
    }

    fn get_knowledge_link(&self, element: &EdgeTarget) -> CommandResult<TraversalLink> {
        let stored = StorageRead::nodes_get_by_id(self.storage, element)
            .map_err(map_traversal_error)?
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InternalError,
                    "invalid traversal index data",
                )
            })?;
        let NodeMetadata::Knowledge(node) = stored.metadata else {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid traversal index data",
            ));
        };
        if node.id.as_str() != element.as_str() || node.uri != node.id {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid traversal index data",
            ));
        }
        let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
            .map_err(map_traversal_error)?;
        let mut flags = self.observe_bindings(&node)?;
        let file = map_document_file(self.filesystem.file_get_by_path(Path::new(path.as_str())))
            .map_err(map_traversal_error)?;
        let parsed = file
            .as_ref()
            .and_then(|file| self.markdown.file_parse(&path, &file.bytes).ok().flatten());
        if !is_current_knowledge(&node, parsed.as_ref()) {
            flags.insert(SearchFlag::Stale);
        }
        Ok(TraversalLink {
            element_id: ElementAddress::parse(node.id.as_str())?,
            path: ElementLocation::Local(path),
            lines: Some(node.lines),
            revision: self
                .storage
                .project_get_by_id()
                .map_err(map_traversal_error)?,
            flags,
        })
    }

    fn get_source_link(&self, uri: &SourceUri) -> CommandResult<TraversalLink> {
        let observation = self
            .sources
            .get_source(uri)
            .map_err(map_traversal_error)?
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InternalError,
                    "invalid traversal index data",
                )
            })?;
        if observation.metadata.uri != *uri {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid traversal index data",
            ));
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
            revision: self
                .storage
                .project_get_by_id()
                .map_err(map_traversal_error)?,
            flags,
        })
    }
}
impl<F: FileReader, C: GraphRead> LinkObservations<'_, F, C> {
    /// Observe outgoing source bindings in bounded pages independently of traversal filters.
    fn observe_bindings(&self, node: &KnowledgeMetadata) -> CommandResult<BTreeSet<SearchFlag>> {
        let id = ElementAddress::parse(node.id.as_str())?;
        let handle = self
            .storage
            .nodes_get_by_id(&id)
            .map_err(map_traversal_error)?
            .ok_or_else(|| {
                CommandError::new(
                    CommandErrorKind::InternalError,
                    "invalid traversal index data",
                )
            })?;
        let types = BTreeSet::from([EdgeType::parse("external").map_err(map_traversal_error)?]);
        let mut after = None;
        let mut flags = BTreeSet::new();
        loop {
            let page = self
                .storage
                .edges_get_by_row_id_and_type(&NeighborPageRequest {
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
                    EdgeTarget::Knowledge(_) => Err(CommandError::new(
                        CommandErrorKind::InternalError,
                        "invalid traversal index data",
                    )),
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

impl<F, C> fmt::Debug for TraversalService<F, C> {
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
                storage: &read,
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
                storage: &read,
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
                storage: &read,
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
                storage: &read,
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
            fn nodes_get_by_id(&self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
                assert_eq!(id.as_str(), "b.md");
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
                EmbeddingResult, GraphService, KnowledgeMetadata, NodeMetadata,
                ProvenanceObservation, ProvenanceRequest, QueryEmbedding, QueryEmbeddingRequest,
                RefreshOutcome, Refresher, SearchFlag, SnapshotError, SourceMetadata,
                SourceObservation, SourceProvenance, SourceReader, SourceRevisionComparison,
                SourceRevisionRequest, SourceService, StorageRead, TraversalDirection,
                TraversalMode, TraversalRequest, Traverser,
            },
            storage::TestDatabases,
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
                        .cache
                        .with_write_tx(|| {
                            let write = &storage.cache;
                            write
                                .edges_save_by_source_id_and_target_id_and_type(&build_edge(
                                    "b.md", "c.md", "includes",
                                ))
                                .unwrap();

                            write.get_publication_revision()
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
                        .storage
                        .with_read_tx(|| service.storage.project_get_by_id())
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
                        .storage
                        .with_read_tx(|| service.storage.project_get_by_id())
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
                    .cache
                    .with_write_tx(|| {
                        let write = &storage.cache;
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

                        write.get_publication_revision()
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
                    inner: open_storage(self.root.path()).cache,
                    events: self.events.clone(),
                    after_pin: RefCell::new(after_pin),
                    pages: std::cell::Cell::new(0),
                    fail_page: matches!(failure, Failure::Page).then_some(2),
                });
                TraversalService {
                    storage: Rc::clone(&storage),
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
                .cache
                .with_write_tx(|| {
                    let write = &storage.cache;
                    write
                        .edges_delete_by_source_id(&KnowledgeId::parse("a.md").unwrap())
                        .unwrap();
                    write
                        .edges_delete_by_source_id(&KnowledgeId::parse("b.md").unwrap())
                        .unwrap();
                    let NodeMetadata::Knowledge(mut replacement) = build_knowledge("b.md", S2)
                    else {
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

                    write.get_publication_revision()
                })
                .unwrap();
        }
        fn open_storage(root: &Path) -> TestDatabases {
            crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db"))
                .unwrap()
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
                ElementAddress, EmbeddingFailure, EmbeddingWork, FileRow, GraphNeighbor,
                GraphNodeRef, GraphRead, NeighborPageRequest, QueryError, StorageError,
                StorageErrorKind, StorageRead, StorageResult, StoredNode, TextMatch, VectorMatch,
                VectorOutcome,
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
        use crate::service::utils::cache_test_support::delegate_cache_writes;
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
            fn nodes_get_by_id(&self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
                self.events
                    .borrow_mut()
                    .push(format!("resolve:{}", id.as_str()));
                GraphRead::nodes_get_by_id(&self.inner, id)
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
            fn get_vector_coverage(&self) -> StorageResult<u64> {
                self.inner.get_vector_coverage()
            }
            fn embed_failures_get_by_kind_and_search_hash_and_attempts(
                &self,
                max_attempts: u64,
            ) -> StorageResult<Vec<EmbeddingFailure>> {
                self.inner
                    .embed_failures_get_by_kind_and_search_hash_and_attempts(max_attempts)
            }

            fn nodes_get_by_id(&self, id: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
                self.events
                    .borrow_mut()
                    .push(format!("metadata:{}", id.as_str()));
                StorageRead::nodes_get_by_id(&self.inner, id)
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
            fn text_index_get_by_expression(&self, _: &str) -> StorageResult<Vec<TextMatch>> {
                panic!("query text")
            }
            fn nodes_get_by_kind_and_search_hash_and_attempts_and_id(
                &self,
                expected: &Sha256,
                max_attempts: u64,
                after: Option<&KnowledgeId>,
            ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
                self.inner
                    .nodes_get_by_kind_and_search_hash_and_attempts_and_id(
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
}
