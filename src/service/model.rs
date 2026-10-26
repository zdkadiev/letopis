use super::{EmbeddingConfig, EmbeddingErrorKind};
use crate::domain::{
    ByteSpan, Edge, Element, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, SourceUri,
    TreeOid,
};
use std::{collections::BTreeSet, fmt, num::NonZeroUsize};
/// Searchable knowledge metadata; document text is never retained here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeMetadata {
    /// Identity of the knowledge element.
    pub id: KnowledgeId,
    /// Address of the element; equal to its identity for knowledge nodes.
    pub uri: KnowledgeId,
    /// Vertex or metavertex classification derived from outgoing edges.
    pub kind: ElementKind,
    /// SHA-256 fingerprint of the element as parsed.
    pub hash: Sha256,
    /// Author-supplied creation timestamp.
    pub created_at: u64,
    /// Git tree of the source snapshot declared by the author, if any.
    pub source_snapshot: Option<TreeOid>,
    /// Line range the element occupies in its file.
    pub lines: LineRange,
    /// SHA-256 of the search text, used as the shared vector key.
    pub search_hash: Sha256,
}
/// Derived source projection, including missing local or unfetched remote bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceMetadata {
    /// Canonical local address or verbatim opaque HTTP/HTTPS address.
    pub uri: SourceUri,
    /// SHA-256 of addressed bytes; `None` for missing local or unknown remote bytes.
    pub hash: Option<Sha256>,
    /// Earliest creation timestamp among the elements referencing this source.
    pub created_at: u64,
    /// Local addressed line range, including missing ranges; `None` for remote URLs.
    pub lines: Option<LineRange>,
}
/// Persistable graph metadata, with distinct knowledge and source identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeMetadata {
    /// A knowledge element declared in Markdown.
    Knowledge(KnowledgeMetadata),
    /// An external source address referenced by an edge.
    Source(SourceMetadata),
}
/// A node and its stable FTS join key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredNode {
    /// Stable row identifier used to join the full-text index.
    pub row_id: i64,
    /// Persisted node metadata.
    pub metadata: NodeMetadata,
}
/// Change-detection state for a Markdown candidate, marked or unmarked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    /// Root-relative path of the candidate file.
    pub path: RepoPath,
    /// Size in bytes at the last observation.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// Inode number at the last observation.
    pub inode: u64,
    /// SHA-256 of the file bytes.
    pub hash: Sha256,
    /// Clock sample, in nanoseconds since the Unix epoch, taken before the file was read.
    pub checked_at_ns: i64,
    /// Whether the file parsed into a valid projection.
    pub valid: bool,
    /// Safe local message when the file is invalid.
    pub error: Option<String>,
}
/// A contentless FTS match, ordered by native BM25 then identity.
#[derive(Debug, Clone, PartialEq)]
pub struct TextMatch {
    /// Identity of the matching knowledge element.
    pub id: KnowledgeId,
    /// Row identifier of the matching node.
    pub row_id: i64,
    /// Native BM25 score; lower values rank first.
    pub bm25: f64,
}

/// Expected configuration comparison outcome, distinct from storage failures.
#[derive(Debug, Clone, PartialEq)]
pub enum VectorOutcome<T> {
    /// The operation used the expected configuration identity.
    Current(T),
    /// The stored configuration differs from the caller's identity or prior observation.
    ConfigChanged,
}
/// Stable text-addressed vector key and whether this unit inserted it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredVectorKey {
    /// Row identifier of the vector key.
    pub row_id: i64,
    /// Whether this unit created the key rather than finding an existing one.
    pub inserted: bool,
}
/// A native cosine match joined to a currently published knowledge element.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorMatch {
    /// Identity of the matching knowledge element.
    pub id: KnowledgeId,
    /// Search hash whose vector matched.
    pub search_hash: Sha256,
    /// Native cosine distance; lower values are closer.
    pub distance: f64,
}

/// Durable reservation count and the latest safe failure for one search hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingFailure {
    /// Search hash the attempts were reserved for.
    pub search_hash: Sha256,
    /// Number of durably reserved attempts.
    pub attempts: u64,
    /// Latest safe failure message, if any.
    pub error: Option<String>,
}
/// Latest safe query embedding failure, retained until explicit successful clear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    /// Unix seconds at which the failure was recorded.
    pub at: u64,
    /// Safe failure message.
    pub error: String,
}

/// One eligible knowledge row; equal search hashes can have several representatives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmbeddingWork {
    /// Identity of one knowledge element carrying this search hash.
    pub id: KnowledgeId,
    /// Search hash that still needs a vector.
    pub search_hash: Sha256,
}

/// A whole valid file's projection; raw Markdown is not retained.
#[derive(Debug)]
pub struct ParsedFile {
    /// Knowledge elements in document order.
    pub elements: Vec<ParsedElement>,
    /// Graph edges declared by the file.
    pub edges: BTreeSet<Edge>,
}

/// One parsed element with transient shared search text and its fingerprint.
#[derive(Debug)]
pub struct ParsedElement {
    /// Parsed element metadata and source coordinates.
    pub element: Element,
    /// Transient text shared by search and embedding.
    pub search_text: String,
    /// SHA-256 of `search_text`.
    pub search_hash: Sha256,
}

