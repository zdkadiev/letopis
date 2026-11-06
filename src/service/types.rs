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

/// Transaction contexts in which a use case may begin.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionRequirement {
    /// Require no inherited read or write scope.
    Inactive,
    /// Permit an inherited read, while rejecting an inherited write.
    ReadOnly,
}

/// Complete generated configuration written by initialization.
pub(crate) const GENERATED_CONFIG: &[u8] = b"{\n  \"embedding_model\": {\n    \"enabled\": false,\n    \"provider\": null,\n    \"model\": null,\n    \"parameters\": {},\n    \"max_retries\": 0\n  },\n  \"discovery\": {\n    \"respect_gitignore\": true\n  }\n}\n";

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
    /// Return the stable identity without mutable content observations.
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
    /// Normalize local knowledge/source addresses or preserve opaque absolute HTTP/HTTPS spelling.
    ///
    /// # Errors
    /// Returns invalid request for malformed absolute URLs or invalid local knowledge/source addresses.
    pub fn parse(raw: &str) -> CommandResult<Self> {
        let absolute_http = raw.split_once("://").is_some_and(|(scheme, _)| {
            scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https")
        });
        // Absolute URLs must not become local knowledge through path normalization.
        let normalized = if absolute_http {
            SourceUri::parse(raw).map(|source| source.as_str().to_owned())
        } else {
            KnowledgeId::parse(raw)
                .map(|id| id.as_str().to_owned())
                .or_else(|_| SourceUri::parse(raw).map(|source| source.as_str().to_owned()))
        }
        .map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InvalidRequest,
                "invalid element address",
                cause,
            )
        })?;
        Ok(Self(normalized))
    }
    /// Return the normalized selector, preserving case and Unicode.
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

/// Literal search text, normalized restrictions and final result bound.
#[derive(Debug)]
pub struct SearchRequest {
    /// Uninterpreted user query, tokenized by the search service.
    pub query: String,
    /// Maximum links after combining complete candidate channels.
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
    /// Return the normalized local path or verbatim remote URL.
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
    /// Wrap a backend node handle for its originating read view.
    pub fn new(value: i64) -> Self {
        Self(value)
    }
    /// Return the backend handle without extending its read-view lifetime.
    pub fn get_value(self) -> i64 {
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
    /// Return whether the observed flags satisfy the recheck restriction.
    pub(crate) fn matches_flags(&self, flags: &BTreeSet<SearchFlag>) -> bool {
        match self.needs_recheck {
            RecheckFilter::Any => true,
            RecheckFilter::Required => !flags.is_empty(),
            RecheckFilter::NotRequired => flags.is_empty(),
        }
    }
}

/// A tree whose complete local closure has a confirmed permanent service ref.
#[derive(Debug)]
pub struct RetainedTree {
    oid: TreeOid,
}

impl RetainedTree {
    pub(crate) fn new(oid: TreeOid) -> Self {
        Self { oid }
    }
    /// The exact retained tree OID.
    pub fn get_oid(&self) -> &TreeOid {
        &self.oid
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
    /// Enumerate untracked paths without consulting standard exclusion sources.
    Disabled,
}

/// Author-controlled corpus enumeration policy, independent of embedding identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DiscoveryConfig {
    respect_gitignore: bool,
}

impl Configuration {
    /// Parse and validate both supported configuration sections.
    ///
    /// Configured embedding identity excludes discovery, enabled and the root wrapper.
    ///
    /// # Errors
    /// Rejects malformed/duplicate JSON, unknown fields, invalid sections and legacy flat documents.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let root = parse_config_root(bytes)?;
        let section = root
            .get(ROOT_SECTION)
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
        Ok(Self {
            embedding_model: parse_section(section)?,
            discovery: parse_discovery_section(&root)?,
        })
    }

    /// Return whether configured embedding execution is enabled.
    pub fn get_enabled(&self) -> bool {
        matches!(self.embedding_model, EmbeddingState::Enabled(_))
    }

    /// Return the configured retry count, including unconfigured settings.
    pub fn get_max_retries(&self) -> u64 {
        match &self.embedding_model {
            EmbeddingState::Unconfigured { max_retries } => *max_retries,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => {
                config.get_max_attempts().saturating_sub(1)
            }
        }
    }

