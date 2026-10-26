use super::graph::INCLUDES_EDGE_TYPE;
use super::{Edge, EdgeTarget, KnowledgeId};
use std::collections::{BTreeMap, BTreeSet};

/// A current graph inconsistency derived from validated knowledge values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphProblem {
    /// An existing source declares a knowledge target absent from the graph.
    DanglingTarget {
        /// Existing knowledge element that owns the declaration.
        source: KnowledgeId,
        /// Knowledge identity that currently has no element.
        target: KnowledgeId,
    },
    /// A strongly connected includes component containing at least two elements.
    IncludesCycle {
        /// Component identities in lexicographic order.
        members: Vec<KnowledgeId>,
    },
}

struct IncludesGraph<'a> {
    nodes: Vec<&'a KnowledgeId>,
    outgoing: Vec<Vec<usize>>,
    incoming: Vec<Vec<usize>>,
}

struct FinishingSearch<'a> {
    outgoing: &'a [Vec<usize>],
    visited: Vec<bool>,
    order: Vec<usize>,
    stack: Vec<(usize, usize)>,
}

struct ComponentSearch<'a> {
    incoming: &'a [Vec<usize>],
    visited: Vec<bool>,
    stack: Vec<usize>,
}

/// Find current dangling targets and includes cycles without changing the inputs.
///
/// Dangling pairs are deduplicated across relation types and sorted by source/target,
/// followed by lexicographically sorted includes components. Only existing sources
/// participate; external source addresses are excluded. Cycles require existing
/// knowledge endpoints. Validated edges already exclude direct self-loops.
pub fn find_graph_problems(
    nodes: &BTreeSet<KnowledgeId>,
    edges: &BTreeSet<Edge>,
) -> Vec<GraphProblem> {
    let mut problems = find_dangling_targets(nodes, edges);
    problems.extend(find_includes_cycles(nodes, edges));
    problems
}

fn find_dangling_targets(
    nodes: &BTreeSet<KnowledgeId>,
    edges: &BTreeSet<Edge>,
) -> Vec<GraphProblem> {
    let pairs: BTreeSet<(&KnowledgeId, &KnowledgeId)> = edges
        .iter()
        .filter_map(|edge| {
            let EdgeTarget::Knowledge(target) = edge.get_target() else {
                return None;
            };
            (nodes.contains(edge.get_source()) && !nodes.contains(target))
                .then_some((edge.get_source(), target))
        })
        .collect();
    pairs
        .into_iter()
        .map(|(source, target)| GraphProblem::DanglingTarget {
            source: source.clone(),
            target: target.clone(),
        })
        .collect()
}

fn find_includes_cycles(
    nodes: &BTreeSet<KnowledgeId>,
    edges: &BTreeSet<Edge>,
) -> Vec<GraphProblem> {
    let graph = build_includes_graph(nodes, edges);
    let order = get_finishing_order(&graph.outgoing);
    let mut search = ComponentSearch {
        incoming: &graph.incoming,
        visited: vec![false; graph.nodes.len()],
        stack: Vec::new(),
    };
    let mut components = Vec::new();
    for root in order.into_iter().rev() {
        if search.visited[root] {
            continue;
        }
        let component = search.collect_component(root);
        if component.len() < 2 {
            continue;
        }
        let mut members: Vec<KnowledgeId> = component
            .into_iter()
            .map(|index| graph.nodes[index].clone())
            .collect();
        members.sort_unstable();
        components.push(members);
    }
    components.sort_unstable();
    components
        .into_iter()
        .map(|members| GraphProblem::IncludesCycle { members })
        .collect()
}

fn build_includes_graph<'a>(
    nodes: &'a BTreeSet<KnowledgeId>,
    edges: &BTreeSet<Edge>,
) -> IncludesGraph<'a> {
    let indices: BTreeMap<&KnowledgeId, usize> = nodes
        .iter()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    let mut graph = IncludesGraph {
        nodes: nodes.iter().collect(),
        outgoing: vec![Vec::new(); nodes.len()],
        incoming: vec![Vec::new(); nodes.len()],
    };
    for edge in edges {
        if edge.get_edge_type().as_str() != INCLUDES_EDGE_TYPE {
            continue;
        }
        let EdgeTarget::Knowledge(target) = edge.get_target() else {
            continue;
        };
        let (Some(&source), Some(&target)) = (indices.get(edge.get_source()), indices.get(target))
        else {
            continue;
        };
        graph.outgoing[source].push(target);
        graph.incoming[target].push(source);
    }
    graph
}

fn get_finishing_order(outgoing: &[Vec<usize>]) -> Vec<usize> {
    FinishingSearch {
        outgoing,
        visited: vec![false; outgoing.len()],
        order: Vec::with_capacity(outgoing.len()),
        stack: Vec::new(),
    }
    .collect_order()
}

impl FinishingSearch<'_> {
    fn collect_order(mut self) -> Vec<usize> {
        for root in 0..self.outgoing.len() {
            self.visit_root(root);
        }
        self.order
    }

    // Store the next child in each frame so finishing order follows DFS without recursion.
    fn visit_root(&mut self, root: usize) {
        if self.visited[root] {
            return;
        }
        self.visited[root] = true;
        self.stack.push((root, 0));
        while let Some((node, next_child)) = self.stack.pop() {
            let Some(&child) = self.outgoing[node].get(next_child) else {
                self.order.push(node);
                continue;
            };
            self.stack.push((node, next_child + 1));
            if !self.visited[child] {
                self.visited[child] = true;
                self.stack.push((child, 0));
            }
        }
    }
}

impl ComponentSearch<'_> {
    fn collect_component(&mut self, root: usize) -> Vec<usize> {
        let mut members = Vec::new();
        self.stack.push(root);
        self.visited[root] = true;
        while let Some(node) = self.stack.pop() {
            members.push(node);
            self.visit_parents(node);
        }
        members
    }

    fn visit_parents(&mut self, node: usize) {
        for &parent in &self.incoming[node] {
            if self.visited[parent] {
                continue;
            }
            self.visited[parent] = true;
            self.stack.push(parent);
        }
    }
}

#[cfg(test)]
mod tests {
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
}
