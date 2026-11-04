use super::{
    local::LocalFileSystem,
    path::{
        FILE_FLAGS, FileParent, file_build_identity, file_build_stat, file_get_entry,
        file_map_cause, file_map_io, file_map_observed_open_io, file_require_identity,
        file_require_type, file_require_unchanged, file_verify_entry, path_get_components,
        path_get_file_components,
    },
};
use crate::service::{
    CapturedFile, FileError, FileErrorKind, FileIdentity, FileReader, FileResult, FileStat,
};
use rustix::{
    fd::OwnedFd,
    fs::{FileType, Mode, Stat, fstat, openat},
};
use std::{
    ffi::OsStr,
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    time::SystemTime,
};

pub(super) fn symlink_is_exist(files: &LocalFileSystem, path: &Path) -> FileResult<bool> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(files, &components)? else {
        return Ok(false);
    };
    let observed = file_get_entry(&parent.directory, &name)?;
    let is_symlink = if let Some(before) = observed {
        file_verify_entry(&parent.directory, &name, &before)?;
        FileType::from_raw_mode(before.st_mode) == FileType::Symlink
    } else {
        false
    };
    parent.directory_verify_links(files)?;
    Ok(is_symlink)
}

pub(super) fn file_get_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<CapturedFile>> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(files, &components)? else {
        return Ok(None);
    };
    let Some(before) =
        file_get_observation_by_parent_and_name(files, &parent, &name, file_require_unchanged)?
    else {
        return Ok(None);
    };
    #[cfg(test)]
    if let Some(hook) = files.before_open.take() {
        hook();
    }
    let mut file = File::from(
        openat(&parent.directory, &name, FILE_FLAGS, Mode::empty())
            .map_err(file_map_observed_open_io)?,
    );
    let opened = fstat(&file).map_err(file_map_io)?;
    file_require_unchanged(&before, &opened)?;
    let checked_at_ns = file_get_time_ns()?;
    #[cfg(test)]
    let checked_at_ns = files.capture_time.unwrap_or(checked_at_ns);
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).map_err(file_map_cause)?;
    #[cfg(test)]
    if let Some(hook) = files.after_read.take() {
        hook();
    }
    file_require_unchanged(&opened, &fstat(&file).map_err(file_map_io)?)?;
    file_require_unchanged(
        &opened,
        &file_verify_entry(&parent.directory, &name, &opened)?,
    )?;
    parent.directory_verify_links(files)?;
    super::path::directory_verify_root(files)?;
    Ok(Some(CapturedFile {
        bytes,
        stat: file_build_stat(&opened)?,
        checked_at_ns,
    }))
}

pub(super) fn file_get_stat_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<FileStat>> {
    file_get_observation_by_path(files, path, file_require_unchanged)?
        .as_ref()
        .map(file_build_stat)
        .transpose()
}

pub(super) fn file_get_identity_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<FileIdentity>> {
    file_get_observation_by_path(files, path, file_require_identity)?
        .as_ref()
        .map(file_build_identity)
        .transpose()
}

pub(super) fn file_is_exist(files: &LocalFileSystem, path: &Path) -> FileResult<bool> {
    Ok(files.file_get_identity_by_path(path)?.is_some())
}

pub(super) fn file_verify_identity_by_path(
    files: &LocalFileSystem,
    path: &Path,
    identity: Option<FileIdentity>,
) -> FileResult<()> {
    file_get_verified_identity_by_path(files, path, identity).map(|_| ())
}

pub(super) fn file_verify_distinct_by_paths(
    files: &LocalFileSystem,
    path: &Path,
    other: &Path,
) -> FileResult<()> {
    let first: PathBuf = path_get_components(path)?.iter().collect();
    let second: PathBuf = path_get_components(other)?.iter().collect();
    let before = files.file_get_identity_by_path(path)?;
    let after = files.file_get_identity_by_path(other)?;
    if first == second || before.is_some() && before == after {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    #[cfg(test)]
    if let Some(hook) = files.before_distinct_verify.take() {
        hook();
    }
    let current_first = file_get_verified_identity_by_path(files, path, before)?;
    let current_second = file_get_verified_identity_by_path(files, other, after)?;
    if current_first.is_some() && current_first == current_second {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok(())
}

pub(super) fn directory_get_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<OwnedFd>> {
    let Some(parent) = FileParent::directory_get_by_path(files, path)? else {
        return Ok(None);
    };
    let normalized: PathBuf = path_get_components(path)?.iter().collect();
    if !normalized.as_os_str().is_empty() && !files.directories.borrow().contains_key(&normalized) {
        files.directories.borrow_mut().insert(
            normalized,
            parent.directory.try_clone().map_err(file_map_cause)?,
        );
    }
    parent.directory_verify_links(files)?;
    parent
        .directory
        .try_clone()
        .map(OwnedFd::from)
        .map(Some)
        .map_err(file_map_cause)
}

fn file_get_verified_identity_by_path(
    files: &LocalFileSystem,
    path: &Path,
    identity: Option<FileIdentity>,
) -> FileResult<Option<FileIdentity>> {
    let current = files.file_get_identity_by_path(path)?;
    if identity.is_some() && current != identity {
        return Err(FileError::new(FileErrorKind::Changed));
    }
    Ok(current)
}

fn file_get_observation_by_path(
    files: &LocalFileSystem,
    path: &Path,
    verify: fn(&Stat, &Stat) -> FileResult<()>,
) -> FileResult<Option<Stat>> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(files, &components)? else {
        return Ok(None);
    };
    file_get_observation_by_parent_and_name(files, &parent, &name, verify)
}

/// Verify both leaf observations and ancestry before classifying an excluded entry.
/// Replacement must remain an integrity error even when either leaf is nonregular.
fn file_get_observation_by_parent_and_name(
    files: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    verify: fn(&Stat, &Stat) -> FileResult<()>,
) -> FileResult<Option<Stat>> {
    let before = file_get_entry(&parent.directory, name)?;
    #[cfg(test)]
    if let Some(hook) = files.between_stats.take() {
        hook();
    }
    let after = file_get_entry(&parent.directory, name)?;
    match (&before, &after) {
        (None, None) => {}
        (Some(before), Some(after)) => verify(before, after)?,
        _ => return Err(FileError::new(FileErrorKind::Changed)),
    }
    parent.directory_verify_links(files)?;
    super::path::directory_verify_root(files)?;
    if let Some(stat) = &before {
        file_require_type(stat, FileType::RegularFile, FileErrorKind::NonRegular)?;
    }
    Ok(before)
}

fn file_get_time_ns() -> FileResult<i64> {
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(file_map_cause)?;
    i64::try_from(elapsed.as_nanos()).map_err(file_map_cause)
}
