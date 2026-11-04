use super::tree::get_entry;
use crate::service::{SnapshotError, SnapshotErrorKind};
use std::{cmp::Ordering, collections::BTreeSet};

const TREE_INTEGRITY_MESSAGE: &str = "Git tree integrity failed";
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
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            TREE_INTEGRITY_MESSAGE,
        ));
    }
    let mut names = BTreeSet::new();
    let mut previous = None;
    while !bytes.is_empty() {
        // Git's tree descriptor also checks the final entry's NUL boundary.
        if bytes.get(bytes.len().saturating_sub(oid_len / 2 + 1)) != Some(&0) || bytes[0] == b'0' {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                TREE_INTEGRITY_MESSAGE,
            ));
        }
        let (entry, remaining) = get_entry(bytes, oid_len)?;
        if !VALID_MODES.contains(&entry.mode)
            || entry.oid.iter().all(|byte| *byte == 0)
            || !names.insert(entry.name)
        {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                TREE_INTEGRITY_MESSAGE,
            ));
        }
        validate_name(entry.name, entry.mode)?;
        if let Some((name, mode)) = previous
            && compare_names(name, mode, entry.name, entry.mode) != Ordering::Less
        {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                TREE_INTEGRITY_MESSAGE,
            ));
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
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            TREE_INTEGRITY_MESSAGE,
        ));
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
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                TREE_INTEGRITY_MESSAGE,
            ));
        }
    }
    // Only .gitmodules is protected after backslashes by Git's structural check.
    if name.iter().enumerate().any(|(offset, byte)| {
        *byte == b'\\' && is_ntfs_alias(&name[offset + 1..], b"gitmodules", b"gi7eba")
    }) {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            TREE_INTEGRITY_MESSAGE,
        ));
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

#[cfg(test)]
mod tests {
    use super::validate_tree_bytes;
    use crate::{
        process_test_support::{FORMATS, ObjectFormat, TempRepo},
        service::SnapshotErrorKind,
    };

