use super::path::{DIRECTORY_FLAGS, file_map_cause, file_map_io};
use crate::service::{
    CapturedFile, FileIdentity, FileLocker, FileReader, FileResult, FileStat, FileWriter,
};
use rustix::{
    fd::OwnedFd,
    fs::{Mode, open},
};
use std::{
    cell::RefCell,
    collections::BTreeMap,
    fs::File,
    io::{self, ErrorKind},
    num::NonZeroU32,
    path::{Path, PathBuf},
};

#[cfg(test)]
type DirectorySyncHook = Box<dyn Fn(&Path) + Send>;

/// Pins one directory root and explicitly requested directory identities.
pub(crate) struct LocalFileSystem {
    root: File,
    #[cfg(test)]
    pub(super) capture_time: Option<i64>,
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
impl LocalFileSystem {
    /// Pin an existing directory without creating entries or changing permissions.
    /// # Errors
    /// Reports unsafe roots or operating-system failures.
    pub(crate) fn new(root: &Path) -> FileResult<Self> {
        let root_path = std::path::absolute(root).map_err(file_map_cause)?;
        let root = open(root, DIRECTORY_FLAGS, Mode::empty()).map_err(file_map_io)?;
        let filesystem = Self {
            root: File::from(root),
            #[cfg(test)]
            capture_time: None,
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
    /// Run a fixture callback after each successful directory sync.
    pub(crate) fn directory_set_after_sync(&self, hook: DirectorySyncHook) {
        self.after_directory_sync.replace(Some(hook));
    }
    pub(super) fn directory_get_root(&self) -> &File {
        &self.root
    }
}

impl LocalFileSystem {
    /// Resolve an author-supplied path, preserving existing symlink canonicalization.
    /// This observes paths only; it does not pin or confine subsequent I/O.
    ///
    /// # Errors
    /// Reports unsafe paths, nonregular leaves and operating-system failures.
    pub(crate) fn file_get_resolved_path_by_path(path: &Path) -> FileResult<PathBuf> {
        super::resolve::file_get_resolved_path_by_path(path)
    }

    /// Resolve two author-supplied paths and require distinct paths and file identities.
    ///
    /// # Errors
    /// Reports unsafe/nonregular paths, aliases and operating-system failures.
    pub(crate) fn file_get_distinct_paths_by_paths(
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<[PathBuf; 2]> {
        super::resolve::file_get_distinct_paths_by_paths(first_path, second_path)
    }
}

impl FileReader for LocalFileSystem {
    fn file_get_by_path(&self, path: &Path) -> FileResult<Option<CapturedFile>> {
        super::file::file_get_by_path(self, path)
    }
    fn file_get_stat_by_path(&self, path: &Path) -> FileResult<Option<FileStat>> {
        super::file::file_get_stat_by_path(self, path)
    }
    fn file_get_identity_by_path(&self, path: &Path) -> FileResult<Option<FileIdentity>> {
        super::file::file_get_identity_by_path(self, path)
    }
    fn file_is_exist(&self, path: &Path) -> FileResult<bool> {
        super::file::file_is_exist(self, path)
    }
    fn file_verify_identity_by_path(
        &self,
        path: &Path,
        identity: Option<FileIdentity>,
    ) -> FileResult<()> {
        super::file::file_verify_identity_by_path(self, path, identity)
    }
    fn file_verify_distinct_by_paths(
        &self,
        first_path: &Path,
        second_path: &Path,
    ) -> FileResult<()> {
        super::file::file_verify_distinct_by_paths(self, first_path, second_path)
    }
    fn directory_get_by_path(&self, path: &Path) -> FileResult<Option<OwnedFd>> {
        super::file::directory_get_by_path(self, path)
    }
    fn lock_get_owner_by_path(&self, path: &Path) -> FileResult<Option<NonZeroU32>> {
        super::lock::lock_get_owner_by_path(self, path)
    }
}

impl FileWriter for LocalFileSystem {
    fn file_create_if_absent_by_path(&self, path: &Path, bytes: &[u8]) -> FileResult<()> {
        super::publication::file_create_if_absent_by_path(self, path, bytes)
    }
    fn directory_create_by_path(&self, path: &Path) -> FileResult<OwnedFd> {
        super::publication::directory_create_by_path(self, path)
    }
    fn directory_sync_by_path(&self, path: &Path) -> FileResult<()> {
        super::publication::directory_sync_by_path(self, path)
    }
}

impl FileLocker for LocalFileSystem {
    type Lock = PersistentLock;
    fn lock_acquire_by_path(&self, path: &Path) -> FileResult<Self::Lock> {
        super::lock::lock_acquire_by_path(self, path)
    }
    fn lock_try_acquire_by_path(&self, path: &Path) -> FileResult<Option<Self::Lock>> {
        super::lock::lock_try_acquire_by_path(self, path)
    }
}

pub(super) fn file_retry_io(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    loop {
        match operation() {
            Err(cause) if cause.kind() == ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

#[cfg(test)]
type FileTest = (i64, Box<dyn FnOnce() + Send>);
#[cfg(test)]
thread_local! { static NEXT_FILE_TEST: RefCell<Option<FileTest>> = const { RefCell::new(None) }; }
#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;
