use super::fail_integrity;
use crate::service::SnapshotError;

/// Decode verified tree bytes with Git's `canon_mode` type rules, skipping gitlinks.
pub(in crate::git) fn read_children(
    mut bytes: &[u8],
    oid_len: usize,
) -> Result<Vec<(String, String)>, SnapshotError> {
    let width = oid_len / 2;
    let mut children = Vec::new();
    while !bytes.is_empty() {
        let space = bytes
            .iter()
            .position(|b| *b == b' ')
            .ok_or_else(fail_integrity)?;
        if space == 0 {
            return Err(fail_integrity());
        }
        // Git parses raw modes into uint16_t; permissions are normalized by type.
        let mut mode = 0_u16;
        for digit in &bytes[..space] {
            if !(b'0'..=b'7').contains(digit) {
                return Err(fail_integrity());
            }
            mode = mode.wrapping_mul(8).wrapping_add(u16::from(digit - b'0'));
        }
        bytes = &bytes[space + 1..];
        let nul = bytes
            .iter()
            .position(|b| *b == 0)
            .ok_or_else(fail_integrity)?;
        if nul == 0 {
            return Err(fail_integrity());
        }
        bytes = &bytes[nul + 1..];
        let raw = bytes.get(..width).ok_or_else(fail_integrity)?;
        let kind = match mode & 0o170000 {
            0o100000 | 0o120000 => Some("blob"),
            0o040000 => Some("tree"),
            _ => None,
        };
        if let Some(kind) = kind {
            let oid: String = raw.iter().map(|b| format!("{b:02x}")).collect();
            children.push((oid, kind.into()));
        }
        bytes = &bytes[width..];
    }
    Ok(children)
}
