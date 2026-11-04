use super::{
    local::{LocalFileSystem, file_retry_io},
    path::{
        FileParent, file_get_entry, file_map_cause, file_map_io, file_require_type,
        path_get_components, path_get_file_components,
    },
};
use crate::service::{FileError, FileErrorKind, FileReader, FileResult};
use rustix::{
    fd::OwnedFd,
    fs::{AtFlags, FileType, Mode, OFlags, fchmod, fstat, linkat, openat, unlinkat},
    io::Errno,
};
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
};

const TEMP_FLAGS: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const TEMP_ATTEMPTS: usize = 16;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyncTarget {
    File,
    Directory,
}

#[cfg(test)]
thread_local! {
    static NEXT_SYNC_FAULT: std::cell::RefCell<Option<SyncTarget>> = const { std::cell::RefCell::new(None) };
}

struct TemporaryEntry {
    directory: File,
    name: String,
}

#[cfg(test)]
pub(super) fn file_fail_next_sync(target: SyncTarget) {
    NEXT_SYNC_FAULT.set(Some(target));
}

pub(super) fn file_create_if_absent_by_path(
    files: &LocalFileSystem,
    path: &Path,
    bytes: &[u8],
) -> FileResult<()> {
    let (components, name) = path_get_file_components(path)?;
    let parent = FileParent::directory_get_by_components(files, &components)?
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound))?;
    if let Some(before) = file_get_entry(&parent.directory, &name)? {
        file_require_type(&before, FileType::RegularFile, FileErrorKind::NonRegular)?;
        parent.directory_verify_links(files)?;
        directory_sync(&parent.directory)?;
        parent.directory_verify_links(files)?;
        return Ok(());
    }
    let (temporary, mut file) = file_create_temporary(&parent.directory)?;
    file_retry_io(|| file.write_all(bytes)).map_err(file_map_cause)?;
    fchmod(&file, Mode::RUSR | Mode::WUSR).map_err(file_map_io)?;
    file_sync(&file)?;
    parent.directory_verify_links(files)?;
    match linkat(
        &parent.directory,
        temporary.name.as_str(),
        &parent.directory,
        &name,
        AtFlags::empty(),
    ) {
        Ok(()) => {}
        Err(Errno::EXIST) => {
            let winner = file_get_entry(&parent.directory, &name)?
                .ok_or_else(|| FileError::new(FileErrorKind::Changed))?;
            file_require_type(&winner, FileType::RegularFile, FileErrorKind::NonRegular)?;
        }
        Err(cause) => return Err(file_map_io(cause)),
    }
    temporary.file_delete()?;
    directory_sync(&parent.directory)?;
    parent.directory_verify_links(files)?;
    super::path::directory_verify_root(files)
}

pub(super) fn directory_create_by_path(
    files: &LocalFileSystem,
    path: &Path,
) -> FileResult<OwnedFd> {
    let components = path_get_components(path)?;
    let parent = super::path::directory_create_components(files, &components)?;
    fchmod(&parent.directory, Mode::RWXU).map_err(file_map_io)?;
    parent.directory_verify_links(files)?;
    files
        .directory_get_by_path(path)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))
}

pub(super) fn directory_sync_by_path(files: &LocalFileSystem, path: &Path) -> FileResult<()> {
    let parent = FileParent::directory_get_by_path(files, path)?
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound))?;
    directory_sync(&parent.directory)?;
    #[cfg(test)]
    if let Some(hook) = files.after_directory_sync.borrow().as_ref() {
        hook(path);
    }
    parent.directory_verify_links(files)?;
    super::path::directory_verify_root(files)
}

impl TemporaryEntry {
    fn file_delete(mut self) -> FileResult<()> {
        unlinkat(&self.directory, self.name.as_str(), AtFlags::empty()).map_err(file_map_io)?;
        self.name.clear();
        Ok(())
    }
}

impl Drop for TemporaryEntry {
    fn drop(&mut self) {
        if !self.name.is_empty() {
            let _ = unlinkat(&self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

fn file_create_temporary(directory: &File) -> FileResult<(TemporaryEntry, File)> {
    // Own cleanup before creating an entry so descriptor-duplication failure leaks nothing.
    let directory = directory.try_clone().map_err(file_map_cause)?;
    for _ in 0..TEMP_ATTEMPTS {
        let name = format!(
            ".file.tmp.{}.{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        match openat(
            &directory,
            name.as_str(),
            TEMP_FLAGS,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => return Ok((TemporaryEntry { directory, name }, File::from(file))),
            Err(Errno::EXIST) => {}
            Err(cause) => return Err(file_map_io(cause)),
        }
    }
    Err(FileError::new_with_cause(
        FileErrorKind::Io,
        io::Error::other("temporary file name collisions"),
    ))
}

fn file_sync(file: &File) -> FileResult<()> {
    file_verify_sync(SyncTarget::File)?;
    file_retry_io(|| file_sync_data(file)).map_err(file_map_cause)
}

#[cfg(target_os = "macos")]
fn file_sync_data(file: &File) -> io::Result<()> {
    rustix::fs::fcntl_fullfsync(file).map_err(io::Error::from)
}

#[cfg(not(target_os = "macos"))]
fn file_sync_data(file: &File) -> io::Result<()> {
    file.sync_all()
}

fn directory_sync(directory: &File) -> FileResult<()> {
    file_verify_sync(SyncTarget::Directory)?;
    // A retained descriptor identifies the directory even if its path is concurrently changed.
    fstat(directory).map_err(file_map_io)?;
    file_retry_io(|| directory.sync_all()).map_err(file_map_cause)
}

#[cfg_attr(
    not(test),
    expect(
        clippy::unnecessary_wraps,
        reason = "Production and test builds share the fallible synchronization fault interface"
    )
)]
fn file_verify_sync(_target: SyncTarget) -> FileResult<()> {
    #[cfg(test)]
    if NEXT_SYNC_FAULT.with(|next| {
        if next.borrow().as_ref() == Some(&_target) {
            next.borrow_mut().take();
            true
        } else {
            false
        }
    }) {
        return Err(FileError::new_with_cause(
            FileErrorKind::Io,
            io::Error::other("injected file synchronization failure"),
        ));
    }
    Ok(())
}
