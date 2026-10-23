use pulldown_cmark::Event;
use std::ops::Range;

/// Markdown parser events paired with their byte ranges in the source text.
pub(super) type Events<'a> = Vec<(Event<'a>, Range<usize>)>;
