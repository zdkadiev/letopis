use super::LocalFileSystem;
use super::init::{SyncTarget, fail_next_sync};
use crate::filesystem::test_support::Fixture;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    sync::{Arc, Barrier},
    thread,
};

const FIRST: &[u8] = b"{\"embedding_model\":{\"enabled\":false,\"provider\":null,\"model\":null,\"parameters\":{},\"max_retries\":0}}\n";
const SECOND: &[u8] = b"{\"embedding_model\":{\"enabled\":false,\"provider\":null,\"model\":null,\"parameters\":{},\"max_retries\":3}}\n";

#[test]
fn installs_exact_private_complete_bytes() {
    // Given
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();

    // When
    files.install_config_if_absent(FIRST).unwrap();

    // Then
    let path = fixture.0.join(".letopis/config.json");
    assert_eq!(fs::read(&path).unwrap(), FIRST);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn existing_configuration_is_never_replaced() {
    // Given
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.install_config_if_absent(FIRST).unwrap();

    // When
    files.install_config_if_absent(SECOND).unwrap();

    // Then
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
}

#[test]
fn concurrent_publishers_leave_one_complete_configuration() {
    // Given: distinct complete payloads racing for one absent leaf.
    let fixture = Fixture::new();
    let paths = Arc::new(fixture.0.clone());
    let barrier = Arc::new(Barrier::new(6));
    let mut workers = Vec::new();
    for index in 0..6 {
        let path = Arc::clone(&paths);
        let barrier = Arc::clone(&barrier);
        workers.push(thread::spawn(move || {
            let files = LocalFileSystem::open(&path).unwrap();
            let payload = if index % 2 == 0 { FIRST } else { SECOND };
            barrier.wait();
            files.install_config_if_absent(payload)
        }));
    }

    // When / Then: every loser accepts the winner, and no mixed file exists.
    for worker in workers {
        worker.join().unwrap().unwrap();
    }
    let written = fs::read(fixture.0.join(".letopis/config.json")).unwrap();
    assert!(written == FIRST || written == SECOND);
}

#[test]
fn rejects_symlink_and_nonregular_configuration_leaves() {
    // Given: an outside target and two unsafe destination leaves.
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("config.json");
    fs::write(&target, b"PRIVATE").unwrap();
    fs::create_dir_all(fixture.0.join(".letopis")).unwrap();
    symlink(&target, fixture.0.join(".letopis/config.json")).unwrap();
    let files = LocalFileSystem::open(&fixture.0).unwrap();

    // When / Then
    assert!(files.install_config_if_absent(FIRST).is_err());
    assert_eq!(fs::read(&target).unwrap(), b"PRIVATE");
    fs::remove_file(fixture.0.join(".letopis/config.json")).unwrap();
    fs::create_dir(fixture.0.join(".letopis/config.json")).unwrap();
    assert!(files.install_config_if_absent(FIRST).is_err());
}

#[test]
fn file_sync_failure_leaves_absence_and_retry_publishes() {
    // Given: a service directory but no visible configuration.
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.open_service_directory().unwrap();
    fail_next_sync(SyncTarget::ConfigFile);

    // When
    let error = files.install_config_if_absent(FIRST).unwrap_err();

    // Then: no partial config or temporary entry remains, and retry completes.
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert!(!fixture.0.join(".letopis/config.json").exists());
    assert_eq!(fs::read_dir(fixture.0.join(".letopis")).unwrap().count(), 0);
    files.install_config_if_absent(FIRST).unwrap();
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
}

#[test]
fn directory_sync_failure_after_existing_leaf_retries_durability() {
    // Given: an existing configuration and a failing directory sync.
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.install_config_if_absent(FIRST).unwrap();
    fail_next_sync(SyncTarget::ServiceDirectory);

    // When
    let error = files.install_config_if_absent(SECOND).unwrap_err();

    // Then: the existing bytes survive and a retry finishes the sync step.
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
    assert!(!format!("{error:?}").contains("PRIVATE"));
    files.install_config_if_absent(SECOND).unwrap();
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
}

#[test]
fn service_state_sync_retries_after_root_failure_without_creating_files() {
    // Given: a completed config publication.
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.install_config_if_absent(FIRST).unwrap();
    let before = fs::read_dir(fixture.0.join(".letopis")).unwrap().count();
    fail_next_sync(SyncTarget::RootDirectory);

    // When
    let error = files.sync_service_state().unwrap_err();

    // Then: failure creates nothing and retry can complete durability work.
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert_eq!(
        fs::read_dir(fixture.0.join(".letopis")).unwrap().count(),
        before
    );
    files.sync_service_state().unwrap();
}

#[test]
fn rejects_replaced_service_directory_and_missing_state_sync() {
    // Given: a filesystem which pinned the original service directory.
    let fixture = Fixture::new();
    let files = LocalFileSystem::open(&fixture.0).unwrap();
    files.open_service_directory().unwrap();
    assert!(files.sync_service_state().is_ok());
    fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();

    // When / Then
    assert!(files.install_config_if_absent(FIRST).is_err());
    assert!(files.sync_service_state().is_err());
    let empty = Fixture::new();
    let fresh = LocalFileSystem::open(&empty.0).unwrap();
    assert!(fresh.sync_service_state().is_err());
}
