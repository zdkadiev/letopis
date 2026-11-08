use super::{GitIgnoreMode, GitRepository, paths_filter_by_patterns};
use crate::git::durability::SNAPSHOT_REF_PREFIX;
use crate::process_test_support as fixture;
use crate::{
    domain::RepoPath,
    open_git_snapshots,
    service::{GitSnapshots, SnapshotErrorKind, SnapshotFile},
};
use fixture::{ObjectFormat, TempRepo};
use gix::glob::pattern::Case;
use std::{
    collections::BTreeSet,
    ffi::OsStr,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
};

#[test]
fn exclusion_modes_match_git_membership_without_user_state_changes() {
    use crate::service::GitWorktreeListing;
    // Given: both object formats contain tracked, deleted and independently ignored paths.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        prepare_discovery_files(&repo);
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let before = repo.capture_fixture_state();
        for (mode, arguments) in [
            (
                GitIgnoreMode::Standard,
                vec![
                    "ls-files",
                    "-z",
                    "--cached",
                    "--others",
                    "--exclude-standard",
                ],
            ),
            (
                GitIgnoreMode::Disabled,
                vec!["ls-files", "-z", "--cached", "--others"],
            ),
        ] {
            // When: the Git library and independent Git CLI enumerate the same repository.
            let actual = git
                .worktree_paths_get_by_ignore_mode_and_patterns(mode, &[])
                .unwrap();
            let oracle = repo.run_git_successfully(&arguments, None);
            // Then: all raw paths agree, including deleted tracked membership.
            assert_eq!(get_path_set(&actual), get_path_set(&oracle));
            assert!(get_path_set(&actual).contains(b"deleted.md".as_slice()));
        }
        // When: an additional rule targets tracked membership while standard exclusions are off.
        let filtered = git
            .worktree_paths_get_by_ignore_mode_and_patterns(
                GitIgnoreMode::Disabled,
                b"tracked.md\n",
            )
            .unwrap();
        // Then: only that tracked file is excluded and Git user state is unchanged.
        let paths = get_path_set(&filtered);
        assert!(!paths.contains(b"tracked.md".as_slice()));
        assert!(paths.contains(b"ignored.md".as_slice()));
        assert!(paths.contains(b"nested/nested.md".as_slice()));
        assert!(paths.contains(b"info.md".as_slice()));
        assert!(paths.contains(b"global.md".as_slice()));
        assert_eq!(repo.capture_fixture_state(), before);
    }
}

#[test]
fn worktree_paths_preserve_raw_names_and_non_document_entries() {
    use crate::service::GitWorktreeListing;
    // Given: an index containing a non-UTF-8 Markdown path and an ordinary text file.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let oid = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
    let mut entry = format!("100644 {oid}\t").into_bytes();
    entry.extend_from_slice(b"bad\xff.md\0");
    repo.run_git_successfully(&["update-index", "-z", "--index-info"], Some(&entry));
    fs::write(repo.get_root().join("plain.txt"), b"text").unwrap();
    let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
    // When: the infrastructure boundary enumerates repository paths.
    let paths = git.worktree_paths_get_by_ignore_mode_and_patterns(GitIgnoreMode::Standard, &[]);
    // Then: document encoding and extension policies do not reject Git paths.
    let paths = paths.unwrap();
    assert!(
        paths
            .split(|byte| *byte == 0)
            .any(|path| path == b"bad\xff.md")
    );
    assert!(
        paths
            .split(|byte| *byte == 0)
            .any(|path| path == b"plain.txt")
    );
    assert!(paths.ends_with(&[0]));
}

#[test]
fn repository_open_counter_excludes_clones() {
    // Given: an independently prepared repository and this thread's open count.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let before = GitRepository::repository_get_open_count();
    // When: one discovery is followed by a cheap clone and an independent open.
    let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
    let shared = git.clone();
    // Then: clones share both the repository and reader without discovering again.
    assert_eq!(GitRepository::repository_get_open_count() - before, 1);
    assert!(Rc::ptr_eq(&git.repo, &shared.repo));
    assert!(Rc::ptr_eq(&git.reader, &shared.reader));
    assert!(Arc::ptr_eq(&git.cleanup, &shared.cleanup));
    // When: an independent caller opens the same worktree.
    let independent =
        GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
    // Then: it owns fresh state and performs its own discovery.
    assert_eq!(GitRepository::repository_get_open_count() - before, 2);
    assert!(!Rc::ptr_eq(&git.repo, &independent.repo));
}

