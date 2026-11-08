//! Shared application data, requests, results and pure value rules.
use super::error::{
    CommandError, CommandErrorKind, CommandProblem, CommandResult, EmbeddingError,
    EmbeddingErrorKind, EmbeddingResult,
};
use crate::{
    domain::{
        BlobOid, Edge, EdgeTarget, EdgeType, Element, ElementKind, KnowledgeId, LineRange,
        RepoPath, Sha256, SourceUri, TreeOid,
    },
    utils::JsonValue,
};
use serde_json::{Map, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    num::{NonZeroU32, NonZeroUsize},
};

/// Default and maximum number of knowledge hits returned by each search channel.
pub(crate) const MAX_SEARCH_LIMIT: NonZeroUsize = match NonZeroUsize::new(100) {
    Some(limit) => limit,
    None => panic!("the fixed search limit must be positive"),
};

/// Internal bound shared by neighbor producers and consumers.
pub(crate) const NEIGHBOR_PAGE_QUANTUM: usize = 32;

/// Smallest embedding vector length a provider may return.
pub(crate) const MIN_DIMENSIONS: usize = 2;
/// Largest embedding vector length a provider may return.
pub(crate) const MAX_DIMENSIONS: usize = 4096;
const ROOT_FIELDS: [&str; 4] = ["provider", "model", "parameters", "max_retries"];

const ROOT_SECTION: &str = "embedding_model";
const DISCOVERY_SECTION: &str = "discovery";
const RESPECT_GITIGNORE: &str = "respect_gitignore";
const SECTION_FIELDS: [&str; 5] = ["enabled", "provider", "model", "parameters", "max_retries"];
const LEGACY_FIELDS: [&str; 4] = ["provider", "model", "parameters", "max_retries"];

/// Complete generated configuration written by initialization.
pub(crate) const GENERATED_CONFIG: &[u8] = b"{\n  \"embedding_model\": {\n    \"enabled\": false,\n    \"provider\": null,\n    \"model\": null,\n    \"parameters\": {},\n    \"max_retries\": 0\n  },\n  \"discovery\": {\n    \"respect_gitignore\": true\n  }\n}\n";

/// Transaction contexts in which a use case may begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionRequirement {
    /// Require no inherited read or write scope.
    Inactive,
    /// Permit an inherited read, while rejecting an inherited write.
    ReadOnly,
}

/// Searchable knowledge metadata; document text is never retained here.
///
/// Storage requires `id == uri` and reconstructs both from the persisted address.
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
    pub is_inserted: bool,
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

/// Stable file identity; mutable content and timestamps are deliberately excluded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileIdentity {
    /// Device number.
    pub device: u64,
    /// Inode number.
    pub inode: u64,
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

impl FileStat {
    /// Returns the file identity.
    ///
    /// Size and modification time are not part of the identity.
    pub fn get_identity(&self) -> FileIdentity {
        FileIdentity {
            device: self.device,
            inode: self.inode,
        }
    }
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
    pub is_published: bool,
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
    /// A valid configuration explicitly disables embedding execution.
    Disabled,
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

/// A normalized element selector whose membership is resolved by the service.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ElementAddress(String);

impl ElementAddress {
    /// Parses one element address.
    ///
    /// Absolute HTTP/HTTPS URLs keep their spelling; local knowledge and source addresses are
    /// normalized.
    ///
    /// # Arguments
    ///
    /// * `raw` — Address text to normalize or keep as an absolute HTTP/HTTPS URL.
    ///
    /// # Returns
    /// The normalized element address.
    ///
    /// # Errors
    /// Reports an invalid request for malformed absolute URLs or invalid local addresses.
    pub fn parse(raw: &str) -> CommandResult<Self> {
        let absolute_http = raw.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
        // Absolute URLs must not become local knowledge through path normalization.
        let normalized = if absolute_http {
            SourceUri::parse(raw).map(map_source_uri_to_string)
        } else {
            KnowledgeId::parse(raw)
                .map(map_knowledge_id_to_string)
                .or_else(|_| SourceUri::parse(raw).map(map_source_uri_to_string))
        }
        .map_err(map_error_to_invalid_element_address_command_error)?;
        Ok(Self(normalized))
    }

