use super::{
    LocalFileSystem, file_get_lock_stat, file_reopen_lock, file_retry_io, lock_finish_acquisition,
    lock_get_file_by_path, lock_retry_acquisition,
};
use crate::filesystem::test_support::{Fixture, run_bounded};
use crate::service::FileErrorKind;
use crate::service::{FileLocker, FileReader, FileWriter};
use rustix::io::Errno;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::{
    fs::{self, TryLockError},
    io::ErrorKind,
};

#[test]
fn concurrent_creation_after_absent_observation_reopens_the_same_inode() {
    for name in ["embed.lock", "index.lock"] {
        // Given: a second opener exactly after the first observed absence.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        let peer = LocalFileSystem::new(&fixture.0).unwrap();
        let path = Path::new(".letopis").join(name);
        let peer_path = path.clone();
        let (send, receive) = std::sync::mpsc::channel();
        files.before_open.replace(Some(Box::new(move || {
            let (_, _, file) = lock_get_file_by_path(&peer, &peer_path).unwrap();
            send.send(file).unwrap();
        })));
        // When: CREATE|EXCL opens the competitor's persistent inode.
        let (_, _, file) = lock_get_file_by_path(&files, &path).unwrap();
        let peer_file = receive.recv().unwrap();
        // Then: ownership identity is shared; neither opener unlinks/replaces it.
        let inode = file.metadata().unwrap().ino();
        assert_eq!(peer_file.metadata().unwrap().ino(), inode);
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
        "filesystem::lock::tests::owner_drop_unlocks_while_duplicate_descriptor_remains_open",
    ) {
        return;
    }
    // Given: a held lock with a duplicate file descriptor.
    let fixture = Fixture::new();
    let owner = LocalFileSystem::new(&fixture.0).unwrap();
    let contender = LocalFileSystem::new(&fixture.0).unwrap();
    let owner_guard = owner
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let duplicate = owner_guard.file.try_clone().unwrap();
    let inode = duplicate.metadata().unwrap().ino();
    assert!(
        contender
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_none()
    );
    // When / Then: closure of the last fd cannot make this pass.
    drop(owner_guard);
    assert!(
        contender
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
            .is_some()
    );
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
        "filesystem::lock::tests::lock_open_rejects_replacement_and_never_recreates_observed_deletion",
    ) {
        return;
    }
    for action in ["regular", "symlink", "fifo", "delete"] {
        // Given: an existing lock and a process waiting to replace or delete it.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        files
            .directory_create_by_path(std::path::Path::new(".letopis"))
            .unwrap();
        fs::write(fixture.0.join(".letopis/embed.lock"), b"old owner").unwrap();
        let mut mutator = fixture.spawn_mutator(".letopis/embed.lock", action);
        // When: the mutation runs between observation and open.
        files.before_open.replace(Some(Box::new(move || {
            mutator.resume();
            mutator.finish();
        })));
        let result = lock_get_file_by_path(&files, Path::new(".letopis/embed.lock"));
        // Then: opening fails and an observed deletion remains absent.
        assert!(result.is_err(), "{action}");
        if action == "delete" {
            assert!(!fixture.0.join(".letopis/embed.lock").exists());
        }
    }
}

#[test]
fn status_lock_open_rejects_inode_replacement() {
    // Given: an existing owner file with a replacement hook.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(std::path::Path::new(".letopis"))
        .unwrap();
    let path = fixture.0.join(".letopis/index.lock");
    fs::write(&path, b"old owner").unwrap();

    // When: observation opens after the name has changed.
    let changed_path = path.clone();
    let old_path = fixture.0.join("old-index.lock");
    files.before_open.replace(Some(Box::new(move || {
        let path = changed_path;
        fs::rename(&path, old_path).unwrap();
        fs::write(&path, b"new owner").unwrap();
    })));
    let result = files.lock_get_owner_by_path(Path::new(".letopis/index.lock"));

    // Then: the replacement is rejected and its bytes are not modified.
    assert!(result.is_err());
    assert_eq!(fs::read(&path).unwrap(), b"new owner");
}

