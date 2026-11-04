//! Shared application failures, diagnostics and typed result aliases.
use crate::domain::{ByteSpan, Sha256};
use serde_json::Value;
use std::{collections::BTreeMap, error::Error, fmt};

/// Failure in a transaction boundary or its caller's operation.
#[derive(Debug)]
pub enum TransactionError<E> {
    /// The operation returned an error.
    Operation(E),
    /// The storage transaction failed.
    Storage(StorageError),
}

impl TransactionError<StorageError> {
    /// Consume a storage-only scope error while retaining its original classification and cause.
    pub fn into_inner(self) -> StorageError {
        match self {
            Self::Operation(error) | Self::Storage(error) => error,
        }
    }
}

impl<E: fmt::Display> fmt::Display for TransactionError<E> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Operation(error) => error.fmt(formatter),
            Self::Storage(error) => error.fmt(formatter),
        }
    }
}

impl<E: Error + 'static> Error for TransactionError<E> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Operation(error) => Some(error),
            Self::Storage(error) => Some(error),
        }
    }
}

impl<E> TransactionError<TransactionError<E>> {
    /// Remove an internal transaction-error layer while preserving its source.
    pub fn flatten(self) -> TransactionError<E> {
        match self {
            Self::Storage(error) | Self::Operation(TransactionError::Storage(error)) => {
                TransactionError::Storage(error)
            }
            Self::Operation(TransactionError::Operation(error)) => {
                TransactionError::Operation(error)
            }
        }
    }
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
    /// A local Markdown navigation path differs from the declared graph file.
    LinkPathMismatch,
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
            Self::LinkPathMismatch => "Markdown link path does not match letopis path",
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
            Self::LinkPathMismatch => "link_path_mismatch",
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

impl Error for EmbeddingError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_ref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}

struct PrivateCause {
    _cause: Box<dyn Error + Send + Sync>,
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

/// Typed result of an embedding operation.
pub type EmbeddingResult<T> = Result<T, EmbeddingError>;

/// Stable classification for snapshot failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapshotErrorKind {
    /// Invalid path, object format or object type.
    InvalidRequest,
    /// The requested root or path does not exist.
    NotFound,
    /// Git, repository integrity or retention failed.
    InternalError,
}

/// A safe diagnostic, retaining I/O or exit causes without subprocess payloads.
#[derive(Debug)]
pub struct SnapshotError {
    kind: SnapshotErrorKind,
    message: &'static str,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl SnapshotError {
    pub(crate) fn new(kind: SnapshotErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            cause: None,
        }
    }
    pub(crate) fn new_with_cause(
        message: &'static str,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind: SnapshotErrorKind::InternalError,
            message,
            cause: Some(Box::new(cause)),
        }
    }
    /// Classification suitable for application error mapping.
    pub fn get_kind(&self) -> SnapshotErrorKind {
        self.kind
    }
    /// Static operation context, without repository paths or file bytes.
    pub fn get_message(&self) -> &str {
        self.message
    }
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)?;
        if let Some(c) = &self.cause {
            write!(f, ": {c}")?;
        }
        Ok(())
    }
}

impl Error for SnapshotError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_ref().map(|c| c.as_ref() as &dyn Error)
    }
}

/// Stable application classification for storage failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageErrorKind {
    /// A request violates a storage input invariant.
    InvalidRequest,
    /// The requested stored value is absent.
    NotFound,
    /// `SQLite`, native capabilities, integrity or I/O failed.
    InternalError,
}

/// Fixed safe diagnostic with an optional internal cause.
#[derive(Debug)]
pub struct StorageError {
    kind: StorageErrorKind,
    message: &'static str,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl StorageError {
    pub(crate) fn new(kind: StorageErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            cause: None,
        }
    }
    pub(crate) fn new_with_cause(
        kind: StorageErrorKind,
        message: &'static str,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message,
            cause: Some(Box::new(cause)),
        }
    }
    /// Classification suitable for application error mapping.
    pub fn get_kind(&self) -> StorageErrorKind {
        self.kind
    }
    /// Safe static context without SQL or parameter payloads.
    pub fn get_message(&self) -> &str {
        self.message
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl Error for StorageError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}

/// A storage operation's typed outcome.
pub type StorageResult<T> = Result<T, StorageError>;

/// Stable classification for confined file operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileErrorKind {
    /// A path is absolute, escapes its root, contains a NUL, or has an unsafe parent.
    UnsafePath,
    /// A stable final entry is not a regular file.
    NonRegular,
    /// A previously observed entry or pinned directory changed or disappeared.
    Changed,
    /// Two paths identify the same file, or a lock has another hardlink.
    Aliased,
    /// A required parent or directory is absent.
    NotFound,
    /// An observed locked file contains no valid bounded process identifier.
    InvalidOwner,
    /// An operating-system, clock, representation or durability operation failed.
    Io,
}

/// File failure with a safe classification and a privately retained cause.
pub struct FileError {
    kind: FileErrorKind,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl FileError {
    pub(crate) fn new(kind: FileErrorKind) -> Self {
        Self { kind, cause: None }
    }

    pub(crate) fn new_with_cause(
        kind: FileErrorKind,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            cause: Some(Box::new(cause)),
        }
    }

    /// Return the classification without exposing paths or file bytes.
    pub fn get_kind(&self) -> FileErrorKind {
        self.kind
    }
}

