use super::{ByteSpan, KnowledgeId, LineRange, Sha256, SourceUri, TreeOid, ValueError};
use std::collections::BTreeSet;

/// A validated edge type, including the system types includes and external.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct EdgeType(String);
impl EdgeType {
    /// Validate the X-4 lowercase ASCII type grammar.
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
    /// The validated type name.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Graph targets distinguish knowledge references from source addresses.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum EdgeTarget {
    Knowledge(KnowledgeId),
    Source(SourceUri),
}
impl EdgeTarget {
    /// The target's normalized identity.
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
    /// Reject self-loops and source/knowledge target mismatches; dangling targets are allowed.
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
    /// The containing knowledge element.
    pub fn get_source(&self) -> &KnowledgeId {
        &self.source
    }
    /// The declared target, whether or not it currently exists.
    pub fn get_target(&self) -> &EdgeTarget {
        &self.target
    }
    /// The declared relation type.
    pub fn get_edge_type(&self) -> &EdgeType {
        &self.edge_type
    }
}

/// Derived classification, never stored in the canonical metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ElementKind {
    Vertex,
    Metavertex,
}
/// An element is a metavertex exactly when it has an outgoing includes edge.
pub fn derive_element_kind(id: &KnowledgeId, edges: &BTreeSet<Edge>) -> ElementKind {
    if edges
        .iter()
        .any(|e| e.get_source() == id && e.get_edge_type().as_str() == "includes")
    {
        ElementKind::Metavertex
    } else {
        ElementKind::Vertex
    }
}

/// Canonical metadata supplied by the author; no clock is consulted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ElementMetadata {
    pub created_at: u64,
    pub source_snapshot: Option<TreeOid>,
}
/// One knowledge element's metadata and exact source coordinates; no stored text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Element {
    pub id: KnowledgeId,
    pub metadata: ElementMetadata,
    pub bytes: ByteSpan,
    pub lines: LineRange,
    pub hash: Sha256,
}
