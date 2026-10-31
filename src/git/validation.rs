use super::tree::read_entry;
use crate::service::{SnapshotError, SnapshotErrorKind};
use std::{cmp::Ordering, collections::BTreeSet};

const TREE_MODE: u16 = 0o040000;
const SYMLINK_MODE: u16 = 0o120000;
const VALID_MODES: [u16; 5] = [0o100644, 0o100755, SYMLINK_MODE, TREE_MODE, 0o160000];
const MAX_NAME_BYTES: usize = 4096;

/// Check the own-body acceptance rules of `hash-object -t tree --no-filters`.
///
/// Raw identity is never normalized and referenced objects are not read. Malformed
/// framing, modes, names, null IDs, duplicates and ordering return a safe error.
///
/// # Errors
/// Returns `InternalError` when the body fails Git's structural checks.
pub(super) fn validate_tree_bytes(mut bytes: &[u8], oid_len: usize) -> Result<(), SnapshotError> {
    if !matches!(oid_len, 40 | 64) {
        return Err(make_integrity_error());
    }
    let mut names = BTreeSet::new();
    let mut previous = None;
    while !bytes.is_empty() {
        // Git's tree descriptor also checks the final entry's NUL boundary.
        if bytes.get(bytes.len().saturating_sub(oid_len / 2 + 1)) != Some(&0) || bytes[0] == b'0' {
            return Err(make_integrity_error());
        }
        let (entry, remaining) = read_entry(bytes, oid_len)?;
        if !VALID_MODES.contains(&entry.mode)
            || entry.oid.iter().all(|byte| *byte == 0)
            || !names.insert(entry.name)
        {
            return Err(make_integrity_error());
        }
        validate_name(entry.name, entry.mode)?;
        if let Some((name, mode)) = previous
            && compare_names(name, mode, entry.name, entry.mode) != Ordering::Less
        {
            return Err(make_integrity_error());
        }
        previous = Some((entry.name, entry.mode));
        bytes = remaining;
    }
    Ok(())
}

fn validate_name(name: &[u8], mode: u16) -> Result<(), SnapshotError> {
    if name.len() > MAX_NAME_BYTES
        || matches!(name, b"." | b"..")
        || name.contains(&b'/')
        || is_hfs_alias(name, b"git")
        || name.split(|byte| *byte == b'\\').any(is_ntfs_dotgit)
    {
        return Err(make_integrity_error());
    }
    if mode != SYMLINK_MODE {
        return Ok(());
    }
    for (needle, short_prefix) in [
        (b"gitmodules".as_slice(), b"gi7eba".as_slice()),
        (b"gitattributes", b"gi7d29"),
        (b"gitignore", b"gi250a"),
        (b"mailmap", b"maba30"),
    ] {
        if is_hfs_alias(name, needle) || is_ntfs_alias(name, needle, short_prefix) {
            return Err(make_integrity_error());
        }
    }
    // Only .gitmodules is protected after backslashes by Git's structural check.
    if name.iter().enumerate().any(|(offset, byte)| {
        *byte == b'\\' && is_ntfs_alias(&name[offset + 1..], b"gitmodules", b"gi7eba")
    }) {
        return Err(make_integrity_error());
    }
    Ok(())
}

fn compare_names(left: &[u8], left_mode: u16, right: &[u8], right_mode: u16) -> Ordering {
    let left = left
        .iter()
        .copied()
        .chain((left_mode == TREE_MODE).then_some(b'/'));
    let right = right
        .iter()
        .copied()
        .chain((right_mode == TREE_MODE).then_some(b'/'));
    left.cmp(right)
}

fn is_hfs_alias(name: &[u8], needle: &[u8]) -> bool {
    // Git's HFS decoder treats malformed UTF-8 and U+FFFE/U+FFFF as end of name.
    let valid = match std::str::from_utf8(name) {
        Ok(valid) => valid,
        Err(error) => std::str::from_utf8(&name[..error.valid_up_to()]).unwrap_or_default(),
    };
    let mut characters = valid
        .chars()
        .take_while(|character| !matches!(*character, '\u{fffe}' | '\u{ffff}'))
        .filter(|character| {
            !matches!(*character as u32, 0x200c..=0x200f | 0x202a..=0x202e | 0x206a..=0x206f | 0xfeff)
        });
    if characters.next() != Some('.') {
        return false;
    }
    for byte in needle {
        if characters
            .next()
            .is_none_or(|character| !character.eq_ignore_ascii_case(&char::from(*byte)))
        {
            return false;
        }
    }
    characters.next().is_none()
}

fn is_ntfs_dotgit(name: &[u8]) -> bool {
    let suffix = if name
        .get(..4)
        .is_some_and(|part| part.eq_ignore_ascii_case(b".git"))
    {
        &name[4..]
    } else if name
        .get(..5)
        .is_some_and(|part| part.eq_ignore_ascii_case(b"git~1"))
    {
        &name[5..]
    } else {
        return false;
    };
    has_ntfs_suffix(suffix)
}

fn is_ntfs_alias(name: &[u8], needle: &[u8], short_prefix: &[u8]) -> bool {
    let long_length = needle.len() + 1;
    if name.first() == Some(&b'.')
        && name
            .get(1..long_length)
            .is_some_and(|part| part.eq_ignore_ascii_case(needle))
    {
        return has_ntfs_suffix(&name[long_length..]);
    }
    if name
        .get(..6)
        .is_some_and(|part| part.eq_ignore_ascii_case(&needle[..6]))
        && name.get(6) == Some(&b'~')
        && name.get(7).is_some_and(|byte| (b'1'..=b'4').contains(byte))
    {
        return has_ntfs_suffix(&name[8..]);
    }
    let Some(short) = name.get(..8) else {
        return false;
    };
    let Some(tilde) = short.iter().position(|byte| *byte == b'~') else {
        return false;
    };
    tilde <= short_prefix.len()
        && tilde < 7
        && short[..tilde].eq_ignore_ascii_case(&short_prefix[..tilde])
        && (b'1'..=b'9').contains(&short[tilde + 1])
        && short[tilde + 2..].iter().all(u8::is_ascii_digit)
        && has_ntfs_suffix(&name[8..])
}

fn has_ntfs_suffix(suffix: &[u8]) -> bool {
    suffix
        .iter()
        .take_while(|byte| **byte != b':')
        .all(|byte| matches!(byte, b' ' | b'.'))
}

fn make_integrity_error() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git tree integrity failed",
    )
}

#[cfg(test)]
mod tests;
