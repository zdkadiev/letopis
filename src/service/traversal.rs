use super::utils::map_document_error_to_command_error;
use super::utils::map_file_result_to_document_result;
use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandResult, DocumentError, ElementAddress,
    ElementLocation, Embedder, FileReader, GraphNeighbor, GraphNodeKey, GraphRead, GraphWalk,
    KnowledgeMetadata, MarkdownCodec, NEIGHBOR_PAGE_QUANTUM, NeighborCursor, NeighborDirection,
    NeighborPageRequest, NodeMetadata, ProvenanceRequest, Refresher, SearchFlag,
    SourceAvailability, SourceProvenance, SourceReader, StorageRead, TransactionError, Traversal,
    TraversalLink, TraversalMode, TraversalRequest, Traverser, utils::is_current_knowledge,
};
use crate::domain::{EdgeTarget, EdgeType, Graph, RepoPath, SourceUri};
use std::path::Path;
use std::{collections::BTreeSet, fmt, num::NonZeroUsize, rc::Rc};

/// Refresh, pin, traverse and observe selected links under one scoped storage read.
///
/// Collaborators are independently owned; readiness cannot write through the pinned read view.
pub(crate) struct TraversalService<F, C> {
    /// Exclusive owner of the scoped graph/metadata read capability.
    pub(crate) cache: Rc<C>,
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
    pub(crate) graph: Box<dyn GraphWalk>,
}

struct LinkObservations<'a, F, C> {
    storage_read: &'a C,
    filesystem: &'a F,
    markdown: &'a dyn MarkdownCodec,
    sources: &'a dyn SourceReader,
    provenance: &'a dyn SourceProvenance,
}

impl<F: FileReader, C: CacheDatabase> Traverser for TraversalService<F, C> {
    fn traverse(&mut self, request: &TraversalRequest) -> CommandResult<Traversal> {
        self.refresher
            .refresh(&[])
            .map_err(map_document_error_to_traversal_command_error)?;
        self.embeddings
            .get_configuration()
            .map_err(map_error_to_traversal_command_error)?;
        self.cache
            .with_read_tx(|| {
                let graph = self.graph.traverse(request)?;
                let observations = LinkObservations {
                    storage_read: self.cache.as_ref(),
                    filesystem: &self.filesystem,
                    markdown: self.markdown.as_ref(),
                    sources: self.sources.as_ref(),
                    provenance: self.provenance.as_ref(),
                };
                observations.get_traversal(graph)
            })
            .map_err(map_transaction_error_to_traversal_command_error)
    }
}

impl<F: FileReader, C: StorageRead + GraphRead> LinkObservations<'_, F, C> {
    fn get_traversal(&self, graph: Graph) -> CommandResult<Traversal> {
        let (elements, edges) = graph.into_parts();
        let elements = elements
            .into_iter()
            .map(|element| self.get_link_by_element(element))
            .collect::<CommandResult<Vec<_>>>()?;
        Ok(Traversal { elements, edges })
    }

    fn get_link_by_element(&self, element: EdgeTarget) -> CommandResult<TraversalLink> {
        match element {
            EdgeTarget::Knowledge(_) => self.get_knowledge_link_by_element(&element),
            EdgeTarget::Source(uri) => self.get_source_link_by_uri(&uri),
        }
    }

    fn get_knowledge_link_by_element(&self, element: &EdgeTarget) -> CommandResult<TraversalLink> {
        let node = self.get_knowledge_node_by_element(element)?;
        let path = RepoPath::parse(node.id.as_str().split('#').next().unwrap_or_default())
            .map_err(map_error_to_traversal_command_error)?;
        let mut flags = self.observe_bindings(&node)?;
        let file = map_file_result_to_document_result(
            self.filesystem.file_get_by_path(Path::new(path.as_str())),
        )
        .map_err(map_error_to_traversal_command_error)?;
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
                .storage_read
                .project_get_by_id()
                .map_err(map_error_to_traversal_command_error)?,
            flags,
        })
    }

    /// Reads the stored knowledge metadata for one identity.
    ///
    /// # Arguments
    ///
    /// * `element` — Element identity whose stored row must match the address and its own URI.
    ///
    /// # Errors
    /// Reports storage failures and rows that do not describe the requested element.
    fn get_knowledge_node_by_element(
        &self,
        element: &EdgeTarget,
    ) -> CommandResult<KnowledgeMetadata> {
        let stored = StorageRead::nodes_get_by_uri(self.storage_read, element)
            .map_err(map_error_to_traversal_command_error)?;
        let Some(stored) = stored else {
            return Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid traversal index data",
            ));
        };
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
        Ok(node)
    }

    fn get_source_link_by_uri(&self, uri: &SourceUri) -> CommandResult<TraversalLink> {
        let observation = self
            .sources
            .get_source_by_uri(uri)
            .map_err(map_error_to_traversal_command_error)?
            .ok_or_else(build_invalid_traversal_index_error)?;
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
                .storage_read
                .project_get_by_id()
                .map_err(map_error_to_traversal_command_error)?,
            flags,
        })
    }
}

