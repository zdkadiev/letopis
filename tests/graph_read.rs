#[path = "support/storage.rs"]
mod storage_support;
use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri},
    open_storage,
    service::{
        CacheWrite, ElementAddress, GraphNeighbor, GraphNodeKey, GraphRead, NeighborCursor,
        NeighborDirection, NeighborPageRequest, Storage, StorageErrorKind, TraversalMode,
    },
};
use std::{collections::BTreeSet, num::NonZeroUsize};
use storage_support::{build_knowledge, build_source};
use tempfile::TempDir;

static EMPTY_TYPES: BTreeSet<EdgeType> = BTreeSet::new();

#[test]
fn pages_neighbors_in_type_and_identity_order() {
    // Arrange a missing endpoint before the first real target.
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("./root.md"))
        .unwrap()
        .unwrap()
        .key;
    let mut request = build_request(key, NeighborDirection::Outgoing, 2);
    // Act and assert each page independently across a type boundary.
    let first = read.read_neighbor_page(&request).unwrap();
    assert_eq!(
        summarize(&first),
        [
            "external:root.md>code.md",
            "external:root.md>https://Host/A?q=1#L0-L9"
        ]
    );
    assert!(matches!(first[0].node.element, EdgeTarget::Source(_)));
    let cursor = build_cursor(first.last().unwrap());
    request.after = Some(&cursor);
    let next = read.read_neighbor_page(&request).unwrap();
    assert_eq!(
        summarize(&next),
        ["includes:root.md>b.md", "supports:root.md>a.md"]
    );
    let cursor = build_cursor(next.last().unwrap());
    request.after = Some(&cursor);
    let last = read.read_neighbor_page(&request).unwrap();
    assert_eq!(summarize(&last), ["supports:root.md>b.md"]);
    let cursor = build_cursor(last.last().unwrap());
    request.after = Some(&cursor);
    assert!(read.read_neighbor_page(&request).unwrap().is_empty());
    assert_eq!(first.len() + next.len() + last.len(), 5);
}

#[test]
fn intersects_modes_and_type_filters_before_the_page_bound() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("root.md"))
        .unwrap()
        .unwrap()
        .key;
    let mut request = build_request(key, NeighborDirection::Outgoing, 1);
    request.mode = TraversalMode::Containment;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["includes:root.md>b.md"]
    );
    request.mode = TraversalMode::Relations;
    let types = BTreeSet::from([EdgeType::parse("supports").unwrap()]);
    request.edge_types = &types;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["supports:root.md>a.md"]
    );
    request.mode = TraversalMode::Containment;
    assert!(read.read_neighbor_page(&request).unwrap().is_empty());
}

#[test]
fn keeps_incoming_edges_in_original_orientation() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("b.md"))
        .unwrap()
        .unwrap()
        .key;
    let mut request = build_request(key, NeighborDirection::Incoming, 2);
    let first = read.read_neighbor_page(&request).unwrap();
    assert_eq!(
        summarize(&first),
        ["includes:root.md>b.md", "supports:a.md>b.md"]
    );
    assert_eq!(first[0].node.element.as_str(), "root.md");
    assert_eq!(first[1].node.element.as_str(), "a.md");
    let cursor = build_cursor(first.last().unwrap());
    request.after = Some(&cursor);
    let next = read.read_neighbor_page(&request).unwrap();
    assert_eq!(summarize(&next), ["supports:root.md>b.md"]);
}

#[test]
fn filters_incoming_modes_and_types_before_limiting() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("b.md"))
        .unwrap()
        .unwrap()
        .key;
    let mut request = build_request(key, NeighborDirection::Incoming, 1);
    request.mode = TraversalMode::Containment;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["includes:root.md>b.md"]
    );
    request.mode = TraversalMode::Relations;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["supports:a.md>b.md"]
    );
    let types = BTreeSet::from([EdgeType::parse("includes").unwrap()]);
    request.edge_types = &types;
    assert!(read.read_neighbor_page(&request).unwrap().is_empty());
}

#[test]
fn retains_one_snapshot_across_pages() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut writer = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("root.md"))
        .unwrap()
        .unwrap()
        .key;
    let types = BTreeSet::from([EdgeType::parse("supports").unwrap()]);
    let mut request = build_request(key, NeighborDirection::Outgoing, 1);
    request.edge_types = &types;
    let first = read.read_neighbor_page(&request).unwrap();
    assert_eq!(summarize(&first), ["supports:root.md>a.md"]);
    let cursor = build_cursor(first.last().unwrap());
    let mut write = writer.begin_cache_write().unwrap();
    write
        .delete_outgoing(&KnowledgeId::parse("root.md").unwrap())
        .unwrap();
    write
        .insert_edge(&build_edge("root.md", "c.md", "supports"))
        .unwrap();
    write.commit().unwrap();
    request.after = Some(&cursor);
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["supports:root.md>b.md"]
    );
    drop(read);
    let mut read = storage.begin_read().unwrap();
    request.node = read
        .resolve_node(&parse_address("root.md"))
        .unwrap()
        .unwrap()
        .key;
    request.after = None;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["supports:root.md>c.md"]
    );
}

