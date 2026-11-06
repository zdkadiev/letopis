use super::{StagedObject, ref_lock_is_busy, ref_retain_by_oid as retain_ref_with_cleanup};
use crate::{process_test_support as fixture, service::SnapshotError};
use fixture::{FORMATS, TempRepo};
use gix::{Repository, hash::ObjectId, objs::Kind};
use std::{
    cell::{Cell, RefCell},
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

mod confirmation {
    use super::{
        CONCURRENT_PUBLICATION, ConcurrentLock, FAILURE, FORMATS, FailurePoint, TempRepo,
        ref_retain_by_oid,
    };
    use super::{LOCK_OBSERVATION, LockObservation};
    use crate::git::durability::{StagedObject, ref_lock_is_busy};
    use gix::{Repository, hash::ObjectId, objs::Kind};
    use std::{cell::Cell, fs};

    #[test]
    fn retry_of_visible_ref_must_complete_sync_before_success() {
        // Given: a ref whose rename succeeded but directory synchronization failed.
        let (fixture, repo, oid) = prepare_repo();
        FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
        let first = ref_retain_by_oid(&repo, oid);
        // When: a retry sees that value while the same sync failure remains active.
        let retry = ref_retain_by_oid(&repo, oid);
        FAILURE.with(|slot| slot.set(None));
        // Then: neither visibility nor retry bypasses the failure; recovery can finish durability.
        assert!(first.is_err());
        assert!(retry.is_err());
        assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
        ref_retain_by_oid(&repo, oid).unwrap();
    }

    #[test]
    fn concurrent_confirmation_cannot_bypass_sync_or_steal_a_lock() {
        // Given: a concurrent publisher renames immediately before this writer's lock attempt.
        for state in [ConcurrentLock::Released, ConcurrentLock::Held] {
            let (fixture, repo, oid) = prepare_repo();
            let path = repo
                .common_dir()
                .join(format!("refs/letopis/snapshots/{oid}"));
            CONCURRENT_PUBLICATION
                .with(|slot| *slot.borrow_mut() = Some((path.clone(), oid, state)));
            FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
            // When: exact bytes become visible through either acquisition outcome.
            let result = ref_retain_by_oid(&repo, oid);
            FAILURE.with(|slot| slot.set(None));
            // Then: confirmation fails until synchronization is possible and never alters a foreign lock.
            assert!(result.is_err());
            assert_eq!(
                ref_lock_is_busy(result.as_ref().unwrap_err()),
                matches!(state, ConcurrentLock::Held)
            );
            if matches!(state, ConcurrentLock::Held) {
                assert_eq!(
                    fs::read(path.with_extension("lock")).unwrap(),
                    b"foreign lock CANARY"
                );
                fs::remove_file(path.with_extension("lock")).unwrap();
            }
            ref_retain_by_oid(&repo, oid).unwrap();
            assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
        }
    }

    #[test]
    fn packed_confirmation_requires_backing_file_and_parent_sync() {
        // Given: an exact packed-only ref without a loose backing file.
        let (fixture, repo, oid) = prepare_repo();
        ref_retain_by_oid(&repo, oid).unwrap();
        fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
        let loose = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        assert!(!loose.exists());
        // When: file or parent synchronization is unavailable for a matching packed value.
        for point in [FailurePoint::RefSync, FailurePoint::RefDirectorySync] {
            FAILURE.with(|slot| slot.set(Some(point)));
            let result = ref_retain_by_oid(&repo, oid);
            FAILURE.with(|slot| slot.set(None));
            // Then: packed visibility alone cannot acknowledge durability.
            assert!(result.is_err(), "{point:?}");
            assert!(!ref_lock_is_busy(result.as_ref().unwrap_err()));
        }
        ref_retain_by_oid(&repo, oid).unwrap();
        assert!(!loose.exists());
        assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
    }

    #[test]
    fn packed_confirmation_does_not_touch_a_foreign_packing_lock() {
        // Given: a packed ref with another packer's lock still held.
        let (fixture, repo, oid) = prepare_repo();
        ref_retain_by_oid(&repo, oid).unwrap();
        fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
        let lock = repo.common_dir().join("packed-refs.lock");
        fs::write(&lock, b"foreign packer CANARY").unwrap();
        // When: confirmation cannot coordinate the backing file's lifetime.
        let result = ref_retain_by_oid(&repo, oid);
        // Then: it reports contention, preserving the foreign lock until its owner releases it.
        assert!(ref_lock_is_busy(&result.unwrap_err()));
        assert_eq!(fs::read(&lock).unwrap(), b"foreign packer CANARY");
        fs::remove_file(lock).unwrap();
        ref_retain_by_oid(&repo, oid).unwrap();
    }

    #[test]
    fn loose_confirmation_syncs_the_existing_backing_without_renaming_it() {
        // Given: an externally published exact loose ref with no known sync history.
        let (fixture, repo, oid) = prepare_repo();
        let path = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, format!("{oid}\n")).unwrap();
        // When: durable confirmation owns the lock, syncs the backing and its parent chain.
        ref_retain_by_oid(&repo, oid).unwrap();
        // Then: bytes remain exact and this isolated scenario also supplies the syscall oracle.
        assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
        assert!(!path.with_extension("lock").exists());
    }

    #[test]
    fn packed_confirmation_syncs_the_existing_backing_without_creating_a_loose_ref() {
        // Given: Git publishes and packs a ref independently of the library helper.
        let (fixture, repo, oid) = prepare_repo();
        let name = format!("refs/letopis/snapshots/{oid}");
        fixture.run_git_successfully(&["update-ref", &name, &oid.to_string()], None);
        fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
        // When: durable confirmation synchronizes packed-refs under both owned locks.
        ref_retain_by_oid(&repo, oid).unwrap();
        // Then: its exact target remains packed; the syscall oracle observes the backing sync.
        assert!(!repo.common_dir().join(&name).exists());
        assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
    }

    #[test]
    fn concurrent_confirmation_syncs_the_visible_winner_before_success() {
        // Given: another writer's rename becomes visible immediately before acquisition.
        let (fixture, repo, oid) = prepare_repo();
        let path = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        CONCURRENT_PUBLICATION.with(|slot| {
            *slot.borrow_mut() = Some((path.clone(), oid, ConcurrentLock::Released));
        });
        // When: confirmation owns the newly released lock and completes the winner's sync.
        ref_retain_by_oid(&repo, oid).unwrap();
        // Then: success has the exact value; the syscall oracle requires a post-rename backing sync.
        assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
        assert!(!path.with_extension("lock").exists());
    }

    #[test]
    fn packed_confirmation_holds_both_locks_during_backing_sync() {
        // Given: a packed-only target and an independent create-new lock probe.
        let (fixture, repo, oid) = prepare_repo();
        ref_retain_by_oid(&repo, oid).unwrap();
        fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
        LOCK_OBSERVATION.with(|slot| slot.set(Some(LockObservation::Pending)));
        // When: confirmation reaches its backing-file sync boundary.
        ref_retain_by_oid(&repo, oid).unwrap();
        let observed = LOCK_OBSERVATION.with(Cell::take);
        // Then: neither a ref writer nor a packer can acquire its lock during confirmation.
        assert_eq!(
            observed,
            Some(LockObservation::Observed {
                is_ref_available: false,
                is_packed_available: false
            })
        );
    }

    #[test]
    fn packed_confirmation_rejects_a_symlink_backing_instead_of_syncing_the_wrong_parent() {
        // Given: a packed-ref symlink whose actual file resides outside the ref-store directory.
        let (fixture, repo, oid) = prepare_repo();
        ref_retain_by_oid(&repo, oid).unwrap();
        fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
        let packed = repo.common_dir().join("packed-refs");
        let external = fixture.get_root().join("external-packed-refs");
        fs::rename(&packed, &external).unwrap();
        std::os::unix::fs::symlink(&external, &packed).unwrap();
        // When: the chosen backing cannot be opened without following that alias.
        let result = ref_retain_by_oid(&repo, oid);
        // Then: the operation fails without acknowledging an unsynchronized external parent.
        assert!(result.is_err());
        assert!(!ref_lock_is_busy(result.as_ref().unwrap_err()));
        assert!(
            fs::symlink_metadata(packed)
                .unwrap()
                .file_type()
                .is_symlink()
        );
    }

    fn prepare_repo() -> (TempRepo, Repository, ObjectId) {
        let fixture = TempRepo::new(FORMATS[0]);
        let repo = gix::open(fixture.get_root()).unwrap();
        let object = StagedObject::new(&repo, Kind::Blob, b"durable confirmation").unwrap();
        let oid = object.object_get_oid();
        object.object_publish().unwrap();
        (fixture, repo, oid)
    }
}