impl<F: FileReader, C: GraphRead> LinkObservations<'_, F, C> {
    /// Observes outgoing source bindings in bounded pages.
    ///
    /// Traversal filters do not apply.
    ///
    /// # Arguments
    ///
    /// * `node` — Knowledge whose outgoing source bindings are observed.
    fn observe_bindings(&self, node: &KnowledgeMetadata) -> CommandResult<BTreeSet<SearchFlag>> {
        let id = ElementAddress::parse(node.id.as_str())?;
        let handle = self
            .storage_read
            .nodes_get_by_uri(&id)
            .map_err(map_error_to_traversal_command_error)?
            .ok_or_else(build_invalid_traversal_index_error)?;
        let types = BTreeSet::from([
            EdgeType::parse("external").map_err(map_error_to_traversal_command_error)?
        ]);
        let mut after = None;
        let mut flags = BTreeSet::new();
        loop {
            let page = self.get_outgoing_page_by_key_and_types_and_after(
                handle.key,
                &types,
                after.as_ref(),
            )?;
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
            let sources = get_page_sources(page)?;
            flags.extend(self.observe_source_page(node, &sources)?);
        }
    }

    /// Reads one page of outgoing relationship neighbors.
    ///
    /// # Arguments
    ///
    /// * `key` — Backend handle of the node whose outgoing edges are read.
    /// * `types` — Relationship types requested by this traversal.
    /// * `after` — Exclusive cursor of the last consumed edge, or `None` for the first page.
    ///
    /// # Errors
    /// Reports storage failures from the shared read scope.
    fn get_outgoing_page_by_key_and_types_and_after(
        &self,
        key: GraphNodeKey,
        types: &BTreeSet<EdgeType>,
        after: Option<&NeighborCursor>,
    ) -> CommandResult<Vec<GraphNeighbor>> {
        self.storage_read
            .edges_get_by_row_id_and_type(&NeighborPageRequest {
                node: key,
                direction: NeighborDirection::Outgoing,
                mode: TraversalMode::Relations,
                edge_types: types,
                after,
                limit: get_page_limit(),
            })
            .map_err(map_error_to_traversal_command_error)
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
            .map_err(map_error_to_traversal_command_error)?
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

/// Collects source addresses from one relation page.
///
/// Rejects knowledge neighbors.
///
/// # Arguments
///
/// * `page` — Relation page whose neighbors must all be external sources.
///
/// # Errors
/// Reports an internal error when a knowledge neighbor appears in a relations page.
fn get_page_sources(page: Vec<GraphNeighbor>) -> CommandResult<BTreeSet<SourceUri>> {
    page.into_iter()
        .map(|item| match item.node.element {
            EdgeTarget::Source(source) => Ok(source),
            EdgeTarget::Knowledge(_) => Err(CommandError::new(
                CommandErrorKind::InternalError,
                "invalid traversal index data",
            )),
        })
        .collect()
}

/// Converts a failure raised while traversing knowledge to a command error.
///
/// # Arguments
///
/// * `cause` — Failure from a traversal step.
fn map_error_to_traversal_command_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot traverse knowledge",
        cause,
    )
}

/// Converts a document failure to a traversal command error.
///
/// # Arguments
///
/// * `cause` — Document failure from a traversal step.
fn map_document_error_to_traversal_command_error(cause: DocumentError) -> CommandError {
    map_document_error_to_command_error(cause, "cannot traverse knowledge")
}

/// Converts a transaction failure from the traversal read to a command error.
///
/// # Arguments
///
/// * `error` — Transaction failure from the pinned read.
fn map_transaction_error_to_traversal_command_error(
    error: TransactionError<CommandError>,
) -> CommandError {
    match error {
        TransactionError::Operation(error) => error,
        TransactionError::Storage(error) => map_error_to_traversal_command_error(error),
    }
}

/// Builds the error reported for inconsistent traversal index data.
fn build_invalid_traversal_index_error() -> CommandError {
    CommandError::new(
        CommandErrorKind::InternalError,
        "invalid traversal index data",
    )
}

#[cfg(test)]
#[path = "traversal_test.rs"]
mod tests;
