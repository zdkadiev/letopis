use super::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, ElementLocation, Embedder,
    FileSystem, GraphRead, GraphTraversal, KnowledgeMetadata, MarkdownCodec, NEIGHBOR_PAGE_QUANTUM,
    NeighborCursor, NeighborDirection, NeighborPageRequest, NodeMetadata, ProvenanceRequest,
    Refresher, SearchFlag, SourceAvailability, SourceProvenance, SourceReader, Storage,
    StorageRead, Traversal, TraversalLink, TraversalMode, TraversalRequest, Traverser,
    knowledge_link::is_current_knowledge,
};
use crate::domain::{EdgeTarget, EdgeType, Graph, RepoPath, SourceUri};
use std::{collections::BTreeSet, error::Error, fmt, num::NonZeroUsize};

/// Independently owned collaborators; readiness cannot write through the pinned read view.
pub(crate) struct TraversalDependencies<S, F> {
    /// Exclusive owner of the scoped graph/metadata read capability.
    pub(crate) storage: S,
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
/// Refresh, pin, traverse and observe selected links under one scoped storage read.
pub(crate) struct TraversalService<S, F> {
    dependencies: TraversalDependencies<S, F>,
}
struct LinkObservations<'a, F> {
    filesystem: &'a F,
    markdown: &'a dyn MarkdownCodec,
    sources: &'a dyn SourceReader,
    provenance: &'a dyn SourceProvenance,
}
impl<S, F> TraversalService<S, F> {
    /// Store owned dependencies without performing I/O.
    pub(crate) fn new(dependencies: TraversalDependencies<S, F>) -> Self {
        Self { dependencies }
    }
}
impl<S: Storage, F: FileSystem> Traverser for TraversalService<S, F> {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal> {
        let dependencies = &mut self.dependencies;
        dependencies
            .refresher
            .refresh()
            .map_err(map_traversal_error)?;
        dependencies
            .embeddings
            .read_configuration()
            .map_err(map_traversal_error)?;
        let mut read = dependencies
            .storage
            .begin_read()
            .map_err(map_traversal_error)?;
        let graph = dependencies.graph.traverse(&mut read, request)?;
        let observations = LinkObservations {
            filesystem: &dependencies.filesystem,
            markdown: dependencies.markdown.as_ref(),
            sources: dependencies.sources.as_ref(),
            provenance: dependencies.provenance.as_ref(),
        };
        Self::read_links(&mut read, graph, &observations)
    }
}
impl<S: Storage, F: FileSystem> TraversalService<S, F> {
    fn read_links(
        read: &mut S::Read<'_>,
        graph: Graph,
        observations: &LinkObservations<'_, F>,
    ) -> CommandResult<Traversal> {
        let (elements, edges) = graph.into_parts();
        let elements = elements
            .into_iter()
            .map(|element| Self::read_link(read, element, observations))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(Traversal { elements, edges })
    }

    fn read_link(
        read: &mut S::Read<'_>,
        element: EdgeTarget,
        observations: &LinkObservations<'_, F>,
    ) -> CommandResult<TraversalLink> {
        match element {
            EdgeTarget::Knowledge(_) => Self::read_knowledge_link(read, &element, observations),
            EdgeTarget::Source(uri) => Self::read_source_link(read, &uri, observations.sources),
        }
    }

