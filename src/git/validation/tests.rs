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
