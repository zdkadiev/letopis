use super::local::LocalFileSystem;
use crate::service::{FileError, FileErrorKind, FileIdentity, FileResult, FileStat};
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, mkdirat, open, openat, statat},
    io::Errno,
};
use std::{
    ffi::{OsStr, OsString},
    fs::File,
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
};

pub(super) const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
pub(super) const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
const NANOS_PER_SECOND: i64 = 1_000_000_000;

struct DirectoryLink {
    parent: File,
    name: OsString,
    identity: Stat,
}

/// Pins only the ancestors needed by one operation and verifies all links after I/O.
pub(super) struct FileParent {
    pub(super) directory: File,
    links: Vec<DirectoryLink>,
}

impl FileParent {
    pub(super) fn directory_get_by_path(
        files: &LocalFileSystem,
        path: &Path,
    ) -> FileResult<Option<Self>> {
        let components = path_get_components(path)?;
        Self::directory_get_by_components(files, &components)
    }

    pub(super) fn directory_get_by_components(
        files: &LocalFileSystem,
        components: &[OsString],
    ) -> FileResult<Option<Self>> {
        directory_verify_root(files)?;
        let mut parent = Self {
            directory: files
                .directory_get_root()
                .try_clone()
                .map_err(file_map_cause)?,
            links: Vec::new(),
        };
        let mut path = PathBuf::new();
        for name in components {
            path.push(name);
            let Some(before) = file_get_entry(&parent.directory, name)? else {
                if files
                    .directories
                    .borrow()
                    .keys()
                    .any(|pinned| pinned.starts_with(&path))
                {
                    return Err(FileError::new(FileErrorKind::Changed));
                }
                parent.directory_verify_links(files)?;
                return Ok(None);
            };
            file_require_type(&before, FileType::Directory, FileErrorKind::UnsafePath)?;
            let next = File::from(
                openat(&parent.directory, name, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(file_map_io)?,
            );
            file_require_identity(&before, &fstat(&next).map_err(file_map_io)?)?;
            if let Some(pinned) = files.directories.borrow().get(&path) {
                file_require_identity(&fstat(pinned).map_err(file_map_io)?, &before)?;
            }
            let previous = std::mem::replace(&mut parent.directory, next);
            parent.links.push(DirectoryLink {
                parent: previous,
                name: name.clone(),
                identity: before,
            });
        }
        parent.directory_verify_links(files)?;
        Ok(Some(parent))
    }

    /// Verify named ancestors and the component's pinned root before completion.
    pub(super) fn directory_verify_links(&self, files: &LocalFileSystem) -> FileResult<()> {
        #[cfg(test)]
        if let Some(hook) = files.before_directory_complete.take() {
            hook();
        }
        for link in &self.links {
            file_verify_entry(&link.parent, &link.name, &link.identity)?;
        }
        directory_verify_root(files)
    }
}

pub(super) fn directory_verify_root(files: &LocalFileSystem) -> FileResult<()> {
    let current = open(&files.root_path, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|cause| FileError::new_with_cause(FileErrorKind::Changed, cause))?;
    file_require_identity(
        &fstat(files.directory_get_root()).map_err(file_map_io)?,
        &fstat(current).map_err(file_map_io)?,
    )
}

pub(super) fn directory_create_components(
    files: &LocalFileSystem,
    components: &[OsString],
) -> FileResult<FileParent> {
    let mut path = PathBuf::new();
    for name in components {
        let parent = FileParent::directory_get_by_path(
            files,
            if path.as_os_str().is_empty() {
                Path::new(".")
            } else {
                &path
            },
        )?
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound))?;
        path.push(name);
        if FileParent::directory_get_by_path(files, &path)?.is_none() {
            match mkdirat(&parent.directory, name, Mode::RWXU) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(cause) => return Err(file_map_io(cause)),
            }
        }
        parent.directory_verify_links(files)?;
        FileParent::directory_get_by_path(files, &path)?
            .ok_or_else(|| FileError::new(FileErrorKind::Changed))?;
    }
    FileParent::directory_get_by_components(files, components)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))
}

pub(super) fn path_get_components(path: &Path) -> FileResult<Vec<OsString>> {
    if path.as_os_str().is_empty() || path.as_os_str().as_bytes().contains(&0) {
        return Err(FileError::new(FileErrorKind::UnsafePath));
    }
    path.components()
        .filter_map(|component| match component {
            Component::CurDir => None,
            Component::Normal(name) => Some(Ok(name.to_owned())),
            _ => Some(Err(FileError::new(FileErrorKind::UnsafePath))),
        })
        .collect()
}

pub(super) fn path_get_file_components(path: &Path) -> FileResult<(Vec<OsString>, OsString)> {
    let mut components = path_get_components(path)?;
    let name = components
        .pop()
        .ok_or_else(|| FileError::new(FileErrorKind::UnsafePath))?;
    Ok((components, name))
}

