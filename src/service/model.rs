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

/// Expected configuration comparison outcome, distinct from storage failures.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorOutcome<T> {
    /// The operation used the expected configuration identity.
    Current(T),
    /// The pinned configuration differs from the caller's identity.
    ConfigChanged,
}
/// Stable text-addressed vector key and whether this unit inserted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVectorKey {
    pub row_id: i64,
    pub inserted: bool,
}
/// A native cosine match joined to a currently published knowledge element.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    pub id: KnowledgeId,
    pub search_hash: Sha256,
    pub distance: f64,
}

/// Durable reservation count and the latest safe failure for one search hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingFailure {
    pub search_hash: Sha256,
    pub attempts: u64,
    pub error: Option<String>,
}
/// Latest safe query embedding failure, retained until explicit successful clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    pub at: u64,
    pub error: String,
}
