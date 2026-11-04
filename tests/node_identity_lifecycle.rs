#[path = "support/storage.rs"]
mod storage_support;
mod support;

use letopis::{
    domain::{Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, Sha256},
    open_storage,
    service::{
        ElementAddress, GraphDiagnostics, GraphRead, NeighborCursor, NeighborDirection,
        NeighborPageRequest, NodeMetadata, Refresher, Storage, StorageRead, TraversalDirection,
        TraversalMode, TraversalRequest, Traverser, VectorOutcome,
    },
    wire_document_refresher, wire_graph_diagnostics, wire_traverser,
};
use rusqlite::Connection;
use std::{collections::BTreeSet, num::NonZeroUsize};
use storage_support::build_knowledge;
use support::documents::{DocumentRepo, build_whole};
use tempfile::TempDir;

#[test]
fn updates_keep_fts_key_and_address_order() {
    // Given: identical searchable text is inserted in reverse address order.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    publish_text(&mut storage, "z.md", "oldtoken", ElementKind::Vertex);
    publish_text(&mut storage, "a.md", "oldtoken", ElementKind::Vertex);
    let db = Connection::open(&cache).unwrap();
    let original = get_numeric_id(&db, "a.md");
    assert!(get_numeric_id(&db, "z.md") < original);
    // When: equal-scoring matches are retrieved through the textual API.
    let mut read = storage.begin_read_tx().unwrap();
    let matches = read.get_text_matches_by_expression("oldtoken").unwrap();
    // Then: lexical addresses, rather than numeric insertion keys, break the tie.
    assert_eq!(
        matches
            .iter()
            .map(|row| row.id.as_str())
            .collect::<Vec<_>>(),
        ["a.md", "z.md"]
    );
    assert_eq!(matches[0].row_id, original);
    drop(read);

    // When: text and derived classification change at the same address.
    publish_text(&mut storage, "a.md", "newtoken", ElementKind::Metavertex);
    // Then: the independently read integer key survives and only the new text matches a.md.
    assert_eq!(get_numeric_id(&db, "a.md"), original);
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(get_match_ids(read.as_mut(), "oldtoken"), ["z.md"]);
    assert_eq!(get_match_ids(read.as_mut(), "newtoken"), ["a.md"]);
    assert_updated_knowledge(read.as_mut(), original);
    drop(read);

    // When: the updated node is deleted.
    let mut write = storage.begin_cache_write_tx().unwrap();
    write.delete_node(&parse_target("a.md")).unwrap();
    write.commit().unwrap();
    // Then: neither the public search nor the native FTS index retains its text.
    let mut read = storage.begin_read_tx().unwrap();
    assert!(get_match_ids(read.as_mut(), "newtoken").is_empty());
    assert_eq!(get_match_ids(read.as_mut(), "oldtoken"), ["z.md"]);
    assert_eq!(get_fts_count(&db, original), 0);
}

#[test]
fn neighbor_cursors_and_work_selection_use_addresses() {
    // Given: both directional streams have numeric keys opposite to address order.
    let root = TempDir::new().unwrap();
    let cache = root.path().join("cache.db");
    let mut storage = open_storage(&cache, &root.path().join("vectors.db")).unwrap();
    let identity = Sha256::digest(b"model");
    storage.reset_vectors(None, &identity).unwrap();
    let mut write = storage.begin_cache_write_tx().unwrap();
    for id in ["z.md", "root.md", "a.md"] {
        write.upsert_node(&build_knowledge(id, id)).unwrap();
    }
    for (source, target) in [
        ("root.md", "z.md"),
        ("root.md", "a.md"),
        ("z.md", "root.md"),
        ("a.md", "root.md"),
    ] {
        write
            .insert_edge(&build_edge(source, target, "supports"))
            .unwrap();
    }
    write.commit().unwrap();
    let db = Connection::open(&cache).unwrap();
    assert!(get_numeric_id(&db, "z.md") < get_numeric_id(&db, "a.md"));
    let mut read = storage.begin_read_tx().unwrap();
    // When / Then: limit-one pages resume strictly after a literal textual cursor in both directions.
    assert_address_pages(read.as_mut(), NeighborDirection::Outgoing);
    assert_address_pages(read.as_mut(), NeighborDirection::Incoming);
    // When: embedding work resumes after an address between a.md and root.md.
    let cursor = KnowledgeId::parse("m.md").unwrap();
    let work = read
        .get_next_embedding_work_by_id_and_max_attempts(&identity, 4, Some(&cursor))
        .unwrap();
    // Then: the first lexical successor wins despite z.md having the smallest numeric key.
    let VectorOutcome::Current(Some(work)) = work else {
        panic!("eligible work")
    };
    assert_eq!(work.id.as_str(), "root.md");
    let next = read
        .get_next_embedding_work_by_id_and_max_attempts(&identity, 4, Some(&work.id))
        .unwrap();
    let VectorOutcome::Current(Some(next)) = next else {
        panic!("next work")
    };
    assert_eq!(next.id.as_str(), "z.md");
    assert_eq!(
        read.get_next_embedding_work_by_id_and_max_attempts(&identity, 4, Some(&next.id))
            .unwrap(),
        VectorOutcome::Current(None)
    );
}