#[test]
fn bounds_large_degrees_and_rejects_excessive_page_limits() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut write = storage.begin_cache_write().unwrap();
    for number in 0..40 {
        let id = format!("n{number:02}.md");
        write.upsert_node(&build_knowledge(&id, "body")).unwrap();
        write
            .insert_edge(&build_edge("c.md", &id, "supports"))
            .unwrap();
    }
    write.commit().unwrap();
    let mut read = storage.begin_read().unwrap();
    let key = read
        .resolve_node(&parse_address("c.md"))
        .unwrap()
        .unwrap()
        .key;
    let mut request = build_request(key, NeighborDirection::Outgoing, 32);
    let first = read.read_neighbor_page(&request).unwrap();
    assert_eq!(first.len(), 32);
    assert_eq!(first.first().unwrap().node.element.as_str(), "n00.md");
    assert_eq!(first.last().unwrap().node.element.as_str(), "n31.md");
    let cursor = build_cursor(first.last().unwrap());
    request.after = Some(&cursor);
    let last = read.read_neighbor_page(&request).unwrap();
    assert_eq!(last.len(), 8);
    assert_eq!(last.first().unwrap().node.element.as_str(), "n32.md");
    assert_eq!(last.last().unwrap().node.element.as_str(), "n39.md");
    request.limit = NonZeroUsize::new(33).unwrap();
    assert_eq!(
        read.read_neighbor_page(&request).unwrap_err().get_kind(),
        StorageErrorKind::InvalidRequest
    );
}

#[test]
fn resolves_local_range_and_opaque_url_identities_from_actual_node_kinds() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let mut read = storage.begin_read().unwrap();
    for (raw, expected, source) in [
        ("./root.md", "root.md", false),
        ("a.md#A", "a.md#A", false),
        ("code.md", "code.md", true),
        ("src.rs#L01-L02", "src.rs#L1-L2", true),
        ("https://Host/A?q=1#L0-L9", "https://Host/A?q=1#L0-L9", true),
    ] {
        let node = read.resolve_node(&parse_address(raw)).unwrap().unwrap();
        assert_eq!(node.element.as_str(), expected);
        assert_eq!(matches!(node.element, EdgeTarget::Source(_)), source);
    }
    assert!(
        read.resolve_node(&parse_address("absent.md"))
            .unwrap()
            .is_none()
    );
}

#[test]
fn does_not_decode_unselected_node_metadata() {
    let root = TempDir::new().unwrap();
    let mut storage = build_fixture(&root);
    let db = rusqlite::Connection::open(root.path().join("cache.db")).unwrap();
    db.execute(
        "UPDATE nodes SET hash='invalid',source_snapshot='invalid' WHERE id='a.md'",
        [],
    )
    .unwrap();
    let mut read = storage.begin_read().unwrap();
    assert!(read.resolve_node(&parse_address("a.md")).unwrap().is_some());
    let key = read
        .resolve_node(&parse_address("root.md"))
        .unwrap()
        .unwrap()
        .key;
    let types = BTreeSet::from([EdgeType::parse("supports").unwrap()]);
    let mut request = build_request(key, NeighborDirection::Outgoing, 1);
    request.edge_types = &types;
    assert_eq!(
        summarize(&read.read_neighbor_page(&request).unwrap()),
        ["supports:root.md>a.md"]
    );
}

fn build_fixture(root: &TempDir) -> impl Storage + use<> {
    let mut storage = open_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap();
    let mut write = storage.begin_cache_write().unwrap();
    for id in ["root.md", "a.md", "a.md#A", "b.md", "c.md"] {
        write.upsert_node(&build_knowledge(id, "body")).unwrap();
    }
    for id in ["code.md", "src.rs#L1-L2", "https://Host/A?q=1#L0-L9"] {
        write.upsert_node(&build_source(id, None, 1)).unwrap();
    }
    for (source, target, kind) in [
        ("root.md", "000missing.md", "external"),
        ("root.md", "code.md", "external"),
        ("root.md", "https://Host/A?q=1#L0-L9", "external"),
        ("root.md", "b.md", "includes"),
        ("root.md", "a.md", "supports"),
        ("root.md", "b.md", "supports"),
        ("a.md", "b.md", "supports"),
    ] {
        write
            .insert_edge(&build_edge(source, target, kind))
            .unwrap();
    }
    write.commit().unwrap();
    storage
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
fn build_request(
    node: GraphNodeKey,
    direction: NeighborDirection,
    limit: usize,
) -> NeighborPageRequest<'static> {
    NeighborPageRequest {
        node,
        direction,
        mode: TraversalMode::Both,
        edge_types: &EMPTY_TYPES,
        after: None,
        limit: NonZeroUsize::new(limit).unwrap(),
    }
}
fn build_cursor(neighbor: &GraphNeighbor) -> NeighborCursor {
    NeighborCursor {
        edge_type: neighbor.edge.get_edge_type().clone(),
        neighbor: parse_address(neighbor.node.element.as_str()),
    }
}
fn parse_address(raw: &str) -> ElementAddress {
    ElementAddress::parse(raw).unwrap()
}
fn summarize(neighbors: &[GraphNeighbor]) -> Vec<String> {
    neighbors
        .iter()
        .map(|n| {
            format!(
                "{}:{}>{}",
                n.edge.get_edge_type().as_str(),
                n.edge.get_source().as_str(),
                n.edge.get_target().as_str()
            )
        })
        .collect()
}
