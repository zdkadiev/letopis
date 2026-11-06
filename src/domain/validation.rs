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

/// Returns current dangling targets and includes cycles without changing the inputs.
///
/// Dangling pairs are deduplicated across relation types and sorted by source/target,
/// followed by lexicographically sorted includes components. Only existing sources
/// participate; external source addresses are excluded. Cycles require existing
/// knowledge endpoints. Validated edges already exclude direct self-loops.
///
/// # Arguments
///
/// * `nodes` — the existing knowledge elements that may own or receive edges.
/// * `edges` — the declared edges to inspect.
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
#[path = "validation_test.rs"]
mod tests;
