use super::LocalFileSystem;
use super::test_support::run_bounded_unprivileged;
use crate::service::{FileErrorKind, FileLocker, FileReader};
use std::io::ErrorKind;
use std::{error::Error, fs, os::unix::fs::PermissionsExt, path::Path};
use tempfile::TempDir;

#[test]
fn contended_lock_rejects_root_replacement_before_returning_absence() {
    // Given: an owner holds an inode under a root that will be replaced on contention.
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("root");
    fs::create_dir(&root).unwrap();
    let files = LocalFileSystem::new(&root).unwrap();
    let guard = files.lock_acquire_by_path(Path::new("owner")).unwrap();
    let saved = fixture.path().join("saved");
    let source = root.clone();
    files.on_contention.replace(Some(Box::new(move || {
        fs::rename(&source, &saved).unwrap();
        fs::create_dir(&source).unwrap();
    })));
    // When: another acquisition observes contention after the root is replaced.
    let error = files
        .lock_try_acquire_by_path(Path::new("owner"))
        .err()
        .unwrap();
    // Then: stale root ownership cannot escape as ordinary absence or alter the owner.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(!root.join("owner").exists());
    drop(guard);
}

#[test]
fn unheld_lock_observation_rejects_root_replacement_before_returning_absence() {
    // Given: an unlocked persistent leaf under a root replaced at its open boundary.
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(root.join("owner"), b"stale").unwrap();
    let files = LocalFileSystem::new(&root).unwrap();
    let saved = fixture.path().join("saved");
    let source = root.clone();
    files.before_open.replace(Some(Box::new(move || {
        fs::rename(&source, &saved).unwrap();
        fs::create_dir(&source).unwrap();
    })));
    // When: the observer opens the retained old root and finds an unheld lock.
    let error = files
        .lock_get_owner_by_path(Path::new("owner"))
        .unwrap_err();
    // Then: root replacement is an integrity error and observation leaves bytes unchanged.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert_eq!(
        fs::read(fixture.path().join("saved/owner")).unwrap(),
        b"stale"
    );
    assert!(!root.join("owner").exists());
}

#[test]
fn directory_completion_rejects_root_replacement_for_existing_paths() {
    // Given: directory traversal will replace its root immediately before completion.
    let path = ".";
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("root");
    fs::create_dir(&root).unwrap();
    let files = LocalFileSystem::new(&root).unwrap();
    let saved = fixture.path().join("saved");
    let source = root.clone();
    files
        .before_directory_complete
        .replace(Some(Box::new(move || {
            fs::rename(&source, &saved).unwrap();
            fs::create_dir(&source).unwrap();
        })));
    // When: traversal completes with either an owned descriptor or an absent parent.
    let error = files.directory_get_by_path(Path::new(path)).unwrap_err();
    // Then: both completion branches reject the replaced root without creating leaves.
    assert_eq!(error.get_kind(), FileErrorKind::Changed, "{path}");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

#[test]
fn missing_directory_completion_rejects_root_replacement() {
    // Given: directory traversal will replace its root immediately before completion.
    let path = "missing/leaf";
    let fixture = TempDir::new().unwrap();
    let root = fixture.path().join("root");
    fs::create_dir(&root).unwrap();
    let files = LocalFileSystem::new(&root).unwrap();
    let saved = fixture.path().join("saved");
    let source = root.clone();
    files
        .before_directory_complete
        .replace(Some(Box::new(move || {
            fs::rename(&source, &saved).unwrap();
            fs::create_dir(&source).unwrap();
        })));
    // When: traversal completes with either an owned descriptor or an absent parent.
    let error = files.directory_get_by_path(Path::new(path)).unwrap_err();
    // Then: both completion branches reject the replaced root without creating leaves.
    assert_eq!(error.get_kind(), FileErrorKind::Changed, "{path}");
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
}

#[test]
fn distinctness_rejects_a_newly_created_hardlink_alias() {
    // Given: the first leaf is absent while the second is an ordinary file.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("second"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    files.before_distinct_verify.replace(Some(Box::new(move || {
        fs::hard_link(path.join("second"), path.join("first")).unwrap();
    })));
    // When: the initially absent leaf becomes an alias before the verification pass.
    let error = files
        .file_verify_distinct_by_paths(Path::new("first"), Path::new("second"))
        .unwrap_err();
    // Then: permissive creation does not permit the distinctness check to accept aliases.
    assert_eq!(error.get_kind(), FileErrorKind::Aliased);
    assert_eq!(fs::read(root.path().join("second")).unwrap(), b"original");
}