impl fmt::Display for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self.kind {
            FileErrorKind::UnsafePath => "unsafe file path",
            FileErrorKind::NonRegular => "regular file required",
            FileErrorKind::Changed => "file identity or observation changed",
            FileErrorKind::Aliased => "distinct file identities required",
            FileErrorKind::NotFound => "file directory missing",
            FileErrorKind::InvalidOwner => "invalid file lock owner",
            FileErrorKind::Io => "file operation failed",
        })
    }
}

impl fmt::Debug for FileError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("FileError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Error for FileError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}

/// Typed result of a confined file operation.
pub type FileResult<T> = Result<T, FileError>;

/// Stable application classification for document refresh failures.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DocumentErrorKind {
    /// A path or request cannot be represented safely.
    InvalidRequest,
    /// Filesystem, Git, storage, clock or integrity checks failed.
    InternalError,
}

/// Fixed safe context with a separately accessible internal cause.
pub struct DocumentError {
    kind: DocumentErrorKind,
    message: &'static str,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl DocumentError {
    pub(crate) fn new(kind: DocumentErrorKind, message: &'static str) -> Self {
        Self {
            kind,
            message,
            cause: None,
        }
    }
    pub(crate) fn new_with_cause(
        kind: DocumentErrorKind,
        message: &'static str,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            message,
            cause: Some(Box::new(cause)),
        }
    }
    /// Classification for application error mapping.
    pub fn get_kind(&self) -> DocumentErrorKind {
        self.kind
    }
    /// Fixed context without paths, document bytes or subprocess output.
    pub fn get_message(&self) -> &str {
        self.message
    }
}

impl fmt::Display for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message)
    }
}

impl fmt::Debug for DocumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DocumentError")
            .field("kind", &self.kind)
            .field("message", &self.message)
            .finish_non_exhaustive()
    }
}

impl Error for DocumentError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}

impl From<StorageError> for DocumentError {
    fn from(cause: StorageError) -> Self {
        Self::new_with_cause(
            DocumentErrorKind::InternalError,
            "document storage operation failed",
            cause,
        )
    }
}

/// A document operation's typed outcome.
pub type DocumentResult<T> = Result<T, DocumentError>;

/// Application service construction or setup failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceErrorKind {
    /// No initialized Git work tree is available.
    RepositoryRequired,
    /// Repository discovery or object policy failed.
    Repository,
    /// Initialization setup failed.
    Initialization,
    /// Corpus validation service setup failed.
    Check,
    /// Search service setup failed.
    Search,
    /// Traversal service setup failed.
    Traversal,
    /// Inspection service setup failed.
    Inspection,
    /// Status service setup failed.
    Status,
    /// Configuration service setup failed.
    Configuration,
    /// Embedding service setup failed.
    Embedding,
    /// Background coordination setup failed.
    Background,
    /// Corpus reindex setup failed.
    Reindex,
}

/// Safe application-service setup error with a private cause.
pub struct ServiceError {
    kind: ServiceErrorKind,
    cause: Option<Box<dyn Error + Send + Sync>>,
}

impl ServiceError {
    /// Construct an error from a service setup kind and private cause.
    pub(crate) fn new_with_cause(
        kind: ServiceErrorKind,
        cause: impl Error + Send + Sync + 'static,
    ) -> Self {
        Self {
            kind,
            cause: Some(Box::new(cause)),
        }
    }
    /// Return the service setup classification.
    pub fn get_kind(&self) -> ServiceErrorKind {
        self.kind
    }
}

impl fmt::Display for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("application service operation failed")
    }
}

impl fmt::Debug for ServiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceError")
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}

impl Error for ServiceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause
            .as_deref()
            .map(|cause| cause as &(dyn Error + 'static))
    }
}

/// Result of constructing an application service.
pub type ServiceResult<T> = Result<T, ServiceError>;

#[cfg(test)]
mod error_tests {
    use super::{DocumentError, DocumentErrorKind, ServiceError, ServiceErrorKind};
    use std::{error::Error, io};

    #[test]
    fn preserves_document_context_without_exposing_its_private_cause() {
        // Given: callers supply safe operation context and a separate raw cause.
        let context = "safe document operation context";
        let error = DocumentError::new_with_cause(
            DocumentErrorKind::InternalError,
            context,
            io::Error::other("PRIVATE_DOCUMENT_CAUSE"),
        );
        // When
        let message = error.get_message();
        let display = error.to_string();
        let debug = format!("{error:?}");
        // Then: public context remains useful while the cause stays internal.
        assert_eq!(message, context);
        assert_eq!(display, context);
        assert!(debug.contains(context));
        assert!(!debug.contains("PRIVATE_DOCUMENT_CAUSE"));
        assert!(
            error
                .source()
                .unwrap()
                .to_string()
                .contains("PRIVATE_DOCUMENT_CAUSE")
        );
    }

    #[test]
    fn service_error_keeps_kind_without_exposing_private_cause() {
        // Given
        let error = ServiceError::new_with_cause(
            ServiceErrorKind::Initialization,
            io::Error::other("PRIVATE_SERVICE_CAUSE"),
        );
        // When
        let diagnostic = format!("{error:?}");
        // Then
        assert_eq!(error.get_kind(), ServiceErrorKind::Initialization);
        assert!(!diagnostic.contains("PRIVATE_SERVICE_CAUSE"));
        assert!(
            error
                .source()
                .unwrap()
                .to_string()
                .contains("PRIVATE_SERVICE_CAUSE")
        );
    }
}
