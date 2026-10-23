//! Read-only X-4 Markdown parsing. One parse is atomic at the file boundary.
mod diagnostic;
mod links;
mod metadata;
mod parser;
mod scan;
mod search;

pub use crate::service::{Diagnostic, DiagnosticKind, ParsedElement, ParsedFile};
pub use parser::MarkdownParser;
pub use parser::parse;
use pulldown_cmark::Event;
use std::ops::Range;

pub(super) type Events<'a> = Vec<(Event<'a>, Range<usize>)>;
