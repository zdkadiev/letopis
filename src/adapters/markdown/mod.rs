//! Read-only X-4 Markdown adapter. One parse is atomic at the file boundary.
mod links;
mod metadata;
mod scan;
mod search;

use crate::domain::{ByteSpan, Edge, Element, RepoPath, Sha256};
use pulldown_cmark::{Event, Options, Parser};
use std::{collections::BTreeSet, ops::Range};
pub(super) type Events<'a> = Vec<(Event<'a>, Range<usize>)>;

/// Local diagnostic reasons; the CLI maps these into its existing error envelope.
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum DiagnosticKind {
    InvalidUtf8,
    InvalidMetadata,
    InvalidAnchor,
    DuplicateAnchor,
    OrphanMetadata,
    NestedElement,
    InvalidLinkTitle,
    InvalidTarget,
    SelfLoop,
}
/// A file error with source coordinates where available and a fixed local message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic {
    pub kind: DiagnosticKind,
    pub bytes: Option<ByteSpan>,
    pub message: String,
}
impl Diagnostic {
    fn new_at(kind: DiagnosticKind, range: Range<usize>) -> Self {
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
/// A whole valid file's projection; raw Markdown is not retained.
#[derive(Debug)]
pub struct ParsedFile {
    pub elements: Vec<ParsedElement>,
    pub edges: BTreeSet<Edge>,
}

/// One parsed element with transient shared search text and its fingerprint.
#[derive(Debug)]
pub struct ParsedElement {
    pub element: Element,
    pub search_text: String,
    pub search_hash: Sha256,
}

/// Parse one root-relative Markdown file without accessing disk, Git, SQL or a model.
/// `Ok(None)` is unmarked Markdown; errors never carry a partial valid projection.
///
/// # Errors
/// Returns local diagnostics for invalid UTF-8 or invalid knowledge metadata,
/// anchors, element layout or declared graph links.
pub fn parse(path: &RepoPath, bytes: &[u8]) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
    let text = std::str::from_utf8(bytes).map_err(|e| {
        vec![Diagnostic::new_at(
            DiagnosticKind::InvalidUtf8,
            e.valid_up_to()..bytes.len(),
        )]
    })?;
    let events: Events<'_> = Parser::new_ext(text, Options::ENABLE_HEADING_ATTRIBUTES)
        .into_offset_iter()
        .collect();
    let Some(layout) = scan::scan(path, text, &events).map_err(|e| vec![e])? else {
        return Ok(None);
    };
    let edges = links::collect(&events, &layout.elements).map_err(|e| vec![e])?;
    Ok(Some(ParsedFile {
        elements: layout
            .elements
            .into_iter()
            .map(|element| {
                let search_text = search::render(&events, element.bytes, &layout.metadata_ranges);
                let search_hash = Sha256::digest(search_text.as_bytes());
                ParsedElement {
                    element,
                    search_text,
                    search_hash,
                }
            })
            .collect(),
        edges,
    }))
}
