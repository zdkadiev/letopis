use super::LocalFileSystem;
use crate::service::{FileErrorKind, FileLocker, FileReader};
use std::{fs, path::Path};
use tempfile::TempDir;

#[test]
fn lock_observation_is_read_only_and_ownership_persists_until_drop() {
    // Given: an absent arbitrary lock below an absent parent.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let observer: &dyn FileReader = &files;
    assert!(
        observer
            .lock_get_owner_by_path(Path::new("private/owner"))
            .unwrap()
            .is_none()
    );
    assert!(!root.path().join("private").exists());
    // When: acquisition creates the lock and a second acquisition contends.
    let guard = files
        .lock_acquire_by_path(Path::new("private/owner"))
        .unwrap();
    let inode = fs::metadata(root.path().join("private/owner")).unwrap();
    assert!(
        files
            .lock_try_acquire_by_path(Path::new("private/owner"))
            .unwrap()
            .is_none()
    );
    // Then: observation returns the real PID and dropping releases the same inode.
    assert_eq!(
        observer
            .lock_get_owner_by_path(Path::new("private/owner"))
            .unwrap()
            .unwrap()
            .get(),
        std::process::id()
    );
    drop(guard);
    assert!(
        observer
            .lock_get_owner_by_path(Path::new("private/owner"))
            .unwrap()
            .is_none()
    );
    assert_eq!(
        rustix::fs::stat(root.path().join("private/owner"))
            .unwrap()
            .st_ino,
        std::os::unix::fs::MetadataExt::ino(&inode)
    );
}

#[test]
fn lock_open_rejects_replacement_before_writing_owner_bytes() {
    // Given: an existing lock and a hook replacing it after initial observation.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("owner"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.before_open.replace(Some(Box::new(move || {
        fs::rename(path.join("owner"), path.join("saved")).unwrap();
        fs::write(path.join("owner"), b"replacement").unwrap();
    })));
    // When: generic lock acquisition crosses the replacement boundary.
    let error = files
        .lock_acquire_by_path(Path::new("owner"))
        .err()
        .unwrap();
    // Then: identity mismatch fails without changing either file's bytes.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert_eq!(fs::read(root.path().join("owner")).unwrap(), b"replacement");
    assert_eq!(fs::read(root.path().join("saved")).unwrap(), b"original");
}

#[test]
fn lock_parent_identity_remains_pinned_after_owner_drop() {
    // Given: generic ownership has created and observed an arbitrary parent.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    drop(
        files
            .lock_acquire_by_path(Path::new("private/owner"))
            .unwrap(),
    );
    // When: the parent is replaced after ownership is dropped.
    fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    // Then: neither another lock nor an observational read accepts the replacement.
    assert_eq!(
        files
            .lock_try_acquire_by_path(Path::new("private/owner"))
            .err()
            .unwrap()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert_eq!(
        files
            .lock_get_owner_by_path(Path::new("private/owner"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
    assert!(!root.path().join("private/owner").exists());
}

#[test]
fn held_lock_rejects_invalid_bounded_pid_records_without_writing_them() {
    // Given: an externally held lock with deliberately malformed owner records.
    let root = TempDir::new().unwrap();
    let path = root.path().join("owner");
    let file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .read(true)
        .write(true)
        .open(&path)
        .unwrap();
    file.lock().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    for record in [
        b"".as_slice(),
        b"0",
        b"-1",
        b"1x",
        b"4294967296",
        b"123456789012",
        b"1\n\n",
        b"\xff",
    ] {
        fs::write(&path, record).unwrap();
        // When: a read-only observer parses the bounded owner metadata.
        let error = files
            .lock_get_owner_by_path(Path::new("owner"))
            .unwrap_err();
        // Then: malformed records fail with a precise kind and remain unchanged.
        assert_eq!(error.get_kind(), FileErrorKind::InvalidOwner);
        assert_eq!(fs::read(&path).unwrap(), record);
    }
}

#[test]
fn generic_locks_reject_hardlinks_and_preserve_external_bytes() {
    // Given: a lock path aliases another ordinary file.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("external"), b"untouched").unwrap();
    fs::hard_link(root.path().join("external"), root.path().join("owner")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: both lock acquisition and observation reject aliased lock files.
    assert_eq!(
        files
            .lock_try_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap()
            .get_kind(),
        FileErrorKind::Aliased
    );
    assert_eq!(
        files
            .lock_get_owner_by_path(Path::new("owner"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Aliased
    );
    assert_eq!(
        fs::read(root.path().join("external")).unwrap(),
        b"untouched"
    );
}

#[test]
fn concurrent_first_lock_creation_has_one_owner_and_one_persistent_inode() {
    // Given: six independent components race for an absent arbitrary lock.
    let root = TempDir::new().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(6));
    let workers: Vec<_> = (0..6)
        .map(|_| {
            let path = root.path().to_owned();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let files = LocalFileSystem::new(&path).unwrap();
                barrier.wait();
                files
                    .lock_try_acquire_by_path(Path::new("private/owner"))
                    .unwrap()
            })
        })
        .collect();
    // When: successful guards stay alive through every attempt.
    let guards: Vec<_> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    // Then: only one actual owner exists and dropping it preserves the lock inode.
    assert_eq!(guards.iter().filter(|guard| guard.is_some()).count(), 1);
    let inode = rustix::fs::stat(root.path().join("private/owner"))
        .unwrap()
        .st_ino;
    drop(guards);
    assert_eq!(
        rustix::fs::stat(root.path().join("private/owner"))
            .unwrap()
            .st_ino,
        inode
    );
}

#[test]
fn generic_owner_drop_unlocks_before_its_last_duplicate_closes() {
    // Given: a held generic lock has a duplicate descriptor that outlives its guard.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let guard = files.lock_acquire_by_path(Path::new("owner")).unwrap();
    let duplicate = guard.file.try_clone().unwrap();
    // When: ownership ends while that duplicate remains open.
    drop(guard);
    let next = files.lock_try_acquire_by_path(Path::new("owner")).unwrap();
    // Then: the next independently opened descriptor can acquire the same inode.
    assert!(next.is_some());
    assert_eq!(
        rustix::fs::fstat(duplicate).unwrap().st_ino,
        rustix::fs::stat(root.path().join("owner")).unwrap().st_ino
    );
}
