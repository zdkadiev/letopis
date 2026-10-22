use super::{Diagnostic, DiagnosticKind, Events, links, scan, search};
use crate::domain::{Edge, Element, RepoPath, Sha256};
use pulldown_cmark::{Options, Parser};
use std::collections::BTreeSet;

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
