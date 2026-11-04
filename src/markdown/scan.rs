use super::{Diagnostic, DiagnosticKind, Events, metadata};
use crate::domain::{Anchor, ByteSpan, Element, KnowledgeId, LineRange, RepoPath, Sha256};
use pulldown_cmark::{Event, Tag};
use std::{collections::BTreeSet, ops::Range};

pub(super) struct Layout {
    pub elements: Vec<Element>,
    pub metadata_ranges: Vec<Range<usize>>,
}
struct Heading {
    start: usize,
    after: usize,
    level: u8,
    anchor: Option<Anchor>,
}

/// Build the complete file layout from X-4 declarations; unmarked files return `None`.
///
/// # Errors
/// Returns a local diagnostic for invalid identifiers, anchors or element layout.
pub(super) fn layout_scan(
    path: &RepoPath,
    text: &str,
    events: &Events<'_>,
) -> Result<Option<Layout>, Diagnostic> {
    let comments = metadata_comments_collect(text, events);
    if comments.is_empty() {
        return Ok(None);
    }
    KnowledgeId::new(path.clone(), None)
        .map_err(|_| Diagnostic::new_at(DiagnosticKind::InvalidTarget, 0..text.len()))?;
    let headings = headings_collect(text, events)?;
    layout_build(path, text, &headings, comments).map(Some)
}

fn metadata_comments_collect(text: &str, events: &Events<'_>) -> Vec<Range<usize>> {
    events
        .iter()
        .filter_map(|(event, range)| match event {
            // Html events are raw lines inside a block, including script/style data.
            // Only a block that itself starts with our comment declares metadata.
            Event::Start(Tag::HtmlBlock)
                if text[range.clone()]
                    .trim_start()
                    .starts_with("<!-- letopis:") =>
            {
                Some(range.clone())
            }
            Event::InlineHtml(s) if s.trim_start().starts_with("<!-- letopis:") => {
                Some(range.clone())
            }
            _ => None,
        })
        .collect()
}

fn headings_collect(text: &str, events: &Events<'_>) -> Result<Vec<Heading>, Diagnostic> {
    let mut headings = Vec::new();
    let mut anchors = BTreeSet::new();
    for (event, range) in events {
        if let Event::Start(Tag::Heading { level, id, .. }) = event {
            let start = line_start_calculate(text, range.start);
            let raw = &text[start..range.end];
            let anchor = match id {
                Some(id) => Some(Anchor::parse(id).map_err(|_| {
                    Diagnostic::new_at(DiagnosticKind::InvalidAnchor, range.clone())
                })?),
                None if raw.trim_end().ends_with("{#}") => {
                    return Err(Diagnostic::new_at(
                        DiagnosticKind::InvalidAnchor,
                        range.clone(),
                    ));
                }
                None => None,
            };
            if let Some(anchor) = &anchor
                && !anchors.insert(anchor.clone())
            {
                return Err(Diagnostic::new_at(
                    DiagnosticKind::DuplicateAnchor,
                    range.clone(),
                ));
            }
            let after = if text.as_bytes().get(range.end.wrapping_sub(1)) == Some(&b'\n') {
                range.end
            } else {
                line_end_calculate(text, range.end)
            };
            headings.push(Heading {
                start,
                after,
                level: *level as u8,
                anchor,
            });
        }
    }
    Ok(headings)
}

fn layout_build(
    path: &RepoPath,
    text: &str,
    headings: &[Heading],
    comments: Vec<Range<usize>>,
) -> Result<Layout, Diagnostic> {
    let mut elements: Vec<Element> = Vec::new();
    let mut metadata_ranges = Vec::new();
    for range in comments {
        let start = line_start_calculate(text, range.start);
        let after = line_end_calculate(text, start);
        let line = text[start..after].trim_end_matches(['\r', '\n']);
        // A service comment must occupy the complete, standalone raw line.
        if range.start != start || range.end > after {
            return Err(Diagnostic::new_at(DiagnosticKind::InvalidMetadata, range));
        }
        let metadata = metadata::metadata_parse(line, start..after)?;
        metadata_ranges.push(start..after);
        let (begin, end, anchor) = element_bounds_calculate(text, headings, start, after)?;
        if elements.last().is_some_and(|e| e.bytes.get_end() > begin) {
            return Err(Diagnostic::new_at(
                DiagnosticKind::NestedElement,
                begin..end,
            ));
        }
        let id = KnowledgeId::new(path.clone(), anchor)
            .map_err(|_| Diagnostic::new_at(DiagnosticKind::InvalidTarget, begin..end))?;
        #[expect(
            clippy::expect_used,
            reason = "element_bounds_calculate returns ordered ranges"
        )]
        let bytes = ByteSpan::new(begin, end).expect("heading ranges are ordered");
        #[expect(clippy::expect_used, reason = "a nonempty element spans ordered lines")]
        let lines = LineRange::new(
            line_number_calculate(text, begin),
            line_number_calculate(text, end.saturating_sub(1)),
        )
        .expect("nonempty element lines are ordered");
        elements.push(Element {
            id,
            metadata,
            bytes,
            lines,
            hash: Sha256::digest(&text.as_bytes()[begin..end]),
        });
    }
    Ok(Layout {
        elements,
        metadata_ranges,
    })
}

fn element_bounds_calculate(
    text: &str,
    headings: &[Heading],
    start: usize,
    after: usize,
) -> Result<(usize, usize, Option<Anchor>), Diagnostic> {
    if start == 0 {
        return Ok((0, text.len(), None));
    }
    let heading = headings
        .iter()
        .find(|h| h.after == start && h.anchor.is_some())
        .ok_or_else(|| Diagnostic::new_at(DiagnosticKind::OrphanMetadata, start..after))?;
    let end = headings
        .iter()
        .find(|next| next.start > heading.start && next.level <= heading.level)
        .map_or(text.len(), |h| h.start);
    Ok((heading.start, end, heading.anchor.clone()))
}

fn line_start_calculate(text: &str, offset: usize) -> usize {
    text.as_bytes()[..offset]
        .iter()
        .rposition(|&c| c == b'\n')
        .map_or(0, |position| position + 1)
}

fn line_end_calculate(text: &str, offset: usize) -> usize {
    text.as_bytes()[offset..]
        .iter()
        .position(|&c| c == b'\n')
        .map_or(text.len(), |position| offset + position + 1)
}

fn line_number_calculate(text: &str, at: usize) -> u64 {
    text.as_bytes()[..at]
        .iter()
        .filter(|&&c| c == b'\n')
        .count() as u64
        + 1
}
