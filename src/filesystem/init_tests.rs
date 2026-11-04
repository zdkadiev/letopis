use super::LocalFileSystem;
use super::publication::{SyncTarget, file_fail_next_sync};
use crate::filesystem::test_support::Fixture;
use crate::service::FileWriter;
use std::path::Path;
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
    // Given: an existing private parent and an absent publication leaf.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();

    // When: complete bytes are published without replacement.
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap();

    // Then: the final bytes are exact and the mode is 0600.
    let path = fixture.0.join(".letopis/config.json");
    assert_eq!(fs::read(&path).unwrap(), FIRST);
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

#[test]
fn protects_a_preexisting_permissive_service_directory() {
    // Given: `.letopis` already exists with group/other access.
    let fixture = Fixture::new();
    let directory = fixture.0.join(".letopis");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();

    // When: initialization opens a private service directory.
    files
        .directory_create_by_path(std::path::Path::new(".letopis"))
        .unwrap();

    // Then: the pinned directory is private before any database is created.
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn existing_configuration_is_never_replaced() {
    // Given: a complete first publication.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap();

    // When: a second payload is published to the same path.
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
        .unwrap();

    // Then: the first winner survives byte for byte.
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
            let files = LocalFileSystem::new(&path).unwrap();
            files
                .directory_create_by_path(Path::new(".letopis"))
                .unwrap();
            let payload = if index % 2 == 0 { FIRST } else { SECOND };
            barrier.wait();
            files.file_create_if_absent_by_path(Path::new(".letopis/config.json"), payload)
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
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();

    // When / Then: no-clobber publication validates the destination; unsafe leaves are rejected without changing outside bytes.
    assert!(
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .is_err()
    );
    assert_eq!(fs::read(&target).unwrap(), b"PRIVATE");
    fs::remove_file(fixture.0.join(".letopis/config.json")).unwrap();
    fs::create_dir(fixture.0.join(".letopis/config.json")).unwrap();
    assert!(
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .is_err()
    );
}

#[test]
fn file_sync_failure_leaves_absence_and_retry_publishes() {
    // Given: a service directory but no visible configuration.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    files
        .directory_create_by_path(std::path::Path::new(".letopis"))
        .unwrap();
    file_fail_next_sync(SyncTarget::File);

    // When: publication tries to sync the temporary file.
    let error = files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap_err();

    // Then: no partial config or temporary entry remains, and retry completes.
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert!(!fixture.0.join(".letopis/config.json").exists());
    assert_eq!(fs::read_dir(fixture.0.join(".letopis")).unwrap().count(), 0);
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap();
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
}

#[test]
fn directory_sync_failure_after_existing_leaf_retries_durability() {
    // Given: an existing configuration and a failing directory sync.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap();
    file_fail_next_sync(SyncTarget::Directory);

    // When: publication retries durability for the retained winner.
    let error = files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
        .unwrap_err();

    // Then: the existing bytes survive and a retry finishes the sync step.
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
    assert!(!format!("{error:?}").contains("PRIVATE"));
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), SECOND)
        .unwrap();
    assert_eq!(
        fs::read(fixture.0.join(".letopis/config.json")).unwrap(),
        FIRST
    );
}

#[test]
fn service_state_sync_retries_after_root_failure_without_creating_files() {
    // Given: a completed config publication.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    files
        .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
        .unwrap();
    let before = fs::read_dir(fixture.0.join(".letopis")).unwrap().count();
    file_fail_next_sync(SyncTarget::Directory);

    // When: the root durability step is attempted and retried.
    let error = files.directory_sync_by_path(Path::new(".")).unwrap_err();

    // Then: failure creates nothing and retry can complete durability work.
    assert!(!format!("{error:?}").contains("PRIVATE"));
    assert_eq!(
        fs::read_dir(fixture.0.join(".letopis")).unwrap().count(),
        before
    );
    files.directory_sync_by_path(Path::new(".")).unwrap();
}

#[test]
fn rejects_replaced_service_directory_and_missing_state_sync() {
    // Given: a filesystem which pinned the original service directory.
    let fixture = Fixture::new();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    files
        .directory_create_by_path(Path::new(".letopis"))
        .unwrap();
    files
        .directory_create_by_path(std::path::Path::new(".letopis"))
        .unwrap();
    assert!(files.directory_sync_by_path(Path::new(".letopis")).is_ok());
    fs::rename(fixture.0.join(".letopis"), fixture.0.join("old-service")).unwrap();
    fs::create_dir(fixture.0.join(".letopis")).unwrap();

    // When / Then: publication and durability operations reuse the original pin; replacement or absence is rejected without recreating state.
    assert!(
        files
            .file_create_if_absent_by_path(Path::new(".letopis/config.json"), FIRST)
            .is_err()
    );
    assert!(files.directory_sync_by_path(Path::new(".letopis")).is_err());
    let empty = Fixture::new();
    let fresh = LocalFileSystem::new(&empty.0).unwrap();
    assert!(fresh.directory_sync_by_path(Path::new(".letopis")).is_err());
}
