use super::read::{ReadStage, read_regular};
use super::test_support::{Fixture, run_bounded};
use crate::service::EmbeddingErrorKind;
use std::{
    error::Error,
    fs::{self, File},
};

#[test]
fn rejects_observed_replacement_between_stat_and_open() {
    if run_bounded("filesystem::read_tests::rejects_observed_replacement_between_stat_and_open") {
        return;
    }
    for action in ["regular", "symlink", "fifo", "delete"] {
        // Arrange: an independent process waits at the exact open boundary.
        let fixture = Fixture::new();
        fs::write(fixture.0.join("note.md"), b"original").unwrap();
        let root = File::open(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("note.md", action);
        // Act
        let result = read_regular(&root, "note.md", |stage| {
            if stage == ReadStage::BeforeOpen {
                mutator.resume();
            }
        });
        mutator.finish();
        // Assert
        let error = result.unwrap_err();
        if action == "symlink" {
            assert_eq!(error.get_kind(), EmbeddingErrorKind::InternalError);
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
        // Arrange
        let fixture = Fixture::new();
        fs::write(fixture.0.join("note.md"), b"original").unwrap();
        let root = File::open(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("note.md", action);
        // Act
        let result = read_regular(&root, "note.md", |stage| {
            if stage == ReadStage::AfterRead {
                mutator.resume();
            }
        });
        mutator.finish();
        // Assert
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn rejects_observed_ancestor_replacement() {
    if run_bounded("filesystem::read_tests::rejects_observed_ancestor_replacement") {
        return;
    }
    for action in ["directory", "symlink", "delete"] {
        // Arrange
        let fixture = Fixture::new();
        fs::create_dir(fixture.0.join("notes")).unwrap();
        fs::write(fixture.0.join("notes/a.md"), b"original").unwrap();
        let root = File::open(&fixture.0).unwrap();
        let mut mutator = fixture.spawn_mutator("notes", action);
        // Act
        let result = read_regular(&root, "notes/a.md", |stage| {
            if stage == ReadStage::AfterRead {
                mutator.resume();
            }
        });
        mutator.finish();
        // Assert
        assert!(result.is_err(), "{action}");
    }
}

#[test]
fn rejects_device_files_without_reading_them() {
    if run_bounded("filesystem::read_tests::rejects_device_files_without_reading_them") {
        return;
    }
    let root = File::open("/dev").unwrap();
    for name in ["null", "zero"] {
        // Act / Assert
        assert!(read_regular(&root, name, |_| panic!("device reached open/read")).is_err());
    }
}
