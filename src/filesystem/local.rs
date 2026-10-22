use crate::{
    domain::RepoPath,
    service::{
        CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileStat, FileSystem,
    },
};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{
        AtFlags, FileType, FlockOperation, Mode, OFlags, Stat, flock, fstat, mkdirat, open, openat,
        statat,
    },
    io::Errno,
};
#[cfg(test)]
use std::cell::RefCell;
use std::{error::Error, fs::File, io::Read, path::Path, time::SystemTime};

const SERVICE_DIRECTORY: &str = ".letopis";
const INDEX_LOCK: &str = "index.lock";
const INDEX_LOCK_PATH: &str = ".letopis/index.lock";
const NANOS_PER_SECOND: i64 = 1_000_000_000;
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

pub(crate) struct LocalFileSystem {
    root: OwnedFd,
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
pub(crate) struct IndexLock {
    file: OwnedFd,
}
impl Drop for IndexLock {
    fn drop(&mut self) {
        // Unlock even if a concurrent fork temporarily inherited this description.
        // Closing remains the fallback if unlocking fails during teardown.
        let _ = retry_flock(&self.file, FlockOperation::Unlock);
    }
}
impl LocalFileSystem {
    pub(crate) fn open(root: &Path) -> DocumentResult<Self> {
        let root = open(root, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|cause| fail_io("cannot open document root", cause))?;
        Ok(Self {
            root,
            #[cfg(test)]
            test_time: None,
            #[cfg(test)]
            after_read: RefCell::new(None),
            #[cfg(test)]
            before_open: RefCell::new(None),
            #[cfg(test)]
            on_contention: RefCell::new(None),
        })
    }

    #[cfg(test)]
    pub(crate) fn configure_process_test(&mut self, time: i64, hook: Box<dyn FnOnce() + Send>) {
        self.test_time = Some(time);
        self.on_contention.replace(Some(hook));
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

    pub(crate) fn open_service_directory(&self) -> DocumentResult<OwnedFd> {
        match mkdirat(&self.root, SERVICE_DIRECTORY, Mode::RWXU) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(cause) => return Err(fail_io("cannot create service directory", cause)),
        }
        openat(
            &self.root,
            SERVICE_DIRECTORY,
            DIRECTORY_FLAGS,
            Mode::empty(),
        )
        .map_err(|cause| fail_io("cannot open service directory", cause))
    }

    fn open_lock_file(&self) -> DocumentResult<OwnedFd> {
        let directory = self.open_service_directory()?;
        openat(
            directory,
            INDEX_LOCK,
            OFlags::RDWR | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        )
        .map_err(|cause| fail_io("cannot open index lock", cause))
    }

    fn acquire_lock(&self, file: &OwnedFd) -> DocumentResult<()> {
        match retry_flock(file, FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return Ok(()),
            Err(Errno::WOULDBLOCK) => {}
            Err(cause) => return Err(fail_io("cannot acquire index lock", cause)),
        }
        #[cfg(test)]
        if let Some(hook) = self.on_contention.take() {
            hook();
        }
        retry_flock(file, FlockOperation::LockExclusive)
            .map_err(|cause| fail_io("cannot acquire index lock", cause))
    }
}

impl FileSystem for LocalFileSystem {
    type IndexLock = IndexLock;

    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
        let before = self.observe_file(path.as_str())?;
        if self.observe_file(path.as_str())? != before {
            return Err(fail_changed());
        }
        Ok(before)
    }

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
        let file = self.open_lock_file()?;
        let before = observe_descriptor(&file)?;
        self.acquire_lock(&file)?;
        let guard = IndexLock { file };
        let Some(current) = self.observe_file(INDEX_LOCK_PATH)? else {
            return Err(fail_changed());
        };
        if current.inode != before.inode || current.device != before.device {
            return Err(fail_changed());
        }
        Ok(guard)
    }
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
        .map_err(|cause| fail_io("unsupported file timestamp", cause))?;
    let nanos = i64::try_from(i128::from(stat.st_mtime_nsec))
        .map_err(|cause| fail_io("unsupported file timestamp", cause))?;
    let mtime_ns = seconds
        .checked_mul(NANOS_PER_SECOND)
        .and_then(|value| value.checked_add(nanos))
        .ok_or_else(|| {
            DocumentError::new(
                DocumentErrorKind::InternalError,
                "unsupported file timestamp",
            )
        })?;
    let device = u64::try_from(i128::from(stat.st_dev))
        .map_err(|cause| fail_io("unsupported file device", cause))?;
    Ok(FileStat {
        size,
        mtime_ns,
        inode: stat.st_ino,
        device,
    })
}

