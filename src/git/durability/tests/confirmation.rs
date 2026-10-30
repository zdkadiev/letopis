use super::{CONCURRENT_PUBLICATION, ConcurrentLock, FAILURE, FORMATS, FailurePoint, TempRepo};
use super::{LOCK_OBSERVATION, LockObservation};
use crate::git::durability::{StagedObject, is_ref_lock_busy, retain_ref};
use gix::{Repository, hash::ObjectId, objs::Kind};
use std::{cell::Cell, fs};

#[test]
fn retry_of_visible_ref_must_complete_sync_before_success() {
    // Given: a ref whose rename succeeded but directory synchronization failed.
    let (fixture, repo, oid) = prepare_repo();
    FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
    let first = retain_ref(&repo, oid);
    // When: a retry sees that value while the same sync failure remains active.
    let retry = retain_ref(&repo, oid);
    FAILURE.with(|slot| slot.set(None));
    // Then: neither visibility nor retry bypasses the failure; recovery can finish durability.
    assert!(first.is_err());
    assert!(retry.is_err());
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
    retain_ref(&repo, oid).unwrap();
}

#[test]
fn concurrent_confirmation_cannot_bypass_sync_or_steal_a_lock() {
    // Given: a concurrent publisher renames immediately before this writer's lock attempt.
    for state in [ConcurrentLock::Released, ConcurrentLock::Held] {
        let (fixture, repo, oid) = prepare_repo();
        let path = repo
            .common_dir()
            .join(format!("refs/letopis/snapshots/{oid}"));
        CONCURRENT_PUBLICATION.with(|slot| *slot.borrow_mut() = Some((path.clone(), oid, state)));
        FAILURE.with(|slot| slot.set(Some(FailurePoint::RefDirectorySync)));
        // When: exact bytes become visible through either acquisition outcome.
        let result = retain_ref(&repo, oid);
        FAILURE.with(|slot| slot.set(None));
        // Then: confirmation fails until synchronization is possible and never alters a foreign lock.
        assert!(result.is_err());
        assert_eq!(
            is_ref_lock_busy(result.as_ref().unwrap_err()),
            matches!(state, ConcurrentLock::Held)
        );
        if matches!(state, ConcurrentLock::Held) {
            assert_eq!(
                fs::read(path.with_extension("lock")).unwrap(),
                b"foreign lock CANARY"
            );
            fs::remove_file(path.with_extension("lock")).unwrap();
        }
        retain_ref(&repo, oid).unwrap();
        assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
    }
}

#[test]
fn packed_confirmation_requires_backing_file_and_parent_sync() {
    // Given: an exact packed-only ref without a loose backing file.
    let (fixture, repo, oid) = prepare_repo();
    retain_ref(&repo, oid).unwrap();
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    let loose = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    assert!(!loose.exists());
    // When: file or parent synchronization is unavailable for a matching packed value.
    for point in [FailurePoint::RefSync, FailurePoint::RefDirectorySync] {
        FAILURE.with(|slot| slot.set(Some(point)));
        let result = retain_ref(&repo, oid);
        FAILURE.with(|slot| slot.set(None));
        // Then: packed visibility alone cannot acknowledge durability.
        assert!(result.is_err(), "{point:?}");
        assert!(!is_ref_lock_busy(result.as_ref().unwrap_err()));
    }
    retain_ref(&repo, oid).unwrap();
    assert!(!loose.exists());
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
}

#[test]
fn packed_confirmation_does_not_touch_a_foreign_packing_lock() {
    // Given: a packed ref with another packer's lock still held.
    let (fixture, repo, oid) = prepare_repo();
    retain_ref(&repo, oid).unwrap();
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    let lock = repo.common_dir().join("packed-refs.lock");
    fs::write(&lock, b"foreign packer CANARY").unwrap();
    // When: confirmation cannot coordinate the backing file's lifetime.
    let result = retain_ref(&repo, oid);
    // Then: it reports contention, preserving the foreign lock until its owner releases it.
    assert!(is_ref_lock_busy(&result.unwrap_err()));
    assert_eq!(fs::read(&lock).unwrap(), b"foreign packer CANARY");
    fs::remove_file(lock).unwrap();
    retain_ref(&repo, oid).unwrap();
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
    retain_ref(&repo, oid).unwrap();
    // Then: bytes remain exact and this isolated scenario also supplies the syscall oracle.
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
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
    retain_ref(&repo, oid).unwrap();
    // Then: its exact target remains packed; the syscall oracle observes the backing sync.
    assert!(!repo.common_dir().join(&name).exists());
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
}

#[test]
fn concurrent_confirmation_syncs_the_visible_winner_before_success() {
    // Given: another writer's rename becomes visible immediately before acquisition.
    let (fixture, repo, oid) = prepare_repo();
    let path = repo
        .common_dir()
        .join(format!("refs/letopis/snapshots/{oid}"));
    CONCURRENT_PUBLICATION
        .with(|slot| *slot.borrow_mut() = Some((path.clone(), oid, ConcurrentLock::Released)));
    // When: confirmation owns the newly released lock and completes the winner's sync.
    retain_ref(&repo, oid).unwrap();
    // Then: success has the exact value; the syscall oracle requires a post-rename backing sync.
    assert_eq!(fixture.read_direct_ref(&oid.to_string()), oid.to_string());
    assert!(!path.with_extension("lock").exists());
}

#[test]
fn packed_confirmation_holds_both_locks_during_backing_sync() {
    // Given: a packed-only target and an independent create-new lock probe.
    let (fixture, repo, oid) = prepare_repo();
    retain_ref(&repo, oid).unwrap();
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    LOCK_OBSERVATION.with(|slot| slot.set(Some(LockObservation::Pending)));
    // When: confirmation reaches its backing-file sync boundary.
    retain_ref(&repo, oid).unwrap();
    let observed = LOCK_OBSERVATION.with(Cell::take);
    // Then: neither a ref writer nor a packer can acquire its lock during confirmation.
    assert_eq!(
        observed,
        Some(LockObservation::Observed {
            ref_available: false,
            packed_available: false
        })
    );
}

#[test]
fn packed_confirmation_rejects_a_symlink_backing_instead_of_syncing_the_wrong_parent() {
    // Given: a packed-ref symlink whose actual file resides outside the ref-store directory.
    let (fixture, repo, oid) = prepare_repo();
    retain_ref(&repo, oid).unwrap();
    fixture.run_git_successfully(&["pack-refs", "--all", "--prune"], None);
    let packed = repo.common_dir().join("packed-refs");
    let external = fixture.get_root().join("external-packed-refs");
    fs::rename(&packed, &external).unwrap();
    std::os::unix::fs::symlink(&external, &packed).unwrap();
    // When: the chosen backing cannot be opened without following that alias.
    let result = retain_ref(&repo, oid);
    // Then: the operation fails without acknowledging an unsynchronized external parent.
    assert!(result.is_err());
    assert!(!is_ref_lock_busy(result.as_ref().unwrap_err()));
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
    let oid = object.get_oid();
    object.publish().unwrap();
    (fixture, repo, oid)
}
