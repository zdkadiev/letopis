use super::{
    confined::{map_io, require_identity, verify_entry},
    read::read_regular,
};
use crate::{
    domain::RepoPath,
    service::{
        CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, EmbedLocker,
        EmbeddingFiles, EmbeddingResult, FileStat, FileSystem,
    },
    utils::SERVICE_DIRECTORY,
};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{AtFlags, FileType, Mode, OFlags, Stat, fchmod, fstat, mkdirat, open, openat, statat},
    io::Errno,
};
#[cfg(test)]
use std::cell::RefCell;
use std::{
    cell::OnceCell,
    error::Error,
    fs::{File, TryLockError},
    io::{self, ErrorKind, Read, Seek, SeekFrom, Write},
    path::Path,
    time::{Duration, SystemTime},
};

const COORDINATION_LOCK_ERROR: &str = "cannot observe coordination lock";
const SERVICE_DIRECTORY_ERROR: &str = "cannot observe service directory";
const TIMESTAMP_ERROR: &str = "unsupported file timestamp";
const INDEX_LOCK: &str = "index.lock";
const EMBED_LOCK: &str = "embed.lock";
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const LOCK_FILE_FLAGS: OFlags = OFlags::RDWR
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

#[cfg(test)]
type ProcessTest = (i64, Box<dyn FnOnce() + Send>);
#[cfg(test)]
thread_local! {
    static NEXT_PROCESS_TEST: RefCell<Option<ProcessTest>> = const { RefCell::new(None) };
}

/// Pins the checkout and lazily retains one private service directory identity.
pub(crate) struct LocalFileSystem {
    root: File,
    service: OnceCell<File>,
    #[cfg(test)]
    test_time: Option<i64>,
    #[cfg(test)]
    after_read: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    before_open: RefCell<Option<Box<dyn FnOnce() + Send>>>,
    #[cfg(test)]
    on_contention: RefCell<Option<Box<dyn FnOnce() + Send>>>,
}
/// Owns the kernel lock and explicitly unlocks before closing its descriptor.
pub(crate) struct PersistentLock {
    file: File,
}
impl Drop for PersistentLock {
    fn drop(&mut self) {
        // Unlock even if a concurrent fork temporarily inherited this description.
        // Closing remains the fallback if unlocking fails during teardown.
        let _ = retry_io(|| self.file.unlock());
    }
}
impl LocalFileSystem {
    /// Pin a regular checkout directory without creating service state.
    ///
    /// Service operations initialize their directory lazily and retain its identity.
    /// # Errors
    /// Rejects symlink/non-directory roots and failures opening the directory.
    pub(crate) fn open(root: &Path) -> DocumentResult<Self> {
        let root = open(root, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|cause| fail_io("cannot open document root", cause))?;
        let filesystem = Self {
            root: File::from(root),
            service: OnceCell::new(),
            #[cfg(test)]
            test_time: None,
            #[cfg(test)]
            after_read: RefCell::new(None),
            #[cfg(test)]
            before_open: RefCell::new(None),
            #[cfg(test)]
            on_contention: RefCell::new(None),
        };
        #[cfg(test)]
        let filesystem = {
            let mut filesystem = filesystem;
            if let Some((time, hook)) = NEXT_PROCESS_TEST.take() {
                filesystem.configure_process_test(time, hook);
            }
            filesystem
        };
        Ok(filesystem)
    }

    #[cfg(test)]
    /// Install a one-use clock/contention hook for the next opener on this test thread.
    pub(crate) fn configure_next_process_test(time: i64, hook: Box<dyn FnOnce() + Send>) {
        NEXT_PROCESS_TEST.set(Some((time, hook)));
    }

    #[cfg(test)]
    /// Override this fixture's clock and consume its hook on first actual contention.
    pub(crate) fn configure_process_test(&mut self, time: i64, hook: Box<dyn FnOnce() + Send>) {
        self.test_time = Some(time);
        self.on_contention.replace(Some(hook));
    }

    /// Create or validate the private service directory and return an owned descriptor.
    ///
    /// The first successful call pins its identity; subsequent calls reject replacement.
    /// The returned duplicate does not transfer the filesystem's retained ownership.
    /// # Errors
    /// Reports unsafe/replaced paths, creation/permission failures or failed duplication.
    pub(crate) fn open_service_directory(&self) -> DocumentResult<OwnedFd> {
        let directory = self.get_service_directory()?;
        directory
            .try_clone()
            .map(OwnedFd::from)
            .map_err(|cause| fail_io("cannot duplicate service directory", cause))
    }

