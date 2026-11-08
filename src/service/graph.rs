use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandResult, ElementAddress, GraphNeighbor,
    GraphNodeKey, GraphRead, GraphWalk, NEIGHBOR_PAGE_QUANTUM, NeighborCursor, NeighborDirection,
    NeighborPageRequest, TransactionError, TraversalDirection, TraversalRequest,
};
use crate::domain::{Edge, EdgeTarget, Graph};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroUsize,
};

/// Walks the indexed graph through the shared cache scope.
#[derive(Debug)]
pub(crate) struct GraphService<C> {
    cache: std::rc::Rc<C>,
}

struct Discovery<'a> {
    graph_read: &'a dyn GraphRead,
    queue: Vec<GraphNodeKey>,
    visited: BTreeSet<GraphNodeKey>,
    elements: Vec<EdgeTarget>,
    edges: Option<BTreeMap<Edge, usize>>,
}

struct NeighborStreams<'a> {
    outgoing: Option<NeighborStream<'a>>,
    incoming: Option<NeighborStream<'a>>,
}

struct NeighborStream<'a> {
    graph_read: &'a dyn GraphRead,
    direction: NeighborDirection,
    buffer: VecDeque<GraphNeighbor>,
    after: Option<NeighborCursor>,
    is_exhausted: bool,
}

impl<C> GraphService<C> {
    /// Creates the service without performing I/O.
    ///
    /// # Arguments
    ///
    /// * `cache` — Shared cache that supplies the traversal's read scopes.
    pub(crate) fn new(cache: std::rc::Rc<C>) -> Self {
        Self { cache }
    }
}

impl<C: CacheDatabase> GraphWalk for GraphService<C> {
    fn traverse(&self, request: &TraversalRequest) -> CommandResult<Graph> {
        self.cache
            .with_read_tx(|| {
                let mut discovery = Discovery::resolve_starts(self.cache.as_ref(), request)?;
                discovery.walk(request)?;
                Ok(discovery.into_graph())
            })
            .map_err(map_transaction_error_to_traversal_graph_command_error)
    }
}

impl<'a> Discovery<'a> {
    fn resolve_starts(
        graph_read: &'a dyn GraphRead,
        request: &TraversalRequest,
    ) -> CommandResult<Self> {
        if request.element_ids.is_empty() {
            return Err(CommandError::new(
                CommandErrorKind::InvalidRequest,
                "empty traversal starts",
            ));
        }
        let mut state = Self {
            graph_read,
            queue: Vec::new(),
            visited: BTreeSet::new(),
            elements: Vec::new(),
            edges: request.edge_limit.map(|_| BTreeMap::new()),
        };
        for id in &request.element_ids {
            let node = state
                .graph_read
                .nodes_get_by_uri(id)
                .map_err(map_error_to_traversal_graph_command_error)?
                .ok_or_else(build_traversal_start_not_found_error)?;
            if state.visited.insert(node.key) {
                state.queue.push(node.key);
            }
        }
        Ok(state)
    }

    /// Walks the graph one depth layer at a time until a bound is reached.
    ///
    /// Layer boundaries carry depth without copying full nodes into the FIFO.
    ///
    /// # Arguments
    ///
    /// * `request` — Traversal bounds selecting depth, element limit, direction and edge filters.
    fn walk(&mut self, request: &TraversalRequest) -> CommandResult<()> {
        let mut head = 0;
        let mut depth = 0;
        while head < self.queue.len() && depth < request.depth {
            let layer_end = self.queue.len();
            while head < layer_end {
                let node = self.queue[head];
                head += 1;
                self.expand(node, request)?;
                if self.elements.len() == request.limit.get() {
                    return Ok(());
                }
            }
            depth += 1;
        }
        Ok(())
    }

    fn expand(&mut self, node: GraphNodeKey, request: &TraversalRequest) -> CommandResult<()> {
        let mut streams = NeighborStreams::new(self.graph_read, request.direction);
        while let Some(neighbor) = streams.get_next_neighbor(node, request)? {
            self.consider(neighbor, request);
            if self.elements.len() == request.limit.get() {
                break;
            }
        }
        Ok(())
    }

    /// Records each edge before discovering its node.
    ///
    /// Back edges and alternative paths are recorded too.
    ///
    /// # Arguments
    ///
    /// * `neighbor` — Edge and node discovered from the current element.
    /// * `request` — Traversal bounds supplying the optional edge limit.
    fn consider(&mut self, neighbor: GraphNeighbor, request: &TraversalRequest) {
        if let (Some(edges), Some(limit)) = (&mut self.edges, request.edge_limit) {
            let ordinal = edges.len();
            if ordinal < limit.get() {
                edges.entry(neighbor.edge).or_insert(ordinal);
            }
        }
        if self.visited.insert(neighbor.node.key) {
            self.queue.push(neighbor.node.key);
            self.elements.push(neighbor.node.element);
        }
    }

    fn into_graph(self) -> Graph {
        let edges = self.edges.map(|edges| {
            let mut ordered: Vec<_> = edges.into_iter().collect();
            ordered.sort_by_key(|(_, ordinal)| *ordinal);
            ordered.into_iter().map(|(edge, _)| edge).collect()
        });
        Graph::new(self.elements, edges)
    }
}