#[test]
fn dangling_target_reappears_under_new_numeric_id() {
    // Given: a canonical includes declaration points to an absent knowledge address.
    let repo = DocumentRepo::new();
    let source = build_whole(1, "[child](b.md \"letopis:type=includes;path=b.md\")");
    repo.write("a.md", &source);
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    // When / Then: the declaration remains stored while diagnostics report the missing target.
    assert_target_state(&repo, &["dangling_target"]);

    // When: the absent target is published.
    repo.write("b.md", build_whole(2, "Target"));
    refresher.refresh().unwrap();
    // Then: the same textual edge resolves in both directions and diagnostics clear.
    assert_target_state(&repo, &[]);
    let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    let original = get_numeric_id(&db, "b.md");
    assert_containment_traversal(&repo);

    // When: only the target file is removed.
    repo.remove("b.md");
    refresher.refresh().unwrap();
    // Then: the incoming declaration survives and becomes dangling again.
    assert_target_state(&repo, &["dangling_target"]);
    assert_eq!(repo.get_state().nodes.len(), 1);

    // When: knowledge reappears at the same textual address.
    repo.write("b.md", build_whole(3, "Recreated target"));
    refresher.refresh().unwrap();
    // Then: a new numeric key resolves the unchanged declaration and both traversal directions.
    assert_ne!(get_numeric_id(&db, "b.md"), original);
    assert_target_state(&repo, &[]);
    assert_containment_traversal(&repo);
    assert_eq!(
        std::fs::read_to_string(repo.get_root().join("a.md")).unwrap(),
        source
    );
}

fn publish_text(storage: &mut impl Storage, id: &str, text: &str, kind: ElementKind) {
    let mut node = build_knowledge(id, text);
    let NodeMetadata::Knowledge(metadata) = &mut node else {
        panic!("knowledge")
    };
    metadata.kind = kind;
    let mut write = storage.begin_cache_write_tx().unwrap();
    write.upsert_node(&node).unwrap();
    write
        .replace_text(&KnowledgeId::parse(id).unwrap(), text)
        .unwrap();
    write.commit().unwrap();
}

fn assert_updated_knowledge(read: &mut dyn StorageRead, original: i64) {
    let node = read.get_node_by_id(&parse_target("a.md")).unwrap().unwrap();
    let NodeMetadata::Knowledge(metadata) = node.metadata else {
        panic!("knowledge")
    };
    assert_eq!(node.row_id, original);
    assert_eq!(metadata.id.as_str(), "a.md");
    assert_eq!(metadata.uri.as_str(), "a.md");
    assert_eq!(metadata.kind, ElementKind::Metavertex);
}

fn get_fts_count(db: &Connection, row_id: i64) -> i64 {
    db.query_row(
        "select count(*) from text_index where rowid = ?1",
        [row_id],
        |row| row.get(0),
    )
    .unwrap()
}

fn get_numeric_id(db: &Connection, uri: &str) -> i64 {
    db.query_row("select id from nodes where uri = ?1", [uri], |row| {
        row.get(0)
    })
    .unwrap()
}

fn get_match_ids(read: &mut dyn StorageRead, expression: &str) -> Vec<String> {
    read.get_text_matches_by_expression(expression)
        .unwrap()
        .into_iter()
        .map(|row| row.id.as_str().to_owned())
        .collect()
}

