use super::tree_get_by_oid;
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