    #[test]
    fn validates_raw_modes_like_hash_object() {
        // Given: canonical modes, historical permissions, and uint16-wrapped octal modes.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (mode, expected) in [
                ("100644", true),
                ("100755", true),
                ("120000", true),
                ("40000", true),
                ("160000", true),
                ("1100644", true),
                ("2100644", true),
                ("100664", false),
                ("100600", false),
                ("040000", false),
                ("0100644", false),
                ("0", false),
                ("000000", false),
                ("1", false),
                ("40001", false),
                ("160001", false),
                ("170000", false),
                ("100888", false),
                ("10064x", false),
                ("-100644", false),
                ("", false),
                ("100644\n", false),
                ("777777777777777777777777", false),
            ] {
                let bytes = build_entry(mode, b"a", width);
                // When / Then: raw mode acceptance agrees with Git without normalization.
                assert_matches_oracle(&repo, &bytes, width, expected);
            }
        }
    }

    #[test]
    fn validates_names_like_hash_object() {
        // Given: raw names include platform aliases and names that remain legal on Unix.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (name, expected) in [
                (b"a".as_slice(), true),
                (b"a\\b", true),
                (b"\n", true),
                (b"\xff", true),
                (b"CON", true),
                (b"a:", true),
                (b"a.", true),
                (b".gitignore", true),
                (b".gitmodules", true),
                (b".gitx", true),
                (b"", false),
                (b".", false),
                (b"..", false),
                (b"a/b", false),
                (b".git", false),
                (b".GiT", false),
                (b".git.", false),
                (b".git :stream", false),
                (b"git~1", false),
                (b"git~2", true),
                (b"a\\.git", false),
                (b".git\\a", false),
                (b"a\\.git\\b", false),
                (b".Gi\xe2\x80\x8ct", false),
                (b".git\xff", false),
                (b".git\xef\xbf\xbd", true),
                (b"a\0b", false),
            ] {
                let bytes = build_entry("100644", name, width);
                // When / Then: reserved aliases fail without rejecting ordinary binary names.
                assert_matches_oracle(&repo, &bytes, width, expected);
            }
            for (length, expected) in [(4096, true), (4097, false)] {
                assert_matches_oracle(
                    &repo,
                    &build_entry("100644", &vec![b'a'; length], width),
                    width,
                    expected,
                );
            }
        }
    }

    #[test]
    fn validates_special_symlink_names_like_hash_object() {
        // Given: special files, HFS/NTFS aliases, and backslash suffixes have distinct rules.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (name, expected) in [
                (b"ordinary".as_slice(), true),
                (b".gitmodules", false),
                (b".gitattributes", false),
                (b".gitignore", false),
                (b".mailmap", false),
                (b".GITIGNORE:stream", false),
                (b".gitmodules.", false),
                (b"gitmod~1", false),
                (b"gitmod~4", false),
                (b"gitmod~5", true),
                (b"gi7eba~1", false),
                (b"gi7d29~1", false),
                (b"gi250a~1", false),
                (b"maba30~1", false),
                (b".gitignore\\x", true),
                (b"x\\.gitignore", true),
                (b"x\\.gitattributes", true),
                (b"x\\.gitmodules", false),
                (b"x\\.gitmodules\\y", true),
                (b".gitmod\xe2\x80\x8cules", false),
            ] {
                // When / Then: symlink restrictions match the exact structural operation.
                assert_matches_oracle(&repo, &build_entry("120000", name, width), width, expected);
            }
        }
    }

    #[test]
    fn validates_hfs_noncharacter_termination_like_hash_object() {
        // Given: Git stops HFS decoding only at these two valid noncharacter scalars.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (name, expected) in [
                (b".git\xef\xbf\xbe".as_slice(), false),
                (b".git\xef\xbf\xbf", false),
                (b".git\xef\xbf\xbetail", false),
                (b".git\xef\xbf\xbftail", false),
                (b".gi\xef\xbf\xbet", true),
                (b"\xef\xbf\xbe.git", true),
                (b"ordinary\xef\xbf\xbe", true),
                (b"ordinary\xef\xbf\xbf", true),
                (b".git\xef\xbf\xbd", true),
                (b".git\xef\xb7\x90", true),
                (b".git\xef\xb7\xaf", true),
                (b".git\xf0\x9f\xbf\xbe", true),
                (b".git\xf0\x9f\xbf\xbf", true),
                (b".git\xf4\x8f\xbf\xbe", true),
                (b".git\xf4\x8f\xbf\xbf", true),
            ] {
                // When / Then: exact alias termination agrees with the independent Git oracle.
                assert_matches_oracle(&repo, &build_entry("100644", name, width), width, expected);
            }
        }
    }

    #[test]
    fn validates_hfs_noncharacter_termination_for_symlinks_like_hash_object() {
        // Given: protected symlink aliases and ordinary names end with noncharacters.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (name, expected) in [
                (b".gitignore\xef\xbf\xbe".as_slice(), false),
                (b".gitignore\xef\xbf\xbf", false),
                (b".gitmodules\xef\xbf\xbe", false),
                (b".gitmodules\xef\xbf\xbf", false),
                (b".gitattributes\xef\xbf\xbe", false),
                (b".gitattributes\xef\xbf\xbf", false),
                (b".mailmap\xef\xbf\xbe", false),
                (b".mailmap\xef\xbf\xbf", false),
                (b".gitignore\xef\xbf\xbd", true),
                (b".gitignore\xef\xb7\x90", true),
                (b".gitignore\xf0\x9f\xbf\xbe", true),
                (b".gitignore\xf4\x8f\xbf\xbf", true),
                (b"ordinary\xef\xbf\xbe", true),
                (b"ordinary\xef\xbf\xbf", true),
            ] {
                // When / Then: decoding termination protects aliases without banning Unicode names.
                assert_matches_oracle(&repo, &build_entry("120000", name, width), width, expected);
            }
        }
    }

    #[test]
    fn validates_entry_boundaries_like_hash_object() {
        // Given: both hash widths and malformed delimiters, object IDs, and trailing bytes.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            let valid = build_entry("100644", b"a", width);
            let mut null_oid = b"100644 a\0".to_vec();
            null_oid.extend(vec![0; width / 2]);
            let mut extra = valid.clone();
            extra.push(1);
            for (bytes, expected) in [
                (Vec::new(), true),
                (valid.clone(), true),
                (null_oid, false),
                (valid[..valid.len() - 1].to_vec(), false),
                (extra, false),
                (b"100644 a".to_vec(), false),
                (b"100644\0a".to_vec(), false),
                (b"100644 a\0".to_vec(), false),
                (vec![0; width / 2 + 3], false),
                (
                    build_entry("100644", b"a", if width == 40 { 64 } else { 40 }),
                    false,
                ),
            ] {
                // When / Then: framing failures are never silently dropped from decoding.
                assert_matches_oracle(&repo, &bytes, width, expected);
            }
        }
    }

    #[test]
    fn validates_order_and_nonadjacent_duplicates_like_hash_object() {
        // Given: directory order includes an implicit slash and may hide duplicate names.
        for format in FORMATS {
            let repo = TempRepo::new(format);
            let width = get_oid_length(format);
            for (entries, expected) in [
                (vec![("100644", "a"), ("100644", "b")], true),
                (vec![("100644", "b"), ("100644", "a")], false),
                (vec![("100644", "a"), ("100644", "a")], false),
                (vec![("100644", "a.c"), ("40000", "a")], true),
                (vec![("40000", "a"), ("100644", "a.c")], false),
                (
                    vec![("100644", "a"), ("100644", "a.c"), ("40000", "a")],
                    false,
                ),
                (vec![("100644", "a"), ("40000", "a")], false),
                (
                    vec![
                        ("100644", "a"),
                        ("100644", "a.c"),
                        ("40000", "a.c"),
                        ("40000", "a"),
                    ],
                    false,
                ),
                (vec![("40000", "a"), ("100644", "a0")], true),
            ] {
                let bytes: Vec<_> = entries
                    .into_iter()
                    .flat_map(|(mode, name)| build_entry(mode, name.as_bytes(), width))
                    .collect();
                // When / Then: sorting and duplicate rejection agree with Git independently.
                assert_matches_oracle(&repo, &bytes, width, expected);
            }
        }
    }

    fn get_oid_length(format: ObjectFormat) -> usize {
        match format {
            ObjectFormat::Sha1 => 40,
            ObjectFormat::Sha256 => 64,
        }
    }

    fn build_entry(mode: &str, name: &[u8], width: usize) -> Vec<u8> {
        let mut bytes = mode.as_bytes().to_vec();
        bytes.push(b' ');
        bytes.extend_from_slice(name);
        bytes.push(0);
        bytes.extend(vec![0x11; width / 2]);
        bytes
    }

    fn assert_matches_oracle(repo: &TempRepo, bytes: &[u8], width: usize, expected: bool) {
        let oracle = repo.run_git(
            &["hash-object", "-t", "tree", "--no-filters", "--stdin"],
            Some(bytes),
        );
        assert_eq!(
            oracle.status.success(),
            expected,
            "Git fixture {bytes:?}: {}",
            String::from_utf8_lossy(&oracle.stderr)
        );
        let result = validate_tree_bytes(bytes, width);
        assert_eq!(result.is_ok(), oracle.status.success(), "fixture {bytes:?}");
        if let Err(error) = result {
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            assert!(!error.to_string().contains("100644"));
        }
    }
}