/// Local diagnostic reasons; the CLI maps these into its existing error envelope.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiagnosticKind {
    /// The file is not valid UTF-8.
    InvalidUtf8,
    /// A metadata comment is malformed or not alone on its line.
    InvalidMetadata,
    /// An explicit heading anchor is not allowed.
    InvalidAnchor,
    /// Two headings declare the same explicit anchor.
    DuplicateAnchor,
    /// Metadata does not immediately follow an anchored heading.
    OrphanMetadata,
    /// One knowledge element lies inside another.
    NestedElement,
    /// A `letopis:` link title is malformed.
    InvalidLinkTitle,
    /// A link target is not a valid graph address.
    InvalidTarget,
    /// An element links to itself.
    SelfLoop,
}
impl DiagnosticKind {
    /// Return the fixed parser message without document contents.
    pub fn get_message(&self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "Markdown is not UTF-8",
            Self::InvalidMetadata => "invalid letopis metadata",
            Self::InvalidAnchor => "invalid explicit anchor",
            Self::DuplicateAnchor => "duplicate explicit anchor",
            Self::OrphanMetadata => "metadata does not immediately follow an anchored heading",
            Self::NestedElement => "knowledge elements cannot be nested",
            Self::InvalidLinkTitle => "invalid letopis link title",
            Self::InvalidTarget => "invalid graph target",
            Self::SelfLoop => "knowledge cannot link to itself",
        }
    }

    /// Return the stable machine-readable parser reason.
    pub fn get_code(&self) -> &'static str {
        match self {
            Self::InvalidUtf8 => "invalid_utf8",
            Self::InvalidMetadata => "invalid_metadata",
            Self::InvalidAnchor => "invalid_anchor",
            Self::DuplicateAnchor => "duplicate_anchor",
            Self::OrphanMetadata => "orphan_metadata",
            Self::NestedElement => "nested_element",
            Self::InvalidLinkTitle => "invalid_link_title",
            Self::InvalidTarget => "invalid_target",
            Self::SelfLoop => "self_loop",
        }
    }
}

/// A file error with source coordinates where available and a fixed local message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    /// Machine-readable reason.
    pub kind: DiagnosticKind,
    /// Byte range of the offending text, where it can be located.
    pub bytes: Option<ByteSpan>,
    /// Fixed local message, free of file contents.
    pub message: String,
}
/// Root-relative regular-file observation used by change detection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    /// Size in bytes.
    pub size: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub mtime_ns: i64,
    /// Inode number.
    pub inode: u64,
    /// Device number.
    pub device: u64,
}
/// Exact file bytes with a verified observation and pre-read clock sample.
#[derive(Debug)]
pub struct CapturedFile {
    /// Exact file bytes.
    pub bytes: Vec<u8>,
    /// Observation verified around the read.
    pub stat: FileStat,
    /// Clock sample, in nanoseconds since the Unix epoch, taken before the read.
    pub checked_at_ns: i64,
}
/// Counts for one refresh or full reindex publication; all counts are zero for a no-op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshOutcome {
    /// Publication revision visible when this refresh returns.
    pub revision: u64,
    /// Whether this call committed a publication.
    pub published: bool,
    /// Selected file observations, including every existing candidate for a full reindex.
    pub files_updated: usize,
    /// Previously stored candidates removed by this publication.
    pub files_removed: usize,
    /// Invalid observations among `files_updated`.
    pub invalid_files_updated: usize,
}

/// Current readiness after validating and synchronizing configuration.
#[derive(Debug)]
pub enum ConfigState {
    /// No configuration file exists.
    Missing,
    /// Configuration cannot currently be used; existing vectors remain intact.
    Invalid(EmbeddingErrorKind),
    /// A valid configuration observation was superseded before synchronization.
    Changed,
    /// Validated configuration synchronized with vector storage.
    Ready(EmbeddingConfig),
}

/// Result of one synchronous document drain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrainOutcome {
    /// All eligible work is stored, skipped, or exhausted.
    Complete,
    /// Configuration is absent or invalid.
    Unavailable,
    /// Another process owns the drain lock.
    AlreadyRunning,
    /// Committed vector identity changed during this invocation.
    ConfigChanged,
}

/// Query vectors remain transient and never consume document reservations.
#[derive(Debug, PartialEq)]
pub enum QueryEmbedding {
    /// A validated vector under its captured configuration identity.
    Available { identity: Sha256, vector: Vec<f32> },
    /// Search can continue without vector ranking.
    Unavailable,
}

/// Immutable configuration and vector dimensions captured for query execution.
pub struct QueryEmbeddingRequest<'a> {
    /// Query text sent once to the configured provider; never persisted.
    pub text: &'a str,
    /// Validated caller-captured configuration; execution never rereads or resets it.
    pub config: &'a EmbeddingConfig,
    /// Native vector dimensions observed by the caller's read view, if established.
    pub dimensions: Option<NonZeroUsize>,
}

impl fmt::Debug for QueryEmbeddingRequest<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryEmbeddingRequest")
            .field("config", &self.config)
            .field("dimensions", &self.dimensions)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::{EmbeddingConfig, QueryEmbeddingRequest};
    use std::num::NonZeroUsize;

    #[test]
    fn query_request_debug_omits_text_and_provider_parameters() {
        // Arrange: each private field has an independent recognizable sentinel.
        let config = EmbeddingConfig::parse(
            br#"{"provider":"ollama","model":"PRIVATE_MODEL","parameters":{"base_url":"http://PRIVATE_ENDPOINT"},"max_retries":0}"#,
        )
        .unwrap();
        let request = QueryEmbeddingRequest {
            text: "PRIVATE_QUERY",
            config: &config,
            dimensions: NonZeroUsize::new(2),
        };
        // Act
        let diagnostic = format!("{request:?}");
        // Assert
        for private in ["PRIVATE_QUERY", "PRIVATE_MODEL", "PRIVATE_ENDPOINT"] {
            assert!(!diagnostic.contains(private));
        }
        assert!(diagnostic.starts_with("QueryEmbeddingRequest {"));
        assert!(diagnostic.contains("dimensions: Some(2)"));
    }
}
