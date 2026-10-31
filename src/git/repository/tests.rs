use super::GitRepository;
use crate::git::constants::SNAPSHOT_REF_PREFIX;
use crate::process_test_support as fixture;
use crate::{
    domain::RepoPath,
    open_git_snapshots,
    service::{GitSnapshots, SnapshotErrorKind, SnapshotFile},
};
use fixture::{ObjectFormat, TempRepo};
use std::{fs, path::PathBuf, rc::Rc, sync::Arc};

mod corpus;

#[test]
fn repository_open_counter_excludes_clones() {
    // Given: an independently prepared repository and this thread's open count.
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let before = GitRepository::get_repository_open_count();
    // When: one discovery is followed by a cheap clone and an independent open.
    let git = GitRepository::open(repo.get_root()).unwrap();
    let shared = git.clone();
    // Then: clones share both the repository and reader without discovering again.
    assert_eq!(GitRepository::get_repository_open_count() - before, 1);
    assert!(Rc::ptr_eq(&git.repo, &shared.repo));
    assert!(Rc::ptr_eq(&git.reader, &shared.reader));
    assert!(Arc::ptr_eq(&git.cleanup, &shared.cleanup));
    // When: an independent caller opens the same worktree.
    let independent = GitRepository::open(repo.get_root()).unwrap();
    // Then: it owns fresh state and performs its own discovery.
    assert_eq!(GitRepository::get_repository_open_count() - before, 2);
    assert!(!Rc::ptr_eq(&git.repo, &independent.repo));
}

#[test]
fn assembly_checks_unique_oids_once_without_weakening_supplied_or_final_validation() {
    use crate::service::SnapshotEntry;
    for format in fixture::FORMATS {
        // Given: one independently written blob supplied under three distinct paths.
        let repo = TempRepo::new(format);
        let git = GitRepository::open(repo.get_root()).unwrap();
        let oid = git.write_blob(b"original bytes").unwrap();
        let entries = ["a.md", "b.md", "nested/c.md"].map(|path| SnapshotEntry {
            path: RepoPath::parse(path).unwrap(),
            oid: oid.clone(),
        });
        let before = git.reader.borrow().get_object_read_count();
        // When: assemble from repeated caller-supplied object identities.
        let tree = git.create_tree_from_blobs(&entries).unwrap();
        // Then: exactly one addressed verification occurs in this assembly.
        assert_eq!(git.reader.borrow().get_object_read_count() - before, 1);
        git.pin(&tree).unwrap();
        // Given: valid compressed bytes replace that blob under its old identity.
        let other = git.write_blob(b"corrupt substitute").unwrap();
        repo.replace_loose_object(oid.as_str(), other.as_str());
        // When / Then: neither another assembly nor strong pin trusts the prior success/ref.
        assert_eq!(
            git.create_tree_from_blobs(&entries).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
        assert_eq!(
            git.pin(&tree).unwrap_err().get_kind(),
            SnapshotErrorKind::InternalError
        );
    }
}

#[test]
fn snapshots_work_without_git_executable() {
    use crate::{
        domain::TreeOid,
        service::{CorpusLister, SnapshotState, SnapshotVerifier},
    };
    // Given: historical packed objects, prepared with an independent Git executable.
    let Ok(root) = std::env::var("LETOPIS_CHILD_FREE_ROOT") else {
        supervise_snapshot_formats();
        return;
    };
    let tree = TreeOid::parse(&std::env::var("LETOPIS_CHILD_FREE_TREE").unwrap()).unwrap();
    // When: every snapshot operation runs with an empty executable search path.
    let git = GitRepository::open(std::path::Path::new(&root)).unwrap();
    let path = RepoPath::parse("exact.md").unwrap();
    let blob = git.write_blob(b"raw\r\n\0bytes").unwrap();
    let captured = git
        .create_tree_from_blobs(&[crate::service::SnapshotEntry {
            path: path.clone(),
            oid: blob,
        }])
        .unwrap();
    let empty = git.create_tree(&[]).unwrap();
    git.retain(&tree).unwrap();
    git.pin(&tree).unwrap();
    git.pin(&captured).unwrap();
    // Then: old and new snapshots remain usable, with exact bytes and corpus membership.
    assert_eq!(
        git.verify_snapshot(&tree).unwrap(),
        SnapshotState::Available
    );
    assert_eq!(
        git.verify_snapshot(&empty).unwrap(),
        SnapshotState::Available
    );
    assert_eq!(git.read_blob(&captured, &path).unwrap(), b"raw\r\n\0bytes");
    assert!(
        git.list_candidates()
            .unwrap()
            .contains(&RepoPath::parse("live.md").unwrap())
    );
}

#[test]
fn strong_pin_rejects_malformed_nested_trees_like_git() {
    // Given: valid roots wrap malformed nested trees written only with --literally.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let git = GitRepository::open(repo.get_root()).unwrap();
        let blob = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b""));
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
            let nested = repo.read_oid(
                &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
                Some(&raw),
            );
            let root = repo.read_oid(
                &["mktree"],
                Some(format!("040000 tree {nested}\tnested\n").as_bytes()),
            );
            let root = crate::domain::TreeOid::parse(&root).unwrap();
            // When: strong pin audits the nested closure.
            let error = git.pin(&root).unwrap_err();
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
    // Given
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let expected_length = match format {
            ObjectFormat::Sha1 => 40,
            ObjectFormat::Sha256 => 64,
        };
        // When
        let git = GitRepository::open(repo.get_subdir()).unwrap();
        // Then
        assert_eq!(git.root, repo.get_root());
        assert_eq!(git.oid_len, expected_length);
    }
}

