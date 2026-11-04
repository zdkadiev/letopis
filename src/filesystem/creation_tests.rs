use super::{
    LocalFileSystem,
    path::{directory_set_before_nested_preflight, file_set_after_absence},
    test_support::{create_fifo, run_bounded},
};
use crate::service::{FileError, FileErrorKind, FileLocker, FileReader, FileWriter};
use rustix::fs::fstat;
use std::{
    fs::{self, File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, symlink},
    path::Path,
    sync::mpsc,
};
use tempfile::TempDir;

#[derive(Clone, Copy, Debug)]
enum UnsafeEntry {
    Symlink,
    Fifo,
    Hardlink,
    Directory,
    Regular,
}
#[derive(Clone, Copy, Debug)]
enum Observation {
    Bytes,
    Stat,
    Identity,
    Exists,
    LockOwner,
}

#[test]
fn held_peer_publication_after_first_absence_preserves_inode_and_owner() {
    for name in ["index.lock", "embed.lock"] {
        // Given: a real peer acquires the initially absent leaf before confirmation.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("private")).unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = Path::new("private").join(name);
        let peer_path = path.clone();
        let peer_root = root.path().to_owned();
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            let peer = LocalFileSystem::new(&peer_root).unwrap();
            let guard = peer.lock_acquire_by_path(&peer_path).unwrap();
            fs::write(peer_root.join(&peer_path), b"123456\n").unwrap();
            send.send(guard).unwrap();
        }));
        // When: the contender resolves publication against the actual kernel owner.
        let attempt = files.lock_try_acquire_by_path(&path).unwrap();
        let peer = receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: contention preserves the peer inode/PID, and later ownership reuses it.
        assert!(attempt.is_none());
        let inode = peer.file.metadata().unwrap().ino();
        assert_eq!(fs::metadata(root.path().join(&path)).unwrap().ino(), inode);
        assert_eq!(fs::read(root.path().join(&path)).unwrap(), b"123456\n");
        drop(peer);
        let guard = files.lock_try_acquire_by_path(&path).unwrap().unwrap();
        assert_eq!(guard.file.metadata().unwrap().ino(), inode);
    }
}

#[test]
fn unheld_peer_publication_after_first_absence_is_acquired_without_replacement() {
    for name in ["index.lock", "embed.lock"] {
        // Given: a peer publishes a regular unlocked inode at the first absence boundary.
        let root = TempDir::new().unwrap();
        fs::create_dir(root.path().join("private")).unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = Path::new("private").join(name);
        let peer_path = root.path().join(&path);
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(peer_path)
                .unwrap();
            send.send(file).unwrap();
        }));
        // When: a blocking acquisition continues after the peer's first publication.
        let guard = files.lock_acquire_by_path(&path).unwrap();
        let peer = receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: no new inode replaces the published leaf and only ownership records PID.
        assert_eq!(
            guard.file.metadata().unwrap().ino(),
            peer.metadata().unwrap().ino()
        );
        assert_eq!(
            fs::read(root.path().join(&path)).unwrap(),
            format!("{}\n", std::process::id()).as_bytes()
        );
    }
}

#[test]
fn first_directory_peer_publication_is_adopted_and_pinned() {
    // Given: a peer creates a safe directory at the first absence boundary.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("private");
    let (send, receive) = mpsc::channel();
    file_set_after_absence(Box::new(move || {
        fs::create_dir(&path).unwrap();
        send.send(File::open(path).unwrap()).unwrap();
    }));
    // When: creation resolves the peer's publication and pins the resulting directory.
    let directory = files
        .directory_create_by_path(Path::new("private"))
        .unwrap();
    let peer = receive
        .try_recv()
        .expect("peer publication hook did not run");
    // Then: the exact published inode is retained.
    assert_eq!(
        fstat(directory).unwrap().st_ino,
        peer.metadata().unwrap().ino()
    );
    // When: a new directory later replaces the now-pinned identity.
    fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
    fs::create_dir(root.path().join("private")).unwrap();
    // Then: creation cannot silently adopt another inode after pinning.
    assert_eq!(
        files
            .directory_create_by_path(Path::new("private"))
            .unwrap_err()
            .get_kind(),
        FileErrorKind::Changed
    );
}