    /// Return retained provider settings, including disabled configurations.
    pub fn get_embedding_config(&self) -> Option<&EmbeddingConfig> {
        match &self.embedding_model {
            EmbeddingState::Unconfigured { .. } => None,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => Some(config),
        }
    }

    /// Return the effective discovery policy.
    pub fn get_discovery(&self) -> DiscoveryConfig {
        self.discovery
    }

    /// Consume the configuration and return retained provider settings, if configured.
    pub fn into_embedding_config(self) -> Option<EmbeddingConfig> {
        match self.embedding_model {
            EmbeddingState::Unconfigured { .. } => None,
            EmbeddingState::Disabled(config) | EmbeddingState::Enabled(config) => Some(config),
        }
    }
}

impl DiscoveryConfig {
    /// Parse discovery without requiring valid embedding settings.
    ///
    /// # Errors
    /// Rejects malformed/duplicate JSON, unknown root/discovery fields and invalid discovery types.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        parse_discovery_section(&parse_config_root(bytes)?)
    }

    /// Return whether standard Git exclusion sources constrain untracked paths.
    pub fn get_respect_gitignore(&self) -> bool {
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
    let JsonValue(value) =
        serde_json::from_slice::<JsonValue<true>>(bytes).map_err(map_config_error)?;
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
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
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

fn parse_section(value: &Value) -> EmbeddingResult<EmbeddingState> {
    let section = value
        .as_object()
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    if section.len() != SECTION_FIELDS.len()
        || section
            .keys()
            .any(|key| !SECTION_FIELDS.contains(&key.as_str()))
    {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    let enabled = section
        .get("enabled")
        .and_then(Value::as_bool)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    let max_retries = section
        .get("max_retries")
        .and_then(Value::as_u64)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    validate_retries(max_retries)?;
    let provider = get_optional_string(section, "provider")?;
    let model = get_optional_string(section, "model")?;
    let parameters = section
        .get("parameters")
        .and_then(Value::as_object)
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
    match (provider, model) {
        (None, None) => parse_unconfigured(enabled, parameters, max_retries),
        (Some(provider), Some(model)) => {
            parse_configured(enabled, provider, model, parameters, max_retries)
        }
        _ => Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)),
    }
}

