#[path = "support/git.rs"]
mod git;

use git::{FORMATS, TempRepo};
use letopis::{
    domain::{BlobOid, RepoPath, TreeOid},
    open_git_snapshots,
    service::{GitSnapshots, SnapshotErrorKind, SnapshotState, SnapshotVerifier},
};
use std::{fs, io::ErrorKind};

const LEAF_NAME: &str = "leaf";
const CANARY_BYTES: &[u8] = b"AlphaControl CORRUPTION_CANARY";

#[derive(Clone, Copy, Debug)]
enum ObjectDamage {
    Truncate,
    Missing,
}

#[test]
fn corrupt_tree_remains_an_error_across_reopen_until_restored() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let (tree, _) = create_tree_case(&repo);
        let leaf = RepoPath::parse(LEAF_NAME).unwrap();
        let original_user_state = repo.capture_fixture_state();
        {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
            git.pin(&tree).unwrap();
        }
        let original = damage_object(&repo, tree.as_str(), ObjectDamage::Truncate);

        // When / Then
        for _ in 0..2 {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            let verify_error = git.verify_snapshot(&tree).unwrap_err();
            assert_eq!(verify_error.get_kind(), SnapshotErrorKind::InternalError);
            assert!(!format!("{verify_error} {verify_error:?}").contains("CORRUPTION_CANARY"));
            let pin_error = git.pin(&tree).unwrap_err();
            assert_eq!(pin_error.get_kind(), SnapshotErrorKind::InternalError);
            assert!(!format!("{pin_error} {pin_error:?}").contains("CORRUPTION_CANARY"));
            assert_eq!(repo.capture_fixture_state(), original_user_state);
        }

        // Given / When / Then: exact fixture restoration returns a fresh handle to health.
        restore_object(&repo, tree.as_str(), &original);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
        git.pin(&tree).unwrap();
        assert_eq!(repo.capture_fixture_state(), original_user_state);
    }
}

#[test]
fn corrupt_child_never_pins_an_incomplete_closure() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let (tree, blob) = create_tree_case(&repo);
        let leaf = RepoPath::parse(LEAF_NAME).unwrap();
        let original_user_state = repo.capture_fixture_state();
        {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
            git.pin(&tree).unwrap();
        }
        let original = damage_object(&repo, blob.as_str(), ObjectDamage::Truncate);

        // When / Then
        for _ in 0..2 {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            assert_eq!(
                git.verify_snapshot(&tree).unwrap(),
                SnapshotState::Available
            );
            let error = git.pin(&tree).unwrap_err();
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(repo.capture_fixture_state(), original_user_state);
            assert!(!format!("{error} {error:?}").contains("CORRUPTION_CANARY"));
        }

        // Given / When / Then: exact fixture restoration returns a fresh handle to health.
        restore_object(&repo, blob.as_str(), &original);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
        git.pin(&tree).unwrap();
        assert_eq!(repo.capture_fixture_state(), original_user_state);
    }
}

#[test]
fn missing_child_is_not_a_new_empty_snapshot() {
    for format in FORMATS {
        // Given
        let repo = TempRepo::new(format);
        let (tree, blob) = create_tree_case(&repo);
        let leaf = RepoPath::parse(LEAF_NAME).unwrap();
        let original_user_state = repo.capture_fixture_state();
        {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
            git.pin(&tree).unwrap();
        }
        let original = damage_object(&repo, blob.as_str(), ObjectDamage::Missing);

        // When / Then: root-only verification stays distinct from closure pinning.
        for _ in 0..2 {
            let git = open_git_snapshots(repo.get_root()).unwrap();
            assert_eq!(
                git.verify_snapshot(&tree).unwrap(),
                SnapshotState::Available
            );
            let error = git.pin(&tree).unwrap_err();
            assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
            assert_eq!(repo.capture_fixture_state(), original_user_state);
            assert!(!format!("{error} {error:?}").contains("CORRUPTION_CANARY"));
        }

        // Given / When / Then: exact fixture restoration returns a fresh handle to health.
        restore_object(&repo, blob.as_str(), &original);
        let git = open_git_snapshots(repo.get_root()).unwrap();
        assert_eq!(git.get_blob(&tree, &leaf).unwrap(), CANARY_BYTES);
        git.pin(&tree).unwrap();
        assert_eq!(repo.capture_fixture_state(), original_user_state);
    }
}

fn create_tree_case(repo: &TempRepo) -> (TreeOid, BlobOid) {
    let blob = repo.get_oid(&["hash-object", "-w", "--stdin"], Some(CANARY_BYTES));
    let entry = format!("100644 blob {blob}\t{LEAF_NAME}\0");
    let tree = repo.get_oid(&["mktree", "-z"], Some(entry.as_bytes()));
    (
        TreeOid::parse(&tree).unwrap(),
        BlobOid::parse(&blob).unwrap(),
    )
}

fn damage_object(repo: &TempRepo, oid: &str, damage: ObjectDamage) -> Vec<u8> {
    let root = fs::canonicalize(repo.get_root()).unwrap();
    let path = repo.build_object_path(oid);
    let canonical = fs::canonicalize(&path).unwrap();
    assert!(canonical.starts_with(&root));
    assert!(fs::symlink_metadata(&path).unwrap().file_type().is_file());
    let original = fs::read(&path).unwrap();
    match damage {
        ObjectDamage::Truncate => {
            fs::remove_file(&path).unwrap();
            fs::write(path, &original[..original.len() / 2]).unwrap();
        }
        ObjectDamage::Missing => fs::remove_file(path).unwrap(),
    }
    original
}

fn restore_object(repo: &TempRepo, oid: &str, original: &[u8]) {
    let root = fs::canonicalize(repo.get_root()).unwrap();
    let path = repo.build_object_path(oid);
    let parent = fs::canonicalize(path.parent().unwrap()).unwrap();
    assert!(parent.starts_with(&root));
    match fs::symlink_metadata(&path) {
        Ok(metadata) => assert!(metadata.file_type().is_file()),
        Err(error) => assert_eq!(error.kind(), ErrorKind::NotFound),
    }
    fs::write(path, original).unwrap();
}