#[test]
fn assembly_checks_unique_oids_once_without_weakening_supplied_or_final_validation() {
    use crate::service::SnapshotEntry;
    for format in fixture::FORMATS {
        // Given: one independently written blob supplied under three distinct paths.
        let repo = TempRepo::new(format);
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let oid = git.blob_write(b"original bytes").unwrap();
        let entries = ["a.md", "b.md", "nested/c.md"].map(|path| SnapshotEntry {
            path: RepoPath::parse(path).unwrap(),
            oid: oid.clone(),
        });
        let before = git.reader.borrow().object_get_read_count();
        // When: assemble from repeated caller-supplied object identities.
        let tree = git.tree_assemble(&entries).unwrap();
        // Then: exactly one addressed verification occurs in this assembly.
        assert_eq!(git.reader.borrow().object_get_read_count() - before, 1);
        git.tree_pin_by_oid(&tree).unwrap();
        // Given: valid compressed bytes replace that blob under its old identity.
        let other = git.blob_write(b"corrupt substitute").unwrap();
        repo.replace_loose_object(oid.as_str(), other.as_str());
        // When / Then: neither another assembly nor strong pin trusts the prior success/ref.
        assert_eq!(
            git.tree_assemble(&entries).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
        assert_eq!(
            git.tree_pin_by_oid(&tree).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}

#[test]
fn snapshots_work_without_git_executable() {
    use crate::{
        domain::TreeOid,
        service::{GitWorktreeListing, SnapshotState, SnapshotVerifier},
    };
    // Given: historical packed objects, prepared with an independent Git executable.
    let Ok(root) = std::env::var("LETOPIS_CHILD_FREE_ROOT") else {
        supervise_snapshot_formats();
        return;
    };
    let tree = TreeOid::parse(&std::env::var("LETOPIS_CHILD_FREE_TREE").unwrap()).unwrap();
    // When: every snapshot operation runs with an empty executable search path.
    let git = GitRepository::open(
        std::path::Path::new(&root),
        &[crate::utils::SERVICE_DIRECTORY],
    )
    .unwrap();
    let path = RepoPath::parse("exact.md").unwrap();
    let blob = git.blob_write(b"raw\r\n\0bytes").unwrap();
    let captured = git
        .tree_assemble(&[crate::service::SnapshotEntry {
            path: path.clone(),
            oid: blob,
        }])
        .unwrap();
    let empty = git.tree_create(&[]).unwrap();
    git.tree_retain_by_oid(&tree).unwrap();
    git.tree_pin_by_oid(&tree).unwrap();
    git.tree_pin_by_oid(&captured).unwrap();
    // Then: old and new snapshots remain usable, with exact bytes and raw worktree path membership.
    assert_eq!(
        git.tree_verify_by_oid(&tree).unwrap(),
        SnapshotState::Available
    );
    assert_eq!(
        git.tree_verify_by_oid(&empty).unwrap(),
        SnapshotState::Available
    );
    assert_eq!(
        git.blob_get_by_tree_oid_and_path(&captured, &path).unwrap(),
        b"raw\r\n\0bytes"
    );
    assert!(
        git.worktree_paths_get_by_ignore_mode_and_patterns(GitIgnoreMode::Standard, &[])
            .unwrap()
            .split(|byte| *byte == 0)
            .any(|path| path == b"live.md")
    );
}

#[test]
fn strong_pin_rejects_malformed_nested_trees_like_git() {
    // Given: valid roots wrap malformed nested trees written only with --literally.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b""));
        let raw_oid: Vec<_> = (0..blob.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&blob[i..i + 2], 16).unwrap())
            .collect();
        for (mode, name) in [
            ("100644", "x/y"),
            ("100644", ".git"),
            ("100644", "."),
            ("100664", "x"),
            ("100600", "x"),
            ("120000", ".gitmodules"),
            ("100644", "git~1"),
            ("100644", ".GIT"),
            ("100644", "a\\.git"),
            ("120000", ".gitignore"),
        ] {
            let mut raw = format!("{mode} {name}\0").into_bytes();
            raw.extend_from_slice(&raw_oid);
            assert!(
                !repo
                    .run_git(
                        &["hash-object", "-t", "tree", "--no-filters", "--stdin"],
                        Some(&raw)
                    )
                    .status
                    .success()
            );
            let nested = repo.get_oid(
                &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                Some(&raw),
            );
            let root = repo.get_oid(
                &["mktree"],
                Some(format!("040000 tree {nested}\tnested\n").as_bytes()),
            );
            let root = crate::domain::TreeOid::parse(&root).unwrap();
            // When: strong pin audits the nested closure.
            let error = git.tree_pin_by_oid(&root).unwrap_err();
            // Then: Git structural checks remain mandatory before publishing a ref.
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            assert!(
                !repo
                    .run_git(
                        &[
                            "show-ref",
                            "--verify",
                            &format!("{SNAPSHOT_REF_PREFIX}{}", root.as_str())
                        ],
                        None
                    )
                    .status
                    .success()
            );
        }
    }
}

#[test]
fn discovers_root_from_subdirectory() {
    // Given: SHA-1 and SHA-256 repositories contain a subdirectory.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let expected_length = match format {
            ObjectFormat::Sha1 => 40,
            ObjectFormat::Sha256 => 64,
        };
        // When: open each repository from its subdirectory.
        let git =
            GitRepository::open(repo.get_subdir(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        // Then: discovery returns the repository root and matching OID length.
        assert_eq!(git.root, repo.get_root());
        assert_eq!(git.oid_len, expected_length);
    }
}

#[test]
fn open_rejects_non_repository_and_bare() {
    // Given: a bare repository and an ordinary empty directory.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let bare = repo.get_root().join("bare");
    repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
    let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
    fs::create_dir(&empty).unwrap();
    // When: attempt to open both paths as working repositories.
    let bare_result = GitRepository::open(&bare, &[crate::utils::SERVICE_DIRECTORY]);
    let empty_result = GitRepository::open(&empty, &[crate::utils::SERVICE_DIRECTORY]);
    // Then: both paths are rejected.
    assert!(bare_result.is_err());
    assert!(empty_result.is_err());
    fs::remove_dir(empty).unwrap();
}

#[test]
fn discovery_rejects_git_metadata_as_a_working_directory() {
    // Given: Git metadata is inside the repository but outside its working tree.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    assert!(
        !repo
            .run_git(&["-C", ".git", "rev-parse", "--show-toplevel"], None)
            .status
            .success()
    );
    // When: discovery begins from the metadata directory itself.
    let result = GitRepository::open(
        &repo.get_root().join(".git"),
        &[crate::utils::SERVICE_DIRECTORY],
    );
    // Then: the adapter preserves the working-copy-only boundary.
    assert_eq!(
        result.unwrap_err().get_kind(),
        SnapshotErrorKind::InvalidRequest
    );
}

