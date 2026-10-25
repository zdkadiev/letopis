use super::{ElementAddress, SearchFlag};
use crate::domain::{Edge, EdgeTarget, EdgeType, LineRange, RepoPath, SourceUri};
use std::{collections::BTreeSet, num::NonZeroUsize};

/// Internal bound shared by neighbor producers and consumers.
pub(crate) const NEIGHBOR_PAGE_QUANTUM: usize = 32;

/// Normalized starts, traversal selection and independent output bounds.
#[derive(Debug)]
pub struct TraversalRequest {
    /// Unique canonical start selectors, all validated before expansion.
    pub element_ids: BTreeSet<ElementAddress>,
    /// Relationship family to follow.
    pub mode: TraversalMode,
    /// Directions to follow, preserving original edge orientation.
    pub direction: TraversalDirection,
    /// Maximum number of transitions from any start, including containment.
    pub depth: u64,
    /// Maximum discovered elements, excluding starts.
    pub limit: NonZeroUsize,
    /// Optional intersection with the mode; empty means unrestricted.
    pub edge_types: BTreeSet<EdgeType>,
    /// Optional independent maximum of first-encounter edges.
    pub edge_limit: Option<NonZeroUsize>,
}
/// Local containing path or opaque remote source location.
#[derive(Debug, PartialEq, Eq)]
pub enum ElementLocation {
    /// Canonical repository-relative file path without source coordinates.
    Local(RepoPath),
    /// Verbatim remote source URL, never fetched.
    Remote(SourceUri),
}
/// One selected element's metadata and observed freshness in a pinned revision.
#[derive(Debug, PartialEq, Eq)]
pub struct TraversalLink {
    /// Canonical selected element identity.
    pub element_id: ElementAddress,
    /// Containing file or verbatim URL.
    pub path: ElementLocation,
    /// Pinned knowledge coordinates or addressed local source range.
    pub lines: Option<LineRange>,
    /// Shared cache publication revision.
    pub revision: u64,
    /// Deduplicated existing X-1 freshness reasons.
    pub flags: BTreeSet<SearchFlag>,
}
/// Metadata-only traversal output with optional original traversed edges.
#[derive(Debug, PartialEq, Eq)]
pub struct Traversal {
    /// Selected links in BFS discovery order, excluding starts.
    pub elements: Vec<TraversalLink>,
    /// Requested first-encounter edge triples, absent when unrequested.
    pub edges: Option<Vec<Edge>>,
}
impl ElementLocation {
    /// Return the normalized local path or verbatim remote URL.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Local(path) => path.as_str(),
            Self::Remote(uri) => uri.as_str(),
        }
    }
}

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
