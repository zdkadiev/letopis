#[path = "support/git.rs"]
mod git;
use git::{FORMATS, ObjectFormat, TempRepo};
use letopis::{
    domain::{RepoPath, TreeOid},
    open_git_snapshots,
    service::ports::git::{GitSnapshotPort, SnapshotErrorKind::*, SnapshotFile},
};
use std::fs;
fn path(s: &str) -> RepoPath {
    RepoPath::parse(s).unwrap()
}
fn oid(s: &str) -> TreeOid {
    TreeOid::parse(s).unwrap()
}
fn external_tree(repo: &TempRepo, mode: &str, kind: &str, child: &str, name: &str) -> TreeOid {
    oid(&repo.oid(
        &["mktree", "-z", "--missing"],
        Some(format!("{mode} {kind} {child}\t{name}\0").as_bytes()),
    ))
}
#[test]
fn exact_bytes_without_index_or_commit() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let filter = repo.script("filter", "#!/bin/sh\ntouch FILTER_RAN\ncat\n");
        repo.ok(
            &["config", "filter.canary.clean", filter.to_str().unwrap()],
            None,
        );
        repo.ok(&["config", "filter.canary.required", "true"], None);
        repo.ok(&["config", "core.autocrlf", "true"], None);
        fs::write(
            repo.root().join(".gitattributes"),
            b"* filter=canary text eol=crlf\n",
        )
        .unwrap();
        let before = repo.head_index_config_and_live_snapshot();
        let git = open_git_snapshots(repo.subdir()).unwrap();
        let paths = [
            path("live.md"),
            path("nested/empty"),
            path("nested/binary"),
            path("文档/й.md"),
            path("lf"),
            path("crlf"),
        ];
        let bytes: [&[u8]; 6] = [
            b"no final newline",
            b"",
            b"\0\xff\x01\r\n",
            "Привет 🌳\n".as_bytes(),
            b"a\nb\n",
            b"a\r\nb\r\n",
        ];
        let mut files: Vec<_> = paths
            .iter()
            .zip(bytes)
            .map(|(path, bytes)| SnapshotFile { path, bytes })
            .collect();
        let s1 = git.create_tree(&files).unwrap();
        for (p, b) in paths.iter().zip(bytes) {
            assert_eq!(git.read_blob(&s1, p).unwrap(), b);
        }
        files.reverse();
        assert_eq!(git.create_tree(&files).unwrap(), s1);
        assert_eq!(repo.head_index_config_and_live_snapshot(), before);
        assert!(!repo.root().join("FILTER_RAN").exists());
        repo.change_and_delete_live_files("live.md");
        assert_eq!(git.read_blob(&s1, &paths[0]).unwrap(), bytes[0]);
        let s2 = git
            .create_tree(&[SnapshotFile {
                path: &paths[0],
                bytes: b"new",
            }])
            .unwrap();
        assert_ne!(s1, s2);
        assert_eq!(git.read_blob(&s1, &paths[0]).unwrap(), bytes[0]);
        let listing = repo.ok(&["ls-tree", "-rz", "--name-only", s1.as_str()], None);
        assert_eq!(
            listing.split(|b| *b == 0).filter(|s| !s.is_empty()).count(),
            6
        );
    }
}
#[test]
fn paths_are_literal_and_never_shell_input() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.subdir()).unwrap();
        let paths = [
            "dir/a:b",
            "-name",
            "literal*.md",
            "$(touch SENTINEL).md",
            "space dir/a b",
        ]
        .map(path);
        let files: Vec<_> = paths
            .iter()
            .map(|p| SnapshotFile {
                path: p,
                bytes: b"literal",
            })
            .collect();
        let tree = git.create_tree(&files).unwrap();
        for p in &paths {
            assert_eq!(git.read_blob(&tree, p).unwrap(), b"literal");
        }
        assert!(!repo.root().join("SENTINEL").exists());
        assert!(!repo.subdir().join("SENTINEL").exists());
    }
}
#[test]
fn empty_tree_and_invalid_paths() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let empty = git.create_tree(&[]).unwrap();
        assert!(repo.ok(&["ls-tree", empty.as_str()], None).is_empty());
        for names in [
            vec!["a", "a"],
            vec!["a", "a/b"],
            vec![".git/x"],
            vec!["dir/.git/x"],
            vec![".letopis/x"],
        ] {
            let paths: Vec<_> = names.into_iter().map(path).collect();
            let files: Vec<_> = paths
                .iter()
                .map(|p| SnapshotFile {
                    path: p,
                    bytes: b"UNIQUE_INVALID_CANARY",
                })
                .collect();
            let before = repo.ok(&["count-objects", "-v"], None);
            let err = git.create_tree(&files).unwrap_err();
            assert_eq!(err.kind(), InvalidRequest);
            assert!(!format!("{err:?} {err}").contains("CANARY"));
            assert_eq!(
                before,
                repo.ok(&["count-objects", "-v"], None),
                "invalid input must not write objects"
            );
        }
    }
}
#[test]
fn tree_types_and_missing_objects_are_distinct() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let p = path("leaf");
        let wrong = match format {
            ObjectFormat::Sha1 => "0".repeat(64),
            ObjectFormat::Sha256 => "0".repeat(40),
        };
        assert_eq!(
            git.read_blob(&oid(&wrong), &p).unwrap_err().kind(),
            InvalidRequest
        );
        let commit = repo.oid(&["rev-parse", "HEAD"], None);
        assert_eq!(
            git.read_blob(&oid(&commit), &p).unwrap_err().kind(),
            InvalidRequest
        );
        let tag_body = format!(
            "object {commit}\ntype commit\ntag fixture\ntagger Fixture <fixture@example.invalid> 1 +0000\n\nfixture\n"
        );
        let tag = repo.oid(
            &["hash-object", "-w", "-t", "tag", "--stdin"],
            Some(tag_body.as_bytes()),
        );
        assert_eq!(
            git.read_blob(&oid(&tag), &p).unwrap_err().kind(),
            InvalidRequest
        );
        let missing = "1".repeat(commit.len());
        assert_eq!(
            git.read_blob(&oid(&missing), &p).unwrap_err().kind(),
            NotFound
        );
        let empty = git.create_tree(&[]).unwrap();
        assert_eq!(git.read_blob(&empty, &p).unwrap_err().kind(), NotFound);
        let dangling = external_tree(&repo, "100644", "blob", &missing, "leaf");
        assert_eq!(
            git.read_blob(&dangling, &p).unwrap_err().kind(),
            InternalError
        );
        let tree_leaf = external_tree(&repo, "040000", "tree", empty.as_str(), "leaf");
        assert_eq!(
            git.read_blob(&tree_leaf, &p).unwrap_err().kind(),
            InvalidRequest
        );
        let gitlink = external_tree(&repo, "160000", "commit", &commit, "leaf");
        assert_eq!(
            git.read_blob(&gitlink, &p).unwrap_err().kind(),
            InvalidRequest
        );
        let blob = repo.oid(&["hash-object", "-w", "--stdin"], Some(b"valid expected"));
        let other = repo.oid(
            &["hash-object", "-w", "--stdin"],
            Some(b"different CORRUPT_CANARY bytes"),
        );
        let tree = external_tree(&repo, "100644", "blob", &blob, "leaf");
        fs::remove_file(
            repo.root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..])),
        )
        .unwrap();
        fs::copy(
            repo.root()
                .join(format!(".git/objects/{}/{}", &other[..2], &other[2..])),
            repo.root()
                .join(format!(".git/objects/{}/{}", &blob[..2], &blob[2..])),
        )
        .unwrap();
        let error = git.read_blob(&tree, &p).unwrap_err();
        assert_eq!(error.kind(), InternalError);
        assert!(!format!("{error} {error:?}").contains("CORRUPT_CANARY"));
    }
}
#[test]
fn replacement_refs_do_not_change_bytes() {
    for format in FORMATS {
        let repo = TempRepo::new(format);
        let git = open_git_snapshots(repo.root()).unwrap();
        let p = path("x");
        let s1 = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"original",
            }])
            .unwrap();
        let s2 = git
            .create_tree(&[SnapshotFile {
                path: &p,
                bytes: b"replacement",
            }])
            .unwrap();
        repo.ok(&["replace", s1.as_str(), s2.as_str()], None);
        assert_eq!(git.read_blob(&s1, &p).unwrap(), b"original");
    }
}
