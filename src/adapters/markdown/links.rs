use super::{Diagnostic, DiagnosticKind, Events};
use crate::domain::{Edge, EdgeTarget, EdgeType, Element, KnowledgeId, SourceUri, ValueError};
use pulldown_cmark::{Event, LinkType, Tag};
use std::collections::BTreeSet;

pub(super) fn collect(
    events: &Events<'_>,
    elements: &[Element],
) -> Result<BTreeSet<Edge>, Diagnostic> {
    let mut edges = BTreeSet::new();
    for (event, range) in events {
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
            .find(|e| range.start >= e.bytes.start() && range.end <= e.bytes.end())
        else {
            continue;
        };
        if !title.starts_with("letopis:") {
            continue;
        }
        let edge_type = title
            .strip_prefix("letopis:type=")
            .and_then(|s| EdgeType::parse(s).ok())
            .ok_or_else(|| Diagnostic::at(DiagnosticKind::InvalidLinkTitle, range.clone()))?;
        let target = if edge_type.as_str() == "external" {
            SourceUri::parse(dest_url).map(EdgeTarget::Source)
        } else {
            KnowledgeId::parse(dest_url).map(EdgeTarget::Knowledge)
        }
        .map_err(|_| Diagnostic::at(DiagnosticKind::InvalidTarget, range.clone()))?;
        let edge = Edge::new(element.id.clone(), target, edge_type).map_err(|e| {
            Diagnostic::at(
                if e == ValueError::SelfLoop {
                    DiagnosticKind::SelfLoop
                } else {
                    DiagnosticKind::InvalidTarget
                },
                range.clone(),
            )
        })?;
        edges.insert(edge);
    }
    Ok(edges)
}
