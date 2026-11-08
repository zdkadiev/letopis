use std::{error::Error, fmt, io};

/// Sanitized I/O cause exposing only the error kind, never library or object payloads.
pub(super) struct SafeIo(pub(super) io::Error);

impl fmt::Display for SafeIo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "I/O failure ({:?})", self.0.kind())
    }
}

impl fmt::Debug for SafeIo {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, formatter)
    }
}

impl Error for SafeIo {}
