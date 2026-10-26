use super::{LocalFileSystem, observe_lock_descriptor, retry_io, retry_lock};
use crate::filesystem::test_support::{Fixture, run_bounded};
use crate::service::{DocumentErrorKind, EmbeddingFiles};
use std::os::unix::fs::MetadataExt;
use std::{
    fs::{self, TryLockError},
    io::ErrorKind,
};

#[test]
fn concurrent_creation_after_absent_observation_reopens_the_same_inode() {
    for name in ["embed.lock", "index.lock"] {
        // Arrange a second opener exactly after the first observed absence.
        let fixture = Fixture::new();
        let files = LocalFileSystem::open(&fixture.0).unwrap();
        let peer = LocalFileSystem::open(&fixture.0).unwrap();
        let mut peer_file = None;
        // Act: CREATE|EXCL must accept the peer's safe persistent file.
        let file = files
            .open_lock_with(name, || {
                peer_file = Some(peer.open_lock_file(name).unwrap());
            })
            .unwrap();
        // Assert ownership identity is shared; neither opener unlinks/replaces it.
        let inode = file.metadata().unwrap().ino();
        assert_eq!(peer_file.unwrap().metadata().unwrap().ino(), inode);
        assert_eq!(
            fs::metadata(fixture.0.join(".letopis").join(name))
                .unwrap()
                .ino(),
            inode
        );
    }
}

#[test]
fn owner_drop_unlocks_while_duplicate_descriptor_remains_open() {
    if run_bounded(
        "filesystem::local::embedding_tests::owner_drop_unlocks_while_duplicate_descriptor_remains_open",
    ) {
        return;
    }
    // Arrange
    let fixture = Fixture::new();
    let owner = LocalFileSystem::open(&fixture.0).unwrap();
    let contender = LocalFileSystem::open(&fixture.0).unwrap();
    let owner_guard = owner.try_lock_embed().unwrap().unwrap();
    let duplicate = owner_guard.file.try_clone().unwrap();
    let inode = duplicate.metadata().unwrap().ino();
    assert!(contender.try_lock_embed().unwrap().is_none());
    // Act / Assert: closure of the last fd cannot make this pass.
    drop(owner_guard);
    assert!(contender.try_lock_embed().unwrap().is_some());
    assert_eq!(duplicate.metadata().unwrap().ino(), inode);
    assert_eq!(
        fs::metadata(fixture.0.join(".letopis/embed.lock"))
            .unwrap()
            .ino(),
        inode
    );
}

#[test]
fn lock_open_rejects_replacement_and_never_recreates_observed_deletion() {
    if run_bounded(
        "filesystem::local::embedding_tests::lock_open_rejects_replacement_and_never_recreates_observed_deletion",
    ) {
        return;
    }
    for action in ["regular", "symlink", "fifo", "delete"] {
        // Arrange
        let fixture = Fixture::new();
        let files = LocalFileSystem::open(&fixture.0).unwrap();
        files.open_service_directory().unwrap();
        fs::write(fixture.0.join(".letopis/embed.lock"), b"old owner").unwrap();
        let mut mutator = fixture.spawn_mutator(".letopis/embed.lock", action);
        // Act
        let result = files.open_lock_with("embed.lock", || mutator.resume());
        mutator.finish();
        // Assert
        assert!(result.is_err(), "{action}");
        if action == "delete" {
            assert!(!fixture.0.join(".letopis/embed.lock").exists());
        }
    }
}

#[test]
fn status_lock_open_rejects_inode_replacement() {
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.open_service_directory().unwrap();
    let path = fixture.0.join(".letopis/index.lock");
    fs::write(&path, b"old owner").unwrap();

    let result = files.open_existing_lock_with("index.lock", || {
        fs::rename(&path, fixture.0.join("old-index.lock")).unwrap();
        fs::write(&path, b"new owner").unwrap();
    });

    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"new owner");
}

#[test]
fn retries_interrupted_io_until_the_operation_succeeds() {
    // Arrange
    let mut results = [Err(ErrorKind::Interrupted.into()), Ok(())].into_iter();
    // Act
    let result = retry_io(|| results.next().expect("unexpected extra retry"));
    // Assert
    assert!(result.is_ok());
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_io_errors() {
    // Arrange
    let mut results = [Err(ErrorKind::PermissionDenied.into())].into_iter();
    // Act
    let error = retry_io(|| results.next().expect("nonretryable error was retried")).unwrap_err();
    // Assert
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}

#[test]
fn rejects_each_prior_lock_identity_mismatch_before_writing_pid() {
    for change_inode in [true, false] {
        // Arrange: hold a real lock, then change exactly one prior observation field.
        let fixture = Fixture::new();
        let files = LocalFileSystem::open(&fixture.0).unwrap();
        let file = files.open_lock_file("embed.lock").unwrap();
        let mut before = observe_lock_descriptor(&file).unwrap();
        if change_inode {
            let other = files.open_lock_file("index.lock").unwrap();
            before.inode = observe_lock_descriptor(&other).unwrap().inode;
        } else {
            before.device ^= 1;
        }
        let path = fixture.0.join(".letopis/embed.lock");
        fs::write(&path, b"previous owner").unwrap();
        file.try_lock().unwrap();
        let duplicate = file.try_clone().unwrap();
        // Act
        let result = files.finish_lock(file, "embed.lock", before);
        // Assert: failed completion releases ownership even with another fd open.
        let error = result.err().expect("identity mismatch was accepted");
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert_eq!(fs::read(&path).unwrap(), b"previous owner");
        let contender = LocalFileSystem::open(&fixture.0).unwrap();
        assert!(contender.try_lock_embed().unwrap().is_some());
        drop(duplicate);
    }
}

#[test]
fn retries_interrupted_lock_acquisition_until_success() {
    // Arrange
    let mut results = [
        Err(TryLockError::Error(ErrorKind::Interrupted.into())),
        Ok(()),
    ]
    .into_iter();
    // Act
    let acquired = retry_lock(|| results.next().expect("unexpected extra retry")).unwrap();
    // Assert
    assert!(acquired);
    assert!(results.next().is_none());
}

#[test]
fn returns_contention_without_retrying() {
    // Arrange
    let mut results = [Err(TryLockError::WouldBlock)].into_iter();
    // Act
    let acquired = retry_lock(|| results.next().expect("contention was retried")).unwrap();
    // Assert
    assert!(!acquired);
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_lock_errors() {
    // Arrange
    let mut results = [Err(TryLockError::Error(ErrorKind::PermissionDenied.into()))].into_iter();
    // Act
    let error =
        retry_lock(|| results.next().expect("nonretryable lock error was retried")).unwrap_err();
    // Assert
    let TryLockError::Error(cause) = error else {
        panic!("expected the original nonretryable I/O error");
    };
    assert_eq!(cause.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}
