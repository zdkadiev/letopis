use super::{StagedObject, retain_ref};
use crate::process_test_support as fixture;
use fixture::{FORMATS, TempRepo};
use gix::objs::Kind;
use std::{fs, os::unix::fs::PermissionsExt};

#[test]
fn stages_syncs_publishes_and_retains_git_compatible_objects() {
    // Given: real repositories and independently computed Git object identities.
    for format in FORMATS {
        let fixture = TempRepo::new(format);
        let repo = gix::open(fixture.get_root()).unwrap();
        let expected = fixture.read_oid(&["hash-object", "--stdin"], Some(b"durable bytes"));
        let object = StagedObject::new(&repo, Kind::Blob, b"durable bytes").unwrap();
        let oid = object.get_oid();
        assert_eq!(oid.to_string(), expected);
        assert!(!object.destination.exists());
        assert_eq!(
            fs::metadata(object.directory.path())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        // When: completed synced contents are promoted before the exact direct ref.
        object.publish().unwrap();
        retain_ref(&repo, oid).unwrap();
        // Then: Git reads the original bytes and duplicate publication is idempotent.
        assert_eq!(
            fixture.run_git_successfully(&["cat-file", "blob", &expected], None),
            b"durable bytes"
        );
        assert_eq!(fixture.read_direct_ref(&expected), expected);
        StagedObject::new(&repo, Kind::Blob, b"durable bytes")
            .unwrap()
            .publish()
            .unwrap();
        retain_ref(&repo, oid).unwrap();
    }
}

#[test]
fn writes_canonical_tree_objects_in_both_formats() {
    // Given: Git independently supplies the identity of the canonical empty tree.
    for format in FORMATS {
        let fixture = TempRepo::new(format);
        let repo = gix::open(fixture.get_root()).unwrap();
        let expected = fixture.read_oid(&["hash-object", "-t", "tree", "--stdin"], Some(b""));
        let object = StagedObject::new(&repo, Kind::Tree, b"").unwrap();
        let oid = object.get_oid();
        // When: the library stages, syncs and publishes that tree with its actual kind header.
        object.publish().unwrap();
        retain_ref(&repo, oid).unwrap();
        // Then: type, identity and payload agree with Git's independent oracle.
        assert_eq!(oid.to_string(), expected);
        assert_eq!(
            fixture.run_git_successfully(&["cat-file", "tree", &expected], None),
            b""
        );
        assert_eq!(fixture.read_direct_ref(&expected), expected);
    }
}

#[test]
fn refuses_corrupt_existing_objects_and_cleans_private_staging() {
    // Given: a pre-existing corrupt loose object at the required identity.
    let fixture = TempRepo::new(FORMATS[0]);
    let repo = gix::open(fixture.get_root()).unwrap();
    let object = StagedObject::new(&repo, Kind::Blob, b"corrupt winner").unwrap();
    let stage = object.directory.path().to_owned();
    let destination = object.destination.clone();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, b"CANARY invalid zlib").unwrap();
    // When: no-clobber promotion encounters that existing inode.
    let result = object.publish();
    // Then: it fails, preserves the corrupt winner and removes its own staging directory.
    assert!(result.is_err());
    assert_eq!(fs::read(destination).unwrap(), b"CANARY invalid zlib");
    assert!(!stage.exists());
}

#[test]
fn rejects_symbolic_malformed_and_conflicting_refs_without_replacing_them() {
    // Given: each invalid direct-ref case at the exact immutable snapshot name.
    for content in [
        "ref: refs/heads/missing\n",
        "malformed CANARY\n",
        "1111111111111111111111111111111111111111\n",
    ] {
        let fixture = TempRepo::new(FORMATS[0]);
        let repo = gix::open(fixture.get_root()).unwrap();
        let object = StagedObject::new(&repo, Kind::Blob, b"target").unwrap();
        let oid = object.get_oid();
        object.publish().unwrap();
        let path = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        // When: a create-only retention attempt checks the exact non-dereferenced target.
        let result = retain_ref(&repo, oid);
        // Then: invalid or conflicting refs are not overwritten.
        assert!(result.is_err(), "{content}");
        assert_eq!(fs::read_to_string(&path).unwrap(), content);
    }
}

