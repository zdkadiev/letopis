#[path = "support/git.rs"]
mod git;
use git::{
    BLOB_KIND, COMMIT_KIND, FORMATS, GITLINK_MODE, REGULAR_FILE_MODE, TAG_KIND, TREE_KIND,
    TREE_MODE, TempRepo,
};
use letopis::{
    domain::{BlobOid, RepoPath, TreeOid},
    open_git_snapshots,
    service::{GitSnapshots, SnapshotEntry, SnapshotErrorKind, SnapshotFile},
};
use std::fs;

#[test]
fn incremental_blobs_reproduce_tree_without_touching_user_state() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        configure_filter_trap(&repo);
        let before = repo.capture_fixture_state();
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let paths = [parse_path("a.md"), parse_path("nested/文档.md")];
        let bytes = [b"a\r\n".as_slice(), "Привет 🌳".as_bytes()];
        // When
        let entries: Vec<_> = paths
            .iter()
            .zip(bytes)
            .map(|(path, bytes)| SnapshotEntry {
                path: path.clone(),
                oid: git.write_blob(bytes).unwrap(),
            })
            .collect();
        let tree = git.create_tree_from_blobs(&entries).unwrap();
        let expected = git
            .create_tree(
                &paths
                    .iter()
                    .zip(bytes)
                    .map(|(path, bytes)| SnapshotFile { path, bytes })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        // Then
        assert_eq!(tree, expected);
        assert!(format!("{:?}", entries[0]).starts_with("SnapshotEntry {"));
        for (path, bytes) in paths.iter().zip(bytes) {
            assert_eq!(git.read_blob(&tree, path).unwrap(), bytes);
        }
        assert_eq!(repo.capture_fixture_state(), before);
        assert!(!repo.get_root().join("FILTER_RAN").exists());
    }
}

#[test]
fn blob_entries_reject_duplicate_prefix_reserved_wrong_type_and_foreign_format() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let blob = git.write_blob(b"expected").unwrap();
        let tree = git.create_tree(&[]).unwrap();
        let missing = BlobOid::parse(&"1".repeat(blob.as_str().len())).unwrap();
        let foreign =
            BlobOid::parse(&"0".repeat(if blob.as_str().len() == 40 { 64 } else { 40 })).unwrap();
        for names in [
            vec!["a", "a"],
            vec!["a", "a/b"],
            vec![".git/x"],
            vec!["dir/.GIT/x"],
            vec![".letopis/x"],
        ] {
            let entries: Vec<_> = names
                .into_iter()
                .map(|p| SnapshotEntry {
                    path: parse_path(p),
                    oid: blob.clone(),
                })
                .collect();
            let before = repo.run_git_successfully(&["count-objects", "-v"], None);
            // When / Then
            assert_eq!(
                git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
                SnapshotErrorKind::InvalidRequest
            );
            assert_eq!(
                repo.run_git_successfully(&["count-objects", "-v"], None),
                before
            );
        }
        for oid in [missing, foreign, BlobOid::parse(tree.as_str()).unwrap()] {
            let entries = [SnapshotEntry {
                path: parse_path("a"),
                oid,
            }];
            assert_eq!(
                git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
                SnapshotErrorKind::InvalidRequest
            );
        }
        let replacement = git.write_blob(b"corrupt replacement").unwrap();
        repo.replace_loose_object(blob.as_str(), replacement.as_str());
        let entries = [SnapshotEntry {
            path: parse_path("a"),
            oid: blob,
        }];
        assert_eq!(
            git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
        assert!(
            repo.run_git_successfully(&["for-each-ref", "refs/letopis"], None)
                .is_empty()
        );
    }
}

