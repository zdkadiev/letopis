use super::{Diagnostic, DiagnosticKind};
use crate::domain::{
    Anchor, ByteSpan, Edge, EdgeTarget, EdgeType, Element, ElementMetadata, KnowledgeId, LineRange,
    RepoPath, Sha256, SourceUri, TreeOid, ValueError,
};
use crate::service::{MarkdownCodec, ParsedElement, ParsedFile};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::{Deserialize, Deserializer, de::Error};
use std::{collections::BTreeSet, ops::Range};

/// Markdown parser events paired with their byte ranges in the source text.
type Events<'a> = Vec<(Event<'a>, Range<usize>)>;

struct Layout {
    elements: Vec<Element>,
    metadata_ranges: Vec<Range<usize>>,
}

struct Heading {
    start: usize,
    after: usize,
    level: u8,
    anchor: Option<Anchor>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    created_at: u64,
    #[serde(default, deserialize_with = "optional_oid_deserialize")]
    source_snapshot: Option<TreeOid>,
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
    fn file_parse(
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
        let Some(layout) = layout_scan(path, text, &events).map_err(|e| vec![e])? else {
            return Ok(None);
        };
        let edges = edges_collect(&events, &layout.elements).map_err(|e| vec![e])?;
        Ok(Some(ParsedFile {
            elements: layout
                .elements
                .into_iter()
                .map(|element| {
                    let search_text =
                        search_text_render(&events, element.bytes, &layout.metadata_ranges);
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

impl Diagnostic {
    fn new_at(kind: DiagnosticKind, range: Range<usize>) -> Self {
        Self {
            kind,
            bytes: ByteSpan::new(range.start, range.end).ok(),
            message: kind.get_message().into(),
        }
    }
}

/// Parses one root-relative Markdown file without accessing disk, Git, SQL or a model.
/// `Ok(None)` is unmarked Markdown; errors never carry a partial valid projection.
///
/// # Arguments
///
/// * `path` — the root-relative path used to form element identifiers.
/// * `bytes` — the file contents; they must be valid UTF-8 for a marked file.
///
/// # Returns
/// The parsed file, or `None` when it declares no marked element.
///
/// # Errors
/// Returns local diagnostics for invalid UTF-8 or invalid knowledge metadata,
/// anchors, element layout or declared graph links.
pub fn file_parse(path: &RepoPath, bytes: &[u8]) -> Result<Option<ParsedFile>, Vec<Diagnostic>> {
    MarkdownParser::new().file_parse(path, bytes)
}

/// Builds the complete file layout from X-4 declarations; unmarked files return `None`.
///
/// # Arguments
///
/// * `path` — the root-relative Markdown path used to form element identifiers.
/// * `text` — the decoded Markdown source whose declarations form the layout.
/// * `events` — the parsed events paired with their byte ranges in `text`.
///
/// # Errors
/// Returns a local diagnostic for invalid identifiers, anchors or element layout.
fn layout_scan(
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
    let mut line_offset = 0;
    let mut line_number = 1;
    for range in comments {
        let start = line_start_calculate(text, range.start);
        let after = line_end_calculate(text, start);
        let line = text[start..after].trim_end_matches(['\r', '\n']);
        // A service comment must occupy the complete, standalone raw line.
        if range.start != start || range.end > after {
            return Err(Diagnostic::new_at(DiagnosticKind::InvalidMetadata, range));
        }
        let metadata = metadata_parse(line, start..after)?;
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
        // Validated elements never overlap, so preceding bytes need no second line scan.
        let line_start = line_number + line_breaks_count(&text.as_bytes()[line_offset..begin]);
        line_offset = end.saturating_sub(1);
        line_number = line_start + line_breaks_count(&text.as_bytes()[begin..line_offset]);
        #[expect(clippy::expect_used, reason = "a nonempty element spans ordered lines")]
        let lines =
            LineRange::new(line_start, line_number).expect("nonempty element lines are ordered");
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

fn line_breaks_count(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|&&c| c == b'\n').count() as u64
}

/// Parses one complete X-4 metadata line without retaining a partial value.
///
/// # Arguments
///
/// * `line` — the complete standalone metadata line, including its delimiters.
/// * `range` — the line's byte range, used to locate diagnostics.
///
/// # Errors
/// Returns a diagnostic covering `range` for invalid delimiters, JSON or metadata fields.
fn metadata_parse(line: &str, range: Range<usize>) -> Result<ElementMetadata, Diagnostic> {
    let invalid = || Diagnostic::new_at(DiagnosticKind::InvalidMetadata, range.clone());
    let json = line
        .strip_prefix("<!-- letopis: ")
        .and_then(|s| s.strip_suffix(" -->"))
        .ok_or_else(invalid)?;
    let metadata: Metadata = serde_json::from_str(json).map_err(|_| invalid())?;
    Ok(ElementMetadata {
        created_at: metadata.created_at,
        source_snapshot: metadata.source_snapshot,
    })
}

fn optional_oid_deserialize<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TreeOid>, D::Error> {
    // The field may be absent, but a present JSON null is forbidden by X-4.
    let raw = String::deserialize(deserializer)?;
    TreeOid::parse(&raw).map(Some).map_err(D::Error::custom)
}

/// Collects the distinct declared edges inside the supplied elements without target I/O.
///
/// # Arguments
///
/// * `events` — the parsed Markdown events paired with their source byte ranges.
/// * `elements` — the elements of the same file; links outside them are ignored.
///
/// # Errors
/// Returns a local diagnostic for an invalid declaration, target or navigation path.
fn edges_collect(events: &Events<'_>, elements: &[Element]) -> Result<BTreeSet<Edge>, Diagnostic> {
    let mut edges = BTreeSet::new();
    let mut image_depth = 0usize;
    for (event, range) in events {
        match event {
            Event::Start(Tag::Image { .. }) => {
                image_depth += 1;
                continue;
            }
            Event::End(TagEnd::Image) => {
                image_depth -= 1;
                continue;
            }
            _ => {}
        }
        // An image's alt text is searchable content, not a graph declaration.
        if image_depth != 0 {
            continue;
        }
        let Event::Start(Tag::Link {
            link_type: LinkType::Inline,
            dest_url,
            title,
            ..
        }) = event
        else {
            continue;
        };
        let Some(element) = elements
            .iter()
            .find(|e| range.start >= e.bytes.get_start() && range.end <= e.bytes.get_end())
        else {
            continue;
        };
        if !title.starts_with("letopis:") {
            continue;
        }
        edges.insert(edge_build(element, dest_url, title, range)?);
    }
    Ok(edges)
}

/// Builds one graph edge from a `letopis:` link title, rejecting malformed declarations.
///
/// # Arguments
///
/// * `element` — the element whose byte range contains the link.
/// * `dest_url` — the link destination used as the navigation path.
/// * `title` — the `letopis:` link title carrying the edge type and terminal address.
/// * `range` — the link's byte range, used to locate diagnostics.
fn edge_build(
    element: &Element,
    dest_url: &str,
    title: &str,
    range: &Range<usize>,
) -> Result<Edge, Diagnostic> {
    let (edge_type, address) =
        link_title_parse(title).map_err(|kind| Diagnostic::new_at(kind, range.clone()))?;
    let target = if edge_type.as_str() == "external" {
        SourceUri::parse(address).map(EdgeTarget::Source)
    } else {
        KnowledgeId::parse(address).map(EdgeTarget::Knowledge)
    }
    .map_err(|_| Diagnostic::new_at(DiagnosticKind::InvalidTarget, range.clone()))?;
    navigation_path_verify(&element.id, dest_url, &target)
        .map_err(|kind| Diagnostic::new_at(kind, range.clone()))?;
    Edge::new(element.id.clone(), target, edge_type).map_err(|e| {
        Diagnostic::new_at(
            if e == ValueError::SelfLoop {
                DiagnosticKind::SelfLoop
            } else {
                DiagnosticKind::InvalidTarget
            },
            range.clone(),
        )
    })
}

/// Parses the fixed title prefix; the complete terminal address remains literal.
///
/// # Arguments
///
/// * `title` — the complete link title, which must start with `letopis:type=` and contain
///   `;path=`.
fn link_title_parse(title: &str) -> Result<(EdgeType, &str), DiagnosticKind> {
    let (kind, address) = title
        .strip_prefix("letopis:type=")
        .and_then(|title| title.split_once(";path="))
        .ok_or(DiagnosticKind::InvalidLinkTitle)?;
    if address.is_empty() {
        return Err(DiagnosticKind::InvalidLinkTitle);
    }
    let edge_type = EdgeType::parse(kind).map_err(|_| DiagnosticKind::InvalidLinkTitle)?;
    Ok((edge_type, address))
}

/// Verifies that the navigation path matches the target path.
///
/// Remote source addresses are accepted without a path comparison.
///
/// # Arguments
///
/// * `source` — the owning knowledge element whose path anchors relative navigation.
/// * `destination` — the link destination to resolve against the owning file.
/// * `target` — the declared edge target; remote addresses are accepted without path comparison.
fn navigation_path_verify(
    source: &KnowledgeId,
    destination: &str,
    target: &EdgeTarget,
) -> Result<(), DiagnosticKind> {
    let target_path = match target {
        EdgeTarget::Knowledge(id) => id
            .as_str()
            .split_once('#')
            .map_or(id.as_str(), |(path, _)| path),
        EdgeTarget::Source(uri) => {
            let Some(local) = uri.get_local() else {
                return Ok(());
            };
            local.get_path().as_str()
        }
    };
    let navigation = navigation_path_resolve(source, destination)?;
    if navigation.as_str() != target_path {
        return Err(DiagnosticKind::LinkPathMismatch);
    }
    Ok(())
}

/// Resolves a relative navigation destination against the owning Markdown file without I/O.
///
/// # Arguments
///
/// * `source` — the owning knowledge element whose directory anchors the relative path.
/// * `destination` — the relative navigation destination, optionally followed by a fragment.
fn navigation_path_resolve(
    source: &KnowledgeId,
    destination: &str,
) -> Result<RepoPath, DiagnosticKind> {
    let raw = destination
        .split_once('#')
        .map_or(destination, |(path, _)| path);
    let first = raw.split('/').next().unwrap_or_default();
    let has_scheme = first.split_once(':').is_some_and(|(scheme, _)| {
        scheme
            .as_bytes()
            .first()
            .is_some_and(u8::is_ascii_alphabetic)
            && scheme
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || b"+-.".contains(&byte))
    });
    if raw.starts_with('/') || raw.contains('?') || has_scheme {
        return Err(DiagnosticKind::LinkPathMismatch);
    }
    let decoded = navigation_path_decode(raw)?;
    let owner = source
        .as_str()
        .split_once('#')
        .map_or(source.as_str(), |(path, _)| path);
    if decoded.is_empty() {
        return RepoPath::parse(owner).map_err(|_| DiagnosticKind::LinkPathMismatch);
    }
    let parent = owner.rsplit_once('/').map_or("", |(parent, _)| parent);
    let joined = if parent.is_empty() {
        decoded
    } else {
        format!("{parent}/{decoded}")
    };
    RepoPath::parse(&joined).map_err(|_| DiagnosticKind::LinkPathMismatch)
}

/// Decodes navigation escapes once, rejecting ambiguous separators and non-UTF-8 paths.
///
/// # Arguments
///
/// * `raw` — the raw navigation path; percent escapes are decoded once and escaped `/`
///   separators are rejected.
fn navigation_path_decode(raw: &str) -> Result<String, DiagnosticKind> {
    let mut decoded = Vec::with_capacity(raw.len());
    let mut bytes = raw.bytes();
    while let Some(byte) = bytes.next() {
        if byte != b'%' {
            decoded.push(byte);
            continue;
        }
        let high = bytes.next().and_then(|byte| char::from(byte).to_digit(16));
        let low = bytes.next().and_then(|byte| char::from(byte).to_digit(16));
        let (Some(high), Some(low)) = (high, low) else {
            return Err(DiagnosticKind::LinkPathMismatch);
        };
        let byte = u8::try_from(high * 16 + low).map_err(|_| DiagnosticKind::LinkPathMismatch)?;
        if byte == b'/' {
            return Err(DiagnosticKind::LinkPathMismatch);
        }
        decoded.push(byte);
    }
    let path = String::from_utf8(decoded).map_err(|_| DiagnosticKind::LinkPathMismatch)?;
    if path
        .chars()
        .any(|character| character.is_control() || character == '\\')
    {
        return Err(DiagnosticKind::LinkPathMismatch);
    }
    Ok(path)
}

/// Renders the element's own visible text from the already parsed full document.
///
/// # Arguments
///
/// * `events` — the parsed document events paired with their source byte ranges.
/// * `span` — the element byte span whose visible text is rendered.
/// * `metadata_ranges` — the metadata line ranges excluded from the rendered text.
fn search_text_render(
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

#[cfg(test)]
#[path = "parser_test.rs"]
mod tests;
