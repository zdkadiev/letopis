use super::{
    confined::{read_stat, require_type},
    local::{LocalFileSystem, fail_io, retry_io},
};
use crate::service::{DocumentError, DocumentErrorKind, DocumentResult, EmbeddingError, InitFiles};
use rustix::{
    fs::{AtFlags, FileType, Mode, OFlags, fchmod, linkat, openat, unlinkat},
    io::Errno,
};
#[cfg(test)]
use std::cell::RefCell;
use std::{
    fs::File,
    io,
    io::Write,
    sync::atomic::{AtomicU64, Ordering},
};

const CONFIG_NAME: &str = "config.json";
const TEMP_PREFIX: &str = "config.json.tmp";
const TEMP_FLAGS: OFlags = OFlags::WRONLY
    .union(OFlags::CREATE)
    .union(OFlags::EXCL)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SyncTarget {
    ConfigFile,
    ServiceDirectory,
    RootDirectory,
}

#[cfg(test)]
thread_local! {
    static NEXT_SYNC_FAULT: RefCell<Option<SyncTarget>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(super) fn fail_next_sync(target: SyncTarget) {
    NEXT_SYNC_FAULT.set(Some(target));
}

impl LocalFileSystem {
    /// Atomically publish complete configuration bytes without replacing an
    /// existing entry. The caller rereads and validates the winning file.
    ///
    /// # Errors
    /// Rejects unsafe service paths or nonregular destinations, and reports file,
    /// link or directory-sync failures. A failed call never removes existing state.
    pub(super) fn install_config_if_absent(&self, bytes: &[u8]) -> DocumentResult<()> {
        let directory = self.get_service_directory()?;
        if let Some(before) = read_stat(directory, CONFIG_NAME).map_err(map_embedding)? {
            require_type(&before, FileType::RegularFile).map_err(map_embedding)?;
            self.verify_service_directory(directory)?;
            sync_directory(directory, SyncTarget::ServiceDirectory)?;
            return Ok(());
        }
        let (name, mut file) = create_temp(directory)?;
        let temporary = TemporaryEntry::new(directory, name)?;
        retry_io(|| file.write_all(bytes))
            .map_err(|cause| fail_io("cannot write embedding configuration", cause))?;
        fchmod(&file, Mode::RUSR | Mode::WUSR)
            .map_err(|cause| fail_io("cannot protect embedding configuration", cause))?;
        sync_file(&file, SyncTarget::ConfigFile)?;
        match linkat(
            directory,
            temporary.name.as_str(),
            directory,
            CONFIG_NAME,
            AtFlags::empty(),
        ) {
            Ok(()) => {
                temporary.remove()?;
                sync_directory(directory, SyncTarget::ServiceDirectory)
            }
            Err(Errno::EXIST) => {
                temporary.remove()?;
                let Some(before) = read_stat(directory, CONFIG_NAME).map_err(map_embedding)? else {
                    return Err(fail_io(
                        "cannot publish embedding configuration",
                        io::Error::other("winning configuration disappeared"),
                    ));
                };
                require_type(&before, FileType::RegularFile).map_err(map_embedding)?;
                self.verify_service_directory(directory)?;
                sync_directory(directory, SyncTarget::ServiceDirectory)
            }
            Err(cause) => Err(fail_io("cannot publish embedding configuration", cause)),
        }
    }

    /// Sync the pinned service directory and its Git-root parent without creating
    /// or modifying any entry. Completed setup is preserved on failure.
    ///
    /// # Errors
    /// Reports an absent or replaced service directory, or a durability-sync failure.
    pub(super) fn sync_service_state(&self) -> DocumentResult<()> {
        let Some(directory) = self.get_existing_service_directory()? else {
            return Err(DocumentError::new(
                DocumentErrorKind::InternalError,
                "service directory missing",
            ));
        };
        sync_directory(directory, SyncTarget::ServiceDirectory)?;
        sync_directory(self.get_root_directory(), SyncTarget::RootDirectory)?;
        self.get_existing_service_directory()?;
        Ok(())
    }
}

impl InitFiles for LocalFileSystem {
    fn install_config_if_absent(&self, bytes: &[u8]) -> DocumentResult<()> {
        self.install_config_if_absent(bytes)
    }

    fn open_service_directory(&self) -> DocumentResult<rustix::fd::OwnedFd> {
        self.open_private_service_directory()
    }

    fn sync_service_state(&self) -> DocumentResult<()> {
        self.sync_service_state()
    }
}

struct TemporaryEntry {
    directory: File,
    name: String,
}

impl TemporaryEntry {
    fn new(directory: &File, name: String) -> DocumentResult<Self> {
        Ok(Self {
            directory: directory
                .try_clone()
                .map_err(|cause| fail_io("cannot retain temporary entry", cause))?,
            name,
        })
    }

    fn remove(mut self) -> DocumentResult<()> {
        unlinkat(&self.directory, self.name.as_str(), AtFlags::empty())
            .map_err(|cause| fail_io("cannot remove temporary configuration", cause))?;
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

fn create_temp(directory: &File) -> DocumentResult<(String, File)> {
    for _ in 0..16 {
        let name = format!(
            "{TEMP_PREFIX}.{}.{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        );
        match openat(
            directory,
            name.as_str(),
            TEMP_FLAGS,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(file) => return Ok((name, File::from(file))),
            Err(Errno::EXIST) => {}
            Err(cause) => {
                return Err(fail_io("cannot create temporary configuration", cause));
            }
        }
    }
    Err(fail_io(
        "cannot create temporary configuration",
        io::Error::other("temporary name collisions"),
    ))
}

fn sync_file(file: &File, target: SyncTarget) -> DocumentResult<()> {
    maybe_fail_sync(target)?;
    retry_io(|| sync_file_data(file))
        .map_err(|cause| fail_io("cannot sync embedding configuration", cause))
}

#[cfg(target_os = "macos")]
fn sync_file_data(file: &File) -> io::Result<()> {
    rustix::fs::fcntl_fullfsync(file).map_err(io::Error::from)
}

#[cfg(not(target_os = "macos"))]
fn sync_file_data(file: &File) -> io::Result<()> {
    file.sync_all()
}

fn sync_directory(directory: &File, target: SyncTarget) -> DocumentResult<()> {
    maybe_fail_sync(target)?;
    let directory = directory
        .try_clone()
        .map_err(|cause| fail_io("cannot retain service directory", cause))?;
    retry_io(|| directory.sync_all())
        .map_err(|cause| fail_io("cannot sync service directory", cause))
}

fn maybe_fail_sync(_target: SyncTarget) -> DocumentResult<()> {
    #[cfg(test)]
    if NEXT_SYNC_FAULT.with(|next| {
        if next.borrow().as_ref() == Some(&_target) {
            next.borrow_mut().take();
            true
        } else {
            false
        }
    }) {
        return Err(fail_io(
            "injected synchronization failure",
            io::Error::other("fixture"),
        ));
    }
    Ok(())
}

fn map_embedding(error: EmbeddingError) -> DocumentError {
    DocumentError::new_with_cause(
        DocumentErrorKind::InvalidRequest,
        "unsafe embedding configuration path",
        error,
    )
}
