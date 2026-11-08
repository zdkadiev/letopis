use super::constants::OBJECT_INTEGRITY_MESSAGE;
use crate::service::{SnapshotError, SnapshotErrorKind};

/// Borrow the raw metadata for one decoded tree entry.
#[derive(Debug)]
pub(super) struct TreeEntry<'a> {
    pub(super) mode: u16,
    pub(super) name: &'a [u8],
    pub(super) oid: &'a [u8],
}

/// Returns one raw tree entry without imposing strict mode, name or ordering rules.
///
/// # Arguments
///
/// * `bytes` — tree bytes positioned at one entry descriptor.
/// * `oid_len` — hexadecimal object ID length used to frame the entry's identity.
///
/// # Errors
/// Returns `InternalError` for malformed entry boundaries or octal modes.
pub(super) fn tree_entry_get(
    bytes: &[u8],
    oid_len: usize,
) -> Result<(TreeEntry<'_>, &[u8]), SnapshotError> {
    let space = bytes.iter().position(|byte| *byte == b' ').ok_or_else(|| {
        SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
    })?;
    if space == 0 {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            OBJECT_INTEGRITY_MESSAGE,
        ));
    }
    // Git parses raw modes into uint16_t; permissions are normalized by type.
    let mut mode = 0_u16;
    for digit in &bytes[..space] {
        if !(b'0'..=b'7').contains(digit) {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                OBJECT_INTEGRITY_MESSAGE,
            ));
        }
        mode = mode.wrapping_mul(8).wrapping_add(u16::from(digit - b'0'));
    }
    let bytes = &bytes[space + 1..];
    let nul = bytes.iter().position(|byte| *byte == 0).ok_or_else(|| {
        SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
    })?;
    if nul == 0 {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            OBJECT_INTEGRITY_MESSAGE,
        ));
    }
    let name = &bytes[..nul];
    let bytes = &bytes[nul + 1..];
    let width = oid_len / 2;
    let oid = bytes.get(..width).ok_or_else(|| {
        SnapshotError::new(SnapshotErrorKind::InternalError, OBJECT_INTEGRITY_MESSAGE)
    })?;
    Ok((TreeEntry { mode, name, oid }, &bytes[width..]))
}