fn retry_flock(file: &OwnedFd, operation: FlockOperation) -> Result<(), Errno> {
    loop {
        match flock(file, operation) {
            Err(Errno::INTR) => {}
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
mod tests {
    use super::LocalFileSystem;
    use crate::{domain::RepoPath, service::FileSystem};
    use rustix::fs::{FlockOperation, flock};
    use std::{
        fs,
        os::unix::fs::{MetadataExt, symlink},
        path::Path,
        process::Command,
        sync::mpsc,
        thread,
        time::Duration,
    };
    use tempfile::tempdir;

    fn create_fifo(path: &Path) {
        assert!(Command::new("mkfifo").arg(path).status().unwrap().success());
    }

    #[test]
    fn read_regular_file_preserves_bytes_and_stat() {
        // Arrange
        let root = tempdir().unwrap();
        let path = RepoPath::parse("заметка with spaces.md").unwrap();
        fs::write(root.path().join(path.as_str()), b"literal\0\xffbytes").unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let before = filesystem.get_time_ns().unwrap();
        // Act
        let captured = filesystem.read_file(&path).unwrap().unwrap();
        let stat = filesystem.stat_file(&path).unwrap().unwrap();
        // Assert
        assert_eq!(captured.bytes, b"literal\0\xffbytes");
        assert_eq!(captured.stat, stat);
        assert_eq!(stat.size, 14);
        assert_eq!(
            stat.inode,
            fs::metadata(root.path().join(path.as_str())).unwrap().ino()
        );
        assert!(captured.checked_at_ns >= before);
        assert!(captured.checked_at_ns <= filesystem.get_time_ns().unwrap());
    }

    #[test]
    fn reject_final_and_parent_symlinks() {
        // Arrange
        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        fs::write(outside.path().join("note.md"), b"secret").unwrap();
        symlink(outside.path().join("note.md"), root.path().join("link.md")).unwrap();
        symlink(outside.path(), root.path().join("parent")).unwrap();
        fs::create_dir(root.path().join("directory.md")).unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        for raw in ["link.md", "parent/note.md", "directory.md"] {
            let path = RepoPath::parse(raw).unwrap();
            // Act / Assert
            assert!(filesystem.read_file(&path).is_err());
            assert!(filesystem.stat_file(&path).is_err());
        }
    }

    #[test]
    fn reject_fifo_without_blocking() {
        // Arrange
        let root = tempdir().unwrap();
        create_fifo(&root.path().join("pipe.md"));
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let path = RepoPath::parse("pipe.md").unwrap();
        // Act / Assert
        assert!(filesystem.read_file(&path).is_err());
        assert!(filesystem.stat_file(&path).is_err());
    }

    #[test]
    fn missing_file_returns_none() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        for raw in ["missing.md", "missing/child.md"] {
            let path = RepoPath::parse(raw).unwrap();
            // Act / Assert
            assert!(filesystem.read_file(&path).unwrap().is_none());
            assert!(filesystem.stat_file(&path).unwrap().is_none());
        }
    }

    #[test]
    fn replacement_during_read_fails() {
        for replacement in ["regular", "fifo", "parent", "deleted", "modified"] {
            // Arrange
            let root = tempdir().unwrap();
            fs::create_dir(root.path().join("dir")).unwrap();
            let full = root.path().join("dir/note.md");
            fs::write(&full, b"before").unwrap();
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let directory = root.path().to_path_buf();
            filesystem
                .after_read
                .replace(Some(Box::new(move || match replacement {
                    "parent" => {
                        fs::rename(directory.join("dir"), directory.join("old")).unwrap();
                        symlink(directory.join("old"), directory.join("dir")).unwrap();
                    }
                    "modified" => fs::write(&full, b"different length").unwrap(),
                    other => {
                        fs::remove_file(&full).unwrap();
                        match other {
                            "regular" => fs::write(full, b"after!").unwrap(),
                            "fifo" => create_fifo(&full),
                            _ => {}
                        }
                    }
                })));
            // Act / Assert
            assert!(
                filesystem
                    .read_file(&RepoPath::parse("dir/note.md").unwrap())
                    .is_err()
            );
        }
    }

    #[test]
    fn fifo_replacement_before_open_is_rejected_without_blocking() {
        // Arrange
        let root = tempdir().unwrap();
        let path = root.path().join("note.md");
        fs::write(&path, b"regular").unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        filesystem.before_open.replace(Some(Box::new(move || {
            fs::remove_file(&path).unwrap();
            create_fifo(&path);
        })));
        let (sender, receiver) = mpsc::channel();
        // Act: timeout is a hang watchdog, not a scheduling mechanism.
        let worker = thread::spawn(move || {
            sender
                .send(filesystem.read_file(&RepoPath::parse("note.md").unwrap()))
                .unwrap();
        });
        let result = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        // Assert
        assert!(result.is_err());
        worker.join().unwrap();
    }

    #[test]
    fn lock_releases_on_drop() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let guard = filesystem.lock_index().unwrap();
        // Model a descriptor briefly inherited by a concurrent fork before exec.
        let inherited = guard.file.try_clone().unwrap();
        let path = root.path().join(".letopis/index.lock");
        let second_fd = fs::File::open(&path).unwrap();
        let inode = second_fd.metadata().unwrap().ino();
        fs::write(&path, b"persistent").unwrap();
        assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_err());
        // Act
        drop(guard);
        // Assert
        assert!(flock(&second_fd, FlockOperation::NonBlockingLockExclusive).is_ok());
        drop(second_fd);
        drop(inherited);
        let _next = filesystem.lock_index().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(fs::read(path).unwrap(), b"persistent");
    }

    #[test]
    fn contended_lock_waits_for_release() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let guard = filesystem.lock_index().unwrap();
        let second = LocalFileSystem::open(root.path()).unwrap();
        let (sender, receiver) = mpsc::channel();
        second
            .on_contention
            .replace(Some(Box::new(move || sender.send(()).unwrap())));
        let worker = thread::spawn(move || second.lock_index().unwrap());
        receiver.recv().unwrap();
        assert!(!worker.is_finished());
        // Act
        drop(guard);
        // Assert
        drop(worker.join().unwrap());
    }

    #[test]
    fn reject_service_directory_and_lock_symlinks() {
        for directory_link in [true, false] {
            // Arrange
            let root = tempdir().unwrap();
            let outside = tempdir().unwrap();
            fs::write(outside.path().join("target"), b"untouched").unwrap();
            if directory_link {
                symlink(outside.path(), root.path().join(".letopis")).unwrap();
            } else {
                fs::create_dir(root.path().join(".letopis")).unwrap();
                symlink(
                    outside.path().join("target"),
                    root.path().join(".letopis/index.lock"),
                )
                .unwrap();
            }
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            // Act / Assert
            assert!(filesystem.lock_index().is_err());
            assert_eq!(
                fs::read(outside.path().join("target")).unwrap(),
                b"untouched"
            );
        }
    }
}
