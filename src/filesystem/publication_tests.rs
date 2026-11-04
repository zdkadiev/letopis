use super::LocalFileSystem;
use crate::service::{FileErrorKind, FileWriter};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
use tempfile::TempDir;

#[test]
fn no_clobber_publication_preserves_winner_and_private_modes() {
    // Given: a writable root and arbitrary caller-selected paths.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    files
        .directory_create_by_path(Path::new("private"))
        .unwrap();
    // When: two complete values are offered for the same file.
    files
        .file_create_if_absent_by_path(Path::new("private/item"), b"winner")
        .unwrap();
    files
        .file_create_if_absent_by_path(Path::new("private/item"), b"loser")
        .unwrap();
    files.directory_sync_by_path(Path::new("private")).unwrap();
    // Then: the first bytes survive, private modes hold and no temporary entries remain.
    assert_eq!(
        fs::read(root.path().join("private/item")).unwrap(),
        b"winner"
    );
    assert_eq!(
        fs::metadata(root.path().join("private"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(root.path().join("private/item"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::read_dir(root.path().join("private")).unwrap().count(),
        1
    );
}

#[test]
fn file_sync_failure_preserves_absence_and_retry_publishes_complete_bytes() {
    use super::publication::{SyncTarget, file_fail_next_sync};
    // Given: an absent destination and an injected file-data sync failure.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    file_fail_next_sync(SyncTarget::File);
    // When: durable publication fails before linking the destination.
    let error = files
        .file_create_if_absent_by_path(Path::new("item"), b"complete")
        .unwrap_err();
    // Then: no destination or temporary file survives; a clean retry succeeds.
    assert_eq!(error.get_kind(), FileErrorKind::Io);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    files
        .file_create_if_absent_by_path(Path::new("item"), b"complete")
        .unwrap();
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"complete");
}

#[test]
fn directory_sync_failure_preserves_published_winner_for_retry() {
    use super::publication::{SyncTarget, file_fail_next_sync};
    // Given: an absent destination and an injected post-publication directory failure.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    file_fail_next_sync(SyncTarget::Directory);
    // When: bytes publish but directory durability cannot be confirmed.
    assert_eq!(
        files
            .file_create_if_absent_by_path(Path::new("item"), b"winner")
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Io
    );
    // Then: retry syncs the first complete value without overwriting or leaking temporaries.
    files
        .file_create_if_absent_by_path(Path::new("item"), b"loser")
        .unwrap();
    assert_eq!(fs::read(root.path().join("item")).unwrap(), b"winner");
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn concurrent_publications_preserve_one_complete_winner() {
    // Given: two independent components publish different complete byte sequences.
    let root = TempDir::new().unwrap();
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let workers: Vec<_> = [b'A', b'B']
        .into_iter()
        .map(|byte| {
            let path = root.path().to_owned();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                let files = LocalFileSystem::new(&path).unwrap();
                barrier.wait();
                files
                    .file_create_if_absent_by_path(Path::new("item"), &vec![byte; 4096])
                    .unwrap();
            })
        })
        .collect();
    // When: both publication protocols race for the same absent destination.
    for worker in workers {
        worker.join().unwrap();
    }
    // Then: one complete value survives with no temporary entries.
    let bytes = fs::read(root.path().join("item")).unwrap();
    assert!(bytes == vec![b'A'; 4096] || bytes == vec![b'B'; 4096]);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

#[test]
fn unsafe_publication_leaves_and_missing_parents_are_preserved() {
    // Given: caller-selected directory and symlink destinations plus an absent parent.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("folder")).unwrap();
    fs::write(root.path().join("external"), b"untouched").unwrap();
    symlink("external", root.path().join("link")).unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    // When / Then: publication rejects unsupported paths without replacing anything.
    for path in ["folder", "link"] {
        assert_eq!(
            files
                .file_create_if_absent_by_path(Path::new(path), b"loser")
                .unwrap_err()
                .get_kind(),
            FileErrorKind::NonRegular
        );
    }
    assert_eq!(
        files
            .file_create_if_absent_by_path(Path::new("missing/item"), b"loser")
            .unwrap_err()
            .get_kind(),
        FileErrorKind::NotFound
    );
    assert_eq!(
        fs::read(root.path().join("external")).unwrap(),
        b"untouched"
    );
    assert!(!root.path().join("missing").exists());
}