#[test]
fn open_rejects_non_repository_and_bare() {
    // Given
    let repo = TempRepo::new(ObjectFormat::Sha1);
    let bare = repo.get_root().join("bare");
    repo.run_git_successfully(&["init", "-q", "--bare", bare.to_str().unwrap()], None);
    let empty = std::env::temp_dir().join(format!("letopis-empty-{}", std::process::id()));
    fs::create_dir(&empty).unwrap();
    // When
    let bare_result = GitRepository::open(&bare);
    let empty_result = GitRepository::open(&empty);
    // Then
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
    let result = GitRepository::open(&repo.get_root().join(".git"));
    // Then: the adapter preserves the working-copy-only boundary.
    assert_eq!(
        result.unwrap_err().get_kind(),
        SnapshotErrorKind::InvalidRequest
    );
}

#[test]
fn discovers_trailing_newline_repository_without_touching_sibling() {
    // Given
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
        // When
        let git = open_git_snapshots(&target).unwrap();
        let tree = git
            .create_tree(&[SnapshotFile {
                path: &path,
                bytes: b"newline repository isolation\0",
            }])
            .unwrap();
        git.pin(&tree).unwrap();
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
        let bytes = git.read_blob(&tree, &path).unwrap();
        let discovered_root = GitRepository::open(&target).unwrap().root;
        // Then
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
    // When
    let error = GitRepository::open(repo.get_root()).expect_err("unsafe object store was accepted");
    // Then
    assert_eq!(error.get_kind(), SnapshotErrorKind::InternalError);
    assert_eq!(fs::read(info).unwrap(), b"not a directory");
}

#[test]
fn root_verification_and_addressed_reads_reject_malformed_tree_structure() {
    use crate::{domain::TreeOid, service::SnapshotVerifier};
    // Given: a hash-correct tree with an invalid file mode, accepted only by --literally.
    for format in fixture::FORMATS {
        let repo = TempRepo::new(format);
        let blob = repo.read_oid(&["hash-object", "-w", "--stdin"], Some(b"bytes"));
        let mut bytes = b"100664 x\0".to_vec();
        bytes.extend_from_slice(
            gix::hash::ObjectId::from_hex(blob.as_bytes())
                .unwrap()
                .as_bytes(),
        );
        let oid = repo.read_oid(
            &["hash-object", "--literally", "-t", "tree", "-w", "--stdin"],
            Some(&bytes),
        );
        let oid = TreeOid::parse(&oid).unwrap();
        let git = GitRepository::open(repo.get_root()).unwrap();
        // When: each operation reaches this root without a full closure audit.
        let verified = git.verify_snapshot(&oid).unwrap_err();
        let retained = git.retain(&oid).unwrap_err();
        let addressed = git
            .read_blob(&oid, &RepoPath::parse("x").unwrap())
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
        let tree = repo.read_oid(&["rev-parse", "HEAD^{tree}"], None);
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
