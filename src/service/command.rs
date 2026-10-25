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
    /// Normalize a plain path, explicit knowledge anchor or source line range.
    ///
    /// # Errors
    /// Returns invalid request for malformed paths, anchors or line ranges.
    pub fn parse(raw: &str) -> CommandResult<Self> {
        let normalized = match raw.split_once('#') {
            None => RepoPath::parse(raw).map(|path| path.as_str().to_owned()),
            Some(_) => KnowledgeId::parse(raw)
                .map(|id| id.as_str().to_owned())
                .or_else(|_| SourceUri::parse(raw).map(|source| source.as_str().to_owned())),
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
    /// Root-relative file path.
    pub path: RepoPath,
    /// Inclusive source or element lines, when available.
    pub lines: Option<LineRange>,
    /// Exact element hash, or absent for an unavailable source.
    pub hash: Option<Sha256>,
}

/// Literal search text and final result bound.
#[derive(Debug)]
pub struct SearchRequest {
    /// Uninterpreted user query, tokenized by the search service.
    pub query: String,
    /// Maximum links after combining complete candidate channels.
    pub limit: NonZeroUsize,
}
/// Baseline knowledge freshness observations.
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
    fn normalize_addresses_without_membership_lookup() {
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