    fn read_knowledge_link(
        read: &mut S::Read<'_>,
        element: &EdgeTarget,
        observations: &LinkObservations<'_, F>,
    ) -> CommandResult<TraversalLink> {
        let stored = read
            .read_node(element)
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
        let mut flags = observe_bindings(read, &node, observations.provenance)?;
        if !is_current_knowledge(&node, &path, observations.filesystem, observations.markdown)
            .map_err(map_traversal_error)?
        {
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

    fn read_source_link(
        read: &mut S::Read<'_>,
        uri: &SourceUri,
        sources: &dyn SourceReader,
    ) -> CommandResult<TraversalLink> {
        let observation = sources
            .read_source(read, uri)
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
}

impl<S, F> fmt::Debug for TraversalDependencies<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraversalDependencies")
            .finish_non_exhaustive()
    }
}
impl<S, F> fmt::Debug for TraversalService<S, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TraversalService").finish_non_exhaustive()
    }
}
/// Observe each outgoing source binding once in bounded pages, regardless of traversal filters.
fn observe_bindings(
    read: &mut dyn GraphRead,
    node: &KnowledgeMetadata,
    provenance: &dyn SourceProvenance,
) -> CommandResult<BTreeSet<SearchFlag>> {
    let id = ElementAddress::parse(node.id.as_str())?;
    let handle = read
        .resolve_node(&id)
        .map_err(map_traversal_error)?
        .ok_or_else(fail_invalid_traversal_data)?;
    let types = BTreeSet::from([EdgeType::parse("external").map_err(map_traversal_error)?]);
    let mut after = None;
    let mut flags = BTreeSet::new();
    loop {
        let page = read
            .read_neighbor_page(&NeighborPageRequest {
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
                flags.extend(observe_source_page(node, &BTreeSet::new(), provenance)?);
            }
            return Ok(flags);
        }
        after = page
            .last()
            .map(|last| {
                ElementAddress::parse(last.node.element.as_str()).map(|neighbor| NeighborCursor {
                    edge_type: last.edge.get_edge_type().clone(),
                    neighbor,
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
        flags.extend(observe_source_page(node, &sources, provenance)?);
    }
}
fn observe_source_page(
    node: &KnowledgeMetadata,
    sources: &BTreeSet<SourceUri>,
    provenance: &dyn SourceProvenance,
) -> CommandResult<BTreeSet<SearchFlag>> {
    Ok(provenance
        .observe_sources(ProvenanceRequest {
            source_snapshot: node.source_snapshot.as_ref(),
            sources,
        })
        .map_err(map_traversal_error)?
        .flags)
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
    use super::observe_bindings;
    use crate::{
        domain::{
            Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, Sha256, SourceUri,
            TreeOid,
        },
        service::{
            DocumentError, DocumentErrorKind, DocumentResult, ElementAddress, GraphNeighbor,
            GraphNodeKey, GraphNodeRef, GraphRead, KnowledgeMetadata, NeighborPageRequest,
            ProvenanceObservation, ProvenanceRequest, SearchFlag, SnapshotError, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest, StorageResult,
        },
    };
    use std::{cell::RefCell, collections::BTreeSet, rc::Rc};

    #[test]
    fn observes_each_source_page_before_reading_the_next_and_unions_flags() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut read = BindingRead {
            events: events.clone(),
            count: 33,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: None,
        };
        let node = build_knowledge();
        let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
        assert_eq!(
            *events.borrow(),
            [
                "resolve",
                "page:0",
                "observe:32",
                "page:32",
                "observe:1",
                "page:33"
            ]
        );
        assert_eq!(
            flags,
            BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
        );
    }

    #[test]
    fn observes_empty_bindings_once_to_preserve_basis_validation() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut read = BindingRead {
            events: events.clone(),
            count: 0,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: None,
        };
        let node = build_knowledge();
        let flags = observe_bindings(&mut read, &node, &provenance).unwrap();
        assert_eq!(*events.borrow(), ["resolve", "page:0", "observe:0"]);
        assert_eq!(flags, BTreeSet::from([SearchFlag::SourceChanged]));
    }

    #[test]
    fn propagates_late_provenance_failure_without_partial_flags_or_further_pages() {
        let events = Rc::new(RefCell::new(Vec::new()));
        let mut read = BindingRead {
            events: events.clone(),
            count: 70,
        };
        let provenance = BindingProvenance {
            events: events.clone(),
            fail_on: Some("s032.rs"),
        };
        let node = build_knowledge();
        let error = observe_bindings(&mut read, &node, &provenance).unwrap_err();
        assert_eq!(
            error.get_kind(),
            crate::service::CommandErrorKind::InternalError
        );
        assert_eq!(
            *events.borrow(),
            ["resolve", "page:0", "observe:32", "page:32", "observe:32"]
        );
    }

    struct BindingRead {
        events: Rc<RefCell<Vec<String>>>,
        count: usize,
    }
    struct BindingProvenance {
        events: Rc<RefCell<Vec<String>>>,
        fail_on: Option<&'static str>,
    }
    impl GraphRead for BindingRead {
        fn resolve_node(&mut self, id: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
            assert_eq!(id.as_str(), "b.md");
            self.events.borrow_mut().push("resolve".into());
            Ok(Some(GraphNodeRef {
                key: GraphNodeKey::new(1),
                element: EdgeTarget::Knowledge(KnowledgeId::parse("b.md").unwrap()),
            }))
        }
        fn read_neighbor_page(
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