    /// Returns the normalized address.
    ///
    /// Case and Unicode are preserved.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Validation findings; nonempty findings are still a successful report.
#[derive(Debug)]
pub struct CheckReport {
    /// Safe findings for the requested scope.
    pub errors: Vec<CommandProblem>,
}

/// Metadata observed from one immutable snapshot.
#[derive(Debug)]
pub struct Inspection {
    /// Common immutable Git tree.
    pub snapshot: TreeOid,
    /// Metadata indexed by normalized element addresses.
    pub elements: BTreeMap<String, InspectedElement>,
}

/// Metadata only, without raw knowledge or source contents.
#[derive(Debug)]
pub struct InspectedElement {
    /// Root-relative file path or opaque remote source URL.
    pub path: ElementLocation,
    /// Inclusive source or element lines, when available.
    pub lines: Option<LineRange>,
    /// Exact element hash, or absent for an unavailable source.
    pub hash: Option<Sha256>,
}

/// Literal search text, normalized restrictions and bounded retrieval/result limit.
#[derive(Debug)]
pub struct SearchRequest {
    /// Uninterpreted user query, tokenized by the search service.
    pub query: String,
    /// Positive per-channel and final limit; search caps it at 100.
    pub limit: NonZeroUsize,
    /// Normalized source, revision and current-flag restrictions.
    pub filters: SearchFilters,
}

/// Observed knowledge and source freshness reasons from the existing X-1 contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchFlag {
    /// Current knowledge is missing, invalid, or differs from pinned metadata.
    Stale,
    /// Source bytes differ or the declared basis cannot be verified.
    SourceChanged,
    /// A complete local source address is unavailable.
    SourceNotFound,
}

/// Metadata-only knowledge link from one pinned cache revision.
#[derive(Debug, PartialEq, Eq)]
pub struct SearchLink {
    /// Normalized knowledge identity, including an anchor when present.
    pub element_id: KnowledgeId,
    /// Repository-relative containing file.
    pub path: RepoPath,
    /// Inclusive coordinates in the pinned revision.
    pub lines: LineRange,
    /// Cache publication revision shared by every link in the response.
    pub revision: u64,
    /// Unique observed freshness reasons; empty does not certify source provenance.
    pub flags: BTreeSet<SearchFlag>,
}

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

impl ElementLocation {
    /// Returns the local path or remote URL.
    ///
    /// A remote URL is returned verbatim.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Local(path) => path.as_str(),
            Self::Remote(uri) => uri.as_str(),
        }
    }
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

impl GraphNodeKey {
    /// Creates a node key for one read view.
    ///
    /// # Arguments
    ///
    /// * `handle` — Backend node handle valid only in its read view.
    ///
    /// # Returns
    /// The node key.
    pub fn new(handle: i64) -> Self {
        Self(handle)
    }

    /// Returns the backend node handle.
    ///
    /// The handle stays tied to the key's read view.
    pub fn get_handle(self) -> i64 {
        self.0
    }
}

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

/// One knowledge element's basis and source addresses from one consistent view.
#[derive(Debug, Clone, Copy)]
pub struct ProvenanceRequest<'a> {
    /// Author-declared common source tree, never replaced with current HEAD.
    pub source_snapshot: Option<&'a TreeOid>,
    /// Sources belonging to that same element and view.
    pub sources: &'a BTreeSet<SourceUri>,
}

/// Current provenance findings without source text or persistent state.
#[derive(Debug, PartialEq, Eq)]
pub struct ProvenanceObservation {
    /// Deduplicated existing X-1 reasons for rechecking the knowledge.
    pub flags: BTreeSet<SearchFlag>,
    /// Exact current causes for owner-scoped check diagnostics.
    pub issues: BTreeSet<ProvenanceIssue>,
}