#[test]
fn discovers_trailing_newline_repository_without_touching_sibling() {
    // Given: sibling repositories share a parent and the target name ends with a newline.
    for format in fixture::FORMATS {
        let parent = TempRepo::new(format);
        let (sibling, target) = create_newline_repository_pair(&parent, format);
        let sibling_objects = parent.run_git_successfully(
            &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
            None,
        );
        let sibling_refs =
            parent.run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
        let path = RepoPath::parse("captured.md").unwrap();
        // When: create, retain, and read a snapshot in the target repository.
        let git = open_git_snapshots(&target).unwrap();
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &path,
                bytes: b"newline repository isolation\0",
            }])
            .unwrap();
        git.tree_pin_by_oid(&tree).unwrap();
        let objects_after = parent.run_git_successfully(
            &["-C", sibling.to_str().unwrap(), "count-objects", "-v"],
            None,
        );
        let refs_after =
            parent.run_git_successfully(&["-C", sibling.to_str().unwrap(), "for-each-ref"], None);
        let name = format!("{SNAPSHOT_REF_PREFIX}{}", tree.as_str());
        let actual = parent.run_git(
            &[
                "-C",
                target.to_str().unwrap(),
                "show-ref",
                "--verify",
                &name,
            ],
            None,
        );
        let bytes = git.blob_get_by_tree_oid_and_path(&tree, &path).unwrap();
        let discovered_root = GitRepository::open(&target, &[crate::utils::SERVICE_DIRECTORY])
            .unwrap()
            .root;
        // Then: the target root and bytes are correct, and sibling objects and refs are unchanged.
        assert_eq!(
            objects_after, sibling_objects,
            "opening repo\\n must not write objects into sibling repo"
        );
        assert_eq!(
            refs_after, sibling_refs,
            "opening repo\\n must not pin a ref in sibling repo"
        );
        assert!(
            actual.status.success(),
            "snapshot must be retained in the requested newline-suffixed repository"
        );
        assert_eq!(bytes, b"newline repository isolation\0");
        assert_eq!(discovered_root, target);
    }
}