    /// Pin and duplicate an existing service directory without creating or chmodding it.
    ///
    /// # Errors
    /// Reports unsafe/replaced paths or failed observation/duplication.
    pub(crate) fn open_existing_service_directory(&self) -> DocumentResult<Option<OwnedFd>> {
        let Some(directory) = self.get_existing_service_directory()? else {
            return Ok(None);
        };
        directory
            .try_clone()
            .map(OwnedFd::from)
            .map(Some)
            .map_err(|cause| fail_io("cannot duplicate service directory", cause))
    }
}

impl EmbeddingFiles for LocalFileSystem {
    type EmbedLock = PersistentLock;

    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        let directory = self.get_service_directory().map_err(map_io)?;
        let bytes = read_regular(directory, "config.json", |_| {})?;
        self.get_service_directory().map_err(map_io)?;
        Ok(bytes)
    }

    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>> {
        read_regular(&self.root, path.as_str(), |_| {})
    }

    fn try_lock_embed(&self) -> EmbeddingResult<Option<Self::EmbedLock>> {
        EmbedLocker::try_lock_embed(self).map_err(map_io)
    }

    fn get_time_secs(&self) -> EmbeddingResult<u64> {
        Ok(SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(map_io)?
            .as_secs())
    }

    fn wait(&self, delay: Duration) -> EmbeddingResult<()> {
        std::thread::sleep(delay);
        Ok(())
    }
}

impl EmbedLocker for LocalFileSystem {
    type EmbedLock = PersistentLock;

    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>> {
        let file = self.open_lock_file(EMBED_LOCK)?;
        let before = observe_lock_descriptor(&file)?;
        if !self.try_acquire_lock(&file)? {
            self.verify_lock(&file, EMBED_LOCK)?;
            return Ok(None);
        }
        self.finish_lock(file, EMBED_LOCK, before).map(Some)
    }
}

impl FileSystem for LocalFileSystem {
    type IndexLock = PersistentLock;

    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        let Some(stat) = self.stat_file(path)? else {
            return Ok(None);
        };
        #[cfg(test)]
        if let Some(hook) = self.before_open.take() {
            hook();
        }
        let mut file = self.open_file(path)?;
        if observe_descriptor(&file)? != stat {
            return Err(fail_changed());
        }
        let checked_at_ns = self.get_time_ns()?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .map_err(|cause| fail_io("cannot read document file", cause))?;
        #[cfg(test)]
        if let Some(hook) = self.after_read.take() {
            hook();
        }
        self.verify_read(path, &file, stat, bytes.len())?;
        Ok(Some(CapturedFile {
            bytes,
            stat,
            checked_at_ns,
        }))
    }

    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
        let before = self.observe_file(path.as_str())?;
        if self.observe_file(path.as_str())? != before {
            return Err(fail_changed());
        }
        Ok(before)
    }

    fn get_time_ns(&self) -> DocumentResult<i64> {
        #[cfg(test)]
        if let Some(time) = self.test_time {
            return Ok(time);
        }
        let duration = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_err(|cause| fail_io("unsupported document clock", cause))?;
        i64::try_from(duration.as_nanos())
            .map_err(|cause| fail_io("unsupported document clock", cause))
    }

    fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
        let file = self.open_lock_file(INDEX_LOCK)?;
        let before = observe_lock_descriptor(&file)?;
        if !self.try_acquire_lock(&file)? {
            retry_io(|| file.lock())
                .map_err(|cause| fail_io("cannot acquire coordination lock", cause))?;
        }
        self.finish_lock(file, INDEX_LOCK, before)
    }
}

impl LocalFileSystem {
    pub(super) fn open_existing_lock_file(&self, name: &str) -> DocumentResult<Option<File>> {
        self.open_existing_lock_with(name, || {})
    }

