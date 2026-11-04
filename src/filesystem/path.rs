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

#[cfg(test)]
type AbsenceHook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! {
    static AFTER_ABSENCE: std::cell::RefCell<Option<AbsenceHook>> = const { std::cell::RefCell::new(None) };
    static BEFORE_NESTED_PREFLIGHT: std::cell::RefCell<Option<AbsenceHook>> = const { std::cell::RefCell::new(None) };
}

#[derive(Clone, Copy)]
enum DirectoryObservation {
    Strict,
    CreatingLeaf,
}

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
        Self::directory_get_by_components_and_observation(
            files,
            components,
            DirectoryObservation::Strict,
        )
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

    fn directory_get_for_creation_by_path(
        files: &LocalFileSystem,
        path: &Path,
    ) -> FileResult<Option<Self>> {
        let components = path_get_components(path)?;
        Self::directory_get_by_components_and_observation(
            files,
            &components,
            DirectoryObservation::CreatingLeaf,
        )
    }

    fn directory_get_by_components_and_observation(
        files: &LocalFileSystem,
        components: &[OsString],
        observation: DirectoryObservation,
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
        for (index, name) in components.iter().enumerate() {
            path.push(name);
            let entry = if matches!(observation, DirectoryObservation::CreatingLeaf)
                && index + 1 == components.len()
                && !directory_is_pinned(files, &path)
            {
                file_get_entry_for_creation(&parent.directory, name)?
            } else {
                file_get_entry(&parent.directory, name)?
            };
            let Some(before) = entry else {
                if directory_is_pinned(files, &path) {
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
        #[cfg(test)]
        if path.components().count() > 1
            && let Some(hook) = BEFORE_NESTED_PREFLIGHT.with(|next| next.borrow_mut().take())
        {
            hook();
        }
        if FileParent::directory_get_for_creation_by_path(files, &path)?.is_none() {
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

fn directory_is_pinned(files: &LocalFileSystem, path: &Path) -> bool {
    files
        .directories
        .borrow()
        .keys()
        .any(|pinned| pinned.starts_with(path))
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
    let Some(before) = file_get_entry_once(parent, name)? else {
        return match file_get_entry_once(parent, name)? {
            None => Ok(None),
            Some(_) => Err(FileError::new(FileErrorKind::Changed)),
        };
    };
    Ok(Some(before))
}

/// Observe an entry once before an exclusive create or mkdir resolves peer publication.
/// Callers must validate the resulting inode and may not use this for existing pins.
pub(super) fn file_get_entry_for_creation(parent: &File, name: &OsStr) -> FileResult<Option<Stat>> {
    file_get_entry_once(parent, name)
}

fn file_get_entry_once(parent: &File, name: &OsStr) -> FileResult<Option<Stat>> {
    match statat(parent, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => Ok(Some(stat)),
        Err(Errno::NOENT) => {
            #[cfg(test)]
            if let Some(hook) = AFTER_ABSENCE.with(|next| next.borrow_mut().take()) {
                hook();
            }
            Ok(None)
        }
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
    let nanoseconds =
        i128::from(stat.st_mtime) * i128::from(NANOS_PER_SECOND) + i128::from(stat.st_mtime_nsec);
    let mtime_ns = i64::try_from(nanoseconds).map_err(file_map_cause)?;
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
pub(super) fn directory_set_before_nested_preflight(hook: AbsenceHook) {
    BEFORE_NESTED_PREFLIGHT.set(Some(hook));
}

#[cfg(test)]
pub(super) fn file_set_after_absence(hook: AbsenceHook) {
    AFTER_ABSENCE.set(Some(hook));
}

#[cfg(test)]
mod tests {
    use super::{file_build_stat, file_require_unchanged};
    use crate::service::FileErrorKind;
    use rustix::fs::fstat;
    use std::fs::File;

    #[test]
    fn represents_signed_nanosecond_boundaries() {
        // Given: platform metadata with the exact normalized i64 nanosecond boundaries.
        let file = File::open(".").unwrap();
        let mut stat = fstat(&file).unwrap();
        for (seconds, nanos, expected) in [
            (-9_223_372_037, 145_224_192, i64::MIN),
            (9_223_372_036, 854_775_807, i64::MAX),
        ] {
            stat.st_mtime = seconds;
            stat.st_mtime_nsec = nanos;
            // When: the index metadata is constructed.
            let result = file_build_stat(&stat).unwrap();
            // Then: the exact representable endpoint is retained.
            assert_eq!(result.mtime_ns, expected);
        }
    }

    #[test]
    fn rejects_nanoseconds_outside_signed_range() {
        // Given: platform metadata one nanosecond beyond each representable endpoint.
        let file = File::open(".").unwrap();
        let mut stat = fstat(&file).unwrap();
        for (seconds, nanos) in [(-9_223_372_037, 145_224_191), (9_223_372_036, 854_775_808)] {
            stat.st_mtime = seconds;
            stat.st_mtime_nsec = nanos;
            // When: the index metadata is constructed.
            let error = file_build_stat(&stat).unwrap_err();
            // Then: out-of-range metadata is an I/O error, never a saturated timestamp.
            assert_eq!(error.get_kind(), FileErrorKind::Io);
        }
    }

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
