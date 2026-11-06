use super::tree_bytes_validate;
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
        for (mode, is_expected) in [
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
            assert_matches_oracle(&repo, &bytes, width, is_expected);
        }
    }
}

#[test]
fn validates_names_like_hash_object() {
    // Given: raw names include platform aliases and names that remain legal on Unix.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let width = get_oid_length(format);
        for (name, is_expected) in [
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
            assert_matches_oracle(&repo, &bytes, width, is_expected);
        }
        for (length, is_expected) in [(4096, true), (4097, false)] {
            assert_matches_oracle(
                &repo,
                &build_entry("100644", &vec![b'a'; length], width),
                width,
                is_expected,
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
        for (name, is_expected) in [
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
            assert_matches_oracle(
                &repo,
                &build_entry("120000", name, width),
                width,
                is_expected,
            );
        }
    }
}

#[test]
fn validates_hfs_noncharacter_termination_like_hash_object() {
    // Given: Git stops HFS decoding only at these two valid noncharacter scalars.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let width = get_oid_length(format);
        for (name, is_expected) in [
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
            assert_matches_oracle(
                &repo,
                &build_entry("100644", name, width),
                width,
                is_expected,
            );
        }
    }
}

#[test]
fn validates_hfs_noncharacter_termination_for_symlinks_like_hash_object() {
    // Given: protected symlink aliases and ordinary names end with noncharacters.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let width = get_oid_length(format);
        for (name, is_expected) in [
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
            assert_matches_oracle(
                &repo,
                &build_entry("120000", name, width),
                width,
                is_expected,
            );
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
        for (bytes, is_expected) in [
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
            assert_matches_oracle(&repo, &bytes, width, is_expected);
        }
    }
}

#[test]
fn validates_order_and_nonadjacent_duplicates_like_hash_object() {
    // Given: directory order includes an implicit slash and may hide duplicate names.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let width = get_oid_length(format);
        for (entries, is_expected) in [
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
            assert_matches_oracle(&repo, &bytes, width, is_expected);
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

fn assert_matches_oracle(repo: &TempRepo, bytes: &[u8], width: usize, is_expected: bool) {
    let oracle = repo.run_git(
        &["hash-object", "-t", "tree", "--no-filters", "--stdin"],
        Some(bytes),
    );
    assert_eq!(
        oracle.status.success(),
        is_expected,
        "Git fixture {bytes:?}: {}",
        String::from_utf8_lossy(&oracle.stderr)
    );
    let result = tree_bytes_validate(bytes, width);
    assert_eq!(result.is_ok(), oracle.status.success(), "fixture {bytes:?}");
    if let Err(error) = result {
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert!(!error.to_string().contains("100644"));
    }
}
