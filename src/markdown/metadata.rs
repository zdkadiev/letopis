use super::{Diagnostic, DiagnosticKind};
use crate::domain::{ElementMetadata, TreeOid};
use serde::{Deserialize, Deserializer, de::Error};
use std::ops::Range;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Metadata {
    created_at: u64,
    #[serde(default, deserialize_with = "deserialize_optional_oid")]
    source_snapshot: Option<TreeOid>,
}

fn deserialize_optional_oid<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<TreeOid>, D::Error> {
    // The field may be absent, but a present JSON null is forbidden by X-4.
    let raw = String::deserialize(deserializer)?;
    TreeOid::parse(&raw).map(Some).map_err(D::Error::custom)
}

pub(super) fn parse(line: &str, range: Range<usize>) -> Result<ElementMetadata, Diagnostic> {
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
