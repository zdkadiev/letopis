use super::confined::{
    DIRECTORY_FLAGS, map_io, read_stat, reject_input, require_identity, require_type, verify_entry,
};
use super::read::read_regular;
use crate::domain::RepoPath;
use crate::service::{EmbeddingFiles, EmbeddingResult};
use rustix::fs::{
    AtFlags, FileType, Mode, OFlags, Stat, fchmod, fstat, mkdirat, open, openat, statat,
};
use rustix::io::Errno;
use std::fs::{File, TryLockError};
use std::io::Write;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const SERVICE_DIRECTORY: &str = ".letopis";
const CONFIG_FILE: &str = "config.json";
const EMBED_LOCK_FILE: &str = "embed.lock";
const DIRECTORY_MODE: Mode = Mode::from_bits_retain(0o700);
const FILE_MODE: Mode = Mode::from_bits_retain(0o600);

/// Pinned checkout and service directory handles; no path reopening through symlinks.
pub(crate) struct LocalFileSystem {
    root: File,
    service: File,
}
/// Ownership of one persistent kernel lock; dropping explicitly releases it.
pub(crate) struct EmbedLock {
    file: File,
}
impl Drop for EmbedLock {
    fn drop(&mut self) {
        // Explicit unlock is required even when another descriptor retains this file.
        // Drop cannot report an unlock error; closing the owned handle still runs.
        let _ = self.file.unlock();
    }
}
impl LocalFileSystem {
    pub(crate) fn open(root: &Path) -> EmbeddingResult<Self> {
        let root = File::from(open(root, DIRECTORY_FLAGS, Mode::empty()).map_err(map_io)?);
        match mkdirat(&root, SERVICE_DIRECTORY, DIRECTORY_MODE) {
            Ok(()) | Err(Errno::EXIST) => {}
            Err(error) => return Err(map_io(error)),
        }
        let before = statat(&root, SERVICE_DIRECTORY, AtFlags::SYMLINK_NOFOLLOW).map_err(map_io)?;
        require_type(&before, FileType::Directory)?;
        let service = File::from(
            openat(&root, SERVICE_DIRECTORY, DIRECTORY_FLAGS, Mode::empty()).map_err(map_io)?,
        );
        require_identity(&before, &fstat(&service).map_err(map_io)?)?;
        fchmod(&service, DIRECTORY_MODE).map_err(map_io)?;
        let files = Self { root, service };
        files.verify_service()?;
        Ok(files)
    }

    fn verify_service(&self) -> EmbeddingResult<()> {
        verify_entry(
            &self.root,
            SERVICE_DIRECTORY,
            &fstat(&self.service).map_err(map_io)?,
        )?;
        Ok(())
    }

    fn open_lock_with(&self, observe: impl FnOnce()) -> EmbeddingResult<File> {
        self.verify_service()?;
        let before = read_stat(&self.service, EMBED_LOCK_FILE)?;
        if let Some(stat) = &before {
            require_type(stat, FileType::RegularFile)?;
        }
        observe();
        let flags = OFlags::RDWR | OFlags::NOFOLLOW | OFlags::NONBLOCK | OFlags::CLOEXEC;
        let file = if let Some(before) = &before {
            self.open_existing_lock(before, flags)?
        } else {
            self.create_lock(flags)?
        };
        let opened = fstat(&file).map_err(map_io)?;
        require_type(&opened, FileType::RegularFile)?;
        if let Some(before) = before {
            require_identity(&before, &opened)?;
        }
        self.verify_lock(&file)?;
        Ok(file)
    }

    fn verify_lock(&self, file: &File) -> EmbeddingResult<()> {
        self.verify_service()?;
        verify_entry(
            &self.service,
            EMBED_LOCK_FILE,
            &fstat(file).map_err(map_io)?,
        )?;
        Ok(())
    }

    fn open_existing_lock(&self, before: &Stat, flags: OFlags) -> EmbeddingResult<File> {
        require_type(before, FileType::RegularFile)?;
        let file = File::from(
            openat(&self.service, EMBED_LOCK_FILE, flags, Mode::empty()).map_err(map_io)?,
        );
        require_identity(before, &fstat(&file).map_err(map_io)?)?;
        Ok(file)
    }

    fn create_lock(&self, flags: OFlags) -> EmbeddingResult<File> {
        match openat(
            &self.service,
            EMBED_LOCK_FILE,
            flags | OFlags::CREATE | OFlags::EXCL,
            FILE_MODE,
        ) {
            Ok(fd) => Ok(File::from(fd)),
            Err(Errno::EXIST) => {
                let before = read_stat(&self.service, EMBED_LOCK_FILE)?.ok_or_else(reject_input)?;
                self.open_existing_lock(&before, flags)
            }
            Err(error) => Err(map_io(error)),
        }
    }
}
impl EmbeddingFiles for LocalFileSystem {
    type EmbedLock = EmbedLock;

    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        self.verify_service()?;
        let bytes = read_regular(&self.service, CONFIG_FILE, |_| {})?;
        self.verify_service()?;
        Ok(bytes)
    }

    fn read_document(&self, path: &RepoPath) -> EmbeddingResult<Option<Vec<u8>>> {
        read_regular(&self.root, path.as_str(), |_| {})
    }

    fn try_lock_embed(&self) -> EmbeddingResult<Option<EmbedLock>> {
        let file = self.open_lock_with(|| {})?;
        match file.try_lock() {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                self.verify_lock(&file)?;
                return Ok(None);
            }
            Err(TryLockError::Error(error)) => return Err(map_io(error)),
        }
        let mut guard = EmbedLock { file };
        self.verify_lock(&guard.file)?;
        fchmod(&guard.file, FILE_MODE).map_err(map_io)?;
        guard.file.set_len(0).map_err(map_io)?;
        writeln!(guard.file, "{}", std::process::id()).map_err(map_io)?;
        self.verify_lock(&guard.file)?;
        Ok(Some(guard))
    }

    fn get_time_secs(&self) -> EmbeddingResult<u64> {
        Ok(SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(map_io)?
            .as_secs())
    }

    fn wait(&self, delay: Duration) -> EmbeddingResult<()> {
        std::thread::sleep(delay);
        Ok(())
    }
}

#[cfg(test)]
#[path = "local_tests.rs"]
mod tests;