#[test]
fn incremental_operations_recheck_self_contained_object_policy() {
    // Given: alternates appear after opening the repository.
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let oid = git.write_blob(b"captured").unwrap();
        fs::create_dir_all(repo.get_root().join(".git/objects/info")).unwrap();
        fs::write(repo.get_root().join(".git/objects/info/alternates"), b"").unwrap();
        // When / Then
        assert_eq!(
            git.write_blob(b"later").unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
        let entries = [SnapshotEntry {
            path: parse_path("x"),
            oid,
        }];
        assert_eq!(
            git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}

fn parse_path(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}
fn parse_tree_oid(s: &str) -> TreeOid {
    TreeOid::parse(s).unwrap()
}
fn create_external_tree(
    repo: &TempRepo,
    mode: &str,
    kind: &str,
    child: &str,
    name: &str,
) -> TreeOid {
    parse_tree_oid(&repo.read_oid(
        &["mktree", "-z", "--missing"],
        Some(format!("{mode} {kind} {child}\t{name}\0").as_bytes()),
    ))
}
fn configure_filter_trap(repo: &TempRepo) {
    let filter = repo.write_script("filter", "#!/bin/sh\ntouch FILTER_RAN\ncat\n");
    repo.run_git_successfully(
        &["config", "filter.canary.clean", filter.to_str().unwrap()],
        None,
    );
    repo.run_git_successfully(&["config", "filter.canary.required", "true"], None);
    repo.run_git_successfully(&["config", "core.autocrlf", "true"], None);
    fs::write(
        repo.get_root().join(".gitattributes"),
        b"* filter=canary text eol=crlf\n",
    )
    .unwrap();
}
fn build_captured_inputs() -> ([RepoPath; 6], [&'static [u8]; 6]) {
    (
        [
            "live.md",
            "nested/empty",
            "nested/binary",
            "文档/й.md",
            "lf",
            "crlf",
        ]
        .map(parse_path),
        [
            b"no final newline",
            b"",
            b"\0\xff\x01\r\n",
            "Привет 🌳\n".as_bytes(),
            b"a\nb\n",
            b"a\r\nb\r\n",
        ],
    )
}

#[test]
fn exact_bytes_without_index_or_commit() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        configure_filter_trap(&repo);
        let before = repo.capture_fixture_state();
        let git = open_git_snapshots(repo.get_subdir()).unwrap();
        let (paths, bytes) = build_captured_inputs();
        let mut files: Vec<_> = paths
            .iter()
            .zip(bytes)
            .map(|(path, bytes)| SnapshotFile { path, bytes })
            .collect();
        // When
        let first = git.create_tree(&files).unwrap();
        let captured: Vec<_> = paths
            .iter()
            .map(|p| git.read_blob(&first, p).unwrap())
            .collect();
        files.reverse();
        let reordered = git.create_tree(&files).unwrap();
        let after = repo.capture_fixture_state();
        // Then
        for (actual, expected) in captured.iter().zip(bytes) {
            assert_eq!(actual, expected);
        }
        assert_eq!(reordered, first);
        assert_eq!(after, before);
        assert!(!repo.get_root().join("FILTER_RAN").exists());

        // Given: mutate and remove live files after capturing their bytes.
        repo.change_and_delete_live_files("live.md");
        // When
        let after_live_change = git.read_blob(&first, &paths[0]).unwrap();
        let second = git
            .create_tree(&[SnapshotFile {
                path: &paths[0],
                bytes: b"new",
            }])
            .unwrap();
        let after_new_tree = git.read_blob(&first, &paths[0]).unwrap();
        let listing =
            repo.run_git_successfully(&["ls-tree", "-rz", "--name-only", first.as_str()], None);
        // Then
        assert_eq!(after_live_change, bytes[0]);
        assert_ne!(first, second);
        assert_eq!(after_new_tree, bytes[0]);
        assert_eq!(
            listing.split(|b| *b == 0).filter(|s| !s.is_empty()).count(),
            6
        );
    }
}

#[test]
fn paths_are_literal_and_never_shell_input() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_subdir()).unwrap();
        let paths = [
            "dir/a:b",
            "-name",
            "literal*.md",
            "$(touch SENTINEL).md",
            "space dir/a b",
        ]
        .map(parse_path);
        let files: Vec<_> = paths
            .iter()
            .map(|p| SnapshotFile {
                path: p,
                bytes: b"literal",
            })
            .collect();
        // When
        let tree = git.create_tree(&files).unwrap();
        let captured: Vec<_> = paths
            .iter()
            .map(|p| git.read_blob(&tree, p).unwrap())
            .collect();
        // Then
        for bytes in captured {
            assert_eq!(bytes, b"literal");
        }
        assert!(!repo.get_root().join("SENTINEL").exists());
        assert!(!repo.get_subdir().join("SENTINEL").exists());
    }
}