impl<'a> NeighborStreams<'a> {
    fn new(graph_read: &'a dyn GraphRead, direction: TraversalDirection) -> Self {
        Self {
            outgoing: (direction != TraversalDirection::Incoming)
                .then(|| NeighborStream::new(graph_read, NeighborDirection::Outgoing)),
            incoming: (direction != TraversalDirection::Outgoing)
                .then(|| NeighborStream::new(graph_read, NeighborDirection::Incoming)),
        }
    }

    /// Returns the next neighbor from at most two ordered streams.
    ///
    /// Neighbors are ordered by type, reached identity and original edge.
    ///
    /// # Arguments
    ///
    /// * `node` — Node whose outgoing and incoming neighbors are merged.
    /// * `request` — Traversal mode, edge filters and bounds applied to both direction streams.
    fn get_next_neighbor(
        &mut self,
        node: GraphNodeKey,
        request: &TraversalRequest,
    ) -> CommandResult<Option<GraphNeighbor>> {
        if let Some(stream) = &mut self.outgoing {
            stream.fill(node, request)?;
        }
        if let Some(stream) = &mut self.incoming {
            stream.fill(node, request)?;
        }
        let outgoing = self
            .outgoing
            .as_ref()
            .and_then(|stream| stream.buffer.front());
        let incoming = self
            .incoming
            .as_ref()
            .and_then(|stream| stream.buffer.front());
        let take_outgoing = match (outgoing, incoming) {
            (Some(left), Some(right)) => compare_neighbors(left, right).is_le(),
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => return Ok(None),
        };
        let stream = if take_outgoing {
            &mut self.outgoing
        } else {
            &mut self.incoming
        };
        match stream {
            Some(stream) => stream.take_next(),
            None => Ok(None),
        }
    }
}

impl<'a> NeighborStream<'a> {
    fn new(graph_read: &'a dyn GraphRead, direction: NeighborDirection) -> Self {
        Self {
            graph_read,
            direction,
            buffer: VecDeque::new(),
            after: None,
            is_exhausted: false,
        }
    }

    fn fill(&mut self, node: GraphNodeKey, request: &TraversalRequest) -> CommandResult<()> {
        if !self.buffer.is_empty() || self.is_exhausted {
            return Ok(());
        }
        let limit = NonZeroUsize::new(NEIGHBOR_PAGE_QUANTUM)
            .ok_or_else(build_invalid_neighbor_quantum_error)?;
        let page = self
            .graph_read
            .edges_get_by_row_id_and_type(&NeighborPageRequest {
                node,
                direction: self.direction,
                mode: request.mode,
                edge_types: &request.edge_types,
                after: self.after.as_ref(),
                limit,
            })
            .map_err(map_error_to_traversal_graph_command_error)?;
        self.is_exhausted = page.is_empty();
        self.buffer = page.into();
        Ok(())
    }

    /// Removes the next buffered neighbor.
    ///
    /// The stream cursor advances for the returned neighbor; the other stream's unconsumed
    /// records are kept.
    fn take_next(&mut self) -> CommandResult<Option<GraphNeighbor>> {
        let Some(neighbor) = self.buffer.pop_front() else {
            return Ok(None);
        };
        self.after = Some(NeighborCursor {
            edge_type: neighbor.edge.get_edge_type().clone(),
            neighbor: ElementAddress::parse(neighbor.node.element.as_str())?,
        });
        Ok(Some(neighbor))
    }
}

fn compare_neighbors(left: &GraphNeighbor, right: &GraphNeighbor) -> Ordering {
    (
        left.edge.get_edge_type(),
        left.node.element.as_str(),
        &left.edge,
    )
        .cmp(&(
            right.edge.get_edge_type(),
            right.node.element.as_str(),
            &right.edge,
        ))
}

/// Converts a failure raised while reading the traversal graph to a command error.
///
/// # Arguments
///
/// * `cause` — Failure from a graph read.
fn map_error_to_traversal_graph_command_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read traversal graph",
        cause,
    )
}

/// Converts a transaction failure from the graph read to a command error.
///
/// # Arguments
///
/// * `error` — Transaction failure from the pinned read.
fn map_transaction_error_to_traversal_graph_command_error(
    error: TransactionError<CommandError>,
) -> CommandError {
    match error {
        TransactionError::Operation(error) => error,
        TransactionError::Storage(error) => map_error_to_traversal_graph_command_error(error),
    }
}

/// Builds the error reported when a traversal start is absent.
fn build_traversal_start_not_found_error() -> CommandError {
    CommandError::new(CommandErrorKind::NotFound, "traversal start not found")
}

/// Builds the error reported for an invalid neighbor page quantum.
fn build_invalid_neighbor_quantum_error() -> CommandError {
    CommandError::new(
        CommandErrorKind::InternalError,
        "invalid neighbor page quantum",
    )
}

#[cfg(test)]
#[path = "graph_test.rs"]
mod tests;