#[test]
fn retries_interrupted_io_until_the_operation_succeeds() {
    // Given: an interrupted syscall followed by success.
    let mut results = [Err(ErrorKind::Interrupted.into()), Ok(())].into_iter();
    // When: the file retry helper executes the sequence.
    let result = file_retry_io(|| results.next().expect("unexpected extra retry"));
    // Then: only the interrupted result is retried.
    assert!(result.is_ok());
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_io_errors() {
    // Given: a permission failure with no retryable result.
    let mut results = [Err(ErrorKind::PermissionDenied.into())].into_iter();
    // When: the file retry helper runs.
    let error =
        file_retry_io(|| results.next().expect("nonretryable error was retried")).unwrap_err();
    // Then: the original permission failure returns without another call.
    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}

#[test]
fn rejects_each_prior_lock_identity_mismatch_before_writing_pid() {
    for change_inode in [true, false] {
        // Given: hold a real lock, then change exactly one prior observation field.
        let fixture = Fixture::new();
        let files = LocalFileSystem::new(&fixture.0).unwrap();
        let (parent, name, file) =
            lock_get_file_by_path(&files, Path::new(".letopis/embed.lock")).unwrap();
        let mut before = file_get_lock_stat(&file).unwrap();
        if change_inode {
            let (_, _, other) =
                lock_get_file_by_path(&files, Path::new(".letopis/index.lock")).unwrap();
            before.st_ino = file_get_lock_stat(&other).unwrap().st_ino;
        } else {
            before.st_dev ^= 1;
        }
        let path = fixture.0.join(".letopis/embed.lock");
        fs::write(&path, b"previous owner").unwrap();
        file.try_lock().unwrap();
        let duplicate = file.try_clone().unwrap();
        // When: lock completion compares the prior and current file identities.
        let result = lock_finish_acquisition(&files, &parent, &name, file, before);
        // Then: failed completion releases ownership even with another fd open.
        let error = result.expect_err("identity mismatch was accepted");
        assert_eq!(error.get_kind(), FileErrorKind::Changed);
        assert_eq!(fs::read(&path).unwrap(), b"previous owner");
        let contender = LocalFileSystem::new(&fixture.0).unwrap();
        assert!(
            contender
                .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
                .unwrap()
                .is_some()
        );
        drop(duplicate);
    }
}

#[test]
fn retries_interrupted_lock_acquisition_until_success() {
    // Given: an interrupted lock syscall followed by success.
    let mut results = [
        Err(TryLockError::Error(ErrorKind::Interrupted.into())),
        Ok(()),
    ]
    .into_iter();
    // When: the lock retry helper executes the sequence.
    let acquired =
        lock_retry_acquisition(|| results.next().expect("unexpected extra retry")).unwrap();
    // Then: acquisition succeeds after exactly one retry.
    assert!(acquired);
    assert!(results.next().is_none());
}

#[test]
fn returns_contention_without_retrying() {
    // Given: a nonblocking lock syscall reports contention.
    let mut results = [Err(TryLockError::WouldBlock)].into_iter();
    // When: the lock helper inspects that outcome.
    let acquired =
        lock_retry_acquisition(|| results.next().expect("contention was retried")).unwrap();
    // Then: contention returns false without retrying.
    assert!(!acquired);
    assert!(results.next().is_none());
}

#[test]
fn preserves_nonretryable_lock_errors() {
    // Given: a permission failure from the lock syscall.
    let mut results = [Err(TryLockError::Error(ErrorKind::PermissionDenied.into()))].into_iter();
    // When: the lock retry helper runs.
    let error =
        lock_retry_acquisition(|| results.next().expect("nonretryable lock error was retried"))
            .unwrap_err();
    // Then: the original failure returns without another attempt.
    let TryLockError::Error(cause) = error else {
        panic!("expected the original nonretryable I/O error");
    };
    assert_eq!(cause.kind(), ErrorKind::PermissionDenied);
    assert!(results.next().is_none());
}

#[test]
fn missing_creation_reopens_existing_lock_without_replacing_it() {
    for name in ["index.lock", "embed.lock"] {
        // Given: a competitor created the lock file before the current creation attempt.
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join(name);
        fs::write(&path, b"persistent owner state").unwrap();
        let directory = fs::File::open(root.path()).unwrap();
        let before = fs::metadata(&path).unwrap().ino();
        // When: select exactly the ENOENT creation-error path.
        let reopened =
            file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::NOENT)).unwrap();
        // Then: identity and bytes remain intact, and unrelated errors do not trigger fallback.
        assert_eq!(rustix::fs::fstat(&reopened).unwrap().st_ino, before);
        assert_eq!(fs::read(&path).unwrap(), b"persistent owner state");
        assert_eq!(
            file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::ACCESS))
                .unwrap_err(),
            Errno::ACCESS
        );
    }
}

#[test]
fn missing_creation_reopen_rejects_absent_and_unsafe_lock_names() {
    for name in ["index.lock", "embed.lock"] {
        for kind in ["absent", "symlink", "fifo", "directory"] {
            // Given: an absent or unsafe name under the same live directory descriptor.
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join(name);
            let directory = fs::File::open(root.path()).unwrap();
            let outside = tempfile::tempdir().unwrap();
            let target = outside.path().join("untouched");
            fs::write(&target, b"outside").unwrap();
            match kind {
                "symlink" => std::os::unix::fs::symlink(&target, &path).unwrap(),
                "fifo" => assert!(
                    std::process::Command::new("mkfifo")
                        .arg(&path)
                        .status()
                        .unwrap()
                        .success()
                ),
                "directory" => fs::create_dir(&path).unwrap(),
                _ => {}
            }
            // When: the fallback may open a FIFO, but the regular-file check rejects it.
            let result =
                file_reopen_lock(&directory, std::ffi::OsStr::new(name), Err(Errno::NOENT));
            // Then: no file is created, symlink followed, or nonregular file accepted.
            if let Ok(file) = result {
                assert!(file_get_lock_stat(&std::fs::File::from(file)).is_err());
            }
            if kind == "absent" {
                assert!(!path.exists());
            }
            assert_eq!(fs::read(&target).unwrap(), b"outside");
        }
    }
}
