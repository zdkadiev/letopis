use crate::{
    domain::ByteSpan,
    service::{Diagnostic, DiagnosticKind},
};
use std::ops::Range;

impl Diagnostic {
    pub(super) fn new_at(kind: DiagnosticKind, range: Range<usize>) -> Self {
        Self {
            kind,
            bytes: ByteSpan::new(range.start, range.end).ok(),
            message: kind.get_message().into(),
        }
    }
}