/// A current validation finding rather than an infrastructure error.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum ProvenanceIssue {
    /// Referenced sources have no declared basis.
    MissingSnapshot,
    /// The declared root is absent or is not a valid tree for this repository.
    SnapshotUnavailable {
        /// Author-declared identity, preserved without substitution.
        snapshot: TreeOid,
    },
    /// The full local address is absent in either the current file or basis.
    SourceNotFound {
        /// The exact source address, including its inclusive range.
        source: SourceUri,
    },
}

/// Compare one source address in its knowledge's basis and an explicit revision.
#[derive(Debug, Clone, Copy)]
pub struct SourceRevisionRequest<'a> {
    /// Basis belonging to the same binding as `source`.
    pub source_snapshot: Option<&'a TreeOid>,
    /// This one address is selected identically from both trees.
    pub source: &'a SourceUri,
    /// Exact requested tree; missing or invalid roots are request errors.
    pub revision: &'a TreeOid,
}

/// Exact byte comparison, distinguishing unavailable inputs from inequality.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceRevisionComparison {
    /// Both selected byte sequences are available and equal.
    Equal,
    /// Both selected byte sequences are available and unequal.
    Different,
    /// A complete local path or range is absent in one tree.
    SourceNotFound,
    /// The knowledge has no usable declared basis.
    BasisUnavailable,
    /// Remote bytes are unknown because no fetch is performed.
    Unfetched,
}

/// Current observations; the database fields share one published read view.
#[derive(Debug)]
pub struct StatusReport {
    /// Published revision, or no cache publication.
    pub revision: Option<u64>,
    /// Distinct current knowledge hashes awaiting a usable vector.
    pub embedding_backlog: u64,
    /// Distinct current knowledge hashes with exhausted attempts and no vector.
    pub embedding_failed: u64,
    /// Best-effort recorded PIDs for observed-busy kernel locks.
    pub locks: Vec<HeldLock>,
    /// Deterministically ordered current safe diagnostics.
    pub problems: Vec<CommandProblem>,
}

/// Persistent kernel-lock responsibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockName {
    /// Cache publication exclusion.
    Index,
    /// Embedding drain exclusion.
    Embed,
}

/// A busy lock and its positive recorded PID; ownership is not atomic with PID reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldLock {
    /// Observed lock.
    pub name: LockName,
    /// Recorded positive PID, which may briefly belong to a previous owner.
    pub pid: NonZeroU32,
}

/// Select knowledge by whether its current X-1 flags require rechecking.
/// An empty flag set does not certify truth or remote availability.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RecheckFilter {
    /// Impose no restriction on current flags.
    #[default]
    Any,
    /// Require at least one current freshness flag.
    Required,
    /// Require no current freshness flags.
    NotRequired,
}

/// Normalized search restrictions; fields combine with AND and list values with OR.
/// Empty lists impose no restriction. Sources and revisions must match one binding.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchFilters {
    /// Restriction on current knowledge and source freshness flags.
    pub needs_recheck: RecheckFilter,
    /// Exact canonical source identities, deduplicated and ordered.
    pub sources: BTreeSet<SourceUri>,
    /// Complete requested tree OIDs, deduplicated in validation order.
    pub source_revisions: BTreeSet<TreeOid>,
}

impl SearchFilters {
    /// Returns whether the observed flags satisfy the recheck restriction.
    ///
    /// # Arguments
    ///
    /// * `flags` — Freshness flags to test against the restriction.
    pub(crate) fn is_matched_by_flags(&self, flags: &BTreeSet<SearchFlag>) -> bool {
        match self.needs_recheck {
            RecheckFilter::Any => true,
            RecheckFilter::Required => !flags.is_empty(),
            RecheckFilter::NotRequired => flags.is_empty(),
        }
    }
}

/// Supported finite protocol selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    /// Native Ollama embeddings.
    Ollama,
    /// OpenAI-compatible embeddings.
    Openai,
}

/// Parsed application configuration without process credentials.
#[derive(Clone)]
pub struct EmbeddingConfig {
    provider: ProviderKind,
    model: String,
    parameters: Map<String, Value>,
    identity: Sha256,
    max_attempts: u64,
    dimensions: Option<NonZeroUsize>,
}

