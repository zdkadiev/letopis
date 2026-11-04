use super::{
    local::{LocalFileSystem, PersistentLock, file_retry_io},
    path::{
        FileParent, file_get_entry, file_map_cause, file_map_io, file_map_observed_open_io,
        file_require_identity, file_require_type, file_verify_entry, path_get_file_components,
    },
};
use crate::service::{FileError, FileErrorKind, FileResult, FileWriter};
use rustix::{
    fd::OwnedFd,
    fs::{FileType, Mode, OFlags, Stat, fchmod, fstat, openat},
    io::Errno,
};
use std::{
    ffi::{OsStr, OsString},
    fs::{File, TryLockError},
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    num::NonZeroU32,
    path::{Path, PathBuf},
};

const LOCK_FLAGS: OFlags = OFlags::RDWR
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
const MAX_PID_BYTES: usize = 11;
const PID_READ_LIMIT: u64 = 12;

pub(super) fn lock_acquire_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<PersistentLock> {
    let (parent, name, file) = lock_get_file_by_path(files, path)?;
    let before = file_get_lock_stat(&file)?;
    if !lock_try_acquire(files, &file)? {
        file_retry_io(|| file.lock()).map_err(file_map_cause)?;
    }
    lock_finish_acquisition(files, &parent, &name, file, before)
}

pub(super) fn lock_try_acquire_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<PersistentLock>> {
    let (parent, name, file) = lock_get_file_by_path(files, path)?;
    let before = file_get_lock_stat(&file)?;
    if !lock_try_acquire(files, &file)? {
        file_verify_lock(files, &parent, &name, &file)?;
        return Ok(None);
    }
    lock_finish_acquisition(files, &parent, &name, file, before).map(Some)
}

pub(super) fn lock_get_owner_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<NonZeroU32>> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(files, &components)? else {
        return Ok(None);
    };
    let Some(before) = file_get_entry(&parent.directory, &name)? else {
        parent.directory_verify_links(files)?;
        return Ok(None);
    };
    file_require_lock(&before)?;
    #[cfg(test)]
    if let Some(hook) = files.before_open.take() {
        hook();
    }
    let mut file = File::from(
        openat(&parent.directory, &name, LOCK_FLAGS, Mode::empty())
            .map_err(file_map_observed_open_io)?,
    );
    file_require_identity(&before, &file_get_lock_stat(&file)?)?;
    file_verify_lock(files, &parent, &name, &file)?;
    if lock_try_acquire(files, &file)? {
        let guard = PersistentLock { file };
        file_verify_lock(files, &parent, &name, &guard.file)?;
        return Ok(None);
    }
    let pid = lock_get_pid(&mut file)?;
    file_verify_lock(files, &parent, &name, &file)?;
    super::path::directory_verify_root(files)?;
    Ok(Some(pid))
}

fn lock_get_file_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<(FileParent, OsString, File)> {
    let (components, name) = path_get_file_components(path)?;
    if !components.is_empty() {
        let directory_path: PathBuf = components.iter().collect();
        files.directory_create_by_path(&directory_path)?;
    }
    let parent = FileParent::directory_get_by_components(files, &components)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))?;
    let before = file_get_entry(&parent.directory, &name)?;
    if let Some(before) = &before {
        file_require_lock(before)?;
    }
    #[cfg(test)]
    if let Some(hook) = files.before_open.take() {
        hook();
    }
    let opened = match before {
        Some(_) => openat(&parent.directory, &name, LOCK_FLAGS, Mode::empty()),
        None => file_create_lock(&parent.directory, &name),
    };
    let file = File::from(opened.map_err(|cause| {
        if before.is_some() {
            file_map_observed_open_io(cause)
        } else {
            file_map_io(cause)
        }
    })?);
    let current = file_get_lock_stat(&file)?;
    if let Some(before) = before {
        file_require_identity(&before, &current)?;
    }
    file_verify_lock(files, &parent, &name, &file)?;
    Ok((parent, name, file))
}

fn lock_finish_acquisition(
    files: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    file: File,
    before: Stat,
) -> FileResult<PersistentLock> {
    // Install the unlock fallback before any fallible validation or owner mutation.
    let mut guard = PersistentLock { file };
    file_require_identity(&before, &file_get_lock_stat(&guard.file)?)?;
    file_verify_lock(files, parent, name, &guard.file)?;
    fchmod(&guard.file, Mode::RUSR | Mode::WUSR).map_err(file_map_io)?;
    guard
        .file
        .set_len(0)
        .and_then(|()| guard.file.seek(SeekFrom::Start(0)))
        .and_then(|_| writeln!(guard.file, "{}", std::process::id()))
        .map_err(file_map_cause)?;
    file_verify_lock(files, parent, name, &guard.file)?;
    super::path::directory_verify_root(files)?;
    Ok(guard)
}

fn lock_try_acquire(_files: &LocalFileSystem, file: &File) -> FileResult<bool> {
    if lock_retry_acquisition(|| file.try_lock()).map_err(file_map_cause)? {
        return Ok(true);
    }
    #[cfg(test)]
    if let Some(hook) = _files.on_contention.take() {
        hook();
    }
    Ok(false)
}

fn lock_retry_acquisition(
    mut operation: impl FnMut() -> Result<(), TryLockError>,
) -> Result<bool, TryLockError> {
    loop {
        match operation() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(cause)) if cause.kind() == ErrorKind::Interrupted => {}
            Err(cause) => return Err(cause),
        }
    }
}

fn file_create_lock(directory: &File, name: &OsStr) -> Result<OwnedFd, Errno> {
    let opened = openat(
        directory,
        name,
        LOCK_FLAGS | OFlags::CREATE | OFlags::EXCL,
        Mode::RUSR | Mode::WUSR,
    );
    file_reopen_lock(directory, name, opened)
}

fn file_reopen_lock(
    directory: &File,
    name: &OsStr,
    opened: Result<OwnedFd, Errno>,
) -> Result<OwnedFd, Errno> {
    match opened {
        // macOS may report ENOENT when another caller wins O_CREAT|O_NOFOLLOW.
        Err(Errno::EXIST | Errno::NOENT) => openat(directory, name, LOCK_FLAGS, Mode::empty()),
        result => result,
    }
}

fn file_get_lock_stat(file: &File) -> FileResult<Stat> {
    let stat = fstat(file).map_err(file_map_io)?;
    file_require_lock(&stat)?;
    Ok(stat)
}

fn file_require_lock(stat: &Stat) -> FileResult<()> {
    file_require_type(stat, FileType::RegularFile, FileErrorKind::NonRegular)?;
    if stat.st_nlink != 1 {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok(())
}

fn file_verify_lock(
    files: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    file: &File,
) -> FileResult<()> {
    let current = file_get_lock_stat(file)?;
    let named = file_verify_entry(&parent.directory, name, &current)?;
    file_require_lock(&named)?;
    parent.directory_verify_links(files)
}

fn lock_get_pid(file: &mut File) -> FileResult<NonZeroU32> {
    let mut bytes = Vec::new();
    file.take(PID_READ_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(file_map_cause)?;
    if bytes.len() > MAX_PID_BYTES {
        return Err(FileError::new(FileErrorKind::InvalidOwner));
    }
    let text =
        std::str::from_utf8(&bytes).map_err(|_| FileError::new(FileErrorKind::InvalidOwner))?;
    let digits = text.strip_suffix('\n').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(FileError::new(FileErrorKind::InvalidOwner));
    }
    digits
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(|| FileError::new(FileErrorKind::InvalidOwner))
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
