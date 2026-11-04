use super::Events;
use crate::domain::ByteSpan;
use pulldown_cmark::{Event, TagEnd};
use std::ops::Range;

/// Render the element's own visible text from the already parsed full document.
pub(super) fn search_text_render(
    events: &Events<'_>,
    span: ByteSpan,
    metadata_ranges: &[Range<usize>],
) -> String {
    let mut text = String::new();
    for (event, range) in events {
        if range.start < span.get_start()
            || range.end > span.get_end()
            || metadata_ranges
                .iter()
                .any(|m| range.start >= m.start && range.end <= m.end)
        {
            continue;
        }
        match event {
            Event::Text(s) | Event::Code(s) | Event::Html(s) | Event::InlineHtml(s) => {
                text.push_str(s);
            }
            Event::SoftBreak
            | Event::HardBreak
            | Event::Rule
            | Event::End(
                TagEnd::Paragraph | TagEnd::Heading(_) | TagEnd::CodeBlock | TagEnd::Item,
            ) if !text.ends_with('\n') => text.push('\n'),
            _ => {}
        }
    }
    text.trim_matches('\n').to_owned()
}