impl fmt::Debug for EmbeddingConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddingConfig")
            .field("provider", &self.provider)
            .field("identity", &self.identity)
            .finish_non_exhaustive()
    }
}

/// Validated complete `.letopis/config.json` value.
#[derive(Debug, Clone)]
pub struct Configuration {
    embedding_model: EmbeddingState,
    discovery: DiscoveryConfig,
}

#[derive(Debug, Clone)]
enum EmbeddingState {
    Unconfigured { max_retries: u64 },
    Disabled(EmbeddingConfig),
    Enabled(EmbeddingConfig),
}

/// Selects whether standard Git ignore sources constrain untracked enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitIgnoreMode {
    /// Honor repository, per-directory and global Git exclusion sources.
    Standard,
    /// Return untracked paths without consulting standard exclusion sources.
    Disabled,
}

/// Author-controlled corpus enumeration policy, independent of embedding identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryConfig {
    respect_gitignore: bool,
}

impl Configuration {
    /// Parses the supported configuration sections.
    ///
    /// The embedding identity excludes discovery, enabled and the root wrapper.
    ///
    /// # Arguments
    ///
    /// * `bytes` — Complete configuration document bytes.
    ///
    /// # Returns
    /// The validated configuration.
    ///
    /// # Errors
    /// Rejects malformed or duplicate JSON, unknown fields, invalid sections and legacy flat
    /// documents.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let root = parse_config_root(bytes)?;
        let section = root
            .get(ROOT_SECTION)
            .ok_or_else(build_invalid_embedding_config_error)?;
        Ok(Self {
            embedding_model: parse_section(section)?,
            discovery: parse_discovery_section(&root)?,
        })
    }

    /// Returns whether embedding execution is enabled.
    pub fn is_enabled(&self) -> bool {
        matches!(self.embedding_model, EmbeddingState::Enabled(_))
    }

    /// Returns the configured retry count.
    ///
    /// Unconfigured settings return their retry count too.
    pub fn get_max_retries(&self) -> u64 {
        match &self.embedding_model {
            EmbeddingState::Unconfigured { max_retries } => *max_retries,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => {
                config.get_max_attempts().saturating_sub(1)
            }
        }
    }

    /// Returns the provider settings, including disabled configurations.
    pub fn get_embedding_config(&self) -> Option<&EmbeddingConfig> {
        match &self.embedding_model {
            EmbeddingState::Unconfigured { .. } => None,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => Some(config),
        }
    }

    /// Returns the discovery configuration.
    pub fn get_discovery_config(&self) -> DiscoveryConfig {
        self.discovery
    }

    /// Returns the provider settings, if configured.
    pub fn into_embedding_config(self) -> Option<EmbeddingConfig> {
        match self.embedding_model {
            EmbeddingState::Unconfigured { .. } => None,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => Some(config),
        }
    }
}

impl DiscoveryConfig {
    /// Parses the discovery configuration.
    ///
    /// Embedding settings are not required.
    ///
    /// # Arguments
    ///
    /// * `bytes` — Complete configuration document bytes.
    ///
    /// # Returns
    /// The parsed discovery policy.
    ///
    /// # Errors
    /// Rejects malformed or duplicate JSON, unknown fields and invalid discovery values.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        parse_discovery_section(&parse_config_root(bytes)?)
    }

    /// Returns whether standard Git ignores constrain untracked paths.
    pub fn is_gitignore_respected(&self) -> bool {
        self.respect_gitignore
    }
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
        }
    }
}

fn parse_config_root(bytes: &[u8]) -> EmbeddingResult<Map<String, Value>> {
    let JsonValue(value) = serde_json::from_slice::<JsonValue<true>>(bytes)
        .map_err(map_error_to_embedding_invalid_config_error)?;
    let Value::Object(root) = value else {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    };
    if root.keys().any(|key| LEGACY_FIELDS.contains(&key.as_str())) {
        return Err(EmbeddingError::new(EmbeddingErrorKind::MigrationRequired));
    }
    if root
        .keys()
        .any(|key| key != ROOT_SECTION && key != DISCOVERY_SECTION)
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(root)
}

