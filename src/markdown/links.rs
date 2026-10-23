use super::{Diagnostic, DiagnosticKind, Events};
use crate::domain::{Edge, EdgeTarget, EdgeType, Element, KnowledgeId, SourceUri, ValueError};
use pulldown_cmark::{Event, LinkType, Tag, TagEnd};
use std::{collections::BTreeSet, ops::Range};

pub(super) fn collect(
    events: &Events<'_>,
    elements: &[Element],
) -> Result<BTreeSet<Edge>, Diagnostic> {
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
        edges.insert(build_edge(element, dest_url, title, range)?);
    }
    Ok(edges)
}

/// Build one graph edge from a `letopis:` link title, rejecting malformed declarations.
fn build_edge(
    element: &Element,
    dest_url: &str,
    title: &str,
    range: &Range<usize>,
) -> Result<Edge, Diagnostic> {
    let edge_type = title
        .strip_prefix("letopis:type=")
        .and_then(|s| EdgeType::parse(s).ok())
        .ok_or_else(|| Diagnostic::new_at(DiagnosticKind::InvalidLinkTitle, range.clone()))?;
    let target = if edge_type.as_str() == "external" {
        SourceUri::parse(dest_url).map(EdgeTarget::Source)
    } else {
        KnowledgeId::parse(dest_url).map(EdgeTarget::Knowledge)
    }
    .map_err(|_| Diagnostic::new_at(DiagnosticKind::InvalidTarget, range.clone()))?;
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