#[test]
fn optional_identity_still_permits_creation_of_a_safe_regular_file() {
    // Given: a creating operation originally observed no leaf.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    assert!(
        files
            .file_get_identity_by_path(Path::new("item"))
            .unwrap()
            .is_none()
    );
    // When: a safe regular file is created without any promise of prior identity.
    fs::write(root.path().join("item"), b"created").unwrap();
    files
        .file_verify_identity_by_path(Path::new("item"), None)
        .unwrap();
    // Then: the ordinary identity API keeps its permissive creation contract.
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"created");
}

#[test]
fn captured_leaf_deleted_before_open_is_changed() {
    // Given: an observed regular leaf will be deleted at the descriptor-open boundary.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("item");
    files
        .before_open
        .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
    // When: capture cannot open the leaf it previously observed.
    let error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: deletion remains an integrity failure with its preserved OS cause.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(error.source().is_some());
    assert!(!root.path().join("item").exists());
}

#[test]
fn existing_lock_deleted_before_open_is_changed_for_acquisition() {
    // Given: an existing persistent leaf will disappear at the open boundary.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("owner"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("owner");
    files
        .before_open
        .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
    // When: this generic lock operation opens the already-observed inode.
    let error = files
        .lock_acquire_by_path(Path::new("owner"))
        .err()
        .unwrap();
    // Then: deletion is an integrity failure with its preserved cause and no recreation.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(error.source().is_some());
    assert!(!root.path().join("owner").exists());
}

#[test]
fn existing_lock_deleted_before_open_is_changed_for_nonblocking_acquisition() {
    // Given: an existing persistent leaf will disappear at the open boundary.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("owner"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("owner");
    files
        .before_open
        .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
    // When: this generic lock operation opens the already-observed inode.
    let error = files
        .lock_try_acquire_by_path(Path::new("owner"))
        .err()
        .unwrap();
    // Then: deletion is an integrity failure with its preserved cause and no recreation.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(error.source().is_some());
    assert!(!root.path().join("owner").exists());
}

#[test]
fn existing_lock_deleted_before_open_is_changed_for_observation() {
    // Given: an existing persistent leaf will disappear at the open boundary.
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("owner"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("owner");
    files
        .before_open
        .replace(Some(Box::new(move || fs::remove_file(path).unwrap())));
    // When: this generic lock operation opens the already-observed inode.
    let error = files
        .lock_get_owner_by_path(Path::new("owner"))
        .unwrap_err();
    // Then: deletion is an integrity failure with its preserved cause and no recreation.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
    assert!(error.source().is_some());
    assert!(!root.path().join("owner").exists());
}

#[test]
fn genuine_permission_failure_before_open_remains_io() {
    // Given: an unprivileged fixture observes a leaf that will become unreadable.
    if run_bounded_unprivileged(
        "filesystem::race_tests::genuine_permission_failure_before_open_remains_io",
    ) {
        return;
    }
    let root = TempDir::new().unwrap();
    fs::write(root.path().join("item"), b"original").unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("item");
    files.before_open.replace(Some(Box::new(move || {
        fs::set_permissions(&path, fs::Permissions::from_mode(0o0)).unwrap();
        let denied = fs::File::open(path).unwrap_err();
        assert_eq!(denied.kind(), ErrorKind::PermissionDenied);
        assert_eq!(denied.raw_os_error(), Some(libc::EACCES));
    })));
    // When: the actual open syscall fails because owner access was removed.
    let error = files.file_get_by_path(Path::new("item")).unwrap_err();
    // Then: permission failure preserves its I/O classification and private cause.
    assert_eq!(error.get_kind(), FileErrorKind::Io);
    assert!(error.source().is_some());
}