fn parse_discovery_section(root: &Map<String, Value>) -> EmbeddingResult<DiscoveryConfig> {
    let Some(value) = root.get(DISCOVERY_SECTION) else {
        return Ok(DiscoveryConfig::default());
    };
    let section = value
        .as_object()
        .ok_or_else(build_invalid_embedding_config_error)?;
    if section.keys().any(|key| key != RESPECT_GITIGNORE) {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    let respect_gitignore = match section.get(RESPECT_GITIGNORE) {
        None => true,
        Some(Value::Bool(value)) => *value,
        _ => return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)),
    };
    Ok(DiscoveryConfig { respect_gitignore })
}

/// Metadata and current availability of one referenced source; contains no bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceObservation {
    /// Derived identity/creation time and freshly observed hash and line range.
    pub metadata: SourceMetadata,
    /// Availability of the addressed bytes during this explicit observation.
    pub availability: SourceAvailability,
}

/// Current byte availability, separate from source membership in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceAvailability {
    /// The complete addressed local bytes were captured successfully.
    Available,
    /// Referenced local bytes are absent, including an incomplete line range.
    SourceNotFound,
    /// The source is remote and no fetch was performed; availability is unknown.
    Unfetched,
}

/// Read-only classification of one exact snapshot root object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotState {
    /// The exact tree exists and its own bytes match its object identity.
    Available,
    /// The root object does not exist in the repository.
    Missing,
    /// The object has a different type or an incompatible repository OID format.
    Invalid,
}

/// One captured regular file. The adapter never reads its live path.
#[derive(Debug)]
pub struct SnapshotFile<'a> {
    /// Root-relative path in the resulting tree.
    pub path: &'a RepoPath,
    /// Exact captured bytes, without Git filters or normalization.
    pub bytes: &'a [u8],
}

/// One validated path and previously written blob in a snapshot request.
#[derive(Debug)]
pub struct SnapshotEntry {
    /// Root-relative path in the resulting tree.
    pub path: RepoPath,
    /// Exact captured bytes' blob identity.
    pub oid: BlobOid,
}

fn parse_section(settings: &Value) -> EmbeddingResult<EmbeddingState> {
    let section = settings
        .as_object()
        .ok_or_else(build_invalid_embedding_config_error)?;
    if section.len() != SECTION_FIELDS.len()
        || section
            .keys()
            .any(|key| !SECTION_FIELDS.contains(&key.as_str()))
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    let is_enabled = section
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(build_invalid_embedding_config_error)?;
    let max_retries = section
        .get("max_retries")
        .and_then(Value::as_u64)
        .ok_or_else(build_invalid_embedding_config_error)?;
    validate_retries(max_retries)?;
    let provider = get_optional_string_by_key(section, "provider")?;
    let model = get_optional_string_by_key(section, "model")?;
    let parameters = section
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or_else(build_invalid_embedding_config_error)?;
    match (provider, model) {
        (None, None) => parse_unconfigured(is_enabled, parameters, max_retries),
        (Some(provider), Some(model)) => {
            parse_configured(is_enabled, provider, model, parameters, max_retries)
        }
        _ => Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)),
    }
}

fn get_optional_string_by_key<'a>(
    section: &'a Map<String, Value>,
    field: &str,
) -> EmbeddingResult<Option<&'a str>> {
    match section.get(field) {
        Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        _ => Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)),
    }
}

