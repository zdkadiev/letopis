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
            message: match kind {
                DiagnosticKind::InvalidUtf8 => "Markdown is not UTF-8",
                DiagnosticKind::InvalidMetadata => "invalid letopis metadata",
                DiagnosticKind::InvalidAnchor => "invalid explicit anchor",
                DiagnosticKind::DuplicateAnchor => "duplicate explicit anchor",
                DiagnosticKind::OrphanMetadata => {
                    "metadata does not immediately follow an anchored heading"
                }
                DiagnosticKind::NestedElement => "knowledge elements cannot be nested",
                DiagnosticKind::InvalidLinkTitle => "invalid letopis link title",
                DiagnosticKind::InvalidTarget => "invalid graph target",
                DiagnosticKind::SelfLoop => "knowledge cannot link to itself",
            }
            .into(),
        }
    }
}
