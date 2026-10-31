use super::constants::{BLOB_KIND, TREE_KIND};
use crate::service::{SnapshotError, SnapshotErrorKind};

/// Borrow the raw metadata for one decoded tree entry.
#[derive(Debug)]
pub(super) struct TreeEntry<'a> {
    pub(super) mode: u16,
    pub(super) name: &'a [u8],
    pub(super) oid: &'a [u8],
}

/// Decode verified tree bytes with Git's `canon_mode` type rules, skipping gitlinks.
///
/// # Errors
/// Returns `InternalError` for malformed entry boundaries or octal modes.
pub(super) fn get_children(
    mut bytes: &[u8],
    oid_len: usize,
) -> Result<Vec<(String, String)>, SnapshotError> {
    let mut children = Vec::new();
    while !bytes.is_empty() {
        let (entry, remaining) = get_entry(bytes, oid_len)?;
        let kind = match entry.mode & 0o170000 {
            0o100000 | 0o120000 => Some(BLOB_KIND),
            0o040000 => Some(TREE_KIND),
            _ => None,
        };
        if let Some(kind) = kind {
            let oid: String = entry.oid.iter().map(|byte| format!("{byte:02x}")).collect();
            children.push((oid, kind.into()));
        }
        bytes = remaining;
    }
    Ok(children)
}

/// Read one raw entry without imposing strict mode, name or ordering rules.
///
/// # Errors
/// Returns `InternalError` for malformed entry boundaries or octal modes.
pub(super) fn get_entry(
    bytes: &[u8],
    oid_len: usize,
) -> Result<(TreeEntry<'_>, &[u8]), SnapshotError> {
    let space = bytes
        .iter()
        .position(|byte| *byte == b' ')
        .ok_or_else(make_integrity_error)?;
    if space == 0 {
        return Err(make_integrity_error());
    }
    // Git parses raw modes into uint16_t; permissions are normalized by type.
    let mut mode = 0_u16;
    for digit in &bytes[..space] {
        if !(b'0'..=b'7').contains(digit) {
            return Err(make_integrity_error());
        }
        mode = mode.wrapping_mul(8).wrapping_add(u16::from(digit - b'0'));
    }
    let bytes = &bytes[space + 1..];
    let nul = bytes
        .iter()
        .position(|byte| *byte == 0)
        .ok_or_else(make_integrity_error)?;
    if nul == 0 {
        return Err(make_integrity_error());
    }
    let name = &bytes[..nul];
    let bytes = &bytes[nul + 1..];
    let width = oid_len / 2;
    let oid = bytes.get(..width).ok_or_else(make_integrity_error)?;
    Ok((TreeEntry { mode, name, oid }, &bytes[width..]))
}

fn make_integrity_error() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git object integrity failed",
    )
}
