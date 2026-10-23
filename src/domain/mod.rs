//! Validated knowledge identities and graph values, independent of adapters.
mod graph;
mod identity;
mod sha256;
mod value;
pub use graph::{
    Edge, EdgeTarget, EdgeType, Element, ElementKind, ElementMetadata, derive_element_kind,
};
pub use identity::{Anchor, KnowledgeId, RepoPath, SourceUri};
pub use sha256::Sha256;
pub use value::{BlobOid, ByteSpan, LineRange, TreeOid, ValueError};
pub(crate) use value::{SHA1_OID_HEX_LEN, SHA256_OID_HEX_LEN};