fn parse_unconfigured(
    is_enabled: bool,
    parameters: &Map<String, Value>,
    max_retries: u64,
) -> EmbeddingResult<EmbeddingState> {
    if is_enabled || !parameters.is_empty() {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(EmbeddingState::Unconfigured { max_retries })
}

fn parse_configured(
    is_enabled: bool,
    provider: &str,
    model: &str,
    parameters: &Map<String, Value>,
    max_retries: u64,
) -> EmbeddingResult<EmbeddingState> {
    let projection = Value::Object(Map::from_iter([
        ("provider".into(), Value::String(provider.into())),
        ("model".into(), Value::String(model.into())),
        ("parameters".into(), Value::Object(parameters.clone())),
        ("max_retries".into(), Value::from(max_retries)),
    ]));
    let bytes =
        serde_json::to_vec(&projection).map_err(map_error_to_embedding_invalid_config_error)?;
    let config = EmbeddingConfig::parse(&bytes)?;
    Ok(if is_enabled {
        EmbeddingState::Enabled(config)
    } else {
        EmbeddingState::Disabled(config)
    })
}

fn validate_retries(retries: u64) -> EmbeddingResult<()> {
    retries
        .checked_add(1)
        .filter(|attempts| i64::try_from(*attempts).is_ok())
        .map(|_| ())
        .ok_or_else(build_invalid_embedding_config_error)
}

impl EmbeddingConfig {
    /// Parses the configured-settings projection.
    ///
    /// The identity is computed from the canonical JSON. This is not a config-file reader; use
    /// [`Configuration::parse`](super::Configuration::parse) for
    /// `.letopis/config.json`. Endpoint syntax is checked by the provider boundary.
    ///
    /// # Arguments
    ///
    /// * `bytes` — Configured-settings projection bytes in JSON.
    ///
    /// # Returns
    /// The validated configured-settings projection.
    ///
    /// # Errors
    /// Rejects malformed shapes, forbidden keys, invalid parameters, unsupported
    /// dimensions and budgets not exactly preserved by JCS or storage integers.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let JsonValue(value) = serde_json::from_slice::<JsonValue<true>>(bytes)
            .map_err(map_error_to_embedding_invalid_config_error)?;
        let root = value
            .as_object()
            .ok_or_else(build_invalid_embedding_config_error)?;
        if root.len() != ROOT_FIELDS.len()
            || root.keys().any(|key| !ROOT_FIELDS.contains(&key.as_str()))
        {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
        }
        let provider = match root.get("provider").and_then(Value::as_str) {
            Some("ollama") => ProviderKind::Ollama,
            Some("openai") => ProviderKind::Openai,
            _ => return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)),
        };
        let model = root
            .get("model")
            .and_then(Value::as_str)
            .filter(|model| !model.is_empty())
            .ok_or_else(build_invalid_embedding_config_error)?;
        let parameters = root
            .get("parameters")
            .and_then(Value::as_object)
            .ok_or_else(build_invalid_embedding_config_error)?;
        validate_parameters(parameters)?;
        let dimensions = get_dimensions(parameters)?;
        let retries = root
            .get("max_retries")
            .and_then(Value::as_u64)
            .ok_or_else(build_invalid_embedding_config_error)?;
        let max_attempts = retries
            .checked_add(1)
            .filter(|count| i64::try_from(*count).is_ok())
            .ok_or_else(build_invalid_embedding_config_error)?;
        let canonical = serde_json_canonicalizer::to_vec(&value)
            .map_err(map_error_to_embedding_invalid_config_error)?;
        verify_execution_identity(&value, &canonical)?;
        Ok(Self {
            provider,
            model: model.to_owned(),
            parameters: parameters.clone(),
            identity: Sha256::digest(&canonical),
            max_attempts,
            dimensions,
        })
    }

    /// Returns the provider protocol.
    pub fn get_provider(&self) -> ProviderKind {
        self.provider
    }

    /// Returns the configured model name.
    pub fn get_model(&self) -> &str {
        &self.model
    }

    /// Returns the configuration parameters, including unknown ones.
    pub fn get_parameters(&self) -> &Map<String, Value> {
        &self.parameters
    }

    /// Returns the hash of the canonical configuration.
    pub fn get_identity(&self) -> Sha256 {
        self.identity
    }

    /// Returns the total attempt budget, including the initial attempt.
    pub fn get_max_attempts(&self) -> u64 {
        self.max_attempts
    }

    /// Returns the requested vector dimensions.
    pub fn get_dimensions_by_parameters(&self) -> Option<NonZeroUsize> {
        self.dimensions
    }
}