#[test]
fn rejects_non_absence_errors_when_inspecting_object_alternates() {
    // Given: ENOTDIR is deterministic even when tests run with elevated privileges.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let info = repo.get_root().join(".git/objects/info");
    fs::remove_dir(&info).unwrap();
    fs::write(&info, b"not a directory").unwrap();
    // When: open the repository whose objects/info path is a file.
    let error = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY])
        .expect_err("unsafe object store was accepted");
    // Then: opening reports InternalError and preserves the file bytes.
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert_eq!(fs::read(info).unwrap(), b"not a directory");
}

#[test]
fn root_verification_and_addressed_reads_reject_malformed_tree_structure() {
    use crate::{domain::TreeOid, service::SnapshotVerifier};
    // Given: a hash-correct tree with an invalid file mode, accepted only by --literally.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut bytes = b"100664 x\0".to_vec();
        bytes.extend_from_slice(
            gix::hash::ObjectId::from_hex(blob.as_bytes())
                .unwrap()
                .as_bytes(),
        );
        let oid = repo.get_oid(
            &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            Some(&bytes),
        );
        let oid = TreeOid::parse(&oid).unwrap();
        let git = GitRepository::open(repo.get_root(), &[crate::utils::SERVICE_DIRECTORY]).unwrap();
        // When: each operation reaches this root without a full closure audit.
        let verified = git.tree_verify_by_oid(&oid).unwrap_err();
        let retained = git.tree_retain_by_oid(&oid).unwrap_err();
        let addressed = git
            .blob_get_by_tree_oid_and_path(&oid, &RepoPath::parse("x").unwrap())
            .unwrap_err();
        // Then: hash identity alone never makes malformed tree structure available.
        for error in [verified, retained, addressed] {
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        }
    }
}

fn create_newline_repository_pair(parent: &TempRepo, format: ObjectFormat) -> (PathBuf, PathBuf) {
    let sibling = parent.get_root().join("repo");
    let target = parent.get_root().join("repo\n");
    for root in [&sibling, &target] {
        parent.run_git_successfully(
            &["init", "-q", format.to_init_arg(), root.to_str().unwrap()],
            None,
        );
    }
    (sibling, target)
}

fn supervise_snapshot_formats() {
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let tree = repo.get_oid(&["rev-parse", "HEAD^{tree}"], None);
        repo.run_git_successfully(&["gc", "--prune=now"], None);
        let before = repo.capture_fixture_state();
        fixture::supervise_test(
            "git::repository::tests::snapshots_work_without_git_executable",
            &[
                ("LETOPIS_CHILD_FREE_ROOT", repo.get_root().to_str().unwrap()),
                ("LETOPIS_CHILD_FREE_TREE", &tree),
                ("PATH", "/nonexistent-letopis-git"),
                ("GIT_DIR", "CANARY_DIR"),
                ("GIT_WORK_TREE", "CANARY_TREE"),
                ("GIT_INDEX_FILE", "CANARY_INDEX"),
                ("GIT_OBJECT_DIRECTORY", "CANARY_OBJECTS"),
                ("GIT_CONFIG_COUNT", "1"),
                ("GIT_CONFIG_KEY_0", "core.bare"),
                ("GIT_CONFIG_VALUE_0", "true"),
                ("GIT_COMMON_DIR", "CANARY_COMMON"),
                ("GIT_NAMESPACE", "CANARY_NAMESPACE"),
                ("GIT_CONFIG_PARAMETERS", "CANARY_PARAMETERS"),
                ("GIT_ALTERNATE_OBJECT_DIRECTORIES", "CANARY_ALTERNATES"),
            ],
        );
        assert_eq!(repo.capture_fixture_state(), before);
    }
}