fn get_optional_string<'a>(
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
    enabled: bool,
    parameters: &Map<String, Value>,
    max_retries: u64,
) -> EmbeddingResult<EmbeddingState> {
    if enabled || !parameters.is_empty() {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(EmbeddingState::Unconfigured { max_retries })
}

fn parse_configured(
    enabled: bool,
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
    let bytes = serde_json::to_vec(&projection).map_err(map_config_error)?;
    let config = EmbeddingConfig::parse(&bytes)?;
    Ok(if enabled {
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
        .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
}

impl EmbeddingConfig {
    /// Parse the internal four-field configured-settings projection and compute
    /// its RFC 8785 identity. This is not a config-file reader; use
    /// [`Configuration::parse`](super::Configuration::parse) for
    /// `.letopis/config.json`. Endpoint syntax is checked by the provider boundary.
    ///
    /// # Errors
    /// Rejects malformed shapes, forbidden keys, invalid parameters, unsupported
    /// dimensions and budgets not exactly preserved by JCS or storage integers.
    pub fn parse(bytes: &[u8]) -> EmbeddingResult<Self> {
        let JsonValue(value) =
            serde_json::from_slice::<JsonValue<true>>(bytes).map_err(map_config_error)?;
        let root = value
            .as_object()
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
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
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
        let parameters = root
            .get("parameters")
            .and_then(Value::as_object)
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
        validate_parameters(parameters)?;
        let dimensions = get_dimensions(parameters)?;
        let retries = root
            .get("max_retries")
            .and_then(Value::as_u64)
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
        let max_attempts = retries
            .checked_add(1)
            .filter(|count| i64::try_from(*count).is_ok())
            .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
        let canonical = serde_json_canonicalizer::to_vec(&value).map_err(map_config_error)?;
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
    /// Return the selected protocol.
    pub fn get_provider(&self) -> ProviderKind {
        self.provider
    }
    /// Return the configured model name.
    pub fn get_model(&self) -> &str {
        &self.model
    }
    /// Return allowed configuration values, including preserved unknown parameters.
    pub fn get_parameters(&self) -> &Map<String, Value> {
        &self.parameters
    }
    /// Return the hash of the complete canonical configuration.
    pub fn get_identity(&self) -> Sha256 {
        self.identity
    }
    /// Return the checked total attempt budget, including the initial attempt.
    pub fn get_max_attempts(&self) -> u64 {
        self.max_attempts
    }
    /// Return explicitly requested native dimensions.
    pub fn get_dimensions(&self) -> Option<NonZeroUsize> {
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
        .map(|value| {
            let size = value
                .as_u64()
                .and_then(|size| usize::try_from(size).ok())
                .filter(|size| (MIN_DIMENSIONS..=MAX_DIMENSIONS).contains(size))
                .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))?;
            NonZeroUsize::new(size)
                .ok_or_else(|| EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
        })
        .transpose()
}

fn verify_execution_identity(original: &Value, bytes: &[u8]) -> EmbeddingResult<()> {
    let JsonValue(canonical) =
        serde_json::from_slice::<JsonValue<false>>(bytes).map_err(map_config_error)?;
    if !has_exact_integers(original, &canonical) {
        return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
    }
    Ok(())
}

fn has_exact_integers(original: &Value, canonical: &Value) -> bool {
    match original {
        Value::Number(number) if number.is_u64() => number.as_u64() == canonical.as_u64(),
        Value::Number(number) if number.is_i64() => number.as_i64() == canonical.as_i64(),
        Value::Array(values) => canonical.as_array().is_some_and(|other| {
            values.len() == other.len()
                && values
                    .iter()
                    .zip(other)
                    .all(|(a, b)| has_exact_integers(a, b))
        }),
        Value::Object(values) => canonical.as_object().is_some_and(|other| {
            values.iter().all(|(key, value)| {
                other
                    .get(key)
                    .is_some_and(|other| has_exact_integers(value, other))
            })
        }),
        _ => true,
    }
}

fn map_config_error(cause: impl std::error::Error + Send + Sync + 'static) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InvalidConfig, cause)
}

#[cfg(test)]
mod model_tests {
    use super::{EmbeddingConfig, QueryEmbeddingRequest};
    use std::num::NonZeroUsize;

    #[test]
    fn query_request_debug_omits_text_and_provider_parameters() {
        // Given: each private field has an independent recognizable sentinel.
        let config = EmbeddingConfig::parse(
            br#"{"provider":"ollama","model":"PRIVATE_MODEL","parameters":{"base_url":"http://PRIVATE_ENDPOINT"},"max_retries":0}"#,
        )
        .unwrap();
        let request = QueryEmbeddingRequest {
            text: "PRIVATE_QUERY",
            config: &config,
            dimensions: NonZeroUsize::new(2),
        };
        // When
        let diagnostic = format!("{request:?}");
        // Then
        for private in ["PRIVATE_QUERY", "PRIVATE_MODEL", "PRIVATE_ENDPOINT"] {
            assert!(!diagnostic.contains(private));
        }
        assert!(diagnostic.starts_with("QueryEmbeddingRequest {"));
        assert!(diagnostic.contains("dimensions: Some(2)"));
    }
}

#[cfg(test)]
mod command_tests {
    use super::{CommandError, CommandErrorKind, ElementAddress};
    use std::error::Error;

    #[test]
    fn preserve_local_anchor_and_range_normalization() {
        // Given
        for (raw, expected) in [
            ("./x.md", "x.md"),
            ("a/../x.md#A", "x.md#A"),
            ("./source#L01-L02", "source#L1-L2"),
            ("résumé.MD", "résumé.MD"),
            ("x.md#Lfoo-Lbar", "x.md#Lfoo-Lbar"),
        ] {
            // When / Then
            assert_eq!(ElementAddress::parse(raw).unwrap().as_str(), expected);
        }
        for raw in [
            "",
            "/x",
            "../x",
            "x#L0-L1",
            "x#L2-L1",
            "x.md#",
            "x#anchor",
            "x#L1-L18446744073709551616",
            "x\u{0}",
        ] {
            // When / Then
            assert_eq!(
                ElementAddress::parse(raw).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn preserve_opaque_http_addresses() {
        // Given
        for raw in [
            "https://Host/a?q=1#L0-L2",
            "HTTP://Host/a#Anchor",
            "https://Host",
            "https://Host/a.md",
            "HtTp://Host/a.markdown#A",
        ] {
            // When / Then
            assert_eq!(ElementAddress::parse(raw).unwrap().as_str(), raw);
        }
    }

    #[test]
    fn round_trip_http_prefix_knowledge_addresses() {
        // Given
        for (raw, expected) in [
            ("http:notes.md", "http:notes.md"),
            ("./http:notes.md#A", "http:notes.md#A"),
            ("dir/../HTTP:notes.markdown#A", "HTTP:notes.markdown#A"),
            ("https:/Host/a.md", "https:/Host/a.md"),
        ] {
            // When
            let parsed = ElementAddress::parse(raw).unwrap();
            // Then
            assert_eq!(parsed.as_str(), expected);
            assert_eq!(ElementAddress::parse(expected).unwrap(), parsed);
        }
    }

    #[test]
    fn reject_malformed_absolute_urls_and_source_http_fallbacks() {
        // Given
        for raw in [
            "http:notes",
            "https:/Host/a",
            "HTTP:///a",
            "HTTP:///a.md",
            "https://?q=a.md",
            "http://#A.md",
            "https://",
            "./https:notes",
            "a/../HTTP:notes",
            "https://Host/a b",
        ] {
            // When / Then
            assert!(ElementAddress::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn retain_error_cause_without_exposing_it_in_diagnostics() {
        // Given / When
        let error = CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "safe failure",
            std::io::Error::other("SECRET_SENTINEL"),
        );
        // Then
        assert_eq!(error.get_message(), "safe failure");
        assert!(
            error
                .source()
                .unwrap()
                .to_string()
                .contains("SECRET_SENTINEL")
        );
        assert_eq!(error.to_string(), "safe failure");
        let debug = format!("{error:?}");
        assert!(debug.contains(error.get_message()));
        assert!(!debug.contains("SECRET_SENTINEL"));
    }
}

#[cfg(test)]
mod search_filters_tests {
    use super::{RecheckFilter, SearchFilters};
    use crate::service::SearchFlag;
    use std::collections::BTreeSet;

    #[test]
    fn default_filters_impose_no_restrictions() {
        // Given
        let filters = SearchFilters::default();
        let observations = [
            BTreeSet::new(),
            BTreeSet::from([SearchFlag::Stale]),
            BTreeSet::from([SearchFlag::SourceChanged]),
            BTreeSet::from([SearchFlag::SourceNotFound]),
        ];
        // When / Then
        assert_eq!(filters.needs_recheck, RecheckFilter::Any);
        assert!(filters.sources.is_empty());
        assert!(filters.source_revisions.is_empty());
        for flags in observations {
            assert!(filters.matches_flags(&flags), "{flags:?}");
        }
    }

    #[test]
    fn required_recheck_matches_each_current_flag_and_rejects_no_flags() {
        // Given
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::Required,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), false),
            (BTreeSet::from([SearchFlag::Stale]), true),
            (BTreeSet::from([SearchFlag::SourceChanged]), true),
            (BTreeSet::from([SearchFlag::SourceNotFound]), true),
        ];
        // When / Then
        for (flags, expected) in cases {
            assert_eq!(filters.matches_flags(&flags), expected, "{flags:?}");
        }
    }

    #[test]
    fn not_required_recheck_matches_no_flags_and_rejects_each_current_flag() {
        // Given
        let filters = SearchFilters {
            needs_recheck: RecheckFilter::NotRequired,
            ..SearchFilters::default()
        };
        let cases = [
            (BTreeSet::new(), true),
            (BTreeSet::from([SearchFlag::Stale]), false),
            (BTreeSet::from([SearchFlag::SourceChanged]), false),
            (BTreeSet::from([SearchFlag::SourceNotFound]), false),
        ];
        // When / Then
        for (flags, expected) in cases {
            assert_eq!(filters.matches_flags(&flags), expected, "{flags:?}");
        }
    }
}

#[cfg(test)]
mod embedding_config_tests {
    use super::EmbeddingConfig;
    use crate::utils::JsonValue;

    #[test]
    fn bounds_raw_token_traversal_before_canonicalization() {
        // Given
        let nested = format!("{}0{}", "[".repeat(130), "]".repeat(130));
        // When
        let result = serde_json::from_str::<JsonValue<true>>(&nested);
        // Then
        assert!(result.is_err());
    }
    #[test]
    fn rejects_trailing_json_after_configuration() {
        // Given
        let bytes = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0} null"#;
        // When
        let result = EmbeddingConfig::parse(bytes);
        // Then
        assert!(result.is_err());
    }
}

#[cfg(test)]
mod discovery_configuration_tests {
    use super::{Configuration, DiscoveryConfig};

    #[test]
    fn discovery_defaults_preserve_embedding_identity() {
        // Given: fixed provider settings and three discovery representations.
        for discovery in [
            "",
            ",\"discovery\":{}",
            ",\"discovery\":{\"respect_gitignore\":false}",
        ] {
            let bytes = format!(
                r#"{{"embedding_model":{{"enabled":true,"provider":"ollama","model":"fixture","parameters":{{"base_url":"http://127.0.0.1:9/api"}},"max_retries":2}}{discovery}}}"#
            );
            // When: the complete configuration is parsed.
            let configuration = Configuration::parse(bytes.as_bytes()).unwrap();
            // Then: only the independently specified embedding projection controls identity.
            assert_eq!(
                configuration
                    .get_embedding_config()
                    .unwrap()
                    .get_identity()
                    .to_hex(),
                "c713040f35c46c21c7389aa69b25910ad51fb1ce6ceb26b378513ea5dec8748b"
            );
            assert_eq!(
                configuration.get_discovery().get_respect_gitignore(),
                !discovery.contains("false")
            );
        }
        assert!(DiscoveryConfig::default().get_respect_gitignore());
    }

    #[test]
    fn discovery_projection_ignores_invalid_embedding_settings() {
        // Given: valid discovery objects alongside absent or invalid embedding settings.
        for bytes in [br#"{"discovery":{"respect_gitignore":false}}"#.as_slice(),
            br#"{"embedding_model":null,"discovery":{"respect_gitignore":false}}"#,
            br#"{"embedding_model":{"provider":"unsupported"},"discovery":{"respect_gitignore":false}}"#] {
            // When: discovery and complete validation are requested separately.
            let discovery = DiscoveryConfig::parse(bytes).unwrap();
            let complete = Configuration::parse(bytes);
            // Then: FTS can select its corpus without accepting invalid embedding settings.
            assert!(!discovery.get_respect_gitignore());
            assert!(complete.is_err());
        }
    }

    #[test]
    fn discovery_rejects_ambiguous_or_invalid_input() {
        // Given: ambiguous roots, wrong types and unknown fields.
        for bytes in [
            "null",
            "[]",
            "{invalid}",
            r#"{"future":true}"#,
            r#"{"discovery":null}"#,
            r#"{"discovery":[]}"#,
            r#"{"discovery":{"respect_gitignore":"false"}}"#,
            r#"{"discovery":{"respect_gitignore":0}}"#,
            r#"{"discovery":{"future":false}}"#,
            r#"{"discovery":{},"discovery":{}}"#,
            r#"{"discovery":{"respect_gitignore":true,"respect_gitignore":false}}"#,
        ] {
            // When: the independent discovery projection is parsed.
            let result = DiscoveryConfig::parse(bytes.as_bytes());
            // Then: invalid input never silently chooses a wider corpus.
            assert!(result.is_err(), "{bytes}");
        }
    }
}

#[cfg(test)]
mod configuration_value_tests {
    use crate::service::{CapturedFile, FileIdentity, FileReader, FileResult, FileStat};

    use super::{Configuration, GENERATED_CONFIG};
    use crate::{
        domain::Sha256,
        service::configuration::ConfigurationService,
        service::{
            ConfigReader, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind, EmbeddingProvider,
            EmbeddingResult,
        },
    };
    use serde::Deserialize;
    use std::num::NonZeroUsize;

    const CORPUS: &str = include_str!("../../tests/fixtures/config/corpus.json");
    const ENABLED: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;
    const DISABLED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;

    #[derive(Deserialize)]
    struct Corpus {
        cases: Vec<Case>,
    }
    #[derive(Deserialize)]
    struct Case {
        name: String,
        raw: String,
        parse_valid: bool,
        validate_valid: bool,
    }
    struct Provider;
    struct Files {
        config: Vec<u8>,
    }

    #[test]
    fn parses_generated_defaults_and_preserves_retry_setting() {
        // Given / When: the exact generated default configuration is parsed.
        let configuration = Configuration::parse(GENERATED_CONFIG).unwrap();

        // Then: embeddings remain unconfigured and the retry default is preserved.
        assert!(!configuration.get_enabled());
        assert!(configuration.get_embedding_config().is_none());
        assert_eq!(configuration.get_max_retries(), 0);
    }

    #[test]
    fn identity_excludes_enabled_and_the_root_wrapper() {
        // Given: the same configured projection under both execution states.
        let enabled = Configuration::parse(ENABLED).unwrap();
        let disabled = Configuration::parse(DISABLED).unwrap();
        let expected = Sha256::digest(
            br#"{"max_retries":2,"model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"provider":"ollama"}"#,
        );

        // When / Then: only the four configured settings participate.
        assert_eq!(
            enabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert_eq!(
            disabled.get_embedding_config().unwrap().get_identity(),
            expected
        );
        assert!(enabled.get_enabled());
        assert!(!disabled.get_enabled());
    }

    #[test]
    fn reports_legacy_flat_document_for_migration() {
        // Given
        let raw = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0}"#;

        // When
        let error = Configuration::parse(raw).unwrap_err();

        // Then
        assert_eq!(error.get_kind(), EmbeddingErrorKind::MigrationRequired);
    }

    #[test]
    fn shared_corpus_agrees_with_local_parse_and_validation() {
        // Given: the same corpus the schema checker consumes.
        let corpus: Corpus = serde_json::from_str(CORPUS).unwrap();

        for case in corpus.cases {
            // Given: a corpus document exposed through the configuration service.
            let files = Files {
                config: case.raw.as_bytes().to_vec(),
            };
            let service = ConfigurationService {
                files: &files,
                provider: &Provider,
            };

            // When: structural parsing inspects the document.
            let parsed = Configuration::parse(case.raw.as_bytes());

            // Then: parsing agrees with the independently recorded corpus result.
            assert_eq!(parsed.is_ok(), case.parse_valid, "{}", case.name);
            if parsed.is_ok() {
                // When: the service also validates retained provider settings.
                let validated = service.get_config();

                // Then: validation agrees with the recorded corpus result.
                assert_eq!(validated.is_ok(), case.validate_valid, "{}", case.name);
            }
        }
    }

    impl FileReader for Files {
        fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            Ok(false)
        }
        fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
            panic!("unexpected file_get_by_path")
        }
        fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
            assert_eq!(path, std::path::Path::new(".letopis/config.json"));
            Ok(Some(self.config.clone()))
        }
        fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
            panic!("unexpected file_get_stat_by_path")
        }
        fn file_get_identity_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<FileIdentity>> {
            panic!("unexpected file_get_identity_by_path")
        }
        fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
            panic!("unexpected file_is_exist")
        }
        fn file_verify_identity_by_path(
            &self,
            _: &std::path::Path,
            _: Option<FileIdentity>,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_identity_by_path")
        }
        fn file_verify_distinct_by_paths(
            &self,
            _: &std::path::Path,
            _: &std::path::Path,
        ) -> FileResult<()> {
            panic!("unexpected file_verify_distinct_by_paths")
        }
        fn directory_get_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::os::fd::OwnedFd>> {
            Ok(None)
        }
        fn lock_get_owner_by_path(
            &self,
            _: &std::path::Path,
        ) -> FileResult<Option<std::num::NonZeroU32>> {
            Ok(None)
        }
    }

    impl EmbeddingProvider for Provider {
        fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()> {
            let url = config
                .get_parameters()
                .get("base_url")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if url.starts_with("http://") || url.starts_with("https://") {
                Ok(())
            } else {
                Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig))
            }
        }

        fn vectors_compute_by_texts(
            &self,
            _: &EmbeddingConfig,
            _: &[&str],
            _: Option<NonZeroUsize>,
        ) -> EmbeddingResult<Vec<Vec<f32>>> {
            Err(EmbeddingError::new(EmbeddingErrorKind::Input))
        }
    }
}
