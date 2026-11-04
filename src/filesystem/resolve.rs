use super::local::LocalFileSystem;
use crate::service::{FileError, FileErrorKind, FileIdentity, FileResult};
use std::{
    fs, io,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
};

/// Resolve two caller-selected paths and reject identical paths or hardlink aliases.
/// Existing symlinks are resolved explicitly; this does not pin or confine later I/O.
///
/// # Errors
/// Reports unsafe paths, nonregular leaves, aliases and operating-system failures.
pub(super) fn file_get_distinct_paths_by_paths(
    path: &Path,
    other: &Path,
) -> FileResult<[PathBuf; 2]> {
    let first = LocalFileSystem::file_get_resolved_path_by_path(path)?;
    let second = LocalFileSystem::file_get_resolved_path_by_path(other)?;
    let first_identity = file_get_identity(&first)?;
    let second_identity = file_get_identity(&second)?;
    if first == second || first_identity.is_some() && first_identity == second_identity {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok([first, second])
}

/// Resolve an author-supplied file path against its existing canonical parent.
/// Existing symlinks resolve to their regular-file target; broken symlinks fail.
/// This is path resolution only, not a retained observation or confinement guard.
///
/// # Errors
/// Reports empty/NUL paths, nonregular leaves, inaccessible parents or I/O failures.
pub(super) fn file_get_resolved_path_by_path(path: &Path) -> FileResult<PathBuf> {
    if path.as_os_str().is_empty() || path.as_os_str().as_bytes().contains(&0) {
        return Err(FileError::new(FileErrorKind::UnsafePath));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let resolved = fs::canonicalize(path).map_err(file_map_io)?;
            file_get_identity(&resolved)?;
            Ok(resolved)
        }
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => {
            let name = path
                .file_name()
                .ok_or_else(|| FileError::new(FileErrorKind::UnsafePath))?;
            let parent = path
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
                .unwrap_or(Path::new("."));
            let resolved = fs::canonicalize(parent).map_err(file_map_io)?.join(name);
            file_get_identity(&resolved)?;
            Ok(resolved)
        }
        Err(cause) => Err(file_map_io(cause)),
    }
}

fn file_get_identity(path: &Path) -> FileResult<Option<FileIdentity>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })),
        Ok(_) => Err(FileError::new(FileErrorKind::NonRegular)),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(cause) => Err(file_map_io(cause)),
    }
}

fn file_map_io(cause: io::Error) -> FileError {
    let kind = if cause.raw_os_error() == Some(rustix::io::Errno::LOOP.raw_os_error()) {
        FileErrorKind::UnsafePath
    } else {
        match cause.kind() {
            io::ErrorKind::NotADirectory | io::ErrorKind::InvalidInput => FileErrorKind::UnsafePath,
            _ => FileErrorKind::Io,
        }
    };
    FileError::new_with_cause(kind, cause)
}
