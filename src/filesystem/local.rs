use crate::service::{
    CapturedFile, FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult,
    FileStat, FileWriter,
};
use rustix::{
    fd::OwnedFd,
    fs::{
        AtFlags, FileType, Mode, OFlags, Stat, fchmod, fstat, linkat, mkdirat, open, openat,
        statat, unlinkat,
    },
    io::Errno,
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs::{self, File, TryLockError},
    io::{self, ErrorKind, Read, Seek, SeekFrom, Write},
    num::NonZeroU32,
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Component, Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
    time::SystemTime,
};

const TEMP_FLAGS: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

const TEMP_ATTEMPTS: usize = 16;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[cfg(test)]
type DirectorySyncHook = Box<dyn Fn(&Path) + Send>;

#[cfg(test)]
type FileTest = (i64, Box<dyn FnOnce() + Send>);

#[cfg(test)]
thread_local! {
    static NEXT_SYNC_FAULT: RefCell<Option<SyncTarget>> = const { RefCell::new(None) };
}
#[cfg(test)]
thread_local! { static NEXT_FILE_TEST: RefCell<Option<FileTest>> = const { RefCell::new(None) }; }

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);

const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

#[cfg(test)]
type AbsenceHook = Box<dyn FnOnce()>;
#[cfg(test)]
thread_local! {
    static AFTER_ABSENCE: std::cell::RefCell<Option<AbsenceHook>> = const { std::cell::RefCell::new(None) };
    static BEFORE_NESTED_PREFLIGHT: std::cell::RefCell<Option<AbsenceHook>> = const { std::cell::RefCell::new(None) };
}

const NANOS_PER_SECOND: i64 = 1_000_000_000;

const LOCK_FLAGS: OFlags = OFlags::RDWR
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

const MAX_PID_BYTES: usize = 11;

const PID_READ_LIMIT: u64 = 12;

/// Which file-system object the next injected synchronization failure targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyncTarget {
    File,
    Directory,
}

struct TemporaryEntry {
    directory: File,
    name: String,
}

/// Pins one directory root and explicitly requested directory identities.
pub(crate) struct LocalFileSystem {
    root: File,
    #[cfg(test)]
    pub(super) capture_time: Option<i64>,
    #[cfg(test)]
    pub(super) mtime_seconds_override: Option<i64>,
    #[cfg(test)]
    pub(super) before_clock: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    pub(super) root_path: PathBuf,
    pub(super) directories: RefCell<BTreeMap<PathBuf, File>>,
    #[cfg(test)]
    pub(super) after_read: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) before_open: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) between_stats: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) on_contention: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) before_directory_complete: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) before_distinct_verify: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    pub(super) after_directory_sync: RefCell<Option<DirectorySyncHook>>,
}

/// Owns the kernel lock and explicitly unlocks before closing its descriptor.
#[derive(Debug)]
pub(crate) struct PersistentLock {
    pub(super) file: File,
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
struct FileParent {
    directory: File,
    links: Vec<DirectoryLink>,
}

impl TemporaryEntry {
    fn file_delete(mut self) -> FileResult<()> {
        unlinkat(&self.directory, self.name.as_str(), AtFlags::empty()).map_err(file_map_io)?;
        self.name.clear();
        Ok(())
    }
}

impl LocalFileSystem {
    /// Pins an existing directory without creating entries or changing permissions.
    ///
    /// # Arguments
    ///
    /// * `root` — the directory to pin as the confinement root; it must exist and be safe to
    ///   open.
    ///
    /// # Errors
    /// Reports unsafe roots or operating-system failures.
    pub(crate) fn new(root: &Path) -> FileResult<Self> {
        let root_path = std::path::absolute(root)
            .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
        let root = open(root, DIRECTORY_FLAGS, Mode::empty()).map_err(file_map_io)?;
        let filesystem = Self {
            root: File::from(root),
            #[cfg(test)]
            capture_time: None,
            #[cfg(test)]
            mtime_seconds_override: None,
            #[cfg(test)]
            before_clock: RefCell::new(None),
            root_path,
            directories: RefCell::new(BTreeMap::new()),
            #[cfg(test)]
            after_read: RefCell::new(None),
            #[cfg(test)]
            before_open: RefCell::new(None),
            #[cfg(test)]
            between_stats: RefCell::new(None),
            #[cfg(test)]
            on_contention: RefCell::new(None),
            #[cfg(test)]
            before_directory_complete: RefCell::new(None),
            #[cfg(test)]
            before_distinct_verify: RefCell::new(None),
            #[cfg(test)]
            after_directory_sync: RefCell::new(None),
        };
        #[cfg(test)]
        let filesystem = {
            let mut filesystem = filesystem;
            if let Some((time, hook)) = NEXT_FILE_TEST.take() {
                filesystem.configure_file_test(time, hook);
            }
            filesystem
        };
        Ok(filesystem)
    }

