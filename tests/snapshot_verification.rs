#[path = "support/git.rs"]
mod git;

use git::{FORMATS, ObjectFormat, TempRepo};
use letopis::{
    domain::TreeOid,
    open_git_snapshots,
    service::{SnapshotError, SnapshotErrorKind, SnapshotState, SnapshotVerifier},
};
use std::fs;

const ABSENT_SHA1: &str = "1111111111111111111111111111111111111111";
const ABSENT_SHA256: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn verify_snapshot_accepts_exact_tree_without_writes() {
    for format in FORMATS {
        // Given: the index deliberately differs from both HEAD and live bytes.
        let repo = TempRepo::new(format);
        let tree = parse_tree(&repo.read_oid(&["write-tree"], None));
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let before = repo.capture_fixture_state();
        let before_refs = repo.run_git_successfully(&["for-each-ref"], None);
        let before_objects = repo.run_git_successfully(&["count-objects", "-v"], None);

        // When
        let state = git.verify_snapshot(&tree).unwrap();

        // Then
        assert_eq!(state, SnapshotState::Available);
        assert_eq!(repo.capture_fixture_state(), before);
        assert_eq!(
            repo.run_git_successfully(&["for-each-ref"], None),
            before_refs
        );
        assert_eq!(
            repo.run_git_successfully(&["count-objects", "-v"], None),
            before_objects
        );
    }
}

#[test]
fn verify_snapshot_classifies_absent_foreign_and_non_tree_objects() {
    for format in FORMATS {
        // Given: tags deliberately point directly to a valid tree.
        let repo = TempRepo::new(format);
        let tree = repo.read_oid(&["write-tree"], None);
        let blob = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"blob bytes"));
        let commit = repo.read_oid(&["rev-parse", "HEAD"], None);
        let tag_bytes = format!(
            "object {tree}\ntype tree\ntag snapshot\ntagger Fixture <fixture@example.invalid> 0 +0000\n\ntag bytes\n"
        );
        let tag = repo.read_oid(
            &["hash-object", "-t", "tag", "-w", "--stdin"],
            Some(tag_bytes.as_bytes()),
        );
        let (missing, foreign) = get_absent_oids(format);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let before_refs = repo.run_git_successfully(&["for-each-ref"], None);

        // When / Then: neither commits nor tags may be peeled into trees.
        for (oid, expected) in [
            (missing, SnapshotState::Missing),
            (blob.as_str(), SnapshotState::Invalid),
            (commit.as_str(), SnapshotState::Invalid),
            (tag.as_str(), SnapshotState::Invalid),
            (foreign, SnapshotState::Invalid),
        ] {
            // When / Then
            assert_eq!(git.verify_snapshot(&parse_tree(oid)).unwrap(), expected);
        }
        // Then
        assert_eq!(
            repo.run_git_successfully(&["for-each-ref"], None),
            before_refs
        );
    }
}

#[test]
fn verify_snapshot_propagates_policy_and_integrity_failures() {
    for format in FORMATS {
        // Given: alternates appear only after opening the adapter.
        let repo = TempRepo::new(format);
        let tree = parse_tree(&repo.read_oid(&["write-tree"], None));
        let replacement = repo.read_oid(&["mktree"], Some(b""));
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let before_refs = repo.run_git_successfully(&["for-each-ref"], None);
        let alternates = repo.get_root().join(".git/objects/info/alternates");
        fs::write(&alternates, b"/DONOR_PATH_CANARY/objects\n").unwrap();

        // When / Then
        assert_safe_internal_error(&git.verify_snapshot(&tree).unwrap_err(), &repo, &tree);
        assert_eq!(
            repo.run_git_successfully(&["for-each-ref"], None),
            before_refs
        );

        // Given: valid tree bytes stored under a different tree's identity.
        fs::remove_file(alternates).unwrap();
        repo.replace_loose_object(tree.as_str(), &replacement);

        // When / Then
        assert_safe_internal_error(&git.verify_snapshot(&tree).unwrap_err(), &repo, &tree);
        assert_eq!(
            repo.run_git_successfully(&["for-each-ref"], None),
            before_refs
        );
    }
}

#[test]
fn verify_snapshot_checks_root_integrity_without_requiring_child_closure() {
    for format in FORMATS {
        // Given: the exact root exists, but its referenced blob never existed.
        let repo = TempRepo::new(format);
        let (missing, _) = get_absent_oids(format);
        let entry = format!("100644 blob {missing}\tabsent-child\0");
        let tree =
            parse_tree(&repo.read_oid(&["mktree", "-z", "--missing"], Some(entry.as_bytes())));
        let git = open_git_snapshots(repo.get_root()).unwrap();
        let before_refs = repo.run_git_successfully(&["for-each-ref"], None);

        // When
        let state = git.verify_snapshot(&tree).unwrap();

        // Then
        assert_eq!(state, SnapshotState::Available);
        assert_eq!(
            repo.run_git_successfully(&["for-each-ref"], None),
            before_refs
        );
        assert!(
            !repo
                .run_git(&["cat-file", "-e", missing], None)
                .status
                .success()
        );
    }
}

fn parse_tree(oid: &str) -> TreeOid {
    TreeOid::parse(oid).unwrap()
}

fn get_absent_oids(format: ObjectFormat) -> (&'static str, &'static str) {
    match format {
        ObjectFormat::Sha1 => (ABSENT_SHA1, ABSENT_SHA256),
        ObjectFormat::Sha256 => (ABSENT_SHA256, ABSENT_SHA1),
    }
}

fn assert_safe_internal_error(error: &SnapshotError, repo: &TempRepo, tree: &TreeOid) {
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    let diagnostic = format!("{error} {error:?}");
    assert!(!diagnostic.contains("CANARY"));
    assert!(!diagnostic.contains(repo.get_root().to_str().unwrap()));
    assert!(!diagnostic.contains(tree.as_str()));
}
