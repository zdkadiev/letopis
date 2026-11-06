//! Gitignore matching over raw repository paths.
use crate::service::{GitError, GitResult};
use gix::{bstr::ByteSlice, glob::pattern::Case, ignore::Search};

/// Apply root-relative patterns to a NUL-framed union without interpreting document roles.
pub(super) fn paths_filter_by_patterns(
    raw: &[u8],
    patterns: &[u8],
    case: Case,
) -> GitResult<Vec<u8>> {
    let mut search = Search::default();
    search
        .add_patterns_buffer(patterns, "ignore", None, Default::default())
        .map_err(GitError::new)?;
    let mut filtered = Vec::new();
    for path in raw.split(|byte| *byte == 0).filter(|path| !path.is_empty()) {
        if !path_is_excluded(&search, path, case) {
            filtered.extend_from_slice(path);
            filtered.push(0);
        }
    }
    Ok(filtered)
}

fn path_is_excluded(search: &Search, path: &[u8], case: Case) -> bool {
    for (index, _) in path.iter().enumerate().filter(|(_, byte)| **byte == b'/') {
        if search
            .pattern_matching_relative_path(path[..index].as_bstr(), Some(true), case)
            .is_some_and(|matched| !matched.pattern.is_negative())
        {
            return true;
        }
    }
    search
        .pattern_matching_relative_path(path.as_bstr(), Some(false), case)
        .is_some_and(|matched| !matched.pattern.is_negative())
}

#[cfg(test)]
mod tests {
    use super::paths_filter_by_patterns;
    use crate::process_test_support::{ObjectFormat, TempRepo};
    use gix::glob::pattern::Case;
    use std::{collections::BTreeSet, ffi::OsStr, fs, os::unix::ffi::OsStrExt, path::Path};

    #[test]
    fn matches_anchors_basenames_and_directory_boundaries_like_git() {
        // Given: root anchoring, basename and directory-only patterns.
        for (patterns, included, excluded) in [
            (
                b"/root.md\n".as_slice(),
                &[b"sub/root.md".as_slice()][..],
                &[b"root.md".as_slice()][..],
            ),
            (
                b"draft.md\n",
                &[b"draft.markdown".as_slice()][..],
                &[b"draft.md".as_slice(), b"sub/draft.md"][..],
            ),
            (
                b"notes/\n",
                &[b"other/keep.md".as_slice()][..],
                &[b"notes/keep.md".as_slice()][..],
            ),
        ] {
            // When / Then: literal membership and an independent Git oracle agree.
            assert_matches_git(patterns, included, excluded, Case::Sensitive);
        }
    }

    #[test]
    fn excluded_ancestors_override_child_negations_like_git() {
        // Given: child negation cannot bypass an excluded parent, but eligible parents permit it.
        for (patterns, included, excluded) in [
            (
                b"notes/\n!notes/keep.md\n".as_slice(),
                &[b"other/keep.md".as_slice()][..],
                &[b"notes/keep.md".as_slice()][..],
            ),
            (
                b"notes/*\n!notes/keep.md\n",
                &[b"notes/keep.md".as_slice()][..],
                &[b"notes/drop.md".as_slice()][..],
            ),
            (
                b"notes/**\n!notes/deep/\n!notes/deep/keep.md\n",
                &[b"notes/deep/keep.md".as_slice()][..],
                &[b"notes/deep/drop.md".as_slice()][..],
            ),
            (
                b"notes/\n!notes/deep/\n!notes/deep/keep.md\n",
                &[b"keep.md".as_slice()][..],
                &[b"notes/deep/keep.md".as_slice()][..],
            ),
            (
                b"notes/\n!notes/\nnotes/*\n!notes/keep.md\nnotes/keep.md\n",
                &[b"keep.md".as_slice()][..],
                &[b"notes/keep.md".as_slice()][..],
            ),
        ] {
            // When / Then: the matcher obeys directory precedence and last-match ordering.
            assert_matches_git(patterns, included, excluded, Case::Sensitive);
        }
    }

    #[test]
    fn matches_globs_escapes_comments_and_raw_bytes_like_git() {
        // Given: Git syntax includes slash-aware wildcards, escaping, CRLF and byte names.
        for (patterns, included, excluded) in [
            (b"**/generated?.[mM][dD]\n".as_slice(), &[b"a/generated11.md".as_slice()][..], &[b"generated1.md".as_slice(), b"a/b/generated1.MD"][..]),
            (b"docs/**/draft.md\n", &[b"other/draft.md".as_slice()][..], &[b"docs/draft.md".as_slice(), b"docs/a/b/draft.md"][..]),
            (b"docs/*.md\n", &[b"docs/deep/a.md".as_slice()][..], &[b"docs/a.md".as_slice()][..]),
            (b"\n# comment\r\n\\#note.md\r\n\\!note.md\n\\*.md\nname\\ \nignored.md   \n$cash.md\n", &[b"comment".as_slice(), b"name", b"cash.md"][..], &[b"#note.md".as_slice(), b"!note.md", b"*.md", b"name ", b"ignored.md", b"$cash.md"][..]),
            (b"[invalid\ntrailing\\\n", &[b"ordinary.md".as_slice()][..], &[][..]),
            ("**/заметка.md\n".as_bytes(), &[b"keep.md".as_slice()][..], &["a/заметка.md".as_bytes()][..]),
            (b"bad\xff.md\n", &[b"good.md".as_slice()][..], &[b"bad\xff.md".as_slice()][..]),
        ] {
            // When / Then: supplied bytes and native Git produce the literal excluded set.
            assert_matches_git(patterns, included, excluded, Case::Sensitive);
        }
    }

    #[test]
    fn matching_uses_the_repository_case_mode() {
        // Given: identical uppercase patterns with both Git case policies.
        assert_matches_git(b"NOTE.md\n", &[b"note.md"], &[], Case::Sensitive);
        // When / Then: case folding excludes the lowercase leaf as Git does.
        assert_matches_git(b"NOTE.md\n", &[], &[b"note.md"], Case::Fold);
    }

    fn assert_matches_git(patterns: &[u8], included: &[&[u8]], excluded: &[&[u8]], case: Case) {
        let names: Vec<_> = included.iter().chain(excluded).copied().collect();
        let mut raw = names.join(&b"\0"[..]);
        raw.push(0);
        let actual = paths_filter_by_patterns(&raw, patterns, case).unwrap();
        let actual: BTreeSet<_> = actual
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .collect();
        assert_eq!(
            actual,
            included.iter().copied().collect(),
            "patterns={patterns:?}"
        );
        let repo = TempRepo::new(ObjectFormat::Sha1);
        fs::write(repo.get_root().join(".gitignore"), patterns).unwrap();
        for name in &names {
            let path = Path::new(OsStr::from_bytes(name));
            if let Some(parent) = path.parent() {
                fs::create_dir_all(repo.get_root().join(parent)).unwrap();
            }
        }
        repo.run_git_successfully(
            &[
                "config",
                "core.ignoreCase",
                if case == Case::Fold { "true" } else { "false" },
            ],
            None,
        );
        let oracle = repo.run_git(&["check-ignore", "--no-index", "-z", "--stdin"], Some(&raw));
        assert!(matches!(oracle.status.code(), Some(0 | 1)), "{oracle:?}");
        let ignored: BTreeSet<_> = oracle
            .stdout
            .split(|byte| *byte == 0)
            .filter(|path| !path.is_empty())
            .collect();
        assert_eq!(
            ignored,
            excluded.iter().copied().collect(),
            "Git patterns={patterns:?}"
        );
    }
}
