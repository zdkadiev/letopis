//! Read-only X-4 Markdown parsing. One parse is atomic at the file boundary.
mod diagnostic;
mod links;
mod metadata;
mod parser;
mod scan;
mod search;

pub use diagnostic::{Diagnostic, DiagnosticKind};
pub use parser::{ParsedElement, ParsedFile, parse};
use pulldown_cmark::Event;
use std::ops::Range;

pub(super) type Events<'a> = Vec<(Event<'a>, Range<usize>)>;