    #[cfg(test)]
    pub(crate) fn configure_file_test(&mut self, time: i64, hook: Box<dyn FnOnce() + Send>) {
        self.capture_time = Some(time);
        self.on_contention.replace(Some(hook));
    }

    #[cfg(test)]
    pub(crate) fn configure_next_file_test(time: i64, hook: Box<dyn FnOnce() + Send>) {
        NEXT_FILE_TEST.set(Some((time, hook)));
    }

    #[cfg(test)]
    /// Runs a fixture callback after each successful directory sync.
    ///
    /// # Arguments
    ///
    /// * `hook` — the callback invoked after every successful directory sync; it replaces
    ///   any previously installed hook.
    pub(crate) fn directory_set_after_sync(&self, hook: DirectorySyncHook) {
        self.after_directory_sync.replace(Some(hook));
    }

    /// Resolves an author-supplied path, preserving existing symlink canonicalization.
    /// This observes paths only; it does not pin or confine subsequent I/O.
    ///
    /// # Arguments
    ///
    /// * `path` — the author-supplied path to resolve; an absent leaf resolves against its
    ///   existing canonical parent.
    ///
    /// # Errors
    /// Reports unsafe paths, nonregular leaves and operating-system failures.
    pub(crate) fn file_get_resolved_path_by_path(path: &Path) -> FileResult<PathBuf> {
        file_get_resolved_path_by_path(path)
    }

    /// Returns two resolved author-supplied paths, requiring distinct paths and file identities.
    ///
    /// # Arguments
    ///
    /// * `first_path` — the first author-supplied path to resolve.
    /// * `second_path` — the second path; it must resolve to a different path and file
    ///   identity than `first_path`.
    ///
    /// # Errors
    /// Reports unsafe/nonregular paths, aliases and operating-system failures.
    pub(crate) fn file_get_distinct_paths_by_paths(
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<[PathBuf; 2]> {
        file_get_distinct_paths_by_paths(first_path, second_path)
    }

    pub(super) fn directory_get_root(&self) -> &File {
        &self.root
    }
}

impl FileParent {
    fn directory_get_by_path(
        filesystem: &LocalFileSystem,
        path: &Path,
    ) -> FileResult<Option<Self>> {
        let components = path_get_components(path)?;
        Self::directory_get_by_components(filesystem, &components)
    }

    fn directory_get_by_components(
        filesystem: &LocalFileSystem,
        components: &[OsString],
    ) -> FileResult<Option<Self>> {
        Self::directory_get_by_components_and_observation(
            filesystem,
            components,
            DirectoryObservation::Strict,
        )
    }

    /// Verifies the named ancestors and the pinned root.
    ///
    /// # Arguments
    ///
    /// * `filesystem` — the pinned filesystem whose root identity must still match.
    fn directory_verify_links(&self, filesystem: &LocalFileSystem) -> FileResult<()> {
        #[cfg(test)]
        if let Some(hook) = filesystem.before_directory_complete.take() {
            hook();
        }
        for link in &self.links {
            file_verify_entry(&link.parent, &link.name, &link.identity)?;
        }
        directory_verify_root(filesystem)
    }

    fn directory_get_for_creation_by_path(
        filesystem: &LocalFileSystem,
        path: &Path,
    ) -> FileResult<Option<Self>> {
        let components = path_get_components(path)?;
        Self::directory_get_by_components_and_observation(
            filesystem,
            &components,
            DirectoryObservation::CreatingLeaf,
        )
    }

