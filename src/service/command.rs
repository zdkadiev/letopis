use super::{ElementLocation, SearchFilters};
use crate::domain::{KnowledgeId, LineRange, RepoPath, Sha256, SourceUri, TreeOid};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
    num::NonZeroUsize,
};

/// Stable X-1 classification for command failures and safe findings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandErrorKind {
    /// Malformed command input.
    InvalidRequest,
    /// A requested element is absent.
    NotFound,
    /// Validation failed.
    ValidationFailed,
    /// Embedding failed.
    EmbeddingFailed,
    /// An operation or its infrastructure failed.
    InternalError,
    /// The operation was cancelled.
    Cancelled,
}

/// A fixed public diagnostic with an optional private causal chain.
pub struct CommandError {
    kind: CommandErrorKind,
    message: &'static str,
    cause: Option<Box<dyn Error + Send + Sync>>,
}
impl CommandError {
    /// Create an error from safe static context.
    pub fn new(kind: CommandErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            cause: None,
        }
    }
    /// Retain an internal cause without including it in Display or Debug.
    pub fn new_with_cause(
        kind: CommandErrorKind,
        message: &'static str,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message,
            cause: Some(Box::new(cause)),
        }
    }
    /// Return the stable public classification.
    pub fn get_kind(&self) -> CommandErrorKind {
        self.kind
    }
    /// Return the safe static diagnostic.
    pub fn get_message(&self) -> &'static str {
        self.message
    }
}
impl fmt::Debug for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for CommandError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}
impl Error for CommandError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|cause| cause.as_ref() as &dyn Error)
    }
}
/// Result of an application command.
pub type CommandResult<T> = Result<T, CommandError>;

/// A safe finding or per-call warning; details must never contain raw payloads.
#[derive(Debug)]
pub struct CommandProblem {
    /// Stable X-1 code.
    pub code: CommandErrorKind,
    /// Safe static diagnostic.
    pub message: &'static str,
    /// Explicitly safe structured context, supplied by the owning service.
    pub details: BTreeMap<String, Value>,
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

#[cfg(test)]
mod tests {
    use super::{CommandError, CommandErrorKind, ElementAddress};
    use std::error::Error;

    #[test]
    fn preserve_local_anchor_and_range_normalization() {
        for (raw, expected) in [
            ("./x.md", "x.md"),
            ("a/../x.md#A", "x.md#A"),
            ("./source#L01-L02", "source#L1-L2"),
            ("résumé.MD", "résumé.MD"),
            ("x.md#Lfoo-Lbar", "x.md#Lfoo-Lbar"),
        ] {
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
            assert_eq!(
                ElementAddress::parse(raw).unwrap_err().get_kind(),
                CommandErrorKind::InvalidRequest
            );
        }
    }

    #[test]
    fn preserve_opaque_http_addresses() {
        for raw in [
            "https://Host/a?q=1#L0-L2",
            "HTTP://Host/a#Anchor",
            "https://Host",
            "https://Host/a.md",
            "HtTp://Host/a.markdown#A",
        ] {
            assert_eq!(ElementAddress::parse(raw).unwrap().as_str(), raw);
        }
    }

    #[test]
    fn round_trip_http_prefix_knowledge_addresses() {
        for (raw, expected) in [
            ("http:notes.md", "http:notes.md"),
            ("./http:notes.md#A", "http:notes.md#A"),
            ("dir/../HTTP:notes.markdown#A", "HTTP:notes.markdown#A"),
            ("https:/Host/a.md", "https:/Host/a.md"),
        ] {
            let parsed = ElementAddress::parse(raw).unwrap();
            assert_eq!(parsed.as_str(), expected);
            assert_eq!(ElementAddress::parse(expected).unwrap(), parsed);
        }
    }

    #[test]
    fn reject_malformed_absolute_urls_and_source_http_fallbacks() {
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
            assert!(ElementAddress::parse(raw).is_err(), "{raw}");
        }
    }

    #[test]
    fn retain_error_cause_without_exposing_it_in_diagnostics() {
        let error = CommandError::new_with_cause(
            CommandErrorKind::InternalError,
            "safe failure",
            std::io::Error::other("SECRET_SENTINEL"),
        );
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