pub(super) fn file_get_entry(parent: &File, name: &OsStr) -> FileResult<Option<Stat>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(Errno::NOENT) => match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
            Err(Errno::NOENT) => Ok(None),
            Ok(_) => Err(FileError::new(FileErrorKind::Changed)),
            Err(cause) => Err(file_map_io(cause)),
        },
        Err(cause) => Err(file_map_io(cause)),
    }
}

pub(super) fn file_verify_entry(parent: &File, name: &OsStr, before: &Stat) -> FileResult<Stat> {
    let current = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|cause| FileError::new_with_cause(FileErrorKind::Changed, cause))?;
    file_require_identity(before, &current)?;
    Ok(current)
}

pub(super) fn file_require_identity(before: &Stat, after: &Stat) -> FileResult<()> {
    if before.st_dev != after.st_dev
        || before.st_ino != after.st_ino
        || FileType::from_raw_mode(before.st_mode) != FileType::from_raw_mode(after.st_mode)
    {
        return Err(FileError::new(FileErrorKind::Changed));
    }
    Ok(())
}

pub(super) fn file_require_unchanged(before: &Stat, after: &Stat) -> FileResult<()> {
    file_require_identity(before, after)?;
    if before.st_size != after.st_size
        || before.st_mode != after.st_mode
        || before.st_mtime != after.st_mtime
        || before.st_mtime_nsec != after.st_mtime_nsec
        || before.st_ctime != after.st_ctime
        || before.st_ctime_nsec != after.st_ctime_nsec
    {
        return Err(FileError::new(FileErrorKind::Changed));
    }
    Ok(())
}

pub(super) fn file_require_type(
    stat: &Stat,
    expected: FileType,
    kind: FileErrorKind,
) -> FileResult<()> {
    if FileType::from_raw_mode(stat.st_mode) != expected {
        return Err(FileError::new(kind));
    }
    Ok(())
}

pub(super) fn file_build_stat(stat: &Stat) -> FileResult<FileStat> {
    let seconds = i64::try_from(i128::from(stat.st_mtime)).map_err(file_map_cause)?;
    let nanos = i64::try_from(i128::from(stat.st_mtime_nsec)).map_err(file_map_cause)?;
    let mtime_ns = seconds
        .checked_mul(NANOS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| FileError::new(FileErrorKind::Io))?;
    Ok(FileStat {
        size: u64::try_from(stat.st_size).map_err(file_map_cause)?,
        mtime_ns,
        inode: stat.st_ino,
        device: file_build_identity(stat)?.device,
    })
}

pub(super) fn file_build_identity(stat: &Stat) -> FileResult<FileIdentity> {
    Ok(FileIdentity {
        device: u64::try_from(i128::from(stat.st_dev)).map_err(file_map_cause)?,
        inode: stat.st_ino,
    })
}

/// Classify failure to open a leaf already observed as present.
/// Disappearance is an integrity change; other OS classifications remain intact.
pub(super) fn file_map_observed_open_io(cause: Errno) -> FileError {
    if cause == Errno::NOENT {
        FileError::new_with_cause(FileErrorKind::Changed, cause)
    } else {
        file_map_io(cause)
    }
}

pub(super) fn file_map_io(cause: Errno) -> FileError {
    let kind = match cause {
        Errno::LOOP | Errno::NOTDIR => FileErrorKind::UnsafePath,
        _ => FileErrorKind::Io,
    };
    FileError::new_with_cause(kind, cause)
}

pub(super) fn file_map_cause(cause: impl std::error::Error + Send + Sync + 'static) -> FileError {
    FileError::new_with_cause(FileErrorKind::Io, cause)
}

#[cfg(test)]
mod tests {
    use super::file_require_unchanged;
    use crate::service::FileErrorKind;
    use rustix::fs::fstat;
    use std::fs::File;

    #[test]
    fn rejects_each_independent_stat_change() {
        // Given: real platform metadata with one independent field changed in each copy.
        let file = File::open(".").unwrap();
        let before = fstat(&file).unwrap();
        let mut changes = [before; 8];
        changes[0].st_size += 1;
        changes[1].st_mtime += 1;
        changes[2].st_mtime_nsec += 1;
        changes[3].st_ctime += 1;
        changes[4].st_ctime_nsec += 1;
        changes[5].st_mode ^= 0o100;
        changes[6].st_ino ^= 1;
        changes[7].st_dev ^= 1;
        // When / Then: identical metadata succeeds and every independent mutation fails.
        assert!(file_require_unchanged(&before, &before).is_ok());
        for after in changes {
            assert_eq!(
                file_require_unchanged(&before, &after)
                    .unwrap_err()
                    .get_kind(),
                FileErrorKind::Changed
            );
        }
    }
}