fn prepare_discovery_files(repo: &TempRepo) {
    fs::create_dir_all(repo.get_root().join("nested")).unwrap();
    for name in [
        "tracked.md",
        "deleted.md",
        "ignored.md",
        "nested/nested.md",
        "info.md",
        "global.md",
    ] {
        fs::write(repo.get_root().join(name), b"bytes").unwrap();
    }
    fs::write(
        repo.get_root().join(".gitignore"),
        b"ignored.md\ntracked.md\n",
    )
    .unwrap();
    fs::write(repo.get_root().join("nested/.gitignore"), b"nested.md\n").unwrap();
    fs::write(repo.get_root().join(".git/info/exclude"), b"info.md\n").unwrap();
    let global = repo.get_root().join("global-ignore");
    fs::write(&global, b"global.md\n").unwrap();
    repo.run_git_successfully(
        &["config", "core.excludesFile", global.to_str().unwrap()],
        None,
    );
    repo.run_git_successfully(&["add", "-f", "tracked.md", "deleted.md"], None);
    fs::remove_file(repo.get_root().join("deleted.md")).unwrap();
}

fn get_path_set(raw: &[u8]) -> std::collections::BTreeSet<Vec<u8>> {
    raw.split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}

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
        (
            b"**/generated?.[mM][dD]\n".as_slice(),
            &[b"a/generated11.md".as_slice()][..],
            &[b"generated1.md".as_slice(), b"a/b/generated1.MD"][..],
        ),
        (
            b"docs/**/draft.md\n",
            &[b"other/draft.md".as_slice()][..],
            &[b"docs/draft.md".as_slice(), b"docs/a/b/draft.md"][..],
        ),
        (
            b"docs/*.md\n",
            &[b"docs/deep/a.md".as_slice()][..],
            &[b"docs/a.md".as_slice()][..],
        ),
        (
            b"\n# comment\r\n\\#note.md\r\n\\!note.md\n\\*.md\nname\\ \nignored.md   \n$cash.md\n",
            &[b"comment".as_slice(), b"name", b"cash.md"][..],
            &[
                b"#note.md".as_slice(),
                b"!note.md",
                b"*.md",
                b"name ",
                b"ignored.md",
                b"$cash.md",
            ][..],
        ),
        (
            b"[invalid\ntrailing\\\n",
            &[b"ordinary.md".as_slice()][..],
            &[][..],
        ),
        (
            "**/заметка.md\n".as_bytes(),
            &[b"keep.md".as_slice()][..],
            &["a/заметка.md".as_bytes()][..],
        ),
        (
            b"bad\xff.md\n",
            &[b"good.md".as_slice()][..],
            &[b"bad\xff.md".as_slice()][..],
        ),
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

mod tree_reads {
    use super::super::tree_get_by_oid;
    use crate::{
        domain::TreeOid,
        git::{GitRepository, validation::tree_validation_get_count},
        process_test_support::{FORMATS, TempRepo},
        service::{GitSnapshots, SnapshotError, SnapshotErrorKind},
    };
    use std::fs;

    #[test]
    fn tree_reads_reuse_structure_only_within_the_owned_reader() {
        for format in FORMATS {
            // Given: an independently written Git tree and a fresh product reader.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let expected = fixture.run_git_successfully(&["cat-file", "tree", &oid], None);
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let before = tree_validation_get_count();
            // When: the same immutable body is demanded twice by this reader.
            assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            // Then: structural work is shared, but a separate owner starts unvalidated.
            assert_eq!(tree_validation_get_count() - before, 1);
            let separate = GitRepository::open(fixture.get_root(), &[]).unwrap();
            assert_eq!(get_tree(&separate, &oid).unwrap(), expected);
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn memoized_tree_reads_reject_replacements_and_recover_after_restoration() {
        for format in FORMATS {
            // Given: a previously validated tree and independently written replacement objects.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let empty = fixture.get_oid(&["hash-object", "-w", "-t", "tree", "--stdin"], Some(b""));
            let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"CANARY"));
            let original = fs::read(fixture.build_object_path(&oid)).unwrap();
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let expected = get_tree(&git, &oid).unwrap();
            for (replacement, kind) in [
                (Some(&empty), SnapshotErrorKind::InternalError),
                (Some(&blob), SnapshotErrorKind::InvalidRequest),
                (None, SnapshotErrorKind::NotFound),
            ] {
                // When: the object's current backing bytes change or disappear after memoization.
                if let Some(replacement) = replacement {
                    fixture.replace_loose_object(&oid, replacement);
                } else {
                    fs::remove_file(fixture.build_object_path(&oid)).unwrap();
                }
                let error = get_tree(&git, &oid).unwrap_err();
                // Then: current failures remain visible; restoring valid bytes permits recovery.
                assert_eq!(error.get_kind(), kind);
                assert!(!format!("{error:?}").contains("CANARY"));
                if replacement.is_some() {
                    fs::remove_file(fixture.build_object_path(&oid)).unwrap();
                }
                fs::write(fixture.build_object_path(&oid), &original).unwrap();
                assert_eq!(get_tree(&git, &oid).unwrap(), expected);
            }
        }
    }

