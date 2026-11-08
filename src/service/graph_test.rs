use crate::service::StorageError;
use crate::service::TransactionError;

use super::GraphService;
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, Graph, KnowledgeId, SourceUri},
    service::{
        CacheDatabase, CommandErrorKind, ElementAddress, GraphNeighbor, GraphNodeKey, GraphNodeRef,
        GraphRead, GraphWalk, NeighborDirection, NeighborPageRequest, NodeMetadata, SourceMetadata,
        StorageRead, StorageResult, TraversalDirection, TraversalMode, TraversalRequest,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    num::NonZeroUsize,
};

#[test]
fn graph_services_join_the_same_cache_snapshot() {
    use std::rc::Rc;
    // Given: two graph services share a cache containing one published source.
    let root = tempfile::TempDir::new().unwrap();
    let storage = crate::storage::open_test_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let source = SourceUri::parse("code.rs").unwrap();
    assert_eq!(publish_source(&storage, &source), 1);
    let cache = Rc::new(storage.cache_storage);
    let first = GraphService::new(Rc::clone(&cache));
    let second = GraphService::new(Rc::clone(&cache));
    let mut request = make_request();
    request.element_ids = BTreeSet::from([parse_address("code.rs")]);
    request.depth = 0;
    // When: another publisher removes the source between the two service calls.
    cache
        .with_read_tx(|| -> crate::service::StorageResult<()> {
            first.traverse(&request).unwrap();
            assert_eq!(remove_source(root.path(), &source), 2);
            second.traverse(&request).unwrap();
            assert_eq!(cache.project_get_by_id()?, 1);
            Ok(())
        })
        .unwrap();
    // Then: nested services saw the old membership; the next scope sees removal.
    assert_eq!(
        second.traverse(&request).unwrap_err().get_kind(),
        CommandErrorKind::NotFound
    );
}

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
        let read = GeneratedRead::build_fixed();
        let mut request = make_request();
        request.mode = mode;
        request.direction = direction;
        // When
        let graph = traverse_graph(&read, &request).unwrap();
        // Then
        assert_eq!(get_ids(&graph), expected, "{mode:?}/{direction:?}");
    }
}

#[test]
fn keeps_edge_budget_independent_from_node_budget() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.edge_limit = NonZeroUsize::new(1);
    // When
    let graph = traverse_graph(&read, &request).unwrap();
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
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.mode = TraversalMode::Containment;
    request.depth = 1;
    // When
    let graph = traverse_graph(&read, &request).unwrap();
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
    let error = traverse_graph(&read, &request).unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
    assert_eq!(*read.resolved.borrow(), ["a.md", "unknown.md"]);
    assert_eq!(read.page_calls.get(), 0);
}

#[test]
fn returns_requested_empty_edges_after_validating_depth_zero() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.depth = 0;
    request.edge_limit = NonZeroUsize::new(5);
    // When
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert!(graph.get_elements().is_empty());
    assert_eq!(graph.get_edges(), Some([].as_slice()));
    assert_eq!(*read.resolved.borrow(), ["a.md"]);
    assert_eq!(read.page_calls.get(), 0);
}

#[test]
fn stops_buffered_transitions_and_queries_at_node_cap() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.limit = NonZeroUsize::new(2).unwrap();
    request.edge_limit = NonZeroUsize::new(20);
    // When
    let graph = traverse_graph(&read, &request).unwrap();
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
    assert_eq!(read.page_calls.get(), 1);
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
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert_eq!(get_ids(&graph), ["code.rs", "b.md", "d.md"]);
    assert_eq!(*read.resolved.borrow(), ["a.md", "c.md"]);
    assert_eq!(
        read.expanded
            .borrow()
            .iter()
            .filter(|key| **key == 1)
            .count(),
        1
    );
}

#[test]
fn terminates_includes_cycle_without_returning_start_twice() {
    // Given
    let mut read = GeneratedRead::build_fixed();
    read.is_cyclic = true;
    let mut request = make_request();
    request.mode = TraversalMode::Containment;
    request.depth = u64::MAX;
    request.limit = NonZeroUsize::MAX;
    // When
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert_eq!(get_ids(&graph), ["b.md", "c.md"]);
}