    fn open_existing_lock_with(
        &self,
        name: &str,
        observe: impl FnOnce(),
    ) -> DocumentResult<Option<File>> {
        let Some(directory) = self.get_existing_service_directory()? else {
            return Ok(None);
        };
        let before = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => stat,
            Err(Errno::NOENT) => return Ok(None),
            Err(cause) => return Err(fail_io(COORDINATION_LOCK_ERROR, cause)),
        };
        build_file_stat(&before)?;
        observe();
        let file = File::from(
            openat(directory, name, LOCK_FILE_FLAGS, Mode::empty())
                .map_err(|cause| fail_io("cannot open coordination lock", cause))?,
        );
        let current = fstat(&file).map_err(|cause| fail_io(COORDINATION_LOCK_ERROR, cause))?;
        require_identity(&before, &current)
            .map_err(|cause| fail_io("coordination lock changed", cause))?;
        observe_lock_descriptor(&file)?;
        self.verify_existing_lock(&file, name)?;
        Ok(Some(file))
    }

    pub(super) fn verify_existing_lock(&self, file: &File, name: &str) -> DocumentResult<()> {
        let Some(directory) = self.get_existing_service_directory()? else {
            return Err(fail_changed());
        };
        let pinned = fstat(file).map_err(|cause| fail_io(COORDINATION_LOCK_ERROR, cause))?;
        verify_entry(directory, name, &pinned)
            .map(|_| ())
            .map_err(|cause| fail_io("coordination lock changed", cause))
    }

    fn open_lock_file(&self, name: &str) -> DocumentResult<File> {
        self.open_lock_with(name, || {})
    }

    fn open_lock_with(&self, name: &str, observe: impl FnOnce()) -> DocumentResult<File> {
        let directory = self.get_service_directory()?;
        // Absence is allowed to change: another owner may create this persistent
        // lock next. CREATE|EXCL and the opened-inode checks resolve that race.
        let before = match statat(directory, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(stat) => Some(stat),
            Err(Errno::NOENT) => None,
            Err(cause) => return Err(fail_io(COORDINATION_LOCK_ERROR, cause)),
        };
        if let Some(stat) = &before {
            build_file_stat(stat)?;
        }
        observe();
        let opened = match before {
            Some(_) => openat(directory, name, LOCK_FILE_FLAGS, Mode::empty()),
            None => create_lock_file(&directory, name),
        };
        let file = opened
            .map(File::from)
            .map_err(|cause| fail_io("cannot open coordination lock", cause))?;
        let current = fstat(&file).map_err(|cause| fail_io(COORDINATION_LOCK_ERROR, cause))?;
        if let Some(before) = before {
            require_identity(&before, &current)
                .map_err(|cause| fail_io("coordination lock changed", cause))?;
        }
        observe_lock_descriptor(&file)?;
        self.verify_lock(&file, name)?;
        Ok(file)
    }

    pub(super) fn try_acquire_lock(&self, file: &File) -> DocumentResult<bool> {
        let acquired = retry_lock(|| file.try_lock())
            .map_err(|cause| fail_io("cannot acquire coordination lock", cause))?;
        if acquired {
            return Ok(true);
        }
        #[cfg(test)]
        if let Some(hook) = self.on_contention.take() {
            hook();
        }
        Ok(false)
    }

    fn finish_lock(
        &self,
        file: File,
        name: &str,
        before: FileStat,
    ) -> DocumentResult<PersistentLock> {
        // Own the unlock fallback before any fallible verification or PID writes.
        let mut guard = PersistentLock { file };
        self.verify_lock(&guard.file, name)?;
        let path = format!("{SERVICE_DIRECTORY}/{name}");
        let Some(current) = self.observe_file(&path)? else {
            return Err(fail_changed());
        };
        if current.inode != before.inode || current.device != before.device {
            return Err(fail_changed());
        }
        observe_lock_descriptor(&guard.file)?;
        fchmod(&guard.file, Mode::RUSR | Mode::WUSR)
            .map_err(|cause| fail_io("cannot protect coordination lock", cause))?;
        guard
            .file
            .set_len(0)
            .and_then(|()| guard.file.seek(SeekFrom::Start(0)))
            .and_then(|_| writeln!(guard.file, "{}", std::process::id()))
            .map_err(|cause| fail_io("cannot write coordination lock owner", cause))?;
        self.verify_lock(&guard.file, name)?;
        Ok(guard)
    }

    fn verify_lock(&self, file: &File, name: &str) -> DocumentResult<()> {
        let directory = self.get_service_directory()?;
        let pinned = fstat(file).map_err(|cause| fail_io(COORDINATION_LOCK_ERROR, cause))?;
        verify_entry(directory, name, &pinned)
            .map(|_| ())
            .map_err(|cause| fail_io("coordination lock changed", cause))
    }

    fn get_service_directory(&self) -> DocumentResult<&File> {
        if self.service.get().is_none() {
            let directory = self.create_service_directory()?;
            let _ = self.service.set(directory);
        }
        let directory = self.service.get().ok_or_else(fail_changed)?;
        self.verify_service_directory(directory)?;
        Ok(directory)
    }

    fn get_existing_service_directory(&self) -> DocumentResult<Option<&File>> {
        if self.service.get().is_none() {
            let before = match statat(&self.root, SERVICE_DIRECTORY, AtFlags::SYMLINK_NOFOLLOW) {
                Ok(stat) => stat,
                Err(Errno::NOENT) => return Ok(None),
                Err(cause) => return Err(fail_io(SERVICE_DIRECTORY_ERROR, cause)),
            };
            let file = File::from(
                openat(
                    &self.root,
                    SERVICE_DIRECTORY,
                    DIRECTORY_FLAGS,
                    Mode::empty(),
                )
                .map_err(|cause| fail_io(SERVICE_DIRECTORY_ERROR, cause))?,
            );
            let opened = fstat(&file).map_err(|cause| fail_io(SERVICE_DIRECTORY_ERROR, cause))?;
            require_identity(&before, &opened)
                .map_err(|cause| fail_io("service directory changed", cause))?;
            let _ = self.service.set(file);
        }
        let directory = self.service.get().ok_or_else(fail_changed)?;
        self.verify_service_directory(directory)?;
        Ok(Some(directory))
    }

    fn verify_service_directory(&self, directory: &File) -> DocumentResult<()> {
        let pinned = fstat(directory).map_err(|cause| fail_io(SERVICE_DIRECTORY_ERROR, cause))?;
        verify_entry(&self.root, SERVICE_DIRECTORY, &pinned)
            .map(|_| ())
            .map_err(|cause| fail_io("service directory changed", cause))
    }

    fn create_service_directory(&self) -> DocumentResult<File> {
        match mkdirat(&self.root, SERVICE_DIRECTORY, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(cause) => return Err(fail_io("cannot create service directory", cause)),
        }
        let before = statat(&self.root, SERVICE_DIRECTORY, AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|cause| fail_io(SERVICE_DIRECTORY_ERROR, cause))?;
        let file = File::from(
            openat(
                &self.root,
                SERVICE_DIRECTORY,
                DIRECTORY_FLAGS,
                Mode::empty(),
            )
            .map_err(|cause| fail_io("cannot open service directory", cause))?,
        );
        let opened = fstat(&file).map_err(|cause| fail_io(SERVICE_DIRECTORY_ERROR, cause))?;
        require_identity(&before, &opened)
            .map_err(|cause| fail_io("service directory changed", cause))?;
        fchmod(&file, Mode::RWXU)
            .map_err(|cause| fail_io("cannot protect service directory", cause))?;
        Ok(file)
    }

    fn open_file(&self, path: &RepoPath) -> DocumentResult<File> {
        let (parent, name) = self
            .open_parent(path.as_str())
            .map_err(|cause| fail_io("cannot open document parent", cause))?;
        openat(parent, name, FILE_FLAGS, Mode::empty())
            .map(File::from)
            .map_err(|cause| fail_io("cannot open document file", cause))
    }

    fn verify_read(
        &self,
        path: &RepoPath,
        file: &File,
        before: FileStat,
        size: usize,
    ) -> DocumentResult<()> {
        let after = observe_descriptor(file)?;
        if after != before
            || self.observe_file(path.as_str())? != Some(before)
            || u64::try_from(size).ok() != Some(before.size)
        {
            return Err(fail_changed());
        }
        Ok(())
    }

    fn observe_file(&self, path: &str) -> DocumentResult<Option<FileStat>> {
        let observation = self
            .open_parent(path)
            .and_then(|(parent, name)| statat(parent, name, AtFlags::SYMLINK_NOFOLLOW));
        match observation {
            Ok(stat) => build_file_stat(&stat).map(Some),
            Err(Errno::NOENT) => Ok(None),
            Err(cause) => Err(fail_io("cannot observe document file", cause)),
        }
    }

    fn open_parent(&self, path: &str) -> Result<(OwnedFd, String), Errno> {
        let mut parts = path.split('/').peekable();
        let mut parent = openat(&self.root, ".", DIRECTORY_FLAGS, Mode::empty())?;
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                return Ok((parent, part.to_owned()));
            }
            parent = openat(&parent, part, DIRECTORY_FLAGS, Mode::empty())?;
        }
        Err(Errno::INVAL)
    }
}