    fn directory_get_by_components_and_observation(
        filesystem: &LocalFileSystem,
        components: &[OsString],
        observation: DirectoryObservation,
    ) -> FileResult<Option<Self>> {
        directory_verify_root(filesystem)?;
        let mut parent = Self {
            directory: filesystem
                .directory_get_root()
                .try_clone()
                .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?,
            links: Vec::new(),
        };
        let mut path = PathBuf::new();
        for (index, name) in components.iter().enumerate() {
            path.push(name);
            let entry = if matches!(observation, DirectoryObservation::CreatingLeaf)
                && index + 1 == components.len()
                && !directory_is_pinned(filesystem, &path)
            {
                file_get_entry_for_creation(&parent.directory, name)?
            } else {
                file_get_entry(&parent.directory, name)?
            };
            let Some(before) = entry else {
                if directory_is_pinned(filesystem, &path) {
                    return Err(FileError::new(FileErrorKind::Changed));
                }
                parent.directory_verify_links(filesystem)?;
                return Ok(None);
            };
            file_verify_type(&before, FileType::Directory, FileErrorKind::UnsafePath)?;
            let next = File::from(
                openat(&parent.directory, name, DIRECTORY_FLAGS, Mode::empty())
                    .map_err(file_map_io)?,
            );
            file_verify_identity(&before, &fstat(&next).map_err(file_map_io)?)?;
            if let Some(pinned) = filesystem.directories.borrow().get(&path) {
                file_verify_identity(&fstat(pinned).map_err(file_map_io)?, &before)?;
            }
            let previous = std::mem::replace(&mut parent.directory, next);
            parent.links.push(DirectoryLink {
                parent: previous,
                name: name.clone(),
                identity: before,
            });
        }
        parent.directory_verify_links(filesystem)?;
        Ok(Some(parent))
    }
}

impl Drop for TemporaryEntry {
    fn drop(&mut self) {
        if !self.name.is_empty() {
            let _ = unlinkat(&self.directory, self.name.as_str(), AtFlags::empty());
        }
    }
}

impl std::fmt::Debug for LocalFileSystem {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalFileSystem")
            .finish_non_exhaustive()
    }
}

impl Drop for PersistentLock {
    fn drop(&mut self) {
        // Unlock even if a concurrent fork temporarily inherited this description.
        // Closing remains the fallback if unlocking fails during teardown.
        let _ = file_retry_io(|| self.file.unlock());
    }
}

impl FileReader for LocalFileSystem {
    fn symlink_is_exist(&self, path: &Path) -> FileResult<bool> {
        symlink_is_exist(self, path)
    }

    fn file_get_by_path(&self, path: &Path) -> FileResult<Option<CapturedFile>> {
        file_get_by_path(self, path)
    }

    fn file_get_bytes_by_path(&self, path: &Path) -> FileResult<Option<Vec<u8>>> {
        file_get_bytes_by_path(self, path)
    }

    fn file_get_stat_by_path(&self, path: &Path) -> FileResult<Option<FileStat>> {
        file_get_stat_by_path(self, path)
    }

    fn file_get_identity_by_path(&self, path: &Path) -> FileResult<Option<FileIdentity>> {
        file_get_identity_by_path(self, path)
    }

    fn file_is_exist(&self, path: &Path) -> FileResult<bool> {
        file_is_exist(self, path)
    }

    fn file_verify_identity_by_path(
        &self,
        path: &Path,
        identity: Option<FileIdentity>,
    ) -> FileResult<()> {
        file_verify_identity_by_path(self, path, identity)
    }

    fn file_verify_distinct_by_paths(
        &self,
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<()> {
        file_verify_distinct_by_paths(self, first_path, second_path)
    }

    fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<OwnedFd>> {
        directory_get_by_path(self, path)
    }

    fn lock_get_owner_by_path(&self, path: &Path) -> FileResult<Option<NonZeroU32>> {
        lock_get_owner_by_path(self, path)
    }
}

impl FileWriter for LocalFileSystem {
    fn file_create_if_absent_by_path(&self, path: &Path, bytes: &[u8]) -> FileResult<()> {
        file_create_if_absent_by_path(self, path, bytes)
    }

    fn directory_create_by_path(&self, path: &Path) -> FileResult<OwnedFd> {
        directory_create_by_path(self, path)
    }

    fn directory_sync_by_path(&self, path: &Path) -> FileResult<()> {
        directory_sync_by_path(self, path)
    }
}

impl FileLocker for LocalFileSystem {
    type Lock = PersistentLock;

    fn lock_acquire_by_path(&self, path: &Path) -> FileResult<Self::Lock> {
        lock_acquire_by_path(self, path)
    }

