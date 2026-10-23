use super::{Diagnostic, DiagnosticKind, Events, links, scan, search};
use crate::domain::{RepoPath, Sha256};
use crate::service::{MarkdownCodec, ParsedElement, ParsedFile};
use pulldown_cmark::{Options, Parser};

/// Parse one root-relative Markdown file without accessing disk, Git, SQL or a model.
/// `Ok(None)` is unmarked Markdown; errors never carry a partial valid projection.
///
/// # Errors
/// Returns local diagnostics for invalid UTF-8 or invalid knowledge metadata,
/// anchors, element layout or declared graph links.
pub fn parse(path: &RepoPath, bytes: &[u8]) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
    MarkdownParser::new().parse_file(path, bytes)
}

/// The shared pure X-4 Markdown codec used by application services.
#[derive(Debug)]
pub struct MarkdownParser;
impl MarkdownParser {
    pub(crate) fn new() -> Self {
        Self
    }
}
impl MarkdownCodec for MarkdownParser {
    fn parse_file(
        &self,
        path: &RepoPath,
        bytes: &[u8],
    ) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
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
                    let search_text =
                        search::render(&events, element.bytes, &layout.metadata_ranges);
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
}

#[cfg(test)]
mod tests {
    use super::{MarkdownParser, parse};
    use crate::{domain::RepoPath, service::MarkdownCodec};

    #[test]
    fn parser_interface_preserves_all_free_function_outcomes() {
        // Arrange
        let path = RepoPath::parse("note.md").unwrap();
        let parser = MarkdownParser::new();
        let cases: &[&[u8]] = &[
            include_bytes!("../../tests/fixtures/markdown/whole.md"),
            include_bytes!("../../tests/fixtures/markdown/sections.md"),
            b"unmarked",
            b"\xff",
            b"<!-- letopis: {} -->\ntext",
        ];
        for bytes in cases {
            // Act
            let actual = parser.parse_file(&path, bytes);
            let existing = parse(&path, bytes);
            // Assert: parity only; existing X-4 tests carry independent behavior expectations.
            assert_eq!(format!("{actual:?}"), format!("{existing:?}"));
        }
    }
}