#[test]
fn empty_tree_and_invalid_paths() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        // When
        let empty = git.create_tree(&[]).unwrap();
        let listing = repo.run_git_successfully(&["ls-tree", empty.as_str()], None);
        // Then
        assert!(listing.is_empty());

        // Given
        for names in [
            vec!["a", "a"],
            vec!["a", "a/b"],
            vec![".git/x"],
            vec!["dir/.git/x"],
            vec![".letopis/x"],
        ] {
            let paths: Vec<_> = names.into_iter().map(parse_path).collect();
            let files: Vec<_> = paths
                .iter()
                .map(|p| SnapshotFile {
                    path: p,
                    bytes: b"UNIQUE_INVALID_CANARY",
                })
                .collect();
            let before = repo.run_git_successfully(&["count-objects", "-v"], None);
            // When
            let error = git.create_tree(&files).unwrap_err();
            let after = repo.run_git_successfully(&["count-objects", "-v"], None);
            // Then
            assert_eq!(error.get_kind(), SnapshotErrorKind::InvalidRequest);
            assert!(!format!("{error:?} {error}").contains("CANARY"));
            assert_eq!(before, after, "invalid input must not write objects");
        }
    }
}

fn build_tree_error_cases(repo: &TempRepo, empty: &TreeOid) -> Vec<(TreeOid, SnapshotErrorKind)> {
    let commit = repo.read_oid(&["rev-parse", "HEAD"], None);
    let wrong_length = match commit.len() {
        40 => 64,
        64 => 40,
        _ => panic!("fixture Git uses an unsupported OID length"),
    };
    let tag_body = format!(
        "object {commit}\ntype commit\ntag fixture\ntagger Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
    );
    let tag = repo.read_oid(
        &["hash-object", "-w", "-t", TAG_KIND, "--stdin"],
        Some(tag_body.as_bytes()),
    );
    let missing = "1".repeat(commit.len());
    vec![
        (
            parse_tree_oid(&"0".repeat(wrong_length)),
            SnapshotErrorKind::InvalidRequest,
        ),
        (parse_tree_oid(&commit), SnapshotErrorKind::InvalidRequest),
        (parse_tree_oid(&tag), SnapshotErrorKind::InvalidRequest),
        (parse_tree_oid(&missing), SnapshotErrorKind::NotFound),
        (empty.clone(), SnapshotErrorKind::NotFound),
        (
            create_external_tree(repo, REGULAR_FILE_MODE, BLOB_KIND, &missing, "leaf"),
            SnapshotErrorKind::InternalError,
        ),
        (
            create_external_tree(repo, TREE_MODE, TREE_KIND, empty.as_str(), "leaf"),
            SnapshotErrorKind::InvalidRequest,
        ),
        (
            create_external_tree(repo, GITLINK_MODE, COMMIT_KIND, &commit, "leaf"),
            SnapshotErrorKind::InvalidRequest,
        ),
    ]
}
fn create_corrupted_blob_tree(repo: &TempRepo) -> TreeOid {
    let blob = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"valid expected"));
    let other = repo.read_oid(
        &["hash-object", "-w", "--stdin"],
        Some(b"different CORRUPT_CANARY bytes"),
    );
    let tree = create_external_tree(repo, REGULAR_FILE_MODE, BLOB_KIND, &blob, "leaf");
    repo.replace_loose_object(&blob, &other);
    tree
}

#[test]
fn tree_types_and_missing_objects_are_distinct() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let leaf = parse_path("leaf");
        let empty = git.create_tree(&[]).unwrap();
        for (tree, expected) in build_tree_error_cases(&repo, &empty) {
            // When
            let error = git.read_blob(&tree, &leaf).unwrap_err();
            // Then
            assert_eq!(error.get_kind(), expected);
        }
        // Given
        let tree = create_corrupted_blob_tree(&repo);
        // When
        let error = git.read_blob(&tree, &leaf).unwrap_err();
        // Then
        assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
        assert!(!format!("{error} {error:?}").contains("CORRUPT_CANARY"));
    }
}

#[test]
fn replacement_refs_do_not_change_bytes() {
    // Given
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let p = parse_path("x");
        let original = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"original",
            }])
            .unwrap();
        let replacement = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"replacement",
            }])
            .unwrap();
        repo.run_git_successfully(&["replace", original.as_str(), replacement.as_str()], None);
        // When
        let bytes = git.read_blob(&original, &p).unwrap();
        // Then
        assert_eq!(bytes, b"original");
    }
}