fn validate_parameters(parameters: &Map<String, Value>) -> EmbeddingResult<()> {
    if parameters
        .get("base_url")
        .and_then(Value::as_str)
        .filter(|url| !url.is_empty())
        .is_none()
        || parameters.keys().any(|key| {
            key.eq_ignore_ascii_case("authorization") || key.eq_ignore_ascii_case("api_key")
        })
        || parameters.get("keep_alive").is_some_and(|v| !v.is_string())
        || parameters.get("options").is_some_and(|v| !v.is_object())
        || parameters
            .get("truncate")
            .is_some_and(|v| v.as_bool() != Some(false))
        || parameters
            .get("encoding_format")
            .is_some_and(|v| v.as_str() != Some("float"))
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(())
}

fn get_dimensions(parameters: &Map<String, Value>) -> EmbeddingResult<Option<NonZeroUsize>> {
    parameters
        .get("dimensions")
        .map(map_dimensions_value_to_dimension_count)
        .transpose()
}

fn verify_execution_identity(original: &Value, bytes: &[u8]) -> EmbeddingResult<()> {
    let JsonValue(canonical) = serde_json::from_slice::<JsonValue<false>>(bytes)
        .map_err(map_error_to_embedding_invalid_config_error)?;
    if !is_exact_integers(original, &canonical) {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(())
}

fn is_exact_integers(original: &Value, canonical: &Value) -> bool {
    match original {
        Value::Number(number) if number.is_u64() => number.as_u64() == canonical.as_u64(),
        Value::Number(number) if number.is_i64() => number.as_i64() == canonical.as_i64(),
        Value::Array(values) => canonical.as_array().is_some_and(|other| {
            values.len() == other.len()
                && values
                    .iter()
                    .zip(other)
                    .all(|(a, b)| is_exact_integers(a, b))
        }),
        Value::Object(values) => canonical.as_object().is_some_and(|other| {
            values.iter().all(|(key, value)| {
                other
                    .get(key)
                    .is_some_and(|other| is_exact_integers(value, other))
            })
        }),
        _ => true,
    }
}

/// Converts a configuration decoding failure to an invalid embedding configuration.
///
/// # Arguments
///
/// * `cause` — Configuration or JSON failure.
fn map_error_to_embedding_invalid_config_error<E: std::error::Error + Send + Sync + 'static>(
    cause: E,
) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
}

/// Converts a parse failure to an invalid element address command error.
///
/// # Arguments
///
/// * `cause` — Identity or source parsing failure.
fn map_error_to_invalid_element_address_command_error<
    E: std::error::Error + Send + Sync + 'static,
>(
    cause: E,
) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InvalidRequest,
        "invalid element address",
        cause,
    )
}

/// Converts a source URI to its owned string form.
///
/// # Arguments
///
/// * `source` — Source URI to project.
#[expect(
    clippy::needless_pass_by_value,
    reason = "map passes the parsed value by value to the mapper"
)]
fn map_source_uri_to_string(source: SourceUri) -> String {
    source.as_str().to_owned()
}

/// Converts a knowledge identity to its owned string form.
///
/// # Arguments
///
/// * `id` — Knowledge identity to project.
#[expect(
    clippy::needless_pass_by_value,
    reason = "map passes the parsed value by value to the mapper"
)]
fn map_knowledge_id_to_string(id: KnowledgeId) -> String {
    id.as_str().to_owned()
}

/// Converts a dimensions value to its validated dimension count.
///
/// # Arguments
///
/// * `value` — JSON value stored under the dimensions key.
fn map_dimensions_value_to_dimension_count(value: &Value) -> EmbeddingResult<NonZeroUsize> {
    let size = value
        .as_u64()
        .and_then(|size| usize::try_from(size).ok())
        .filter(|size| (MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(size))
        .ok_or_else(build_invalid_embedding_config_error)?;
    NonZeroUsize::new(size).ok_or_else(build_invalid_embedding_config_error)
}

/// Builds the error reported for an invalid embedding configuration.
fn build_invalid_embedding_config_error() -> EmbeddingError {
    EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)
}

#[cfg(test)]
#[path = "types_test.rs"]
mod tests;