fn assert_address_pages(read: &mut dyn GraphRead, direction: NeighborDirection) {
    let key = read
        .get_node_ref_by_id(&ElementAddress::parse("root.md").unwrap())
        .unwrap()
        .unwrap()
        .key;
    let types = BTreeSet::new();
    let mut request = NeighborPageRequest {
        node: key,
        direction,
        mode: TraversalMode::Relations,
        edge_types: &types,
        after: None,
        limit: NonZeroUsize::new(1).unwrap(),
    };
    let first = read.get_neighbor_page(&request).unwrap();
    assert_eq!(first.len(), 1);
    assert_eq!(first[0].node.element.as_str(), "a.md");
    let cursor = NeighborCursor {
        edge_type: EdgeType::parse("supports").unwrap(),
        neighbor: ElementAddress::parse("a.md").unwrap(),
    };
    request.after = Some(&cursor);
    let second = read.get_neighbor_page(&request).unwrap();
    assert_eq!(second.len(), 1);
    assert_eq!(second[0].node.element.as_str(), "z.md");
    let cursor = NeighborCursor {
        edge_type: EdgeType::parse("supports").unwrap(),
        neighbor: ElementAddress::parse("z.md").unwrap(),
    };
    request.after = Some(&cursor);
    assert!(read.get_neighbor_page(&request).unwrap().is_empty());
}

fn assert_target_state(repo: &DocumentRepo, expected_reasons: &[&str]) {
    let mut storage = open_storage(
        &repo.get_root().join(".letopis/cache.db"),
        &repo.get_root().join(".letopis/vectors.db"),
    )
    .unwrap();
    let mut read = storage.begin_read_tx().unwrap();
    let edges = read.get_edges_by_source_id(None).unwrap();
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].get_source().as_str(), "a.md");
    assert_eq!(edges[0].get_target().as_str(), "b.md");
    assert_eq!(edges[0].get_edge_type().as_str(), "includes");
    let problems = wire_graph_diagnostics()
        .find_problems(read.as_mut(), &[])
        .unwrap();
    let reasons: Vec<_> = problems
        .iter()
        .map(|problem| problem.details["reason"].as_str().unwrap())
        .collect();
    assert_eq!(reasons, expected_reasons);
    if !problems.is_empty() {
        assert_eq!(problems[0].details.get("target").unwrap(), "b.md");
        assert_eq!(
            get_neighbor_ids(read.as_mut(), "a.md", NeighborDirection::Outgoing),
            Vec::<String>::new()
        );
    }
}

fn assert_containment_traversal(repo: &DocumentRepo) {
    let mut traverser = wire_traverser(repo.get_root()).unwrap();
    for (start, direction, expected) in [
        ("a.md", TraversalDirection::Outgoing, "b.md"),
        ("b.md", TraversalDirection::Incoming, "a.md"),
    ] {
        let result = traverser
            .traverse(&TraversalRequest {
                element_ids: BTreeSet::from([ElementAddress::parse(start).unwrap()]),
                mode: TraversalMode::Containment,
                direction,
                depth: 1,
                limit: NonZeroUsize::new(1).unwrap(),
                edge_types: BTreeSet::new(),
                edge_limit: Some(NonZeroUsize::new(1).unwrap()),
            })
            .unwrap();
        assert_eq!(
            result
                .elements
                .iter()
                .map(|link| link.element_id.as_str())
                .collect::<Vec<_>>(),
            [expected]
        );
        let edges = result.edges.unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].get_source().as_str(), "a.md");
        assert_eq!(edges[0].get_target().as_str(), "b.md");
        assert_eq!(edges[0].get_edge_type().as_str(), "includes");
    }
}

fn get_neighbor_ids(
    read: &mut dyn GraphRead,
    id: &str,
    direction: NeighborDirection,
) -> Vec<String> {
    let node = read
        .get_node_ref_by_id(&ElementAddress::parse(id).unwrap())
        .unwrap()
        .unwrap();
    read.get_neighbor_page(&NeighborPageRequest {
        node: node.key,
        direction,
        mode: TraversalMode::Containment,
        edge_types: &BTreeSet::new(),
        after: None,
        limit: NonZeroUsize::new(1).unwrap(),
    })
    .unwrap()
    .into_iter()
    .map(|neighbor| neighbor.node.element.as_str().to_owned())
    .collect()
}

fn build_edge(source: &str, target: &str, kind: &str) -> Edge {
    Edge::new(
        KnowledgeId::parse(source).unwrap(),
        parse_target(target),
        EdgeType::parse(kind).unwrap(),
    )
    .unwrap()
}

fn parse_target(id: &str) -> EdgeTarget {
    EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap())
}
