use super::LocalFileSystem;
use super::test_support::{Fixture, run_bounded};
use crate::service::FileErrorKind;
use crate::service::FileReader;
use std::path::Path;
use std::{error::Error, fs};

#[test]
fn symlink_observation_distinguishes_missing_and_nonregular_leaves() {
    // Given: ordinary, directory, FIFO, dangling symlink and absent entries.
    let fixture = Fixture::new();
    fs::write(fixture.0.join("file"), b"bytes").unwrap();
    fs::create_dir(fixture.0.join("directory")).unwrap();
    assert!(
        std::process::Command::new("mkfifo")
            .arg(fixture.0.join("fifo"))
            .status()
            .unwrap()
            .success()
    );
    std::os::unix::fs::symlink("missing", fixture.0.join("link")).unwrap();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    // When / Then: only the final symlink is true and no entry body is opened.
    assert!(files.symlink_is_exist(Path::new("link")).unwrap());
    for name in ["file", "directory", "fifo", "missing", "missing/child"] {
        assert!(!files.symlink_is_exist(Path::new(name)).unwrap(), "{name}");
    }
}

#[test]
fn raw_symlink_probe_never_follows_a_parent() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    // Given: a raw filename beneath an outside-pointing symlink parent.
    let fixture = Fixture::new();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), fixture.0.join("parent")).unwrap();
    let files = LocalFileSystem::new(&fixture.0).unwrap();
    let path = Path::new(OsStr::from_bytes(b"parent/raw\xff.md"));
    // When: a generic symlink observation resolves the raw path.
    let error = files.symlink_is_exist(path).unwrap_err();
    // Then: the parent is rejected before the invalid-byte leaf is touched.
    assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
}

#[test]
fn rejects_observed_replacement_between_stat_and_open() {
    if run_bounded("filesystem::read_tests::rejects_observed_replacement_between_stat_and_open") {
        return;
    }
    for action in ["regular", "symlink", "fifo", "delete"] {
        // Given: an independent process waits at the exact open boundary.
        let fixture = Fixture::new();
        fs::write(fixture.0.join("note.md"), b"original").unwrap();
        let root = LocalFileSystem::new(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("note.md", action);
        // When: capture resumes after replacement or deletion.
        root.before_open.replace(Some(Box::new(move || {
            mutator.resume();
            mutator.finish();
        })));
        let result = root.file_get_by_path(Path::new("note.md"));
        // Then: the unsafe or changed file cannot be captured.
        let error = result.unwrap_err();
        if action == "symlink" {
            assert_eq!(error.get_kind(), FileErrorKind::UnsafePath);
            assert!(
                error.source().is_some(),
                "NOFOLLOW must reject the open syscall"
            );
        }
    }
}

#[test]
fn rejects_observed_changes_after_reading_bytes() {
    if run_bounded("filesystem::read_tests::rejects_observed_changes_after_reading_bytes") {
        return;
    }
    for action in ["regular", "delete", "write"] {
        // Given: a regular file with an after-read mutation barrier.
        let fixture = Fixture::new();
        fs::write(fixture.0.join("note.md"), b"original").unwrap();
        let root = LocalFileSystem::new(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("note.md", action);
        // When: capture revalidates bytes after the mutator runs.
        root.after_read.replace(Some(Box::new(move || {
            mutator.resume();
            mutator.finish();
        })));
        let result = root.file_get_by_path(Path::new("note.md"));
        // Then: every content or identity change fails capture.
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn rejects_observed_ancestor_replacement() {
    if run_bounded("filesystem::read_tests::rejects_observed_ancestor_replacement") {
        return;
    }
    for action in ["directory", "symlink", "delete"] {
        // Given: a file under a directory that another process replaces.
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("notes")).unwrap();
        fs::write(fixture.0.join("notes/a.md"), b"original").unwrap();
        let root = LocalFileSystem::new(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("notes", action);
        // When: capture revalidates ancestry after reading.
        root.after_read.replace(Some(Box::new(move || {
            mutator.resume();
            mutator.finish();
        })));
        let result = root.file_get_by_path(Path::new("notes/a.md"));
        // Then: the replaced ancestry invalidates the observation.
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn rejects_device_files_without_reading_them() {
    if run_bounded("filesystem::read_tests::rejects_device_files_without_reading_them") {
        return;
    }
    // Given: known device leaves under the device directory.
    let root = LocalFileSystem::new(Path::new("/dev")).unwrap();
    for name in ["null", "zero"] {
        // Given: each device has independent guards against entering opening or reading.
        root.before_open.replace(Some(Box::new(move || {
            panic!("device {name} reached opening protocol")
        })));
        root.after_read.replace(Some(Box::new(move || {
            panic!("device {name} reached reading protocol")
        })));
        // When: generic capture observes this device's metadata.
        let error = root.file_get_by_path(Path::new(name)).unwrap_err();
        // Then: rejection happens before either I/O guard can run.
        assert_eq!(error.get_kind(), FileErrorKind::NonRegular);
    }
}
