use super::LocalFileSystem;
use crate::filesystem::test_support::{Fixture, run_bounded};
use crate::service::EmbeddingFiles;
use std::fs;
use std::os::unix::fs::MetadataExt;

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
