use super::{
    CommandError, CommandErrorKind, CommandResult, ElementAddress, GraphNeighbor, GraphNodeKey,
    GraphRead, GraphTraversal, NEIGHBOR_PAGE_QUANTUM, NeighborCursor, NeighborDirection,
    NeighborPageRequest, StorageError, TraversalDirection, TraversalRequest,
};
use crate::domain::{Edge, EdgeTarget, Graph};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet, VecDeque},
    num::NonZeroUsize,
};

/// Incremental, metadata-free traversal over the caller's pinned graph reader.
#[derive(Debug)]
pub(crate) struct GraphService;
struct Discovery {
    queue: Vec<GraphNodeKey>,
    visited: BTreeSet<GraphNodeKey>,
    elements: Vec<EdgeTarget>,
    edges: Option<BTreeMap<Edge, usize>>,
}
struct NeighborStreams {
    outgoing: Option<NeighborStream>,
    incoming: Option<NeighborStream>,
}
struct NeighborStream {
    direction: NeighborDirection,
    buffer: VecDeque<GraphNeighbor>,
    after: Option<NeighborCursor>,
    exhausted: bool,
}
impl GraphService {
    /// Construct the stateless traversal algorithm without I/O.
    pub(crate) fn new() -> Self {
        Self
    }
}
impl GraphTraversal for GraphService {
    fn traverse(
        &self,
        read: &mut dyn GraphRead,
        request: &TraversalRequest,
    ) -> CommandResult<Graph> {
        let mut discovery = Discovery::resolve_starts(read, request)?;
        discovery.walk(read, request)?;
        Ok(discovery.into_graph())
    }
}
impl Discovery {
    fn resolve_starts(read: &mut dyn GraphRead, request: &TraversalRequest) -> CommandResult<Self> {
        if request.element_ids.is_empty() {
            return Err(CommandError::new(
                CommandErrorKind::InvalidRequest,
                "empty traversal starts",
            ));
        }
        let mut state = Self {
            queue: Vec::new(),
            visited: BTreeSet::new(),
            elements: Vec::new(),
            edges: request.edge_limit.map(|_| BTreeMap::new()),
        };
        for id in &request.element_ids {
            let node = read
                .resolve_node_by_id(id)
                .map_err(map_graph_error)?
                .ok_or_else(|| {
                    CommandError::new(CommandErrorKind::NotFound, "traversal start not found")
                })?;
            if state.visited.insert(node.key) {
                state.queue.push(node.key);
            }
        }
        Ok(state)
    }

    /// Layer boundaries carry depth without copying full nodes into the FIFO.
    fn walk(&mut self, read: &mut dyn GraphRead, request: &TraversalRequest) -> CommandResult<()> {
        let mut head = 0;
        let mut depth = 0;
        while head < self.queue.len() && depth < request.depth {
            let layer_end = self.queue.len();
            while head < layer_end {
                let node = self.queue[head];
                head += 1;
                self.expand(read, node, request)?;
                if self.elements.len() == request.limit.get() {
                    return Ok(());
                }
            }
            depth += 1;
        }
        Ok(())
    }

    fn expand(
        &mut self,
        read: &mut dyn GraphRead,
        node: GraphNodeKey,
        request: &TraversalRequest,
    ) -> CommandResult<()> {
        let mut streams = NeighborStreams::new(request.direction);
        while let Some(neighbor) = streams.get_next(read, node, request)? {
            self.consider(neighbor, request);
            if self.elements.len() == request.limit.get() {
                break;
            }
        }
        Ok(())
    }

    /// An edge is considered before discovery, including back edges and alternative paths.
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
impl NeighborStreams {
    fn new(direction: TraversalDirection) -> Self {
        Self {
            outgoing: (direction != TraversalDirection::Incoming)
                .then(|| NeighborStream::new(NeighborDirection::Outgoing)),
            incoming: (direction != TraversalDirection::Outgoing)
                .then(|| NeighborStream::new(NeighborDirection::Incoming)),
        }
    }

    /// Merge at most two bounded pages by type, reached identity and original triple.
    fn get_next(
        &mut self,
        read: &mut dyn GraphRead,
        node: GraphNodeKey,
        request: &TraversalRequest,
    ) -> CommandResult<Option<GraphNeighbor>> {
        if let Some(stream) = &mut self.outgoing {
            stream.fill(read, node, request)?;
        }
        if let Some(stream) = &mut self.incoming {
            stream.fill(read, node, request)?;
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
impl NeighborStream {
    fn new(direction: NeighborDirection) -> Self {
        Self {
            direction,
            buffer: VecDeque::new(),
            after: None,
            exhausted: false,
        }
    }

    fn fill(
        &mut self,
        read: &mut dyn GraphRead,
        node: GraphNodeKey,
        request: &TraversalRequest,
    ) -> CommandResult<()> {
        if !self.buffer.is_empty() || self.exhausted {
            return Ok(());
        }
        let limit = NonZeroUsize::new(NEIGHBOR_PAGE_QUANTUM).ok_or_else(|| {
            CommandError::new(
                CommandErrorKind::InternalError,
                "invalid neighbor page quantum",
            )
        })?;
        let page = read
            .get_neighbor_page(&NeighborPageRequest {
                node,
                direction: self.direction,
                mode: request.mode,
                edge_types: &request.edge_types,
                after: self.after.as_ref(),
                limit,
            })
            .map_err(map_graph_error)?;
        self.exhausted = page.is_empty();
        self.buffer = page.into();
        Ok(())
    }

    /// Advance only the consumed stream; retain the other stream's unconsumed records.
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
fn map_graph_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read traversal graph",
        cause,
    )
}
#[cfg(test)]
mod tests;
