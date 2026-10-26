use super::{local::LocalFileSystem, read::read_regular};
use crate::service::{
    CommandError, CommandErrorKind, CommandResult, DocumentError, EmbeddingError,
    EmbeddingErrorKind, EmbeddingResult, HeldLock, LockName, StatusFiles,
};
use std::{fs::File, io::Read, num::NonZeroU32};

const MAX_PID_BYTES: u64 = 12;
const INVALID_LOCK_PID: &str = "cannot observe coordination lock owner";

impl StatusFiles for LocalFileSystem {
    fn read_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        let Some(directory) = self
            .open_existing_service_directory()
            .map_err(map_config_error)?
        else {
            return Ok(None);
        };
        let bytes = read_regular(&File::from(directory), "config.json", |_| {})?;
        self.open_existing_service_directory()
            .map_err(map_config_error)?;
        Ok(bytes)
    }

    fn observe_locks(&self) -> CommandResult<Vec<HeldLock>> {
        let mut locks = Vec::new();
        if let Some(lock) = self.observe_lock("index.lock", LockName::Index)? {
            locks.push(lock);
        }
        if let Some(lock) = self.observe_lock("embed.lock", LockName::Embed)? {
            locks.push(lock);
        }
        Ok(locks)
    }
}

impl LocalFileSystem {
    fn observe_lock(&self, path: &str, name: LockName) -> CommandResult<Option<HeldLock>> {
        let Some(mut file) = self.open_existing_lock_file(path).map_err(map_lock_error)? else {
            return Ok(None);
        };
        let acquired = self.try_acquire_lock(&file).map_err(map_lock_error)?;
        if acquired {
            self.verify_existing_lock(&file, path)
                .map_err(map_lock_error)?;
            file.unlock().map_err(map_lock_error)?;
            return Ok(None);
        }
        let pid = read_pid(&mut file)?;
        self.verify_existing_lock(&file, path)
            .map_err(map_lock_error)?;
        Ok(Some(HeldLock { name, pid }))
    }
}

fn read_pid(file: &mut File) -> CommandResult<NonZeroU32> {
    let mut bytes = Vec::new();
    file.take(MAX_PID_BYTES)
        .read_to_end(&mut bytes)
        .map_err(map_lock_error)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| invalid_pid())?;
    let digits = text.strip_suffix('\n').unwrap_or(text);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(invalid_pid());
    }
    digits
        .parse::<u32>()
        .ok()
        .and_then(NonZeroU32::new)
        .ok_or_else(invalid_pid)
}

fn map_config_error(cause: DocumentError) -> EmbeddingError {
    EmbeddingError::new_with_cause(EmbeddingErrorKind::InternalError, cause)
}

fn map_lock_error(cause: impl std::error::Error + Send + Sync + 'static) -> CommandError {
    CommandError::new_with_cause(CommandErrorKind::InternalError, INVALID_LOCK_PID, cause)
}

fn invalid_pid() -> CommandError {
    CommandError::new(CommandErrorKind::InternalError, INVALID_LOCK_PID)
}
