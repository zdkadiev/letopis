use std::{error::Error, fmt};

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

impl From<StorageError> for DocumentError {
    fn from(cause: StorageError) -> Self {
        Self::new_with_cause(
            DocumentErrorKind::InternalError,
            "document storage operation failed",
            cause,
        )
    }
}

#[cfg(test)]
mod tests {
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
