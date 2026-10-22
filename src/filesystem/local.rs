use crate::{
    domain::RepoPath,
    service::{
        CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, EmbedLocks, FileStat,
        FileSystem,
    },
};
use rustix::{
    fd::{AsFd, OwnedFd},
    fs::{AtFlags, FileType, Mode, OFlags, Stat, fstat, mkdirat, open, openat, statat},
    io::Errno,
};
#[cfg(test)]
use std::cell::RefCell;
use std::{
    error::Error,
    fs::{File, TryLockError},
    io::{self, Read, Seek, SeekFrom, Write},
    path::Path,
    time::SystemTime,
};

const SERVICE_DIRECTORY: &str = ".letopis";
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
    pub(crate) fn open(root: &Path) -> DocumentResult<Self> {
        let root = open(root, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|cause| fail_io("cannot open document root", cause))?;
        let filesystem = Self {
            root,
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
    pub(crate) fn configure_next_process_test(time: i64, hook: Box<dyn FnOnce() + Send>) {
        NEXT_PROCESS_TEST.set(Some((time, hook)));
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

    fn open_lock_file(&self, name: &str) -> DocumentResult<File> {
        let directory = self.open_service_directory()?;
        let created = openat(
            &directory,
            name,
            LOCK_FILE_FLAGS | OFlags::CREATE,
            Mode::RUSR | Mode::WUSR,
        );
        reopen_concurrently_created_lock(&directory, name, created)
            .map(File::from)
            .map_err(|cause| fail_io("cannot open coordination lock", cause))
    }

    fn try_acquire_lock(&self, file: &File) -> DocumentResult<bool> {
        loop {
            match file.try_lock() {
                Ok(()) => return Ok(true),
                Err(TryLockError::WouldBlock) => break,
                Err(TryLockError::Error(cause)) if cause.kind() == io::ErrorKind::Interrupted => {}
                Err(cause) => return Err(fail_io("cannot acquire coordination lock", cause)),
            }
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
        let path = format!("{SERVICE_DIRECTORY}/{name}");
        let Some(current) = self.observe_file(&path)? else {
            return Err(fail_changed());
        };
        if current.inode != before.inode || current.device != before.device {
            return Err(fail_changed());
        }
        observe_lock_descriptor(&guard.file)?;
        guard
            .file
            .set_len(0)
            .and_then(|()| guard.file.seek(SeekFrom::Start(0)))
            .and_then(|_| writeln!(guard.file, "{}", std::process::id()))
            .map_err(|cause| fail_io("cannot write coordination lock owner", cause))?;
        Ok(guard)
    }
}

impl FileSystem for LocalFileSystem {
    type IndexLock = PersistentLock;

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
        let file = self.open_lock_file(INDEX_LOCK)?;
        let before = observe_lock_descriptor(&file)?;
        if !self.try_acquire_lock(&file)? {
            retry_io(|| file.lock())
                .map_err(|cause| fail_io("cannot acquire coordination lock", cause))?;
        }
        self.finish_lock(file, INDEX_LOCK, before)
    }
}

impl EmbedLocks for LocalFileSystem {
    type EmbedLock = PersistentLock;

    fn try_lock_embed(&self) -> DocumentResult<Option<Self::EmbedLock>> {
        let file = self.open_lock_file(EMBED_LOCK)?;
        let before = observe_lock_descriptor(&file)?;
        if !self.try_acquire_lock(&file)? {
            return Ok(None);
        }
        self.finish_lock(file, EMBED_LOCK, before).map(Some)
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
    let stat = fstat(file).map_err(|cause| fail_io("cannot observe coordination lock", cause))?;
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

fn retry_io(mut operation: impl FnMut() -> io::Result<()>) -> io::Result<()> {
    loop {
        match operation() {
            Err(cause) if cause.kind() == io::ErrorKind::Interrupted => {}
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
    use super::{LocalFileSystem, observe_descriptor, reopen_concurrently_created_lock};
    use crate::{
        domain::RepoPath,
        service::{DocumentErrorKind, EmbedLocks, FileSystem},
    };
    use rustix::{
        fs::{FlockOperation, flock, fstat},
        io::Errno,
    };
    use std::{
        fs,
        io::{BufRead, BufReader, Read, Write},
        os::unix::{
            fs::{MetadataExt, PermissionsExt, symlink},
            net::{UnixListener, UnixStream},
        },
        path::Path,
        process::{Child, Command, Stdio},
        sync::{Barrier, mpsc},
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
    fn index_owner_writes_pid_after_acquiring_lock() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        // Act
        let _guard = filesystem.lock_index().unwrap();
        // Assert
        assert_eq!(
            fs::read_to_string(root.path().join(".letopis/index.lock")).unwrap(),
            format!("{}\n", std::process::id())
        );
    }

    #[test]
    fn embed_probe_preserves_busy_owner_bytes_and_independent_index_lock() {
        // Arrange a stale PID longer than this process's PID.
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join(".letopis")).unwrap();
        let path = root.path().join(".letopis/embed.lock");
        fs::write(&path, b"99999999999999999999999999999999\n").unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        // Act: a stale PID does not prevent ownership; busy probes do not write.
        let guard = filesystem.try_lock_embed().unwrap().unwrap();
        let bytes = fs::read(&path).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        let _index = filesystem.lock_index().unwrap();
        assert!(filesystem.try_lock_embed().unwrap().is_none());
        // Assert
        assert_eq!(bytes, format!("{}\n", std::process::id()).as_bytes());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        drop(guard);
        let _next = filesystem.try_lock_embed().unwrap().unwrap();
        assert_eq!(fs::metadata(path).unwrap().ino(), inode);
    }

    #[test]
    fn hardlinked_lock_targets_are_rejected_without_modifying_corpus_or_external_bytes() {
        for name in ["index.lock", "embed.lock"] {
            for external in [false, true] {
                // Arrange each target on the same filesystem as its lock hard link.
                let root = tempdir().unwrap();
                let outside = tempdir().unwrap();
                let target_root = if external {
                    outside.path()
                } else {
                    root.path()
                };
                let target = target_root.join("private-note.md");
                let bytes = b"private corpus or external contents\0\xff";
                fs::write(&target, bytes).unwrap();
                fs::create_dir(root.path().join(".letopis")).unwrap();
                let path = root.path().join(".letopis").join(name);
                fs::hard_link(&target, &path).unwrap();
                let inode = fs::metadata(&target).unwrap().ino();
                let filesystem = LocalFileSystem::open(root.path()).unwrap();
                // Act
                let result = if name == "index.lock" {
                    filesystem.lock_index().map(Some)
                } else {
                    filesystem.try_lock_embed()
                };
                // Assert the error neither mutates the inode nor exposes private data.
                assert_eq!(
                    fs::read(&target).unwrap(),
                    bytes,
                    "{name} external={external}"
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                assert_eq!(fs::metadata(&target).unwrap().ino(), inode);
                assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
                let error = result.err().expect("hard-linked lock must fail");
                assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
                let diagnostic = format!("{error} {error:?}");
                assert!(!diagnostic.contains("private corpus"));
                assert!(!diagnostic.contains(target.to_str().unwrap()));
            }
        }
    }

    #[test]
    fn aliased_index_and_embed_names_are_rejected_without_mutation() {
        // Arrange
        let root = tempdir().unwrap();
        fs::create_dir(root.path().join(".letopis")).unwrap();
        let index = root.path().join(".letopis/index.lock");
        let embed = root.path().join(".letopis/embed.lock");
        fs::write(&index, b"shared lock inode bytes").unwrap();
        fs::hard_link(&index, &embed).unwrap();
        let inode = fs::metadata(&index).unwrap().ino();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        for name in ["index.lock", "embed.lock"] {
            // Act
            let result = if name == "index.lock" {
                filesystem.lock_index().map(Some)
            } else {
                filesystem.try_lock_embed()
            };
            // Assert
            assert_eq!(fs::read(&index).unwrap(), b"shared lock inode bytes");
            assert_eq!(fs::read(&embed).unwrap(), b"shared lock inode bytes");
            assert_eq!(fs::metadata(&index).unwrap().ino(), inode);
            assert_eq!(fs::metadata(&embed).unwrap().ino(), inode);
            let error = result.err().expect("aliased lock names must fail");
            assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        }
    }

    #[test]
    fn hardlink_added_during_index_contention_is_rejected_after_acquiring_ownership() {
        // Arrange an initially private lock descriptor and observe actual contention.
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let guard = filesystem.lock_index().unwrap();
        let path = root.path().join(".letopis/index.lock");
        fs::write(&path, b"holder bytes must survive").unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        let waiting = LocalFileSystem::open(root.path()).unwrap();
        let (sender, receiver) = mpsc::channel();
        waiting
            .on_contention
            .replace(Some(Box::new(move || sender.send(()).unwrap())));
        let worker = thread::spawn(move || waiting.lock_index());
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        let alias = root.path().join("note.md");
        fs::hard_link(&path, &alias).unwrap();
        // Act
        drop(guard);
        let result = worker.join().unwrap();
        // Assert the pre-acquisition check cannot hide a subsequently added link.
        assert_eq!(fs::read(&path).unwrap(), b"holder bytes must survive");
        assert_eq!(fs::read(&alias).unwrap(), b"holder bytes must survive");
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
        let error = result
            .err()
            .expect("new hard link must fail after contention");
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
        // Removing the alias permits reacquisition; failure released its kernel lock.
        fs::remove_file(alias).unwrap();
        drop(filesystem.lock_index().unwrap());
    }

    #[test]
    fn busy_embed_probe_rejects_new_hardlink_without_touching_owner_bytes() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let _guard = filesystem.try_lock_embed().unwrap().unwrap();
        let path = root.path().join(".letopis/embed.lock");
        let alias = root.path().join("note.md");
        let bytes = fs::read(&path).unwrap();
        let inode = fs::metadata(&path).unwrap().ino();
        fs::hard_link(&path, &alias).unwrap();
        // Act
        let result = filesystem.try_lock_embed();
        // Assert an unsafe descriptor is an error even when another owner is busy.
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert_eq!(fs::read(&alias).unwrap(), bytes);
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(fs::metadata(&alias).unwrap().ino(), inode);
        let error = result.err().expect("hard-linked busy lock must fail");
        assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
    }

    #[test]
    fn ordinary_hardlinked_document_reads_remain_supported() {
        // Arrange
        let root = tempdir().unwrap();
        fs::write(root.path().join("note.md"), b"unchanged document bytes").unwrap();
        fs::hard_link(root.path().join("note.md"), root.path().join("alias.md")).unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        let inode = fs::metadata(root.path().join("note.md")).unwrap().ino();
        for name in ["note.md", "alias.md"] {
            // Act
            let captured = filesystem
                .read_file(&RepoPath::parse(name).unwrap())
                .unwrap()
                .unwrap();
            // Assert lock-specific validation must not reject ordinary document hard links.
            assert_eq!(captured.bytes, b"unchanged document bytes");
            assert_eq!(captured.stat.inode, inode);
            assert_eq!(
                fs::read(root.path().join(name)).unwrap(),
                b"unchanged document bytes"
            );
        }
    }

    #[test]
    fn both_lock_names_reject_unsafe_files_and_service_directories() {
        for name in ["index.lock", "embed.lock"] {
            for kind in [
                "symlink",
                "fifo",
                "directory",
                "service_link",
                "service_file",
                "service_fifo",
            ] {
                // Arrange
                let root = tempdir().unwrap();
                let outside = tempdir().unwrap();
                let target = outside.path().join("untouched");
                fs::write(&target, b"private outside bytes").unwrap();
                let service = root.path().join(".letopis");
                match kind {
                    "service_link" => symlink(outside.path(), &service).unwrap(),
                    "service_file" => fs::write(&service, b"invalid").unwrap(),
                    "service_fifo" => create_fifo(&service),
                    _ => {
                        fs::create_dir(&service).unwrap();
                        let path = service.join(name);
                        match kind {
                            "symlink" => symlink(&target, path).unwrap(),
                            "fifo" => create_fifo(&path),
                            _ => fs::create_dir(path).unwrap(),
                        }
                    }
                }
                let filesystem = LocalFileSystem::open(root.path()).unwrap();
                // Act / Assert
                if name == "index.lock" {
                    assert!(filesystem.lock_index().is_err());
                } else {
                    assert!(filesystem.try_lock_embed().is_err());
                }
                assert_eq!(fs::read(target).unwrap(), b"private outside bytes");
            }
        }
    }

    #[test]
    fn replacement_during_index_contention_is_rejected_before_pid_write() {
        for replacement in ["regular", "fifo", "service_link"] {
            // Arrange
            let root = tempdir().unwrap();
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let guard = filesystem.lock_index().unwrap();
            let waiting = LocalFileSystem::open(root.path()).unwrap();
            let (sender, receiver) = mpsc::channel();
            waiting
                .on_contention
                .replace(Some(Box::new(move || sender.send(()).unwrap())));
            let worker = thread::spawn(move || waiting.lock_index());
            receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            let path = root.path().join(".letopis/index.lock");
            let old = root.path().join("old.lock");
            fs::rename(&path, &old).unwrap();
            match replacement {
                "regular" => fs::write(&path, b"replacement must survive").unwrap(),
                "fifo" => create_fifo(&path),
                _ => {
                    fs::rename(
                        root.path().join(".letopis"),
                        root.path().join("service-old"),
                    )
                    .unwrap();
                    symlink(
                        root.path().join("service-old"),
                        root.path().join(".letopis"),
                    )
                    .unwrap();
                }
            }
            // Act
            drop(guard);
            // Assert
            assert!(worker.join().unwrap().is_err());
            assert_eq!(
                fs::read(old).unwrap(),
                format!("{}\n", std::process::id()).as_bytes()
            );
            if replacement == "regular" {
                assert_eq!(fs::read(path).unwrap(), b"replacement must survive");
            }
        }
    }

    #[test]
    fn std_file_and_rustix_locks_exclude_each_other_for_both_names() {
        for name in ["index.lock", "embed.lock"] {
            // Arrange
            let root = tempdir().unwrap();
            let filesystem = LocalFileSystem::open(root.path()).unwrap();
            let guard = if name == "index.lock" {
                filesystem.lock_index().unwrap()
            } else {
                filesystem.try_lock_embed().unwrap().unwrap()
            };
            let file = fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open(root.path().join(".letopis").join(name))
                .unwrap();
            // Act / Assert: std owner excludes a rustix peer, then reverse roles.
            assert_eq!(
                flock(&file, FlockOperation::NonBlockingLockExclusive),
                Err(Errno::WOULDBLOCK)
            );
            drop(guard);
            flock(&file, FlockOperation::NonBlockingLockExclusive).unwrap();
            if name == "embed.lock" {
                let before = fs::read(root.path().join(".letopis/embed.lock")).unwrap();
                assert!(filesystem.try_lock_embed().unwrap().is_none());
                assert_eq!(
                    fs::read(root.path().join(".letopis/embed.lock")).unwrap(),
                    before
                );
            } else {
                let waiting = LocalFileSystem::open(root.path()).unwrap();
                let (sender, receiver) = mpsc::channel();
                waiting
                    .on_contention
                    .replace(Some(Box::new(move || sender.send(()).unwrap())));
                let worker = thread::spawn(move || waiting.lock_index().unwrap());
                receiver.recv_timeout(Duration::from_secs(5)).unwrap();
                assert!(!worker.is_finished());
                flock(&file, FlockOperation::Unlock).unwrap();
                drop(worker.join().unwrap());
            }
        }
    }

    #[test]
    fn created_lock_files_and_service_directory_have_restrictive_modes() {
        // Arrange
        let root = tempdir().unwrap();
        let filesystem = LocalFileSystem::open(root.path()).unwrap();
        // Act
        let _index = filesystem.lock_index().unwrap();
        let _embed = filesystem.try_lock_embed().unwrap().unwrap();
        // Assert
        for (name, allowed) in [
            (".letopis", 0o700),
            (".letopis/index.lock", 0o600),
            (".letopis/embed.lock", 0o600),
        ] {
            let mode = fs::metadata(root.path().join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(mode & !allowed, 0);
        }
    }

    const LOCK_CHILD: &str = "filesystem::local::tests::run_lock_child";
    const PROCESS_WAIT: Duration = Duration::from_secs(10);

    #[test]
    #[ignore = "private lock subprocess entry, invoked by parent tests"]
    fn run_lock_child() {
        let socket = std::env::var_os("TK11_LOCK_SOCKET").unwrap();
        let root = std::env::var_os("TK11_LOCK_ROOT").unwrap();
        let mode = std::env::var("TK11_LOCK_MODE").unwrap();
        let mut stream = UnixStream::connect(socket).unwrap();
        stream.set_read_timeout(Some(PROCESS_WAIT)).unwrap();
        // Keep the peer alive until the parent has configured its socket timeouts.
        stream.read_exact(&mut [0]).unwrap();
        let filesystem = LocalFileSystem::open(Path::new(&root)).unwrap();
        let mut contention = stream.try_clone().unwrap();
        if mode == "index" {
            filesystem.on_contention.replace(Some(Box::new(move || {
                writeln!(contention, "contended").unwrap();
            })));
        }
        let guard = if mode == "index" {
            filesystem.lock_index().unwrap()
        } else {
            let Some(guard) = filesystem.try_lock_embed().unwrap() else {
                eprintln!("busy {mode} pid={}", std::process::id());
                writeln!(stream, "busy").unwrap();
                return;
            };
            guard
        };
        eprintln!("owned {mode} pid={}", std::process::id());
        writeln!(stream, "owned").unwrap();
        stream.read_exact(&mut [0]).unwrap();
        drop(guard);
        eprintln!("released {mode} pid={}", std::process::id());
        writeln!(stream, "released").unwrap();
    }

    struct LockWorker {
        child: Child,
        stream: BufReader<UnixStream>,
        _socket: tempfile::TempDir,
    }

    impl LockWorker {
        fn spawn(root: &Path, mode: &str) -> Self {
            let socket = tempfile::Builder::new()
                .prefix("tk11-lock-")
                .tempdir_in("/tmp")
                .unwrap();
            let address = socket.path().join("control");
            let listener = UnixListener::bind(&address).unwrap();
            let evidence = Path::new(".artifacts/tk11/task-2/process");
            fs::create_dir_all(evidence).unwrap();
            let name = socket.path().file_name().unwrap().to_str().unwrap();
            let log = fs::File::create(evidence.join(format!("{name}.log"))).unwrap();
            let mut child = Command::new(std::env::current_exe().unwrap())
                .args(["--exact", LOCK_CHILD, "--ignored", "--nocapture"])
                .env("TK11_LOCK_SOCKET", &address)
                .env("TK11_LOCK_ROOT", root)
                .env("TK11_LOCK_MODE", mode)
                .stdout(Stdio::from(log.try_clone().unwrap()))
                .stderr(log)
                .spawn()
                .unwrap();
            let (sender, receiver) = mpsc::channel();
            let accept = thread::spawn(move || sender.send(listener.accept()).ok());
            let result = receiver.recv_timeout(PROCESS_WAIT);
            if result.is_err() {
                let _ = child.kill();
                let _ = child.wait();
                let _ = UnixStream::connect(address);
                let _ = accept.join();
                panic!("lock child did not connect: {result:?}");
            }
            let (mut stream, _) = result.unwrap().unwrap();
            accept.join().unwrap();
            stream.set_read_timeout(Some(PROCESS_WAIT)).unwrap();
            stream.set_write_timeout(Some(PROCESS_WAIT)).unwrap();
            stream.write_all(b"s").unwrap();
            Self {
                child,
                stream: BufReader::new(stream),
                _socket: socket,
            }
        }

        fn read_event(&mut self) -> String {
            let mut line = String::new();
            assert!(self.stream.read_line(&mut line).unwrap() > 0);
            line.trim().to_owned()
        }

        fn release(&mut self) {
            self.stream.get_mut().write_all(b"r").unwrap();
            assert_eq!(self.read_event(), "released");
            assert!(self.child.wait().unwrap().success());
        }
    }

    impl Drop for LockWorker {
        fn drop(&mut self) {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }

    #[test]
    fn process_owner_drop_or_death_releases_both_persistent_locks() {
        for mode in ["index", "embed"] {
            for kill in [false, true] {
                // Arrange
                let root = tempdir().unwrap();
                let mut owner = LockWorker::spawn(root.path(), mode);
                assert_eq!(owner.read_event(), "owned");
                let path = root.path().join(format!(".letopis/{mode}.lock"));
                let inode = fs::metadata(&path).unwrap().ino();
                let bytes = format!("{}\n", owner.child.id()).into_bytes();
                assert_eq!(fs::read(&path).unwrap(), bytes);
                let filesystem = LocalFileSystem::open(root.path()).unwrap();
                let mut waiter = LockWorker::spawn(root.path(), mode);
                assert_eq!(
                    waiter.read_event(),
                    if mode == "index" { "contended" } else { "busy" }
                );
                assert_eq!(fs::read(&path).unwrap(), bytes);
                // Act
                if kill {
                    owner.child.kill().unwrap();
                    assert!(!owner.child.wait().unwrap().success());
                } else {
                    owner.release();
                }
                // Assert the stale PID and persistent inode survive release/death.
                if mode == "index" {
                    assert_eq!(waiter.read_event(), "owned");
                    assert_eq!(
                        fs::read(&path).unwrap(),
                        format!("{}\n", waiter.child.id()).as_bytes()
                    );
                    waiter.release();
                    drop(filesystem.lock_index().unwrap());
                } else {
                    assert_eq!(fs::read(&path).unwrap(), bytes);
                    assert!(waiter.child.wait().unwrap().success());
                    drop(filesystem.try_lock_embed().unwrap().unwrap());
                }
                assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
                assert_eq!(
                    fs::read(path).unwrap(),
                    format!("{}\n", std::process::id()).as_bytes()
                );
            }
        }
    }

    #[test]
    fn concurrent_first_lock_creation_preserves_one_regular_file() {
        for _ in 0..20 {
            // Arrange both callers before the persistent lock file exists.
            let root = tempdir().unwrap();
            let barrier = Barrier::new(2);
            let inodes = thread::scope(|scope| {
                let workers: Vec<_> = (0..2)
                    .map(|_| {
                        let filesystem = LocalFileSystem::open(root.path()).unwrap();
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            // Act: create/open and acquire the same persistent lock.
                            let guard = filesystem.lock_index().unwrap();
                            fstat(&guard.file).unwrap().st_ino
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|worker| worker.join().unwrap())
                    .collect::<Vec<_>>()
            });
            // Assert: both calls succeed against exactly the same regular inode.
            assert_eq!(inodes[0], inodes[1]);
            assert_eq!(
                fs::metadata(root.path().join(".letopis/index.lock"))
                    .unwrap()
                    .ino(),
                inodes[0]
            );
        }
    }

    #[test]
    fn concurrent_first_embed_lock_creation_has_one_owner_and_stable_inode() {
        for _ in 0..20 {
            // Arrange both probes before the file exists, retain ownership through both probes.
            let root = tempdir().unwrap();
            let barrier = Barrier::new(2);
            let outcomes = thread::scope(|scope| {
                let workers: Vec<_> = (0..2)
                    .map(|_| {
                        let filesystem = LocalFileSystem::open(root.path()).unwrap();
                        let barrier = &barrier;
                        scope.spawn(move || {
                            barrier.wait();
                            // Act
                            let guard = filesystem.try_lock_embed().unwrap();
                            let inode = filesystem
                                .observe_file(".letopis/embed.lock")
                                .unwrap()
                                .unwrap()
                                .inode;
                            barrier.wait();
                            (inode, guard.is_some())
                        })
                    })
                    .collect();
                workers
                    .into_iter()
                    .map(|worker| worker.join().unwrap())
                    .collect::<Vec<_>>()
            });
            // Assert
            assert_eq!(outcomes[0].0, outcomes[1].0);
            assert_ne!(outcomes[0].1, outcomes[1].1);
            assert_eq!(
                fs::metadata(root.path().join(".letopis/embed.lock"))
                    .unwrap()
                    .ino(),
                outcomes[0].0
            );
        }
    }

    #[test]
    fn missing_creation_reopens_existing_lock_without_replacing_it() {
        for name in ["index.lock", "embed.lock"] {
            // Arrange the observed race outcome: a competitor created the file.
            let root = tempdir().unwrap();
            let path = root.path().join(name);
            fs::write(&path, b"persistent owner state").unwrap();
            let directory = fs::File::open(root.path()).unwrap();
            let before = fs::metadata(&path).unwrap().ino();
            // Act: select exactly the ENOENT creation-error path.
            let reopened =
                reopen_concurrently_created_lock(&directory, name, Err(Errno::NOENT)).unwrap();
            // Assert identity/bytes and no fallback for an unrelated creation error.
            assert_eq!(observe_descriptor(&reopened).unwrap().inode, before);
            assert_eq!(fs::read(&path).unwrap(), b"persistent owner state");
            assert_eq!(
                reopen_concurrently_created_lock(&directory, name, Err(Errno::ACCESS)).unwrap_err(),
                Errno::ACCESS
            );
        }
    }

    #[test]
    fn missing_creation_reopen_rejects_absent_and_unsafe_lock_names() {
        for name in ["index.lock", "embed.lock"] {
            for kind in ["absent", "symlink", "fifo", "directory"] {
                // Arrange an absent or unsafe name under the same live directory fd.
                let root = tempdir().unwrap();
                let path = root.path().join(name);
                let directory = fs::File::open(root.path()).unwrap();
                let outside = tempdir().unwrap();
                let target = outside.path().join("untouched");
                fs::write(&target, b"outside").unwrap();
                match kind {
                    "symlink" => symlink(&target, &path).unwrap(),
                    "fifo" => create_fifo(&path),
                    "directory" => fs::create_dir(&path).unwrap(),
                    _ => {}
                }
                // Act: the fallback may open a FIFO, but the regular-file check rejects it.
                let result = reopen_concurrently_created_lock(&directory, name, Err(Errno::NOENT));
                // Assert no creation/following symlinks/acceptance of nonregular files.
                if let Ok(file) = result {
                    assert!(observe_descriptor(&file).is_err());
                }
                if kind == "absent" {
                    assert!(!path.exists());
                }
                assert_eq!(fs::read(&target).unwrap(), b"outside");
            }
        }
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
        assert_eq!(fs::read(&path).unwrap(), b"persistent");
        let _next = filesystem.lock_index().unwrap();
        assert_eq!(fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(
            fs::read(path).unwrap(),
            format!("{}\n", std::process::id()).as_bytes()
        );
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
