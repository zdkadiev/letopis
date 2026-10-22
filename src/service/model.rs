use crate::domain::{
    ByteSpan, Edge, Element, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, SourceUri,
    TreeOid,
};
use std::collections::BTreeSet;
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

/// A whole valid file's projection; raw Markdown is not retained.
#[derive(Debug)]
pub struct ParsedFile {
    pub elements: Vec<ParsedElement>,
    pub edges: BTreeSet<Edge>,
}

/// One parsed element with transient shared search text and its fingerprint.
#[derive(Debug)]
pub struct ParsedElement {
    pub element: Element,
    pub search_text: String,
    pub search_hash: Sha256,
}

/// Local diagnostic reasons; the CLI maps these into its existing error envelope.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiagnosticKind {
    InvalidUtf8,
    InvalidMetadata,
    InvalidAnchor,
    DuplicateAnchor,
    OrphanMetadata,
    NestedElement,
    InvalidLinkTitle,
    InvalidTarget,
    SelfLoop,
}
/// A file error with source coordinates where available and a fixed local message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub bytes: Option<ByteSpan>,
    pub message: String,
}
/// Root-relative regular-file observation used by change detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    pub size: u64,
    pub mtime_ns: i64,
    pub inode: u64,
    pub device: u64,
}
/// Exact file bytes with a verified observation and pre-read clock sample.
#[derive(Debug)]
pub struct CapturedFile {
    pub bytes: Vec<u8>,
    pub stat: FileStat,
    pub checked_at_ns: i64,
}
/// Counts for one refresh publication; all counts are zero for a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// Publication revision visible when this refresh returns.
    pub revision: u64,
    /// Whether this call committed a publication.
    pub published: bool,
    /// Added or changed file observations, including invalid files.
    pub files_updated: usize,
    /// Previously stored candidates removed by this publication.
    pub files_removed: usize,
    /// Invalid observations among `files_updated`.
    pub invalid_files_updated: usize,
}
