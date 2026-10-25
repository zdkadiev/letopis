use super::ElementAddress;
use crate::domain::{Edge, EdgeTarget, EdgeType};
use std::{collections::BTreeSet, num::NonZeroUsize};

/// Eligible graph relationships for a traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalMode {
    /// All relationships except containment.
    Relations,
    /// Only the includes relationship.
    Containment,
    /// All relationships.
    Both,
}
/// Directions selected by a traversal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraversalDirection {
    /// Follow declared edges.
    Outgoing,
    /// Follow edges toward their declaring knowledge.
    Incoming,
    /// Merge both directional streams.
    Both,
}
/// One independently ordered neighbor stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NeighborDirection {
    /// Reach the declared target.
    Outgoing,
    /// Reach the declaring knowledge.
    Incoming,
}
/// Opaque node handle, valid only within the read view that supplied it.
///
/// This is neither a domain identity nor a serializable address.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct GraphNodeKey(i64);
/// Minimal node identity and actual stored classification from a pinned view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNodeRef {
    /// Handle for further reads within this view.
    pub key: GraphNodeKey,
    /// Canonical domain identity classified by the stored node.
    pub element: EdgeTarget,
}
/// Strict continuation position within one fixed directional/filter stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NeighborCursor {
    /// Last returned relationship type.
    pub edge_type: EdgeType,
    /// Last reached neighbor's canonical identity.
    pub neighbor: ElementAddress,
}
/// A reached node and the original, unreversed edge that reaches it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphNeighbor {
    /// Minimal reached identity, without complete metadata.
    pub node: GraphNodeRef,
    /// Stored declaration, preserving source, target and type.
    pub edge: Edge,
}
/// One positive bounded page from a fixed neighbor stream.
#[derive(Debug)]
pub struct NeighborPageRequest<'a> {
    /// Handle supplied by this same pinned view.
    pub node: GraphNodeKey,
    /// Direction of this independently ordered stream.
    pub direction: NeighborDirection,
    /// Relationship family selection.
    pub mode: TraversalMode,
    /// Optional intersection; empty imposes no additional restriction.
    pub edge_types: &'a BTreeSet<EdgeType>,
    /// Last returned position with the same node, direction, mode and types.
    pub after: Option<&'a NeighborCursor>,
    /// Positive page bound, at most the internal page quantum.
    pub limit: NonZeroUsize,
}

impl GraphNodeKey {
    /// Wrap a backend node handle for its originating read view.
    pub fn new(value: i64) -> Self {
        Self(value)
    }
    /// Return the backend handle without extending its read-view lifetime.
    pub fn get_value(self) -> i64 {
        self.0
    }
}
