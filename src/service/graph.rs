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
                .get_node_ref_by_id(id)
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
mod tests {
    use super::GraphService;
    use crate::{
        domain::{Edge, EdgeTarget, EdgeType, Graph, KnowledgeId, SourceUri},
        service::{
            CommandErrorKind, ElementAddress, GraphNeighbor, GraphNodeKey, GraphNodeRef, GraphRead,
            GraphTraversal, NeighborDirection, NeighborPageRequest, StorageResult,
            TraversalDirection, TraversalMode, TraversalRequest,
        },
    };
    use std::{collections::BTreeSet, num::NonZeroUsize};

    #[test]
    fn traverses_each_mode_and_direction_in_bfs_order() {
        // Given
        let cases = [
            (
                TraversalMode::Relations,
                TraversalDirection::Outgoing,
                vec!["code.rs", "c.md", "d.md"],
            ),
            (
                TraversalMode::Relations,
                TraversalDirection::Incoming,
                vec!["d.md", "b.md", "c.md"],
            ),
            (
                TraversalMode::Relations,
                TraversalDirection::Both,
                vec!["code.rs", "c.md", "d.md", "b.md"],
            ),
            (
                TraversalMode::Containment,
                TraversalDirection::Outgoing,
                vec!["b.md", "c.md"],
            ),
            (
                TraversalMode::Containment,
                TraversalDirection::Incoming,
                vec![],
            ),
            (
                TraversalMode::Containment,
                TraversalDirection::Both,
                vec!["b.md", "c.md"],
            ),
            (
                TraversalMode::Both,
                TraversalDirection::Outgoing,
                vec!["code.rs", "b.md", "c.md", "d.md"],
            ),
            (
                TraversalMode::Both,
                TraversalDirection::Incoming,
                vec!["d.md", "b.md", "c.md"],
            ),
            (
                TraversalMode::Both,
                TraversalDirection::Both,
                vec!["code.rs", "b.md", "c.md", "d.md"],
            ),
        ];
        for (mode, direction, expected) in cases {
            let mut read = GeneratedRead::build_fixed();
            let mut request = make_request();
            request.mode = mode;
            request.direction = direction;
            // When
            let graph = GraphService::new().traverse(&mut read, &request).unwrap();
            // Then
            assert_eq!(get_ids(&graph), expected, "{mode:?}/{direction:?}");
        }
    }