#[test]
fn preserves_original_incoming_edges_and_considered_back_edges() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.direction = TraversalDirection::Incoming;
    request.edge_limit = NonZeroUsize::new(20);
    // When
    let graph = traverse_graph(&read, &request).unwrap();
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
    let read = GeneratedRead::build_fixed();
    request.direction = TraversalDirection::Outgoing;
    request.depth = u64::MAX;
    // When
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert_eq!(graph.get_edges().unwrap().len(), 7);
    assert!(get_triples(&graph).contains(&("d.md", "a.md", "z_custom_link")));
}

#[test]
fn intersects_open_types_and_omits_edges_when_not_requested() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request
        .edge_types
        .insert(EdgeType::parse("z_custom_link").unwrap());
    // When
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert_eq!(get_ids(&graph), ["c.md", "d.md"]);
    assert!(graph.get_edges().is_none());
}

#[test]
fn merges_page_crossing_ties_without_skips_and_bounds_high_degree_reads() {
    // Given
    let read = GeneratedRead::build_star(100_000);
    let mut request = make_request();
    request.direction = TraversalDirection::Both;
    request.limit = NonZeroUsize::new(35).unwrap();
    request.edge_limit = NonZeroUsize::new(100);
    // When
    let graph = traverse_graph(&read, &request).unwrap();
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
    assert_eq!(read.page_calls.get(), 4);
}

#[test]
fn continues_short_nonempty_pages_until_the_node_cap() {
    // Given
    let mut read = GeneratedRead::build_star(100_000);
    read.page_size = 7;
    let mut request = make_request();
    request.limit = NonZeroUsize::new(35).unwrap();
    // When
    let graph = traverse_graph(&read, &request).unwrap();
    // Then
    assert_eq!(graph.get_elements().len(), 35);
    assert_eq!(graph.get_elements()[34].as_str(), "n00034.md");
    assert_eq!(read.page_calls.get(), 5);
}

#[test]
fn rejects_a_late_page_error_without_returning_partial_graph() {
    // Given
    let mut read = GeneratedRead::build_star(100_000);
    read.fail_on_page = Some(2);
    let mut request = make_request();
    request.limit = NonZeroUsize::new(35).unwrap();
    // When
    let error = traverse_graph(&read, &request).unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(read.page_calls.get(), 2);
}

#[test]
fn rejects_empty_typed_starts_without_any_read() {
    // Given
    let read = GeneratedRead::build_fixed();
    let mut request = make_request();
    request.element_ids.clear();
    // When
    let error = traverse_graph(&read, &request).unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InvalidRequest);
    assert!(read.resolved.borrow().is_empty());
    assert_eq!(read.page_calls.get(), 0);
}

#[test]
fn traversal_work_does_not_scale_with_unrelated_nodes_or_hub_degree() {
    for degree in [64, 100_000_000] {
        // Given a lazy source: neither degree nor unrelated identities are collected.
        let read = GeneratedRead::build_star(degree);
        let mut request = make_request();
        request.limit = NonZeroUsize::new(3).unwrap();
        request.edge_limit = NonZeroUsize::new(1);
        request.depth = u64::MAX;
        // When.
        let graph = traverse_graph(&read, &request).unwrap();
        // Then identical literal output, independent edge cap and mid-page stop.
        assert_eq!(get_ids(&graph), ["n00000.md", "n00001.md", "n00002.md"]);
        assert_eq!(
            get_triples(&graph),
            [("a.md", "n00000.md", "z_custom_link")]
        );
        assert_eq!(read.page_calls.get(), 1);
        assert_eq!(*read.expanded.borrow(), [1]);
        assert_eq!(*read.resolved.borrow(), ["a.md"]);
    }
}

struct GeneratedRead {
    degree: Option<usize>,
    page_size: usize,
    fail_on_page: Option<usize>,
    is_cyclic: bool,
    expected_roots: usize,
    resolved: RefCell<Vec<String>>,
    page_calls: Cell<usize>,
    expanded: RefCell<Vec<i64>>,
}

