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
    use super::{DocumentError, DocumentErrorKind};
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
}