    fn lock_try_acquire_by_path(&self, path: &Path) -> FileResult<Option<Self::Lock>> {
        lock_try_acquire_by_path(self, path)
    }
}

/// Resolves an author-supplied file path against its existing canonical parent.
/// Existing symlinks resolve to their regular-file target; broken symlinks fail.
/// This is path resolution only, not a retained observation or confinement guard.
///
/// # Arguments
///
/// * `path` — the author-supplied path to resolve; an absent leaf resolves against its
///   existing canonical parent.
///
/// # Errors
/// Reports empty/NUL paths, nonregular leaves, inaccessible parents or I/O failures.
fn file_get_resolved_path_by_path(path: &Path) -> FileResult<PathBuf> {
    if path.as_os_str().is_empty() || path.as_os_str().as_bytes().contains(&0) {
        return Err(FileError::new(FileErrorKind::UnsafePath));
    }
    match fs::symlink_metadata(path) {
        Ok(_) => {
            let resolved = fs::canonicalize(path).map_err(file_map_canonicalize_io)?;
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
            let resolved = fs::canonicalize(parent)
                .map_err(file_map_canonicalize_io)?
                .join(name);
            file_get_identity(&resolved)?;
            Ok(resolved)
        }
        Err(cause) => Err(file_map_canonicalize_io(cause)),
    }
}

/// Returns two resolved caller-selected paths, rejecting identical paths or hardlink aliases.
/// Existing symlinks are resolved explicitly; this does not pin or confine later I/O.
///
/// # Arguments
///
/// * `path` — the first caller-selected path to resolve and compare.
/// * `other` — the second path; it must resolve to a different path and file identity than
///   `path`.
///
/// # Errors
/// Reports unsafe paths, nonregular leaves, aliases and operating-system failures.
fn file_get_distinct_paths_by_paths(path: &Path, other: &Path) -> FileResult<[PathBuf; 2]> {
    let first = LocalFileSystem::file_get_resolved_path_by_path(path)?;
    let second = LocalFileSystem::file_get_resolved_path_by_path(other)?;
    let first_identity = file_get_identity(&first)?;
    let second_identity = file_get_identity(&second)?;
    if first == second || first_identity.is_some() && first_identity == second_identity {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok([first, second])
}

fn file_get_identity(path: &Path) -> FileResult<Option<FileIdentity>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(FileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
        })),
        Ok(_) => Err(FileError::new(FileErrorKind::NonRegular)),
        Err(cause) if cause.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(cause) => Err(file_map_canonicalize_io(cause)),
    }
}

fn file_map_canonicalize_io(cause: io::Error) -> FileError {
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

fn symlink_is_exist(filesystem: &LocalFileSystem, path: &Path) -> FileResult<bool> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(filesystem, &components)? else {
        return Ok(false);
    };
    let observed = file_get_entry(&parent.directory, &name)?;
    let is_symlink = if let Some(before) = observed {
        file_verify_entry(&parent.directory, &name, &before)?;
        FileType::from_raw_mode(before.st_mode) == FileType::Symlink
    } else {
        false
    };
    parent.directory_verify_links(filesystem)?;
    Ok(is_symlink)
}

fn file_get_by_path(filesystem: &LocalFileSystem, path: &Path) -> FileResult<Option<CapturedFile>> {
    let captured = file_get_content_by_path(filesystem, path, || {
        let checked_at_ns = file_get_time_ns(filesystem)?;
        #[cfg(test)]
        let checked_at_ns = filesystem.capture_time.unwrap_or(checked_at_ns);
        Ok(checked_at_ns)
    })?;
    let Some((bytes, opened, checked_at_ns)) = captured else {
        return Ok(None);
    };
    Ok(Some(CapturedFile {
        bytes,
        stat: file_build_stat(&opened)?,
        checked_at_ns,
    }))
}

/// Reads a file under one verified observation.
/// The caller metadata is sampled immediately before I/O.
///
/// # Arguments
///
/// * `filesystem` — the pinned filesystem whose root and ancestors are verified around the read.
/// * `path` — the file path to read; an absent file yields `None`.
/// * `before_read` — the callback sampled immediately before the read, returning caller
///   metadata to accompany the bytes.
fn file_get_content_by_path<T, F>(
    filesystem: &LocalFileSystem,
    path: &Path,
    before_read: F,
) -> FileResult<Option<(Vec<u8>, Stat, T)>>
where
    F: FnOnce() -> FileResult<T>,
{
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(filesystem, &components)? else {
        return Ok(None);
    };
    let Some(before) =
        file_get_observation_by_parent_and_name(filesystem, &parent, &name, file_verify_unchanged)?
    else {
        return Ok(None);
    };
    #[cfg(test)]
    if let Some(hook) = filesystem.before_open.take() {
        hook();
    }
    let mut file = File::from(
        openat(&parent.directory, &name, FILE_FLAGS, Mode::empty())
            .map_err(file_map_observed_open_io)?,
    );
    let opened = fstat(&file).map_err(file_map_io)?;
    file_verify_unchanged(&before, &opened)?;
    let observation = before_read()?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    #[cfg(test)]
    if let Some(hook) = filesystem.after_read.take() {
        hook();
    }
    file_verify_unchanged(&opened, &fstat(&file).map_err(file_map_io)?)?;
    file_verify_unchanged(
        &opened,
        &file_verify_entry(&parent.directory, &name, &opened)?,
    )?;
    parent.directory_verify_links(filesystem)?;
    directory_verify_root(filesystem)?;
    #[cfg(test)]
    let opened = {
        let mut opened = opened;
        if let Some(seconds) = filesystem.mtime_seconds_override {
            opened.st_mtime = seconds;
            opened.st_mtime_nsec = 0;
        }
        opened
    };
    Ok(Some((bytes, opened, observation)))
}

