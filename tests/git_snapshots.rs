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
                oid: git.blob_write(bytes).unwrap(),
            })
            .collect();
        let tree = git.tree_assemble(&entries).unwrap();
        let expected = git
            .tree_create(
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
            assert_eq!(
                git.blob_get_by_tree_oid_and_path(&tree, path).unwrap(),
                bytes
            );
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
        let blob = git.blob_write(b"expected").unwrap();
        let tree = git.tree_create(&[]).unwrap();
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
                git.tree_assemble(&entries).unwrap_err().get_kind(),
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
                git.tree_assemble(&entries).unwrap_err().get_kind(),
                SnapshotErrorKind::InvalidRequest
            );
        }
        let replacement = git.blob_write(b"corrupt replacement").unwrap();
        repo.replace_loose_object(blob.as_str(), replacement.as_str());
        let entries = [SnapshotEntry {
            path: parse_path("a"),
            oid: blob,
        }];
        assert_eq!(
            git.tree_assemble(&entries).unwrap_err().get_kind(),
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
        let oid = git.blob_write(b"captured").unwrap();
        fs::create_dir_all(repo.get_root().join(".git/objects/info")).unwrap();
        fs::write(repo.get_root().join(".git/objects/info/alternates"), b"").unwrap();
        // When / Then
        assert_eq!(
            git.blob_write(b"later").unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
        let entries = [SnapshotEntry {
            path: parse_path("x"),
            oid,
        }];
        assert_eq!(
            git.tree_assemble(&entries).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}

#[test]
fn public_tree_creation_preflights_private_roots_before_writing_objects() {
    for format in FORMATS {
        // Given: a public snapshot handle and independently created blob.
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let blob = BlobOid::parse(
            &repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"supplied blob")),
        )
        .unwrap();
        for name in [".letopis", ".letopis/private.md"] {
            let safe = parse_path("ordinary.md");
            let private = parse_path(name);
            let before = repo.run_git_successfully(&["count-objects", "-v"], None);
            // When: a later entry is excluded after a valid novel blob request.
            let files = [
                SnapshotFile {
                    path: &safe,
                    bytes: b"never write this blob",
                },
                SnapshotFile {
                    path: &private,
                    bytes: b"private",
                },
            ];
            let error = git.tree_create(&files).unwrap_err();
            // Then: the entire request fails before even the first blob write.
            assert_eq!(error.get_kind(), SnapshotErrorKind::InvalidRequest);
            assert_eq!(
                repo.run_git_successfully(&["count-objects", "-v"], None),
                before
            );
            // When: the same private root is supplied through the blob assembly API.
            let error = git
                .tree_assemble(&[SnapshotEntry {
                    path: private,
                    oid: blob.clone(),
                }])
                .unwrap_err();
            // Then: no tree is written through the second public creation path.
            assert_eq!(error.get_kind(), SnapshotErrorKind::InvalidRequest);
            assert_eq!(
                repo.run_git_successfully(&["count-objects", "-v"], None),
                before
            );
        }
    }
}

#[test]
fn public_blob_assembly_rejects_private_roots_before_blob_validation() {
    for format in FORMATS {
        // Given: an excluded root paired with an independently known missing object.
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let width = repo.get_oid(&["rev-parse", "HEAD"], None).len();
        let entry = SnapshotEntry {
            path: parse_path(".letopis/private.md"),
            oid: BlobOid::parse(&"1".repeat(width)).unwrap(),
        };
        let before = repo.run_git_successfully(&["count-objects", "-v"], None);
        // When: the public blob-assembly operation receives the invalid path.
        let error = git.tree_assemble(&[entry]).unwrap_err();
        // Then: path policy, rather than object absence, determines the failure before writes.
        assert_eq!(error.get_kind(), SnapshotErrorKind::InvalidRequest);
        assert_eq!(error.get_message(), "invalid snapshot paths");
        assert_eq!(
            repo.run_git_successfully(&["count-objects", "-v"], None),
            before
        );
    }
}

