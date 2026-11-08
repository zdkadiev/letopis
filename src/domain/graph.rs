use super::{ByteSpan, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid, ValueError};
use std::collections::BTreeSet;

/// The system relation that defines knowledge containment.
pub(crate) const INCLUDES_EDGE_TYPE: &str = "includes";

/// A validated edge type, including the system types includes and external.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeType(String);

impl EdgeType {
    /// Parses a type name using the X-4 lowercase ASCII grammar.
    ///
    /// # Arguments
    ///
    /// * `raw` — the candidate type name to validate.
    ///
    /// # Errors
    /// Returns [`ValueError::InvalidType`] unless the name starts with a lowercase
    /// ASCII letter and contains only lowercase ASCII letters, digits or `_`.
    pub fn parse(raw: &str) -> Result<Self, ValueError> {
        if !raw.as_bytes().first().is_some_and(u8::is_ascii_lowercase)
            || !raw
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        {
            return Err(ValueError::InvalidType);
        }
        Ok(Self(raw.into()))
    }

    /// Returns the validated type name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Graph targets distinguish knowledge references from source addresses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeTarget {
    /// Another knowledge element.
    Knowledge(KnowledgeId),
    /// An external source address.
    Source(SourceUri),
}

impl EdgeTarget {
    /// Returns the target's normalized identity.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Knowledge(k) => k.as_str(),
            Self::Source(s) => s.as_str(),
        }
    }
}

/// Edge identity is exactly the source/target/type triple.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    source: KnowledgeId,
    target: EdgeTarget,
    edge_type: EdgeType,
}

impl Edge {
    /// Creates an edge, rejecting self-loops and target/type mismatches.
    /// Dangling targets are allowed.
    ///
    /// # Arguments
    ///
    /// * `source` — the knowledge element that owns the edge.
    /// * `target` — the declared edge endpoint, either another knowledge element or a source
    ///   address.
    /// * `edge_type` — the validated relation type that determines the allowed target kind.
    ///
    /// # Errors
    /// Returns [`ValueError::SelfLoop`] when source and target identities match,
    /// or [`ValueError::InvalidTarget`] when `external` targets knowledge or
    /// another edge type targets a source address.
    pub fn new(
        source: KnowledgeId,
        target: EdgeTarget,
        edge_type: EdgeType,
    ) -> Result<Self, ValueError> {
        if source.as_str() == target.as_str() {
            return Err(ValueError::SelfLoop);
        }
        if (edge_type.as_str() == "external") != matches!(target, EdgeTarget::Source(_)) {
            return Err(ValueError::InvalidTarget);
        }
        Ok(Self {
            source,
            target,
            edge_type,
        })
    }

    /// Returns the containing knowledge element.
    pub fn get_source(&self) -> &KnowledgeId {
        &self.source
    }

    /// Returns the declared target, whether or not it currently exists.
    pub fn get_target(&self) -> &EdgeTarget {
        &self.target
    }

    /// Returns the declared relation type.
    pub fn get_edge_type(&self) -> &EdgeType {
        &self.edge_type
    }
}

/// Derived classification, never stored in the canonical metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    /// An element with no outgoing `includes` edge.
    Vertex,
    /// An element with an outgoing `includes` edge.
    Metavertex,
}

/// Returns an element's kind from its outgoing includes edges.
/// The kind is `Metavertex` exactly when an outgoing includes edge exists.
///
/// # Arguments
///
/// * `id` — the element whose outgoing edges are inspected.
/// * `edges` — the full edge set searched for an outgoing includes edge from `id`.
pub fn derive_element_kind(id: &KnowledgeId, edges: &BTreeSet<Edge>) -> ElementKind {
    if edges
        .iter()
        .any(|e| e.get_source() == id && e.get_edge_type().as_str() == INCLUDES_EDGE_TYPE)
    {
        ElementKind::Metavertex
    } else {
        ElementKind::Vertex
    }
}

/// Canonical metadata supplied by the author; no clock is consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMetadata {
    /// Author-supplied creation timestamp.
    pub created_at: u64,
    /// Git tree of the source snapshot declared by the author, if any.
    pub source_snapshot: Option<TreeOid>,
}

/// One knowledge element's metadata and exact source coordinates; no stored text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    /// Identity of the element.
    pub id: KnowledgeId,
    /// Author-supplied metadata.
    pub metadata: ElementMetadata,
    /// Byte range the element occupies in its file.
    pub bytes: ByteSpan,
    /// Line range the element occupies in its file.
    pub lines: LineRange,
    /// SHA-256 fingerprint of the element.
    pub hash: Sha256,
}

/// Ordered partial traversal data, independent of storage and traversal execution.
///
/// Selected elements are deduplicated and omit starts. Optional original edges
/// can mention starts or other endpoints absent from the selected element list.
#[derive(Debug, PartialEq, Eq)]
pub struct Graph {
    elements: Vec<EdgeTarget>,
    edges: Option<Vec<Edge>>,
}

impl Graph {
    /// Creates a graph from an already deduplicated traversal result without I/O.
    ///
    /// # Arguments
    ///
    /// * `elements` — selected target identities in BFS discovery order, already deduplicated.
    /// * `edges` — requested original edges, or `None` when edges were not requested.
    pub(crate) fn new(elements: Vec<EdgeTarget>, edges: Option<Vec<Edge>>) -> Self {
        Self { elements, edges }
    }

    /// Returns selected identities in BFS discovery order.
    pub fn get_elements(&self) -> &[EdgeTarget] {
        &self.elements
    }

    /// Returns requested edges in first-encounter order, or none when unrequested.
    pub fn get_edges(&self) -> Option<&[Edge]> {
        self.edges.as_deref()
    }

    /// Returns the selected identities and optional original edges, consuming the graph.
    pub fn into_parts(self) -> (Vec<EdgeTarget>, Option<Vec<Edge>>) {
        (self.elements, self.edges)
    }
}