    #[test]
    fn keeps_edge_budget_independent_from_node_budget() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.edge_limit = NonZeroUsize::new(1);
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(
            graph
                .get_elements()
                .iter()
                .map(EdgeTarget::as_str)
                .collect::<Vec<_>>(),
            ["code.rs", "b.md", "c.md", "d.md"]
        );
        assert_eq!(graph.get_edges().unwrap().len(), 1);
        assert_eq!(
            graph.get_edges().unwrap()[0].get_edge_type().as_str(),
            "external"
        );
    }

    #[test]
    fn includes_consumes_one_depth_step() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.mode = TraversalMode::Containment;
        request.depth = 1;
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), ["b.md"]);
    }

    #[test]
    fn validates_all_starts_even_at_depth_zero() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        read.expected_roots = 2;
        let mut request = make_request();
        request.element_ids.insert(parse_address("unknown.md"));
        request.depth = 0;
        // When
        let error = GraphService::new()
            .traverse(&mut read, &request)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
        assert_eq!(read.resolved, ["a.md", "unknown.md"]);
        assert_eq!(read.page_calls, 0);
    }

    #[test]
    fn returns_requested_empty_edges_after_validating_depth_zero() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.depth = 0;
        request.edge_limit = NonZeroUsize::new(5);
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert!(graph.get_elements().is_empty());
        assert_eq!(graph.get_edges(), Some([].as_slice()));
        assert_eq!(read.resolved, ["a.md"]);
        assert_eq!(read.page_calls, 0);
    }

    #[test]
    fn stops_buffered_transitions_and_queries_at_node_cap() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.limit = NonZeroUsize::new(2).unwrap();
        request.edge_limit = NonZeroUsize::new(20);
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), ["code.rs", "b.md"]);
        let triples = get_triples(&graph);
        assert_eq!(
            triples,
            [
                ("a.md", "code.rs", "external"),
                ("a.md", "b.md", "includes")
            ]
        );
        assert_eq!(read.page_calls, 1);
    }

    #[test]
    fn previsits_multiple_starts_and_deduplicates_cycles_at_max_depth() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        read.expected_roots = 2;
        let mut request = make_request();
        request.element_ids.insert(parse_address("c.md"));
        request.depth = u64::MAX;
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), ["code.rs", "b.md", "d.md"]);
        assert_eq!(read.resolved, ["a.md", "c.md"]);
        assert_eq!(read.expanded.iter().filter(|key| **key == 1).count(), 1);
    }

    #[test]
    fn terminates_includes_cycle_without_returning_start_twice() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        read.includes_cycle = true;
        let mut request = make_request();
        request.mode = TraversalMode::Containment;
        request.depth = u64::MAX;
        request.limit = NonZeroUsize::MAX;
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), ["b.md", "c.md"]);
    }

    #[test]
    fn preserves_original_incoming_edges_and_considered_back_edges() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.direction = TraversalDirection::Incoming;
        request.edge_limit = NonZeroUsize::new(20);
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(
            get_triples(&graph),
            [
                ("d.md", "a.md", "z_custom_link"),
                ("b.md", "d.md", "z_custom_link"),
                ("c.md", "d.md", "z_custom_link")
            ]
        );
        // Given
        let mut read = GeneratedRead::build_fixed();
        request.direction = TraversalDirection::Outgoing;
        request.depth = u64::MAX;
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(graph.get_edges().unwrap().len(), 7);
        assert!(get_triples(&graph).contains(&("d.md", "a.md", "z_custom_link")));
    }

    #[test]
    fn intersects_open_types_and_omits_edges_when_not_requested() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request
            .edge_types
            .insert(EdgeType::parse("z_custom_link").unwrap());
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), ["c.md", "d.md"]);
        assert!(graph.get_edges().is_none());
    }

    #[test]
    fn merges_page_crossing_ties_without_skips_and_bounds_high_degree_reads() {
        // Given
        let mut read = GeneratedRead::build_star(100_000);
        let mut request = make_request();
        request.direction = TraversalDirection::Both;
        request.limit = NonZeroUsize::new(35).unwrap();
        request.edge_limit = NonZeroUsize::new(100);
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(graph.get_elements().len(), 35);
        assert_eq!(graph.get_elements()[0].as_str(), "n00000.md");
        assert_eq!(graph.get_elements()[31].as_str(), "n00031.md");
        assert_eq!(graph.get_elements()[34].as_str(), "n00034.md");
        assert_eq!(graph.get_edges().unwrap().len(), 69);
        assert_eq!(
            get_triples(&graph)[0],
            ("a.md", "n00000.md", "z_custom_link")
        );
        assert_eq!(
            get_triples(&graph)[1],
            ("n00000.md", "a.md", "z_custom_link")
        );
        assert_eq!(read.page_calls, 4);
    }

    #[test]
    fn continues_short_nonempty_pages_until_the_node_cap() {
        // Given
        let mut read = GeneratedRead::build_star(100_000);
        read.page_size = 7;
        let mut request = make_request();
        request.limit = NonZeroUsize::new(35).unwrap();
        // When
        let graph = GraphService::new().traverse(&mut read, &request).unwrap();
        // Then
        assert_eq!(graph.get_elements().len(), 35);
        assert_eq!(graph.get_elements()[34].as_str(), "n00034.md");
        assert_eq!(read.page_calls, 5);
    }

    #[test]
    fn rejects_a_late_page_error_without_returning_partial_graph() {
        // Given
        let mut read = GeneratedRead::build_star(100_000);
        read.fail_on_page = Some(2);
        let mut request = make_request();
        request.limit = NonZeroUsize::new(35).unwrap();
        // When
        let error = GraphService::new()
            .traverse(&mut read, &request)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(read.page_calls, 2);
    }

    #[test]
    fn rejects_empty_typed_starts_without_any_read() {
        // Given
        let mut read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.element_ids.clear();
        // When
        let error = GraphService::new()
            .traverse(&mut read, &request)
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
        assert!(read.resolved.is_empty());
        assert_eq!(read.page_calls, 0);
    }

    #[test]
    fn traversal_work_does_not_scale_with_unrelated_nodes_or_hub_degree() {
        for degree in [64, 100_000_000] {
            // Given a lazy source: neither degree nor unrelated identities are collected.
            let mut read = GeneratedRead::build_star(degree);
            let mut request = make_request();
            request.limit = NonZeroUsize::new(3).unwrap();
            request.edge_limit = NonZeroUsize::new(1);
            request.depth = u64::MAX;
            // When.
            let graph = GraphService::new().traverse(&mut read, &request).unwrap();
            // Then identical literal output, independent edge cap and mid-page stop.
            assert_eq!(get_ids(&graph), ["n00000.md", "n00001.md", "n00002.md"]);
            assert_eq!(
                get_triples(&graph),
                [("a.md", "n00000.md", "z_custom_link")]
            );
            assert_eq!(read.page_calls, 1);
            assert_eq!(read.expanded, [1]);
            assert_eq!(read.resolved, ["a.md"]);
        }
    }

    struct GeneratedRead {
        degree: Option<usize>,
        page_size: usize,
        fail_on_page: Option<usize>,
        includes_cycle: bool,
        expected_roots: usize,
        resolved: Vec<String>,
        page_calls: usize,
        expanded: Vec<i64>,
    }
    impl GeneratedRead {
        fn build_fixed() -> Self {
            Self {
                degree: None,
                page_size: 32,
                fail_on_page: None,
                includes_cycle: false,
                expected_roots: 1,
                resolved: Vec::new(),
                page_calls: 0,
                expanded: Vec::new(),
            }
        }
        fn build_star(degree: usize) -> Self {
            Self {
                degree: Some(degree),
                ..Self::build_fixed()
            }
        }
        fn generate_edges(&self) -> Vec<Edge> {
            let mut edges = vec![
                make_edge("a.md", "b.md", "includes"),
                make_edge("a.md", "c.md", "z_custom_link"),
                make_edge("a.md", "code.rs", "external"),
                make_edge("b.md", "c.md", "includes"),
                make_edge("b.md", "d.md", "z_custom_link"),
                make_edge("c.md", "d.md", "z_custom_link"),
                make_edge("d.md", "a.md", "z_custom_link"),
                make_edge("a.md", "gone.md", "z_custom_link"),
            ];
            if self.includes_cycle {
                edges.push(make_edge("c.md", "a.md", "includes"));
            }
            edges
        }
    }
    impl GraphRead for GeneratedRead {
        fn get_node_ref_by_id(
            &mut self,
            id: &ElementAddress,
        ) -> StorageResult<Option<GraphNodeRef>> {
            self.resolved.push(id.as_str().to_owned());
            let key = match id.as_str() {
                "a.md" => 1,
                "b.md" => 2,
                "c.md" => 3,
                "d.md" => 4,
                "code.rs" => 5,
                _ => return Ok(None),
            };
            Ok(Some(make_node(key, id.as_str())))
        }
        fn get_neighbor_page(
            &mut self,
            request: &NeighborPageRequest<'_>,
        ) -> StorageResult<Vec<GraphNeighbor>> {
            assert_eq!(
                self.resolved.len(),
                self.expected_roots,
                "all starts precede pages"
            );
            assert!(request.limit.get() <= 32);
            self.page_calls += 1;
            if self.fail_on_page == Some(self.page_calls) {
                return Err(crate::service::StorageError::new(
                    crate::service::StorageErrorKind::InternalError,
                    "late page failure",
                ));
            }
            if request.after.is_none() {
                self.expanded.push(request.node.get_value());
            }
            if let Some(degree) = self.degree {
                return Ok(generate_star_page(degree, self.page_size, request));
            }
            let mut page: Vec<_> = self
                .generate_edges()
                .into_iter()
                .filter(|edge| matches_filter(edge, request))
                .filter_map(|edge| select_neighbor(edge, request))
                .filter(|item| is_after(item, request))
                .collect();
            page.sort_by(|a, b| {
                (a.edge.get_edge_type(), a.node.element.as_str())
                    .cmp(&(b.edge.get_edge_type(), b.node.element.as_str()))
            });
            page.truncate(request.limit.get());
            Ok(page)
        }
    }
    fn make_request() -> TraversalRequest {
        TraversalRequest {
            element_ids: BTreeSet::from([parse_address("a.md")]),
            mode: TraversalMode::Both,
            direction: TraversalDirection::Outgoing,
            depth: 2,
            limit: NonZeroUsize::new(20).unwrap(),
            edge_types: BTreeSet::new(),
            edge_limit: None,
        }
    }
    fn get_ids(graph: &Graph) -> Vec<&str> {
        graph
            .get_elements()
            .iter()
            .map(EdgeTarget::as_str)
            .collect()
    }
    fn get_triples(graph: &Graph) -> Vec<(&str, &str, &str)> {
        graph
            .get_edges()
            .unwrap()
            .iter()
            .map(|edge| {
                (
                    edge.get_source().as_str(),
                    edge.get_target().as_str(),
                    edge.get_edge_type().as_str(),
                )
            })
            .collect()
    }
    fn parse_address(raw: &str) -> ElementAddress {
        ElementAddress::parse(raw).unwrap()
    }
    fn make_edge(source: &str, target: &str, kind: &str) -> Edge {
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
    fn make_node(key: i64, id: &str) -> GraphNodeRef {
        let element = if id.ends_with(".md") {
            EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
        } else {
            EdgeTarget::Source(SourceUri::parse(id).unwrap())
        };
        GraphNodeRef {
            key: GraphNodeKey::new(key),
            element,
        }
    }
    fn matches_filter(edge: &Edge, request: &NeighborPageRequest<'_>) -> bool {
        let family = match request.mode {
            TraversalMode::Relations => edge.get_edge_type().as_str() != "includes",
            TraversalMode::Containment => edge.get_edge_type().as_str() == "includes",
            TraversalMode::Both => true,
        };
        family
            && (request.edge_types.is_empty() || request.edge_types.contains(edge.get_edge_type()))
    }
    fn select_neighbor(edge: Edge, request: &NeighborPageRequest<'_>) -> Option<GraphNeighbor> {
        let ids = ["a.md", "b.md", "c.md", "d.md", "code.rs"];
        let anchor = ids[usize::try_from(request.node.get_value() - 1).unwrap()];
        let reached = match request.direction {
            NeighborDirection::Outgoing if edge.get_source().as_str() == anchor => {
                edge.get_target().as_str()
            }
            NeighborDirection::Incoming if edge.get_target().as_str() == anchor => {
                edge.get_source().as_str()
            }
            _ => return None,
        };
        let key = i64::try_from(ids.iter().position(|id| *id == reached)? + 1).unwrap();
        Some(GraphNeighbor {
            node: make_node(key, reached),
            edge,
        })
    }
    fn is_after(item: &GraphNeighbor, request: &NeighborPageRequest<'_>) -> bool {
        request.after.is_none_or(|cursor| {
            (item.edge.get_edge_type(), item.node.element.as_str())
                > (&cursor.edge_type, cursor.neighbor.as_str())
        })
    }
    fn generate_star_page(
        degree: usize,
        page_size: usize,
        request: &NeighborPageRequest<'_>,
    ) -> Vec<GraphNeighbor> {
        assert_eq!(
            request.node.get_value(),
            1,
            "node cap must prevent expanding the star"
        );
        let start = request.after.map_or(0, |cursor| {
            cursor.neighbor.as_str()[1..6].parse::<usize>().unwrap() + 1
        });
        (start..degree)
            .take(request.limit.get().min(page_size))
            .map(|index| {
                let id = format!("n{index:05}.md");
                let edge = match request.direction {
                    NeighborDirection::Outgoing => make_edge("a.md", &id, "z_custom_link"),
                    NeighborDirection::Incoming => make_edge(&id, "a.md", "z_custom_link"),
                };
                GraphNeighbor {
                    node: make_node(i64::try_from(index + 2).unwrap(), &id),
                    edge,
                }
            })
            .collect()
    }
}