fn file_get_time_ns(_files: &LocalFileSystem) -> FileResult<i64> {
    #[cfg(test)]
    if let Some(hook) = _files.before_clock.take() {
        hook();
    }
    let elapsed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    i64::try_from(elapsed.as_nanos())
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))
}

fn file_get_verified_identity_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
    identity: Option<FileIdentity>,
) -> FileResult<Option<FileIdentity>> {
    let current = filesystem.file_get_identity_by_path(path)?;
    if identity.is_some() && current != identity {
        return Err(FileError::new(FileErrorKind::Changed));
    }
    Ok(current)
}

fn file_get_observation_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
    verify: fn(&Stat, &Stat) -> FileResult<()>,
) -> FileResult<Option<Stat>> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(filesystem, &components)? else {
        return Ok(None);
    };
    file_get_observation_by_parent_and_name(filesystem, &parent, &name, verify)
}

/// Returns the regular-file observation after verifying two reads and the ancestry.
/// Replacement remains an integrity error even when either observation is nonregular.
///
/// # Arguments
///
/// * `filesystem` — the pinned filesystem whose root and ancestor links are verified.
/// * `parent` — the opened parent directory holding the entry.
/// * `name` — the entry name to observe twice.
/// * `verify` — the comparison applied to the two observations to detect replacement.
fn file_get_observation_by_parent_and_name(
    filesystem: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    verify: fn(&Stat, &Stat) -> FileResult<()>,
) -> FileResult<Option<Stat>> {
    let before = file_get_entry(&parent.directory, name)?;
    #[cfg(test)]
    if let Some(hook) = filesystem.between_stats.take() {
        hook();
    }
    let after = file_get_entry(&parent.directory, name)?;
    match (&before, &after) {
        (None, None) => {}
        (Some(before), Some(after)) => verify(before, after)?,
        _ => return Err(FileError::new(FileErrorKind::Changed)),
    }
    parent.directory_verify_links(filesystem)?;
    directory_verify_root(filesystem)?;
    if let Some(stat) = &before {
        file_verify_type(stat, FileType::RegularFile, FileErrorKind::NonRegular)?;
    }
    Ok(before)
}

fn file_build_stat(stat: &Stat) -> FileResult<FileStat> {
    let nanoseconds =
        i128::from(stat.st_mtime) * i128::from(NANOS_PER_SECOND) + i128::from(stat.st_mtime_nsec);
    let mtime_ns = i64::try_from(nanoseconds)
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    Ok(FileStat {
        size: u64::try_from(stat.st_size)
            .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?,
        mtime_ns,
        inode: stat.st_ino,
        device: file_build_identity(stat)?.device,
    })
}

fn file_get_bytes_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<Vec<u8>>> {
    Ok(file_get_content_by_path(filesystem, path, || Ok(()))?.map(|(bytes, _, ())| bytes))
}

fn file_get_stat_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<FileStat>> {
    file_get_observation_by_path(filesystem, path, file_verify_unchanged)?
        .as_ref()
        .map(file_build_stat)
        .transpose()
}

fn file_get_identity_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<FileIdentity>> {
    file_get_observation_by_path(filesystem, path, file_verify_identity)?
        .as_ref()
        .map(file_build_identity)
        .transpose()
}

fn file_is_exist(filesystem: &LocalFileSystem, path: &Path) -> FileResult<bool> {
    Ok(filesystem.file_get_identity_by_path(path)?.is_some())
}

fn file_verify_identity_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
    identity: Option<FileIdentity>,
) -> FileResult<()> {
    file_get_verified_identity_by_path(filesystem, path, identity).map(|_| ())
}