fn create_lock_file(directory: &impl AsFd, name: &str) -> Result<OwnedFd, Errno> {
    let created = openat(
        directory,
        name,
        LOCK_FILE_FLAGS | OFlags::CREATE | OFlags::EXCL,
        Mode::RUSR | Mode::WUSR,
    );
    match created {
        Err(Errno::EXIST) => openat(directory, name, LOCK_FILE_FLAGS, Mode::empty()),
        result => reopen_concurrently_created_lock(directory, name, result),
    }
}

fn reopen_concurrently_created_lock(
    directory: &impl AsFd,
    name: &str,
    created: Result<OwnedFd, Errno>,
) -> Result<OwnedFd, Errno> {
    match created {
        // macOS can report ENOENT when another caller wins O_CREAT|O_NOFOLLOW.
        // Reopen once without creation; absent or unsafe names still fail closed.
        Err(Errno::NOENT) => openat(directory, name, LOCK_FILE_FLAGS, Mode::empty()),
        result => result,
    }
}

fn observe_lock_descriptor(file: &File) -> DocumentResult<FileStat> {
    let stat = fstat(file).map_err(|cause| fail_io(COORDINATION_LOCK_ERROR, cause))?;
    if stat.st_nlink != 1 {
        return Err(DocumentError::new(
            DocumentErrorKind::InvalidRequest,
            "private coordination lock required",
        ));
    }
    build_file_stat(&stat)
}

