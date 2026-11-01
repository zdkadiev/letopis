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

/// Refresh, pin, traverse and observe selected links under one scoped storage read.
///
/// Collaborators are independently owned; readiness cannot write through the pinned read view.
pub(crate) struct TraversalService<S, F> {
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
struct LinkObservations<'a, F> {
    filesystem: &'a F,
    markdown: &'a dyn MarkdownCodec,
    sources: &'a dyn SourceReader,
    provenance: &'a dyn SourceProvenance,
}
impl<S: Storage, F: FileSystem> Traverser for TraversalService<S, F> {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal> {
        self.refresher.refresh().map_err(map_traversal_error)?;
        self.embeddings
            .get_configuration()
            .map_err(map_traversal_error)?;
        let mut read = self.storage.begin_read_tx().map_err(map_traversal_error)?;
        let graph = self.graph.traverse(&mut read, request)?;
        let observations = LinkObservations {
            filesystem: &self.filesystem,
            markdown: self.markdown.as_ref(),
            sources: self.sources.as_ref(),
            provenance: self.provenance.as_ref(),
        };
        Self::get_links(&mut read, graph, &observations)
    }
}
impl<S: Storage, F: FileSystem> TraversalService<S, F> {
    fn get_links(
        read: &mut S::Read<'_>,
        graph: Graph,
        observations: &LinkObservations<'_, F>,
    ) -> CommandResult<Traversal> {
        let (elements, edges) = graph.into_parts();
        let elements = elements
            .into_iter()
            .map(|element| Self::get_link(read, element, observations))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(Traversal { elements, edges })
    }

    fn get_link(
        read: &mut S::Read<'_>,
        element: EdgeTarget,
        observations: &LinkObservations<'_, F>,
    ) -> CommandResult<TraversalLink> {
        match element {
            EdgeTarget::Knowledge(_) => Self::get_knowledge_link(read, &element, observations),
            EdgeTarget::Source(uri) => Self::get_source_link(read, &uri, observations.sources),
        }
    }

    fn get_knowledge_link(
        read: &mut S::Read<'_>,
        element: &EdgeTarget,
        observations: &LinkObservations<'_, F>,
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

    fn get_source_link(
        read: &mut S::Read<'_>,
        uri: &SourceUri,
        sources: &dyn SourceReader,
    ) -> CommandResult<TraversalLink> {
        let observation = sources
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
mod tests;