fn file_verify_distinct_by_paths(
    filesystem: &LocalFileSystem,
    path: &Path,
    other: &Path,
) -> FileResult<()> {
    let first: PathBuf = path_get_components(path)?.iter().collect();
    let second: PathBuf = path_get_components(other)?.iter().collect();
    let before = filesystem.file_get_identity_by_path(path)?;
    let after = filesystem.file_get_identity_by_path(other)?;
    if first == second || before.is_some() && before == after {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    #[cfg(test)]
    if let Some(hook) = filesystem.before_distinct_verify.take() {
        hook();
    }
    let current_first = file_get_verified_identity_by_path(filesystem, path, before)?;
    let current_second = file_get_verified_identity_by_path(filesystem, other, after)?;
    if current_first.is_some() && current_first == current_second {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok(())
}

fn file_verify_unchanged(before: &Stat, after: &Stat) -> FileResult<()> {
    file_verify_identity(before, after)?;
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

fn directory_get_by_path(filesystem: &LocalFileSystem, path: &Path) -> FileResult<Option<OwnedFd>> {
    let Some(parent) = FileParent::directory_get_by_path(filesystem, path)? else {
        return Ok(None);
    };
    let normalized: PathBuf = path_get_components(path)?.iter().collect();
    if !normalized.as_os_str().is_empty()
        && !filesystem.directories.borrow().contains_key(&normalized)
    {
        filesystem.directories.borrow_mut().insert(
            normalized,
            parent
                .directory
                .try_clone()
                .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?,
        );
    }
    parent.directory_verify_links(filesystem)?;
    parent
        .directory
        .try_clone()
        .map(OwnedFd::from)
        .map(Some)
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))
}

fn lock_get_owner_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<NonZeroU32>> {
    let (components, name) = path_get_file_components(path)?;
    let Some(parent) = FileParent::directory_get_by_components(filesystem, &components)? else {
        return Ok(None);
    };
    let Some(before) = file_get_entry(&parent.directory, &name)? else {
        parent.directory_verify_links(filesystem)?;
        return Ok(None);
    };
    file_verify_lock_stat(&before)?;
    #[cfg(test)]
    if let Some(hook) = filesystem.before_open.take() {
        hook();
    }
    let mut file = File::from(
        openat(&parent.directory, &name, LOCK_FLAGS, Mode::empty())
            .map_err(file_map_observed_open_io)?,
    );
    file_verify_identity(&before, &file_get_lock_stat(&file)?)?;
    file_verify_lock(filesystem, &parent, &name, &file)?;
    if lock_try_acquire(filesystem, &file)? {
        let guard = PersistentLock { file };
        file_verify_lock(filesystem, &parent, &name, &guard.file)?;
        return Ok(None);
    }
    let pid = lock_get_pid(&mut file)?;
    file_verify_lock(filesystem, &parent, &name, &file)?;
    directory_verify_root(filesystem)?;
    Ok(Some(pid))
}

fn file_create_if_absent_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
    bytes: &[u8],
) -> FileResult<()> {
    let (components, name) = path_get_file_components(path)?;
    let parent = FileParent::directory_get_by_components(filesystem, &components)?
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound))?;
    if let Some(before) = file_get_entry(&parent.directory, &name)? {
        file_verify_type(&before, FileType::RegularFile, FileErrorKind::NonRegular)?;
        parent.directory_verify_links(filesystem)?;
        directory_sync(&parent.directory)?;
        parent.directory_verify_links(filesystem)?;
        return Ok(());
    }
    let (temporary, mut file) = file_create_temporary(&parent.directory)?;
    file_retry_io(|| file.write_all(bytes))
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    fchmod(&file, Mode::RUSR | Mode::WUSR).map_err(file_map_io)?;
    file_sync(&file)?;
    parent.directory_verify_links(filesystem)?;
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
            file_verify_type(&winner, FileType::RegularFile, FileErrorKind::NonRegular)?;
        }
        Err(cause) => return Err(file_map_io(cause)),
    }
    temporary.file_delete()?;
    directory_sync(&parent.directory)?;
    parent.directory_verify_links(filesystem)?;
    directory_verify_root(filesystem)
}

fn file_create_temporary(directory: &File) -> FileResult<(TemporaryEntry, File)> {
    // Own cleanup before creating an entry so descriptor-duplication failure leaks nothing.
    let directory = directory
        .try_clone()
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
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
    file_retry_io(|| file_sync_data(file))
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))
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
    file_retry_io(|| directory.sync_all())
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))
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

fn directory_create_by_path(filesystem: &LocalFileSystem, path: &Path) -> FileResult<OwnedFd> {
    let components = path_get_components(path)?;
    let parent = directory_create_components(filesystem, &components)?;
    fchmod(&parent.directory, Mode::RWXU).map_err(file_map_io)?;
    parent.directory_verify_links(filesystem)?;
    filesystem
        .directory_get_by_path(path)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))
}

/// Creates missing directory components and returns the pinned parent of the leaf.
///
/// # Arguments
///
/// * `filesystem` — the confined filesystem whose root pins every observed component.
/// * `components` — normalized root-relative components to create in order.
///
/// # Errors
/// Reports unsafe or replaced components and any directory creation failure.
fn directory_create_components(
    filesystem: &LocalFileSystem,
    components: &[OsString],
) -> FileResult<FileParent> {
    let mut path = PathBuf::new();
    for name in components {
        let parent = FileParent::directory_get_by_path(
            filesystem,
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
            && let Some(hook) = before_nested_preflight_take()
        {
            hook();
        }
        if FileParent::directory_get_for_creation_by_path(filesystem, &path)?.is_none() {
            match mkdirat(&parent.directory, name, Mode::RWXU) {
                Ok(()) | Err(Errno::EXIST) => {}
                Err(cause) => return Err(file_map_io(cause)),
            }
        }
        parent.directory_verify_links(filesystem)?;
        FileParent::directory_get_by_path(filesystem, &path)?
            .ok_or_else(|| FileError::new(FileErrorKind::Changed))?;
    }
    FileParent::directory_get_by_components(filesystem, components)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))
}

