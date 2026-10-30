use crate::service::SnapshotError;
use std::{error::Error, fmt, io};

struct SafeIo(io::Error);

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

/// Retain an I/O cause without exposing arbitrary library/object payloads.
pub(super) fn make_io_error(message: &'static str, error: io::Error) -> SnapshotError {
    SnapshotError::new_with_cause(message, SafeIo(error))
}