#[test]
fn respects_foreign_locks_and_equal_ref_noops() {
    // Given: a foreign writer owns the ref lock before publication.
    let fixture = TempRepo::new(FORMATS[0]);
    let repo = gix::open(fixture.get_root()).unwrap();
    let object = StagedObject::new(&repo, Kind::Blob, b"lock target").unwrap();
    let oid = object.get_oid();
    object.publish().unwrap();
    let path = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock_path = path.with_extension("lock");
    fs::write(&lock_path, b"foreign lock CANARY").unwrap();
    // When / Then: a missing ref cannot steal the lock or report success.
    assert!(retain_ref(&repo, oid).is_err());
    assert!(!path.exists());
    assert_eq!(fs::read(&lock_path).unwrap(), b"foreign lock CANARY");
    // Given: the concurrent winner now published the exact value, retaining its lock.
    fs::write(&path, format!("{oid}\n")).unwrap();
    // When / Then: idempotent success neither opens nor replaces the foreign lock.
    retain_ref(&repo, oid).unwrap();
    assert_eq!(fs::read(&lock_path).unwrap(), b"foreign lock CANARY");
}

#[test]
fn leaves_private_reflogs_untouched_under_all_policies() {
    // Given: private ref logs are not part of Letopis retention; existing bytes belong to Git.
    for policy in ["always", "false"] {
        let fixture = TempRepo::new(FORMATS[0]);
        fixture.run_git_successfully(&["config", "core.logAllRefUpdates", policy], None);
        let repo = gix::open(fixture.get_root()).unwrap();
        for existing in [false, true] {
            let body = format!("{policy}-{existing}");
            let object = StagedObject::new(&repo, Kind::Blob, body.as_bytes()).unwrap();
            let oid = object.get_oid();
            object.publish().unwrap();
            let log = repo
                .common_dir()
                .join(format!("logs/refs/letopis/snapshots/{oid}"));
            if existing {
                fs::create_dir_all(log.parent().unwrap()).unwrap();
                fs::write(&log, b"user-owned historical log\n").unwrap();
            }
            // When: private retention creates the ref and repeats the idempotent operation.
            retain_ref(&repo, oid).unwrap();
            retain_ref(&repo, oid).unwrap();
            // Then: no log is created or appended, regardless of Git's auto-log configuration.
            assert_eq!(log.exists(), existing);
            if existing {
                assert_eq!(fs::read(log).unwrap(), b"user-owned historical log\n");
            }
        }
    }
}

#[test]
fn concurrent_creators_and_packed_refs_preserve_exact_target() {
    // Given: matching independent writers and an ordinary Git ref packer.
    let fixture = TempRepo::new(FORMATS[0]);
    let root = fixture.get_root().to_owned();
    let expected = fixture.read_oid(&["hash-object", "--stdin"], Some(b"concurrent"));
    // When: every writer stages/publishes independently and races the same create-only ref.
    std::thread::scope(|scope| {
        let writers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let repo = gix::open(&root).unwrap();
                    let object = StagedObject::new(&repo, Kind::Blob, b"concurrent").unwrap();
                    let oid = object.get_oid();
                    object.publish().unwrap();
                    retain_ref(&repo, oid)
                })
            })
            .collect();
        assert!(
            writers
                .into_iter()
                .map(|writer| writer.join().unwrap())
                .filter(Result::is_ok)
                .count()
                >= 1
        );
    });
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    let repo = gix::open(&root).unwrap();
    retain_ref(
        &repo,
        gix::hash::ObjectId::from_hex(expected.as_bytes()).unwrap(),
    )
    .unwrap();
    // Then: Git's packed-ref reader sees exactly the expected direct target.
    assert_eq!(fixture.read_direct_ref(&expected), expected);
}

#[test]
fn injected_failures_preserve_publication_order_and_report_post_link_errors() {
    // Given: independent failures at each file sync/publication boundary.
    for point in [
        FailurePoint::ObjectSync,
        FailurePoint::ObjectPublish,
        FailurePoint::DirectorySync,
        FailurePoint::RefSync,
        FailurePoint::RefPublish,
    ] {
        let fixture = TempRepo::new(FORMATS[0]);
        let repo = gix::open(fixture.get_root()).unwrap();
        FAILURE.with(|slot| slot.set(Some(point)));
        // When: one operation fails at its designated boundary.
        let result = StagedObject::new(&repo, Kind::Blob, b"failure").and_then(|object| {
            let oid = object.get_oid();
            object.publish()?;
            retain_ref(&repo, oid)
        });
        FAILURE.with(|slot| slot.set(None));
        // Then: failure is reported and no retaining ref survives a pre-ref-publication error.
        assert!(result.is_err(), "{point:?}");
        let refs = fixture.run_git_successfully(&["for-each-ref", "refs/letopis/snapshots/"], None);
        assert!(refs.is_empty(), "{point:?}");
        assert!(
            fs::read_dir(repo.objects.store_ref().path())
                .unwrap()
                .all(|entry| !entry
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".letopis-stage-"))
        );
    }
}