fn directory_sync_by_path(filesystem: &LocalFileSystem, path: &Path) -> FileResult<()> {
    let parent = FileParent::directory_get_by_path(filesystem, path)?
        .ok_or_else(|| FileError::new(FileErrorKind::NotFound))?;
    directory_sync(&parent.directory)?;
    #[cfg(test)]
    if let Some(hook) = filesystem.after_directory_sync.borrow().as_ref() {
        hook(path);
    }
    parent.directory_verify_links(filesystem)?;
    directory_verify_root(filesystem)
}

fn lock_acquire_by_path(filesystem: &LocalFileSystem, path: &Path) -> FileResult<PersistentLock> {
    let (parent, name, file) = lock_get_file_by_path(filesystem, path)?;
    let before = file_get_lock_stat(&file)?;
    if !lock_try_acquire(filesystem, &file)? {
        file_retry_io(|| file.lock())
            .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    }
    lock_finish_acquisition(filesystem, &parent, &name, file, before)
}

fn lock_try_acquire_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<Option<PersistentLock>> {
    let (parent, name, file) = lock_get_file_by_path(filesystem, path)?;
    let before = file_get_lock_stat(&file)?;
    if !lock_try_acquire(filesystem, &file)? {
        file_verify_lock(filesystem, &parent, &name, &file)?;
        return Ok(None);
    }
    lock_finish_acquisition(filesystem, &parent, &name, file, before).map(Some)
}

fn lock_get_file_by_path(
    filesystem: &LocalFileSystem,
    path: &Path,
) -> FileResult<(FileParent, OsString, File)> {
    let (components, name) = path_get_file_components(path)?;
    if !components.is_empty() {
        let directory_path: PathBuf = components.iter().collect();
        filesystem.directory_create_by_path(&directory_path)?;
    }
    let parent = FileParent::directory_get_by_components(filesystem, &components)?
        .ok_or_else(|| FileError::new(FileErrorKind::Changed))?;
    let before = file_get_entry_for_creation(&parent.directory, &name)?;
    if let Some(before) = &before {
        file_verify_lock_stat(before)?;
    }
    #[cfg(test)]
    if let Some(hook) = filesystem.before_open.take() {
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
        file_verify_identity(&before, &current)?;
    }
    file_verify_lock(filesystem, &parent, &name, &file)?;
    Ok((parent, name, file))
}

fn lock_finish_acquisition(
    filesystem: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    file: File,
    before: Stat,
) -> FileResult<PersistentLock> {
    // Install the unlock fallback before any fallible validation or owner mutation.
    let mut guard = PersistentLock { file };
    file_verify_identity(&before, &file_get_lock_stat(&guard.file)?)?;
    file_verify_lock(filesystem, parent, name, &guard.file)?;
    fchmod(&guard.file, Mode::RUSR | Mode::WUSR).map_err(file_map_io)?;
    guard
        .file
        .set_len(0)
        .and_then(|()| guard.file.seek(SeekFrom::Start(0)))
        .and_then(|_| writeln!(guard.file, "{}", std::process::id()))
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
    file_verify_lock(filesystem, parent, name, &guard.file)?;
    directory_verify_root(filesystem)?;
    Ok(guard)
}

