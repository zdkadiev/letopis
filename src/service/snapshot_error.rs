use std::{error::Error, fmt};

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