#[test]
fn reports_directory_sync_failure_after_ref_publication() {
    // Given: an object durably published before a ref-directory sync failure.
    let fixture = TempRepo::new(FORMATS[0]);
    let repo = gix::open(fixture.get_root()).unwrap();
    let object = StagedObject::new(&repo, Kind::Blob, b"post publication").unwrap();
    let oid = object.get_oid();
    object.publish().unwrap();
    FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
    // When: rename succeeds but its subsequent directory sync fails.
    let result = retain_ref(&repo, oid);
    FAILURE.with(|slot| slot.set(None));
    // Then: a visible correct ref can remain, but the operation must report failure.
    assert!(result.is_err());
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
}

#[test]
fn linked_worktrees_publish_in_common_store_with_shared_permissions() {
    // Given: a shared repository and a linked worktree with a separate administrative directory.
    let fixture = TempRepo::new(FORMATS[0]);
    fixture.run_git_successfully(&["config", "core.sharedRepository", "group"], None);
    let linked = fixture.get_root().join("linked");
    fixture.run_git_successfully(
        &[
            "worktree",
            "add",
            "--detach",
            linked.to_str().unwrap(),
            "HEAD",
        ],
        None,
    );
    let repo = gix::open(&linked).unwrap();
    let oracle = fixture.read_oid(&["hash-object", "-w", "--stdin"], Some(b"Git mode oracle"));
    let object = StagedObject::new(&repo, Kind::Blob, b"library mode probe").unwrap();
    let destination = object.destination.clone();
    let oid = object.get_oid();
    // When: the linked-worktree handle publishes an object and private ref.
    object.publish().unwrap();
    retain_ref(&repo, oid).unwrap();
    // Then: Git sees them in the common store, with its shared permissions policy preserved.
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
    assert_eq!(
        fs::metadata(&destination).unwrap().permissions().mode() & 0o7777,
        fs::metadata(fixture.build_object_path(&oracle))
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    );
    assert_eq!(
        fs::metadata(destination.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o7777,
        fs::metadata(fixture.build_object_path(&oracle).parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o7777
    );
}

#[test]
fn rejects_filesystem_symlink_refs_without_touching_the_referent() {
    // Given: a filesystem symlink at the exact snapshot ref path.
    let fixture = TempRepo::new(FORMATS[0]);
    let repo = gix::open(fixture.get_root()).unwrap();
    let object = StagedObject::new(&repo, Kind::Blob, b"symlink target").unwrap();
    let oid = object.get_oid();
    object.publish().unwrap();
    let ref_path = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
    let referent = fixture.get_root().join("foreign");
    let bytes = format!("{oid}\n");
    fs::write(&referent, &bytes).unwrap();
    std::os::unix::fs::symlink(&referent, &ref_path).unwrap();
    // When: exact direct retention encounters the symlink.
    let result = retain_ref(&repo, oid);
    // Then: it refuses the indirection and leaves both filesystem entries intact.
    assert!(result.is_err());
    assert!(
        fs::symlink_metadata(ref_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read_to_string(referent).unwrap(), bytes);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum FailurePoint {
    ObjectSync,
    ObjectPublish,
    DirectorySync,
    RefSync,
    RefPublish,
    RefDirectorySync,
}
thread_local! { static FAILURE: std::cell::Cell<Option<FailurePoint>> = const { std::cell::Cell::new(None) }; }

pub(super) fn inject_failure(point: FailurePoint) -> Result<(), crate::service::SnapshotError> {
    if FAILURE.with(|slot| slot.get() == Some(point)) {
        return Err(super::fail_io(std::io::Error::other(
            "injected I/O failure",
        )));
    }
    Ok(())
}