fn lock_try_acquire(_files: &LocalFileSystem, file: &File) -> FileResult<bool> {
    if lock_retry_acquisition(|| file.try_lock())
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?
    {
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
    file_verify_lock_stat(&stat)?;
    Ok(stat)
}

fn file_verify_lock_stat(stat: &Stat) -> FileResult<()> {
    file_verify_type(stat, FileType::RegularFile, FileErrorKind::NonRegular)?;
    if stat.st_nlink != 1 {
        return Err(FileError::new(FileErrorKind::Aliased));
    }
    Ok(())
}

fn file_verify_lock(
    filesystem: &LocalFileSystem,
    parent: &FileParent,
    name: &OsStr,
    file: &File,
) -> FileResult<()> {
    let current = file_get_lock_stat(file)?;
    let named = file_verify_entry(&parent.directory, name, &current)?;
    file_verify_lock_stat(&named)?;
    parent.directory_verify_links(filesystem)
}

fn lock_get_pid(file: &mut File) -> FileResult<NonZeroU32> {
    let mut bytes = Vec::new();
    file.take(PID_READ_LIMIT)
        .read_to_end(&mut bytes)
        .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?;
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

pub(super) fn file_retry_io(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    loop {
        match operation() {
            Err(cause) if cause.kind() == ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

fn directory_verify_root(filesystem: &LocalFileSystem) -> FileResult<()> {
    let current = open(&filesystem.root_path, DIRECTORY_FLAGS, Mode::empty())
        .map_err(|cause| FileError::new_with_cause(FileErrorKind::Changed, cause))?;
    file_verify_identity(
        &fstat(filesystem.directory_get_root()).map_err(file_map_io)?,
        &fstat(current).map_err(file_map_io)?,
    )
}

fn directory_is_pinned(filesystem: &LocalFileSystem, path: &Path) -> bool {
    filesystem
        .directories
        .borrow()
        .keys()
        .any(|pinned| pinned.starts_with(path))
}

fn path_get_components(path: &Path) -> FileResult<Vec<OsString>> {
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

fn path_get_file_components(path: &Path) -> FileResult<(Vec<OsString>, OsString)> {
    let mut components = path_get_components(path)?;
    let name = components
        .pop()
        .ok_or_else(|| FileError::new(FileErrorKind::UnsafePath))?;
    Ok((components, name))
}

fn file_get_entry(parent: &File, name: &OsStr) -> FileResult<Option<Stat>> {
    let Some(before) = file_get_entry_once(parent, name)? else {
        return match file_get_entry_once(parent, name)? {
            None => Ok(None),
            Some(_) => Err(FileError::new(FileErrorKind::Changed)),
        };
    };
    Ok(Some(before))
}

/// Returns one observation of an entry before an exclusive create or mkdir.
/// Callers must validate the resulting inode and may not use this for existing pins.
///
/// # Arguments
///
/// * `parent` — the opened directory in which the entry is observed.
/// * `name` — the entry name; its absence is returned as `None` without retrying.
fn file_get_entry_for_creation(parent: &File, name: &OsStr) -> FileResult<Option<Stat>> {
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

fn file_verify_entry(parent: &File, name: &OsStr, before: &Stat) -> FileResult<Stat> {
    let current = statat(parent, name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|cause| FileError::new_with_cause(FileErrorKind::Changed, cause))?;
    file_verify_identity(before, &current)?;
    Ok(current)
}

fn file_verify_identity(before: &Stat, after: &Stat) -> FileResult<()> {
    if before.st_dev != after.st_dev
        || before.st_ino != after.st_ino
        || FileType::from_raw_mode(before.st_mode) != FileType::from_raw_mode(after.st_mode)
    {
        return Err(FileError::new(FileErrorKind::Changed));
    }
    Ok(())
}

fn file_verify_type(stat: &Stat, expected: FileType, kind: FileErrorKind) -> FileResult<()> {
    if FileType::from_raw_mode(stat.st_mode) != expected {
        return Err(FileError::new(kind));
    }
    Ok(())
}

fn file_build_identity(stat: &Stat) -> FileResult<FileIdentity> {
    Ok(FileIdentity {
        device: u64::try_from(i128::from(stat.st_dev))
            .map_err(|error| FileError::new_with_cause(FileErrorKind::Io, error))?,
        inode: stat.st_ino,
    })
}

/// Returns the error for opening a leaf already observed as present.
/// Disappearance is an integrity change; other OS classifications remain intact.
///
/// # Arguments
///
/// * `cause` — the operating-system error from opening the observed leaf.
fn file_map_observed_open_io(cause: Errno) -> FileError {
    if cause == Errno::NOENT {
        FileError::new_with_cause(FileErrorKind::Changed, cause)
    } else {
        file_map_io(cause)
    }
}

fn file_map_io(cause: Errno) -> FileError {
    let kind = match cause {
        Errno::LOOP | Errno::NOTDIR => FileErrorKind::UnsafePath,
        _ => FileErrorKind::Io,
    };
    FileError::new_with_cause(kind, cause)
}

#[cfg(test)]
fn before_nested_preflight_take() -> Option<AbsenceHook> {
    BEFORE_NESTED_PREFLIGHT.with(|next| next.borrow_mut().take())
}

#[cfg(test)]
fn directory_set_before_nested_preflight(hook: AbsenceHook) {
    BEFORE_NESTED_PREFLIGHT.set(Some(hook));
}

#[cfg(test)]
fn file_set_after_absence(hook: AbsenceHook) {
    AFTER_ABSENCE.set(Some(hook));
}

#[cfg(test)]
pub(super) fn file_fail_next_sync(target: SyncTarget) {
    NEXT_SYNC_FAULT.set(Some(target));
}

#[cfg(test)]
#[path = "local_test.rs"]
mod tests;