fn observe_descriptor(file: &impl AsFd) -> DocumentResult<FileStat> {
    let stat = fstat(file).map_err(|cause| fail_io("cannot observe open document file", cause))?;
    build_file_stat(&stat)
}

fn build_file_stat(stat: &Stat) -> DocumentResult<FileStat> {
    if FileType::from_raw_mode(stat.st_mode) != FileType::RegularFile {
        return Err(DocumentError::new(
            DocumentErrorKind::InvalidRequest,
            "regular document file required",
        ));
    }
    let size =
        u64::try_from(stat.st_size).map_err(|cause| fail_io("unsupported file size", cause))?;
    let seconds = i64::try_from(i128::from(stat.st_mtime))
        .map_err(|cause| fail_io(TIMESTAMP_ERROR, cause))?;
    let nanos = i64::try_from(i128::from(stat.st_mtime_nsec))
        .map_err(|cause| fail_io(TIMESTAMP_ERROR, cause))?;
    let mtime_ns = seconds
        .checked_mul(NANOS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| DocumentError::new(DocumentErrorKind::InternalError, TIMESTAMP_ERROR))?;
    let device = u64::try_from(i128::from(stat.st_dev))
        .map_err(|cause| fail_io("unsupported file device", cause))?;
    Ok(FileStat {
        size,
        mtime_ns,
        inode: stat.st_ino,
        device,
    })
}

/// Retry only interrupted lock attempts, returning false for ordinary contention.
fn retry_lock<F>(mut operation: F) -> Result<bool, TryLockError>
where
    F: FnMut() -> Result<(), TryLockError>,
{
    loop {
        match operation() {
            Ok(()) => return Ok(true),
            Err(TryLockError::WouldBlock) => return Ok(false),
            Err(TryLockError::Error(cause)) if cause.kind() == ErrorKind::Interrupted => {}
            Err(cause) => return Err(cause),
        }
    }
}

fn retry_io(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    loop {
        match operation() {
            Err(cause) if cause.kind() == ErrorKind::Interrupted => {}
            result => return result,
        }
    }
}

fn fail_io(message: &'static str, cause: impl Error + Send + Sync + 'static) -> DocumentError {
    DocumentError::new_with_cause(DocumentErrorKind::InternalError, message, cause)
}
fn fail_changed() -> DocumentError {
    DocumentError::new(
        DocumentErrorKind::InternalError,
        "document path changed during observation",
    )
}

#[cfg(test)]
#[path = "document_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "local_tests.rs"]
mod embedding_tests;