#[test]
fn nested_private_name_remains_valid_and_historical_private_trees_remain_readable() {
    for format in FORMATS {
        // Given: a historical tree containing a private-root file, created by independent Git.
        let repo = TempRepo::new(format);
        let blob = repo.get_oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"historical private bytes"),
        );
        let historical =
            create_external_tree(&repo, REGULAR_FILE_MODE, BLOB_KIND, &blob, ".letopis");
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let nested = parse_path("a/.letopis/note.md");
        // When: the public API creates a nested name and reads an existing private-root tree.
        let tree = git
            .tree_create(&[SnapshotFile {
                path: &nested,
                bytes: b"nested",
            }])
            .unwrap();
        let bytes = git
            .blob_get_by_tree_oid_and_path(&historical, &parse_path(".letopis"))
            .unwrap();
        // Then: only tree creation's first path component is restricted.
        assert_eq!(bytes, b"historical private bytes");
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(&tree, &nested).unwrap(),
            b"nested"
        );
        git.tree_pin_by_oid(&historical).unwrap();
    }
}

fn parse_path(text: &str) -> RepoPath {
    RepoPath::parse(text).unwrap()
}

fn parse_tree_oid(text: &str) -> TreeOid {
    TreeOid::parse(text).unwrap()
}

fn create_external_tree(
    repo: &TempRepo,
    mode: &str,
    kind: &str,
    child: &str,
    name: &str,
) -> TreeOid {
    parse_tree_oid(&repo.get_oid(
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
        let first = git.tree_create(&files).unwrap();
        let captured: Vec<_> = paths
            .iter()
            .map(|p| git.blob_get_by_tree_oid_and_path(&first, p).unwrap())
            .collect();
        files.reverse();
        let reordered = git.tree_create(&files).unwrap();
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
        let after_live_change = git
            .blob_get_by_tree_oid_and_path(&first, &paths[0])
            .unwrap();
        let second = git
            .tree_create(&[SnapshotFile {
                path: &paths[0],
                bytes: b"new",
            }])
            .unwrap();
        let after_new_tree = git
            .blob_get_by_tree_oid_and_path(&first, &paths[0])
            .unwrap();
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
        let tree = git.tree_create(&files).unwrap();
        let captured: Vec<_> = paths
            .iter()
            .map(|p| git.blob_get_by_tree_oid_and_path(&tree, p).unwrap())
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
        let empty = git.tree_create(&[]).unwrap();
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
            let error = git.tree_create(&files).unwrap_err();
            let after = repo.run_git_successfully(&["count-objects", "-v"], None);
            // Then
            assert_eq!(error.get_kind(), SnapshotErrorKind::InvalidRequest);
            assert!(!format!("{error:?} {error}").contains("CANARY"));
            assert_eq!(before, after, "invalid input must not write objects");
        }
    }
}

fn build_tree_error_cases(repo: &TempRepo, empty: &TreeOid) -> Vec<(TreeOid, SnapshotErrorKind)> {
    let commit = repo.get_oid(&["rev-parse", "HEAD"], None);
    let wrong_length = match commit.len() {
        40 => 64,
        64 => 40,
        _ => panic!("fixture Git uses an unsupported OID length"),
    };
    let tag_body = format!(
        "object {commit}\ntype commit\ntag fixture\ntagger Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
    );
    let tag = repo.get_oid(
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
    let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(b"valid expected"));
    let other = repo.get_oid(
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
        let empty = git.tree_create(&[]).unwrap();
        for (tree, expected) in build_tree_error_cases(&repo, &empty) {
            // When
            let error = git.blob_get_by_tree_oid_and_path(&tree, &leaf).unwrap_err();
            // Then
            assert_eq!(error.get_kind(), expected);
        }
        // Given
        let tree = create_corrupted_blob_tree(&repo);
        // When
        let error = git.blob_get_by_tree_oid_and_path(&tree, &leaf).unwrap_err();
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
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"original",
            }])
            .unwrap();
        let replacement = git
            .tree_create(&[SnapshotFile {
                path: &p,
                bytes: b"replacement",
            }])
            .unwrap();
        repo.run_git_successfully(&["replace", original.as_str(), replacement.as_str()], None);
        // When
        let bytes = git.blob_get_by_tree_oid_and_path(&original, &p).unwrap();
        // Then
        assert_eq!(bytes, b"original");
    }
}
