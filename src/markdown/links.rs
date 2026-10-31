use super::{Diagnostic, DiagnosticKind, Events};
use crate::domain::{
    Edge, EdgeTarget, EdgeType, Element, KnowledgeId, RepoPath, SourceUri, ValueError,
};
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
    let (edge_type, address) =
        parse_link_title(title).map_err(|kind| Diagnostic::new_at(kind, range.clone()))?;
    let target = if edge_type.as_str() == "external" {
        SourceUri::parse(address).map(EdgeTarget::Source)
    } else {
        KnowledgeId::parse(address).map(EdgeTarget::Knowledge)
    }
    .map_err(|_| Diagnostic::new_at(DiagnosticKind::InvalidTarget, range.clone()))?;
    verify_navigation_path(&element.id, dest_url, &target)
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

/// Parse the fixed title prefix; the complete terminal address remains literal.
fn parse_link_title(title: &str) -> Result<(EdgeType, &str), DiagnosticKind> {
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

/// Compare local file identities only; remote source addresses stay opaque.
fn verify_navigation_path(
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
    let navigation = resolve_navigation_path(source, destination)?;
    if navigation.as_str() != target_path {
        return Err(DiagnosticKind::LinkPathMismatch);
    }
    Ok(())
}

/// Resolve a relative navigation file against the owning Markdown file without I/O.
fn resolve_navigation_path(
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
    let decoded = decode_navigation_path(raw)?;
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

/// Decode navigation escapes once, rejecting ambiguous separators and non-UTF-8 paths.
fn decode_navigation_path(raw: &str) -> Result<String, DiagnosticKind> {
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

#[cfg(test)]
mod tests {
    use super::decode_navigation_path;
    use crate::service::DiagnosticKind;

    #[test]
    fn rejects_literal_and_encoded_controls_and_backslashes() {
        // Given: dangerous navigation bytes need rejection even without a target mismatch.
        for raw in [
            "cache\\key.rs",
            "cache\u{0}.rs",
            "cache\u{7f}.rs",
            "cache\u{85}.rs",
            "cache%5ckey.rs",
            "cache%00.rs",
            "cache%7f.rs",
            "cache%C2%85.rs",
        ] {
            // When: decode navigation independently of the canonical target.
            let result = decode_navigation_path(raw);
            // Then: literal and decoded forbidden characters have the same safe reason.
            assert_eq!(result, Err(DiagnosticKind::LinkPathMismatch), "{raw:?}");
        }
    }
}
