//! Read-only X-4 Markdown parsing. One parse is atomic at the file boundary.
mod parser;

pub use crate::service::{Diagnostic, DiagnosticKind, ParsedElement, ParsedFile};
pub use parser::MarkdownParser;
pub use parser::file_parse;