#[test]
fn unsafe_lock_peer_publications_fail_before_mutating_external_bytes() {
    if run_bounded(
        "filesystem::creation_tests::unsafe_lock_peer_publications_fail_before_mutating_external_bytes",
    ) {
        return;
    }
    for entry in [
        UnsafeEntry::Symlink,
        UnsafeEntry::Fifo,
        UnsafeEntry::Hardlink,
        UnsafeEntry::Directory,
    ] {
        // Given: a peer publishes an unsafe leaf after the first actual ENOENT.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("external"), b"untouched").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            publish_entry(&path, "owner", entry);
            send.send(()).unwrap();
        }));
        // When: acquisition handles the peer's unsafe first publication.
        let error = files
            .lock_try_acquire_by_path(Path::new("owner"))
            .err()
            .unwrap();
        receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: strict type/alias/no-follow checks fail and preserve external bytes.
        let expected = match entry {
            UnsafeEntry::Symlink => FileErrorKind::UnsafePath,
            UnsafeEntry::Fifo => FileErrorKind::NonRegular,
            UnsafeEntry::Hardlink => FileErrorKind::Aliased,
            UnsafeEntry::Directory => FileErrorKind::Io,
            UnsafeEntry::Regular => unreachable!(),
        };
        assert_eq!(error.get_kind(), expected, "{entry:?}");
        assert_eq!(
            fs::read(root.path().join("external")).unwrap(),
            b"untouched"
        );
    }
}

#[test]
fn unsafe_directory_peer_publications_are_not_adopted() {
    if run_bounded("filesystem::creation_tests::unsafe_directory_peer_publications_are_not_adopted")
    {
        return;
    }
    for entry in [
        UnsafeEntry::Symlink,
        UnsafeEntry::Fifo,
        UnsafeEntry::Regular,
    ] {
        // Given: an unsafe entry occupies the proposed directory after first absence.
        let root = TempDir::new().unwrap();
        fs::write(root.path().join("external"), b"untouched").unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().to_owned();
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            publish_entry(&path, "private", entry);
            send.send(()).unwrap();
        }));
        // When: directory creation encounters the peer publication.
        let error = files
            .directory_create_by_path(Path::new("private"))
            .unwrap_err();
        receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: creation fails closed without following or rewriting the entry.
        assert_eq!(error.get_kind(), FileErrorKind::UnsafePath, "{entry:?}");
        assert_eq!(
            fs::read(root.path().join("external")).unwrap(),
            b"untouched"
        );
    }
}

#[test]
fn ordinary_file_observations_keep_strict_absence_confirmation() {
    for operation in [
        Observation::Bytes,
        Observation::Stat,
        Observation::Identity,
        Observation::Exists,
        Observation::LockOwner,
    ] {
        // Given: a regular entry appears during a non-creating absence observation.
        let root = TempDir::new().unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        let path = root.path().join("item");
        file_set_after_absence(Box::new(move || fs::write(path, b"peer").unwrap()));
        // When: an ordinary reader, identity/presence query or lock observer runs.
        let error = get_observation_error(&files, operation);
        // Then: appearance remains Changed rather than a silently adopted observation.
        assert_eq!(error.get_kind(), FileErrorKind::Changed, "{operation:?}");
        assert_eq!(fs::read(root.path().join("item")).unwrap(), b"peer");
    }
}

#[test]
fn ordinary_directory_observation_keeps_strict_absence_confirmation() {
    // Given: a directory appears while a read-only directory query confirms absence.
    let root = TempDir::new().unwrap();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().join("private");
    file_set_after_absence(Box::new(move || fs::create_dir(path).unwrap()));
    // When: the non-creating directory observation runs.
    let error = files
        .directory_get_by_path(Path::new("private"))
        .unwrap_err();
    // Then: ordinary observation keeps the strict Changed classification.
    assert_eq!(error.get_kind(), FileErrorKind::Changed);
}

