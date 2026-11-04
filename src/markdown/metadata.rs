use super::{Diagnostic, DiagnosticKind};
use crate::domain::{ElementMetadata, TreeOid};
use serde::{Deserialize, Deserializer, de::Error};
use std::ops::Range;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    created_at: u64,
    #[serde(default, deserialize_with = "optional_oid_deserialize")]
    source_snapshot: Option<TreeOid>,
}

/// Parse one complete X-4 metadata line without retaining a partial value.
///
/// # Errors
/// Returns a diagnostic covering `range` for invalid delimiters, JSON or metadata fields.
pub(super) fn metadata_parse(
    line: &str,
    range: Range<usize>,
) -> Result<ElementMetadata, Diagnostic> {
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
