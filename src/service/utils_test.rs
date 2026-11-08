use super::CandidateSelection;
use crate::service::{
    DocumentErrorKind, FileError, FileErrorKind, MockFileReader, MockGitWorktreeListing,
};
use std::{io, path::Path};

#[test]
fn discovery_permission_failure_is_not_treated_as_absence() {
    // Given: the generic filesystem reports permission denial for the ignore file.
    let mut files = MockFileReader::new();
    files.expect_directory_get_by_path().returning(|_| Ok(None));
    files
        .expect_file_get_bytes_by_path()
        .withf(|path| path == Path::new(".letopis/config.json"))
        .returning(|_| Ok(None));
    files
        .expect_file_get_bytes_by_path()
        .withf(|path| path == Path::new(".letopis/ignore"))
        .returning(|_| {
            Err(FileError::new_with_cause(
                FileErrorKind::Io,
                io::Error::new(io::ErrorKind::PermissionDenied, "PRIVATE_PERMISSION"),
            ))
        });
    let mut git = MockGitWorktreeListing::new();
    git.expect_worktree_paths_get_by_ignore_mode_and_patterns()
        .times(0);
    // When: discovery observes both policy inputs.
    let result = CandidateSelection {
        git: &git,
        filesystem: &files,
    }
    .get_candidate_observation_by_paths(&[]);
    // Then: permission failure stops enumeration without exposing the private cause.
    let error = result.unwrap_err();
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("PRIVATE_PERMISSION"));
}