#[test]
fn stages_syncs_publishes_and_retains_git_compatible_objects() {
    // Given: real repositories and independently computed Git object identities.
    for format in FORMATS {
        let fixture = TempRepo::new(format);
        let repo = gix::open(fixture.get_root()).unwrap();
        let expected = fixture.get_oid(&["hash-object", "--stdin"], Some(b"durable bytes"));
        let object = StagedObject::new(&repo, Kind::Blob, b"durable bytes").unwrap();
        let oid = object.object_get_oid();
        assert_eq!(oid.to_string(), expected);
        assert!(!object.destination.exists());
        assert_eq!(
            fs::metadata(object.staged.parent().unwrap().parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        // When: completed synced contents are promoted before the exact direct ref.
        object.object_publish().unwrap();
        ref_retain_by_oid(&repo, oid).unwrap();
        // Then: Git reads the original bytes and duplicate publication is idempotent.
        assert_eq!(
            fixture.run_git_successfully(&["cat-file", "blob", &expected], None),
            b"durable bytes"
        );
        assert_eq!(fixture.get_direct_ref(&expected), expected);
        StagedObject::new(&repo, Kind::Blob, b"durable bytes")
            .unwrap()
            .object_publish()
            .unwrap();
        ref_retain_by_oid(&repo, oid).unwrap();
    }
}

#[test]
fn writes_canonical_tree_objects_in_both_formats() {
    // Given: Git independently supplies the identity of the canonical empty tree.
    for format in FORMATS {
        let fixture = TempRepo::new(format);
        let repo = gix::open(fixture.get_root()).unwrap();
        let expected = fixture.get_oid(&["hash-object", "-t", "tree", "--stdin"], Some(b""));
        let object = StagedObject::new(&repo, Kind::Tree, b"").unwrap();
        let oid = object.object_get_oid();
        // When: the library stages, syncs and publishes that tree with its actual kind header.
        object.object_publish().unwrap();
        ref_retain_by_oid(&repo, oid).unwrap();
        // Then: type, identity and payload agree with Git's independent oracle.
        assert_eq!(oid.to_string(), expected);
        assert_eq!(
            fixture.run_git_successfully(&["cat-file", "tree", &expected], None),
            b""
        );
        assert_eq!(fixture.get_direct_ref(&expected), expected);
    }
}

#[test]
fn refuses_corrupt_existing_objects_and_cleans_private_staging() {
    // Given: a pre-existing corrupt loose object at the required identity.
    let fixture = TempRepo::new(FORMATS[0]);
    let repo = gix::open(fixture.get_root()).unwrap();
    let object = StagedObject::new(&repo, Kind::Blob, b"corrupt winner").unwrap();
    let stage = object.staged.parent().unwrap().parent().unwrap().to_owned();
    let destination = object.destination.clone();
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(&destination, b"CANARY invalid zlib").unwrap();
    // When: no-clobber promotion encounters that existing inode.
    let result = object.object_publish();
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
        let oid = object.object_get_oid();
        object.object_publish().unwrap();
        let path = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        // When: a create-only retention attempt checks the exact non-dereferenced target.
        let result = ref_retain_by_oid(&repo, oid);
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
    let oid = object.object_get_oid();
    object.object_publish().unwrap();
    let path = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let lock_path = path.with_extension("lock");
    fs::write(&lock_path, b"foreign lock CANARY").unwrap();
    // When / Then: a missing ref cannot steal the lock or report success.
    assert!(ref_retain_by_oid(&repo, oid).is_err());
    assert!(!path.exists());
    assert_eq!(fs::read(&lock_path).unwrap(), b"foreign lock CANARY");
    // Given: the concurrent winner now published the exact value, retaining its lock.
    fs::write(&path, format!("{oid}\n")).unwrap();
    // When / Then: matching bytes cannot bypass ownership; the foreign lock stays untouched.
    assert!(ref_lock_is_busy(
        &ref_retain_by_oid(&repo, oid).unwrap_err()
    ));
    assert_eq!(fs::read(&lock_path).unwrap(), b"foreign lock CANARY");
    fs::remove_file(&lock_path).unwrap();
    ref_retain_by_oid(&repo, oid).unwrap();
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
            let oid = object.object_get_oid();
            object.object_publish().unwrap();
            let log = repo
                .common_dir()
                .join(format!("logs/refs/letopis/snapshots/{oid}"));
            if existing {
                fs::create_dir_all(log.parent().unwrap()).unwrap();
                fs::write(&log, b"user-owned historical log\n").unwrap();
            }
            // When: private retention creates the ref and repeats the idempotent operation.
            ref_retain_by_oid(&repo, oid).unwrap();
            ref_retain_by_oid(&repo, oid).unwrap();
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
    let expected = fixture.get_oid(&["hash-object", "--stdin"], Some(b"concurrent"));
    // When: every writer stages/publishes independently and races the same create-only ref.
    std::thread::scope(|scope| {
        let writers: Vec<_> = (0..4)
            .map(|_| {
                scope.spawn(|| {
                    let repo = gix::open(&root).unwrap();
                    let object = StagedObject::new(&repo, Kind::Blob, b"concurrent").unwrap();
                    let oid = object.object_get_oid();
                    object.object_publish().unwrap();
                    retain_after_contention(&repo, oid)
                })
            })
            .collect();
        assert!(
            writers
                .into_iter()
                .all(|writer| writer.join().unwrap().is_ok())
        );
    });
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    let repo = gix::open(&root).unwrap();
    ref_retain_by_oid(&repo, ObjectId::from_hex(expected.as_bytes()).unwrap()).unwrap();
    // Then: Git's packed-ref reader sees exactly the expected direct target.
    assert_eq!(fixture.get_direct_ref(&expected), expected);
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
            let oid = object.object_get_oid();
            object.object_publish()?;
            ref_retain_by_oid(&repo, oid)
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
    let oid = object.object_get_oid();
    object.object_publish().unwrap();
    FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
    // When: rename succeeds but its subsequent directory sync fails.
    let result = ref_retain_by_oid(&repo, oid);
    FAILURE.with(|slot| slot.set(None));
    // Then: a visible correct ref can remain, but the operation must report failure.
    assert!(result.is_err());
    assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
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
    let oracle_bytes = get_unused_blob(&fixture, "Git mode oracle", None);
    let oracle = fixture.get_oid(&["hash-object", "--stdin"], Some(&oracle_bytes));
    let probe_bytes = get_unused_blob(&fixture, "library mode probe", Some(&oracle[..2]));
    let expected = fixture.get_oid(&["hash-object", "--stdin"], Some(&probe_bytes));
    for oid in [&oracle, &expected] {
        let object_path = fixture.build_object_path(oid);
        assert!(!object_path.parent().unwrap().exists());
    }
    assert_eq!(
        fixture.get_oid(&["hash-object", "-w", "--stdin"], Some(&oracle_bytes)),
        oracle
    );
    let object = StagedObject::new(&repo, Kind::Blob, &probe_bytes).unwrap();
    let destination = object.destination.clone();
    let oid = object.object_get_oid();
    assert_eq!(oid.to_string(), expected);
    // When: the linked-worktree handle publishes an object and private ref.
    object.object_publish().unwrap();
    ref_retain_by_oid(&repo, oid).unwrap();
    // Then: Git sees them in the common store, with its shared permissions policy preserved.
    assert_eq!(fixture.get_direct_ref(&oid.to_string()), oid.to_string());
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
    let oid = object.object_get_oid();
    object.object_publish().unwrap();
    let ref_path = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    fs::create_dir_all(ref_path.parent().unwrap()).unwrap();
    let referent = fixture.get_root().join("foreign");
    let bytes = format!("{oid}\n");
    fs::write(&referent, &bytes).unwrap();
    std::os::unix::fs::symlink(&referent, &ref_path).unwrap();
    // When: exact direct retention encounters the symlink.
    let result = ref_retain_by_oid(&repo, oid);
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
thread_local! { static FAILURE: Cell<Option<FailurePoint>> = const { Cell::new(None) }; }

pub(super) fn inject_failure(point: FailurePoint) -> Result<(), SnapshotError> {
    if FAILURE.with(|slot| slot.get() == Some(point)) {
        return Err(crate::service::SnapshotError::new_with_cause(
            "Git durable write failed",
            crate::git::error::SafeIo(std::io::Error::other("injected I/O failure")),
        ));
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum ConcurrentLock {
    Released,
    Held,
}
thread_local! {
    static CONCURRENT_PUBLICATION: RefCell<Option<(PathBuf, ObjectId, ConcurrentLock)>> = const { RefCell::new(None) };
}

pub(super) fn publish_concurrent_ref() {
    let Some((path, oid, state)) = CONCURRENT_PUBLICATION.with(|slot| slot.borrow_mut().take())
    else {
        return;
    };
    let lock = path.with_extension("lock");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&lock, format!("{oid}\n")).unwrap();
    fs::rename(&lock, &path).unwrap();
    if matches!(state, ConcurrentLock::Held) {
        fs::write(lock, b"foreign lock CANARY").unwrap();
    }
}

fn get_unused_blob(fixture: &TempRepo, label: &str, excluded_prefix: Option<&str>) -> Vec<u8> {
    for index in 0..10_000 {
        let bytes = format!("{label}-{index}").into_bytes();
        let oid = fixture.get_oid(&["hash-object", "--stdin"], Some(&bytes));
        if Some(&oid[..2]) != excluded_prefix
            && !fixture.build_object_path(&oid).parent().unwrap().exists()
        {
            return bytes;
        }
    }
    panic!("no unused loose-object prefix available");
}

fn retain_after_contention(repo: &Repository, oid: ObjectId) -> Result<(), SnapshotError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let result = ref_retain_by_oid(repo, oid);
        if !result.as_ref().is_err_and(ref_lock_is_busy) || Instant::now() >= deadline {
            return result;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LockObservation {
    Pending,
    Observed {
        is_ref_available: bool,
        is_packed_available: bool,
    },
}
thread_local! { static LOCK_OBSERVATION: Cell<Option<LockObservation>> = const { Cell::new(None) }; }

pub(super) fn observe_confirmation_locks(repo: &Repository, name: &str) {
    if LOCK_OBSERVATION.with(Cell::get) != Some(LockObservation::Pending) {
        return;
    }
    let is_ref_available =
        try_create_probe_lock(&repo.common_dir().join(name).with_extension("lock"));
    let is_packed_available = try_create_probe_lock(&repo.common_dir().join("packed-refs.lock"));
    LOCK_OBSERVATION.with(|slot| {
        slot.set(Some(LockObservation::Observed {
            is_ref_available,
            is_packed_available,
        }));
    });
}

fn try_create_probe_lock(path: &Path) -> bool {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(file) => {
            drop(file);
            fs::remove_file(path).unwrap();
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => false,
        Err(error) => panic!("unexpected probe failure: {error}"),
    }
}

fn ref_retain_by_oid(
    repo: &gix::Repository,
    oid: gix::hash::ObjectId,
) -> Result<(), crate::service::SnapshotError> {
    retain_ref_with_cleanup(
        repo,
        oid,
        &std::sync::Arc::new(super::GitWriteCleanup::new()),
    )
}
