use std::{error::Error, fmt};

/// Stable classification of embedding failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingErrorKind {
    /// Configuration fails the local contract.
    InvalidConfig,
    /// A legacy flat configuration must be moved under `embedding_model`.
    MigrationRequired,
    /// Compatible-provider credentials are absent or invalid.
    Credentials,
    /// Provider output fails the vector contract.
    InvalidResponse,
    /// Provider returned an unsuccessful HTTP status.
    HttpStatus(u16),
    /// The attempt exceeded its deadline.
    Timeout,
    /// The transport could not complete the request.
    Connection,
    /// Input violates the provider boundary.
    Input,
    /// An internal operation failed.
    InternalError,
}

/// Safe diagnostic retaining a private, non-formatable cause.
pub struct EmbeddingError {
    kind: EmbeddingErrorKind,
    cause: Option<PrivateCause>,
}
struct PrivateCause {
    _cause: Box<dyn Error + Send + Sync>,
}
impl EmbeddingError {
    /// Construct a fixed safe diagnostic.
    pub fn new(kind: EmbeddingErrorKind) -> Self {
        Self { kind, cause: None }
    }
    /// Preserve a cause privately without exposing its payload through error chains.
    pub fn new_with_cause(
        kind: EmbeddingErrorKind,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            cause: Some(PrivateCause {
                _cause: Box::new(cause),
            }),
        }
    }
    /// Return the stable failure classification.
    pub fn get_kind(&self) -> EmbeddingErrorKind {
        self.kind
    }
}
impl fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.kind {
            EmbeddingErrorKind::InvalidConfig => "invalid embedding configuration",
            EmbeddingErrorKind::MigrationRequired => "embedding configuration requires migration",
            EmbeddingErrorKind::Credentials => "embedding credentials unavailable",
            EmbeddingErrorKind::InvalidResponse => "invalid embedding response",
            EmbeddingErrorKind::HttpStatus(_) => "embedding provider rejected request",
            EmbeddingErrorKind::Timeout => "embedding attempt timed out",
            EmbeddingErrorKind::Connection => "embedding connection failed",
            EmbeddingErrorKind::Input => "invalid embedding input",
            EmbeddingErrorKind::InternalError => "embedding operation failed",
        })
    }
}
impl fmt::Debug for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EmbeddingError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
impl fmt::Display for PrivateCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("private embedding failure")
    }
}
impl fmt::Debug for PrivateCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("PrivateCause { .. }")
    }
}
impl Error for PrivateCause {}
impl Error for EmbeddingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}
/// Typed result of an embedding operation.
pub type EmbeddingResult<T> = Result<T, EmbeddingError>;