    #[test]
    fn invalid_tree_bodies_are_validated_on_every_demand() {
        for format in FORMATS {
            // Given: a correctly hashed tree with a forbidden zero-prefixed mode.
            let fixture = TempRepo::new(format);
            let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"source"));
            let mut bytes = b"0100644 file\0".to_vec();
            bytes.extend_from_slice(
                gix::hash::ObjectId::from_hex(blob.as_bytes())
                    .unwrap()
                    .as_bytes(),
            );
            let oid = fixture.get_oid(
                &["hash-object", "-w", "--literally", "-t", "tree", "--stdin"],
                Some(&bytes),
            );
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            let before = tree_validation_get_count();
            // When: the malformed body is demanded twice from the same owner.
            let first = get_tree(&git, &oid).unwrap_err();
            let second = get_tree(&git, &oid).unwrap_err();
            // Then: failures never authorize a later structural-validation bypass.
            assert_eq!(first.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(second.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn full_audits_validate_tree_structure_despite_addressed_read_memoization() {
        for format in FORMATS {
            // Given: the root body has already passed an addressed read on this owner.
            let fixture = TempRepo::new(format);
            let oid = fixture.get_oid(&["rev-parse", "HEAD^{tree}"], None);
            let tree = TreeOid::parse(&oid).unwrap();
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            get_tree(&git, &oid).unwrap();
            let before = tree_validation_get_count();
            // When: two independent full closure audits retain that root.
            git.tree_pin_by_oid(&tree).unwrap();
            git.tree_pin_by_oid(&tree).unwrap();
            // Then: each full audit rechecks its tree body, independent of the memo.
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    #[test]
    fn tree_validation_memo_overflow_revalidates_without_rejecting_valid_objects() {
        for format in FORMATS {
            // Given: 4097 independently written distinct valid trees exceed the metadata budget.
            let fixture = TempRepo::new(format);
            let trees = create_trees(&fixture, 4097);
            let git = GitRepository::open(fixture.get_root(), &[]).unwrap();
            for oid in &trees {
                get_tree(&git, oid).unwrap();
            }
            let before = tree_validation_get_count();
            // When: the last cached and first uncached bodies are each demanded twice.
            for _ in 0..2 {
                get_tree(&git, &trees[4095]).unwrap();
                get_tree(&git, &trees[4096]).unwrap();
            }
            // Then: overflow remains accepted but is structurally checked every time.
            assert_eq!(tree_validation_get_count() - before, 2);
        }
    }

    fn get_tree(git: &GitRepository, oid: &str) -> Result<Vec<u8>, SnapshotError> {
        tree_get_by_oid(
            git,
            oid,
            SnapshotErrorKind::NotFound,
            SnapshotErrorKind::InvalidRequest,
        )
    }

    fn create_trees(fixture: &TempRepo, count: usize) -> Vec<String> {
        let blob = fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(b"source"));
        let blob = gix::hash::ObjectId::from_hex(blob.as_bytes()).unwrap();
        let mut paths = String::new();
        for index in 0..count {
            let path = fixture.get_root().join(format!(".git/memo-input-{index}"));
            let mut bytes = format!("100644 file-{index}\0").into_bytes();
            bytes.extend_from_slice(blob.as_bytes());
            fs::write(&path, bytes).unwrap();
            paths.push_str(path.to_str().unwrap());
            paths.push('\n');
        }
        let output = fixture.run_git_successfully(
            &["hash-object", "-w", "-t", "tree", "--stdin-paths"],
            Some(paths.as_bytes()),
        );
        String::from_utf8(output)
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }
}