#[test]
fn pinned_directory_disappearance_and_reappearance_remain_changed() {
    for pinned in ["private", "private/child"] {
        // Given: a pin or pinned descendant exists before its ancestor disappears.
        let root = TempDir::new().unwrap();
        fs::create_dir_all(root.path().join(pinned)).unwrap();
        let files = LocalFileSystem::new(root.path()).unwrap();
        files
            .directory_get_by_path(Path::new(pinned))
            .unwrap()
            .unwrap();
        fs::rename(root.path().join("private"), root.path().join("saved")).unwrap();
        let path = root.path().to_owned();
        let (send, receive) = mpsc::channel();
        file_set_after_absence(Box::new(move || {
            fs::rename(path.join("saved"), path.join("private")).unwrap();
            send.send(()).unwrap();
        }));
        // When: the same inode reappears during creation's first absence observation.
        let error = files
            .directory_create_by_path(Path::new(pinned))
            .unwrap_err();
        receive
            .try_recv()
            .expect("peer publication hook did not run");
        // Then: even same-identity reappearance cannot erase observed pin disappearance.
        assert_eq!(error.get_kind(), FileErrorKind::Changed, "{pinned}");
        assert!(root.path().join(pinned).is_dir());
    }
}

#[test]
fn observed_unpinned_ancestor_disappearance_and_return_is_changed() {
    // Given: an unpinned ancestor is present when the retained parent observes it.
    let root = TempDir::new().unwrap();
    fs::create_dir(root.path().join("a")).unwrap();
    let inode = fs::metadata(root.path().join("a")).unwrap().ino();
    let files = LocalFileSystem::new(root.path()).unwrap();
    let path = root.path().to_owned();
    directory_set_before_nested_preflight(Box::new(move || {
        fs::rename(path.join("a"), path.join("saved")).unwrap();
    }));
    let path = root.path().to_owned();
    file_set_after_absence(Box::new(move || {
        assert!(!path.join("a").exists());
        assert_eq!(fs::metadata(path.join("saved")).unwrap().ino(), inode);
        fs::rename(path.join("saved"), path.join("a")).unwrap();
    }));
    // When: the observed ancestor disappears and returns during child preflight.
    let result = files.directory_create_by_path(Path::new("a/b"));
    // Then: the same inode returns, but no child is created and disappearance stays Changed.
    assert_eq!(fs::metadata(root.path().join("a")).unwrap().ino(), inode);
    assert!(
        !root.path().join("a/b").exists(),
        "child created after ancestor disappearance"
    );
    assert_eq!(result.unwrap_err().get_kind(), FileErrorKind::Changed);
}

fn publish_entry(root: &Path, name: &str, entry: UnsafeEntry) {
    let path = root.join(name);
    match entry {
        UnsafeEntry::Symlink => symlink("external", path).unwrap(),
        UnsafeEntry::Fifo => create_fifo(&path),
        UnsafeEntry::Hardlink => fs::hard_link(root.join("external"), path).unwrap(),
        UnsafeEntry::Directory => fs::create_dir(path).unwrap(),
        UnsafeEntry::Regular => fs::write(path, b"peer").unwrap(),
    }
}

fn get_observation_error(files: &LocalFileSystem, operation: Observation) -> FileError {
    let path = Path::new("item");
    match operation {
        Observation::Bytes => files.file_get_by_path(path).unwrap_err(),
        Observation::Stat => files.file_get_stat_by_path(path).unwrap_err(),
        Observation::Identity => files.file_get_identity_by_path(path).unwrap_err(),
        Observation::Exists => files.file_is_exist(path).unwrap_err(),
        Observation::LockOwner => files.lock_get_owner_by_path(path).unwrap_err(),
    }
}