impl GeneratedRead {
    fn build_fixed() -> Self {
        Self {
            degree: None,
            page_size: 32,
            fail_on_page: None,
            is_cyclic: false,
            expected_roots: 1,
            resolved: RefCell::new(Vec::new()),
            page_calls: Cell::new(0),
            expanded: RefCell::new(Vec::new()),
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
        if self.is_cyclic {
            edges.push(make_edge("c.md", "a.md", "includes"));
        }
        edges
    }
}

impl GraphRead for GeneratedRead {
    fn nodes_get_by_uri(&self, uri: &ElementAddress) -> StorageResult<Option<GraphNodeRef>> {
        self.resolved.borrow_mut().push(uri.as_str().to_owned());
        let key = match uri.as_str() {
            "a.md" => 1,
            "b.md" => 2,
            "c.md" => 3,
            "d.md" => 4,
            "code.rs" => 5,
            _ => return Ok(None),
        };
        Ok(Some(make_node(key, uri.as_str())))
    }

    fn edges_get_by_row_id_and_type(
        &self,
        request: &NeighborPageRequest<'_>,
    ) -> StorageResult<Vec<GraphNeighbor>> {
        assert_eq!(
            self.resolved.borrow().len(),
            self.expected_roots,
            "all starts precede pages"
        );
        assert!(request.limit.get() <= 32);
        self.page_calls.set(self.page_calls.get() + 1);
        if self.fail_on_page == Some(self.page_calls.get()) {
            return Err(StorageError::new(
                crate::service::StorageErrorKind::InternalError,
                "late page failure",
            ));
        }
        if request.after.is_none() {
            self.expanded.borrow_mut().push(request.node.get_handle());
        }
        if let Some(degree) = self.degree {
            return Ok(generate_star_page(degree, self.page_size, request));
        }
        let mut page: Vec<_> = self
            .generate_edges()
            .into_iter()
            .filter(|edge| is_matched_by_filter(edge, request))
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

fn is_matched_by_filter(edge: &Edge, request: &NeighborPageRequest<'_>) -> bool {
    let family = match request.mode {
        TraversalMode::Relations => edge.get_edge_type().as_str() != "includes",
        TraversalMode::Containment => edge.get_edge_type().as_str() == "includes",
        TraversalMode::Both => true,
    };
    family && (request.edge_types.is_empty() || request.edge_types.contains(edge.get_edge_type()))
}

fn select_neighbor(edge: Edge, request: &NeighborPageRequest<'_>) -> Option<GraphNeighbor> {
    let ids = ["a.md", "b.md", "c.md", "d.md", "code.rs"];
    let anchor = ids[usize::try_from(request.node.get_handle() - 1).unwrap()];
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
        request.node.get_handle(),
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

fn traverse_graph(
    read: &dyn GraphRead,
    request: &TraversalRequest,
) -> crate::service::CommandResult<Graph> {
    let mut discovery = super::Discovery::resolve_starts(read, request)?;
    discovery.walk(request)?;
    Ok(discovery.into_graph())
}

fn publish_source(storage: &crate::storage::TestDatabases, source: &SourceUri) -> u64 {
    storage
        .cache_storage
        .with_write_tx(|| {
            let write = &storage.cache_storage;
            write
                .nodes_upsert_by_id(&NodeMetadata::Source(SourceMetadata {
                    uri: source.clone(),
                    hash: None,
                    created_at: 1,
                    lines: None,
                }))
                .unwrap();

            write.project_get_revision()
        })
        .map_err(TransactionError::into_inner)
        .unwrap()
}

fn remove_source(root: &std::path::Path, source: &SourceUri) -> u64 {
    let storage =
        crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db"))
            .unwrap();
    storage
        .cache_storage
        .with_write_tx(|| {
            let write = &storage.cache_storage;
            write
                .delete_node(&EdgeTarget::Source(source.clone()))
                .unwrap();

            write.project_get_revision()
        })
        .map_err(TransactionError::into_inner)
        .unwrap()
}
