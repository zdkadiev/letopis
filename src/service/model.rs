use crate::domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, SourceUri, TreeOid};
/// Searchable knowledge metadata; document text is never retained here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMetadata {
    pub id: KnowledgeId,
    pub uri: KnowledgeId,
    pub kind: ElementKind,
    pub hash: Sha256,
    pub created_at: u64,
    pub source_snapshot: Option<TreeOid>,
    pub lines: LineRange,
    pub search_hash: Sha256,
}
/// Caller-computed source projection, including sources with missing bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMetadata {
    pub uri: SourceUri,
    pub hash: Option<Sha256>,
    pub created_at: u64,
    pub lines: Option<LineRange>,
}
/// Persistable graph metadata, with distinct knowledge and source identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeMetadata {
    Knowledge(KnowledgeMetadata),
    Source(SourceMetadata),
}
/// A node and its stable FTS join key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNode {
    pub row_id: i64,
    pub metadata: NodeMetadata,
}
/// Change-detection state for a Markdown candidate, marked or unmarked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub path: RepoPath,
    pub size: u64,
    pub mtime_ns: i64,
    pub inode: u64,
    pub hash: Sha256,
    pub checked_at_ns: i64,
    pub valid: bool,
    pub error: Option<String>,
}
/// A contentless FTS match, ordered by native BM25 then identity.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMatch {
    pub id: KnowledgeId,
    pub row_id: i64,
    pub bm25: f64,
}
