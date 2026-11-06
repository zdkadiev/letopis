use super::{GraphProblem, find_graph_problems};
use crate::domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, SourceUri};
use std::collections::BTreeSet;

#[test]
fn deduplicates_dangling_pairs_and_rechecks_node_membership() {
    // Given
    let mut nodes = build_nodes(&["a.md"]);
    let edges = BTreeSet::from([
        build_edge("a.md", "missing.md", "includes"),
        build_edge("a.md", "missing.md", "supports"),
    ]);
    let expected = vec![GraphProblem::DanglingTarget {
        source: parse_id("a.md"),
        target: parse_id("missing.md"),
    }];

    // When / Then
    assert_eq!(find_graph_problems(&nodes, &edges), expected);
    nodes.insert(parse_id("missing.md"));
    assert!(find_graph_problems(&nodes, &edges).is_empty());
    nodes.remove(&parse_id("missing.md"));
    assert_eq!(find_graph_problems(&nodes, &edges), expected);
}

#[test]
fn ignores_absent_sources_and_external_source_targets() {
    // Given
    let nodes = build_nodes(&["a.md"]);
    let external = Edge::new(
        parse_id("a.md"),
        EdgeTarget::Source(SourceUri::parse("source.rs#L1-L2").unwrap()),
        EdgeType::parse("external").unwrap(),
    )
    .unwrap();
    let edges = BTreeSet::from([
        build_edge("absent.md", "missing.md", "includes"),
        build_edge("absent.md", "a.md", "includes"),
        build_edge("a.md", "absent.md", "includes"),
        external,
    ]);

    // When / Then
    assert_eq!(
        find_graph_problems(&nodes, &edges),
        vec![GraphProblem::DanglingTarget {
            source: parse_id("a.md"),
            target: parse_id("absent.md"),
        }]
    );
}

#[test]
fn separates_components_without_including_one_way_descendants() {
    // Given
    let nodes = build_nodes(&["a.md", "b.md", "c.md", "d.md", "e.md"]);
    let edges = BTreeSet::from([
        build_edge("a.md", "b.md", "includes"),
        build_edge("b.md", "a.md", "includes"),
        build_edge("b.md", "c.md", "includes"),
        build_edge("b.md", "d.md", "includes"),
        build_edge("d.md", "e.md", "includes"),
        build_edge("e.md", "d.md", "includes"),
    ]);

    // When / Then
    assert_eq!(
        find_graph_problems(&nodes, &edges),
        vec![
            GraphProblem::IncludesCycle {
                members: vec![parse_id("a.md"), parse_id("b.md")],
            },
            GraphProblem::IncludesCycle {
                members: vec![parse_id("d.md"), parse_id("e.md")],
            },
        ]
    );
}

#[test]
fn accepts_ordinary_cycles_and_shared_child_includes_dags() {
    // Given
    let nodes = build_nodes(&["a.md", "b.md", "c.md", "d.md"]);
    let edges = BTreeSet::from([
        build_edge("a.md", "b.md", "depends_on"),
        build_edge("b.md", "a.md", "depends_on"),
        build_edge("a.md", "c.md", "includes"),
        build_edge("a.md", "d.md", "includes"),
        build_edge("c.md", "b.md", "includes"),
        build_edge("d.md", "b.md", "includes"),
    ]);

    // When / Then
    assert!(find_graph_problems(&nodes, &edges).is_empty());
}

#[test]
fn orders_mixed_findings_and_preserves_inputs() {
    // Given
    let nodes = build_nodes(&["a.md", "b.md", "d.md", "e.md"]);
    let edges = BTreeSet::from([
        build_edge("e.md", "d.md", "includes"),
        build_edge("d.md", "e.md", "includes"),
        build_edge("b.md", "a.md", "includes"),
        build_edge("a.md", "b.md", "includes"),
        build_edge("e.md", "missing.md", "supports"),
        build_edge("a.md", "z.md", "includes"),
        build_edge("a.md", "x.md", "supports"),
    ]);
    let original_nodes = nodes.clone();
    let original_edges = edges.clone();

    // When
    let problems = find_graph_problems(&nodes, &edges);

    // Then
    assert_eq!(
        problems,
        vec![
            GraphProblem::DanglingTarget {
                source: parse_id("a.md"),
                target: parse_id("x.md")
            },
            GraphProblem::DanglingTarget {
                source: parse_id("a.md"),
                target: parse_id("z.md")
            },
            GraphProblem::DanglingTarget {
                source: parse_id("e.md"),
                target: parse_id("missing.md")
            },
            GraphProblem::IncludesCycle {
                members: vec![parse_id("a.md"), parse_id("b.md")]
            },
            GraphProblem::IncludesCycle {
                members: vec![parse_id("d.md"), parse_id("e.md")]
            },
        ]
    );
    assert_eq!(nodes, original_nodes);
    assert_eq!(edges, original_edges);
}

#[test]
fn handles_ten_thousand_nodes_without_recursive_traversal() {
    // Given
    let expected_members: Vec<KnowledgeId> = (0..10_000)
        .map(|index| parse_id(&format!("node-{index:05}.md")))
        .collect();
    let nodes = expected_members.iter().cloned().collect();
    let mut edges: BTreeSet<Edge> = (0..9_999)
        .map(|index| {
            build_edge(
                &format!("node-{index:05}.md"),
                &format!("node-{:05}.md", index + 1),
                "includes",
            )
        })
        .collect();

    // When / Then
    assert!(find_graph_problems(&nodes, &edges).is_empty());
    // Given
    edges.insert(build_edge("node-09999.md", "node-00000.md", "includes"));
    // When
    let problems = find_graph_problems(&nodes, &edges);

    // Then
    assert_eq!(problems.len(), 1);
    let GraphProblem::IncludesCycle { members } = &problems[0] else {
        panic!("expected one includes cycle");
    };
    assert_eq!(members.len(), 10_000);
    assert!(
        members
            .iter()
            .zip(&expected_members)
            .all(|(actual, expected)| actual == expected)
    );
}

fn build_nodes(raw: &[&str]) -> BTreeSet<KnowledgeId> {
    raw.iter().map(|value| parse_id(value)).collect()
}

fn build_edge(source: &str, target: &str, relation: &str) -> Edge {
    Edge::new(
        parse_id(source),
        EdgeTarget::Knowledge(parse_id(target)),
        EdgeType::parse(relation).unwrap(),
    )
    .unwrap()
}

fn parse_id(raw: &str) -> KnowledgeId {
    KnowledgeId::parse(raw).unwrap()
}
