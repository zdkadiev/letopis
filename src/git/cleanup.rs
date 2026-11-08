use super::{durability::lock_map_error, error::SafeIo};
use crate::service::{SnapshotError, SnapshotErrorKind};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
};
use tempfile::TempDir;

const WRITE_CANCELLED_MESSAGE: &str = "Git write cancelled";

/// Owns actual temporary resources independently of the foreground repository.
/// Admission/publication gates are short; payload writes and syncs never hold the gate.
#[derive(Debug, Default)]
pub(crate) struct GitWriteCleanup {
    cancelled: AtomicBool,
    state_mutex: Mutex<WriteState>,
}

#[derive(Debug, Default)]
struct WriteState {
    next_id: u64,
    resources: BTreeMap<u64, WriteResource>,
    #[cfg(test)]
    after_stage_detach: Option<tests::NamespaceCheckpoint>,
}

#[derive(Debug)]
enum WriteResource {
    Stage(TempDir),
    Ref(gix::lock::File),
    Marker(gix::lock::Marker),
}

/// Normal-scope lease; the shared owner retains the actual resource until release.
#[derive(Debug)]
pub(super) struct WriteLease {
    cleanup: Arc<GitWriteCleanup>,
    id: u64,
}

impl GitWriteCleanup {
    /// Creates an independent command lifetime with no registered resources.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Rejects future writes and releases every owned resource before listener exit.
    /// Filesystem failures retain residual owners and do not skip other resources.
    ///
    /// # Errors
    /// Reports failed stage detachment/removal; arbitrary filesystem failures can leave residue.
    pub(crate) fn writes_cancel_and_cleanup(&self) -> Result<(), SnapshotError> {
        self.cancelled.store(true, Ordering::SeqCst);
        let resources = std::mem::take(&mut self.state_lock().resources);
        #[cfg(test)]
        let mut checkpoint = self.state_lock().after_stage_detach.take();
        let mut failure = None;
        for (id, resource) in resources {
            if let WriteResource::Stage(mut directory) = resource
                && let Err(error) = stage_detach_and_remove(
                    &mut directory,
                    #[cfg(test)]
                    checkpoint.take(),
                )
            {
                failure = Some(error);
                self.state_lock()
                    .resources
                    .insert(id, WriteResource::Stage(directory));
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Returns whether writes are cancelled without acquiring a repository or write lock.
    pub(crate) fn writes_is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Creates and registers an outer stage atomically with respect to cancellation.
    ///
    /// # Arguments
    ///
    /// * `root` — directory in which the private staging directory is created.
    pub(super) fn stage_create(
        self: &Arc<Self>,
        root: &Path,
    ) -> Result<(WriteLease, PathBuf), SnapshotError> {
        let mut state = self.write_admit()?;
        let directory = directory_create(root)?;
        let path = directory.path().to_owned();
        let lease = WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Stage(directory)),
        };
        Ok((lease, path))
    }

    /// Acquires and registers the writable lock and clones its descriptor under the gate.
    ///
    /// # Arguments
    ///
    /// * `path` — resource path whose writable lock is acquired without blocking.
    /// * `boundary` — directory boundary for creating and removing missing directories
    ///   on rollback.
    /// * `permissions` — shared repository mode applied to the lock and created directories.
    pub(super) fn ref_acquire_by_path(
        self: &Arc<Self>,
        path: &Path,
        boundary: &Path,
        permissions: i32,
    ) -> Result<(WriteLease, File), SnapshotError> {
        let mut state = self.write_admit()?;
        let mut lock = gix::lock::File::acquire_to_update_resource(
            path,
            gix::lock::acquire::Fail::Immediately,
            Some(boundary.to_owned()),
            permissions,
        )
        .map_err(|error| lock_map_error(&error))?;
        let file = lock.with_mut(|file| file.try_clone()).map_err(|error| {
            SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error))
        })?;
        let lease = WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Ref(lock)),
        };
        Ok((lease, file))
    }

    /// Registers exact-ref or packed-ref confirmation ownership under the gate.
    ///
    /// # Arguments
    ///
    /// * `path` — resource path whose confirmation marker is acquired without blocking.
    /// * `boundary` — directory boundary for creating and removing missing directories
    ///   on rollback.
    /// * `permissions` — shared repository mode applied to the marker and created directories.
    pub(super) fn marker_acquire_by_path(
        self: &Arc<Self>,
        path: &Path,
        boundary: &Path,
        permissions: i32,
    ) -> Result<WriteLease, SnapshotError> {
        let mut state = self.write_admit()?;
        let lock = gix::lock::Marker::acquire_to_hold_resource(
            path,
            gix::lock::acquire::Fail::Immediately,
            Some(boundary.to_owned()),
            permissions,
        )
        .map_err(|error| lock_map_error(&error))?;
        Ok(WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Marker(lock)),
        })
    }

    fn write_admit(&self) -> Result<MutexGuard<'_, WriteState>, SnapshotError> {
        let state = self.state_lock();
        if self.writes_is_cancelled() {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                WRITE_CANCELLED_MESSAGE,
            ));
        }
        Ok(state)
    }

    fn state_lock(&self) -> MutexGuard<'_, WriteState> {
        // A panic must not strand ownership; cleanup remains possible during unwinding.
        self.state_mutex
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl WriteLease {
    /// Removes a completed stage, reporting cleanup errors before returning write success.
    pub(super) fn stage_close(self) -> Result<(), SnapshotError> {
        let mut state = self.cleanup.write_admit()?;
        let Some(WriteResource::Stage(directory)) = state.resources.remove(&self.id) else {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                WRITE_CANCELLED_MESSAGE,
            ));
        };
        directory.close().map_err(|error| {
            SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error))
        })
    }

    /// Publishes an admitted complete object atomically; false means a racing object
    /// already exists.
    ///
    /// # Arguments
    ///
    /// * `staged` — completed object file inside the owned stage.
    /// * `destination` — final object path that is linked without replacement.
    /// * `permissions` — shared repository permissions applied to newly created directories.
    pub(super) fn object_publish(
        &self,
        staged: &Path,
        destination: &Path,
        permissions: i32,
    ) -> Result<bool, SnapshotError> {
        let state = self.cleanup.write_admit()?;
        if !matches!(state.resources.get(&self.id), Some(WriteResource::Stage(_))) {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                WRITE_CANCELLED_MESSAGE,
            ));
        }
        let parent = destination.parent().ok_or_else(|| {
            SnapshotError::new(SnapshotErrorKind::InternalError, WRITE_CANCELLED_MESSAGE)
        })?;
        gix::fs::dir::create::all(parent, Default::default(), permissions).map_err(|error| {
            SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error))
        })?;
        match fs::hard_link(staged, destination) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(SnapshotError::new_with_cause(
                "Git write cleanup failed",
                SafeIo(error),
            )),
        }
    }

    /// Commits and unregisters one actual ref lock without exposing a stale lock pathname.
    pub(super) fn ref_commit(self) -> Result<(), SnapshotError> {
        let mut state = self.cleanup.write_admit()?;
        let Some(WriteResource::Ref(lock)) = state.resources.remove(&self.id) else {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                WRITE_CANCELLED_MESSAGE,
            ));
        };
        lock.commit().map_err(|error| {
            SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error.error))
        })?;
        Ok(())
    }
}

impl WriteState {
    fn register(&mut self, resource: WriteResource) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.resources.insert(id, resource);
        id
    }
}

impl Drop for WriteLease {
    fn drop(&mut self) {
        let mut state = self.cleanup.state_lock();
        if let Some(resource) = state.resources.remove(&self.id) {
            // Keep the actual marker owner alive until removal, never unlink a remembered name.
            match resource {
                WriteResource::Stage(stage) => drop(stage),
                WriteResource::Ref(lock) => drop(lock),
                WriteResource::Marker(marker) => drop(marker),
            }
        }
    }
}

fn stage_detach_and_remove(
    directory: &mut TempDir,
    #[cfg(test)] checkpoint: Option<tests::NamespaceCheckpoint>,
) -> Result<(), SnapshotError> {
    let parent = directory.path().parent().ok_or_else(|| {
        SnapshotError::new(SnapshotErrorKind::InternalError, WRITE_CANCELLED_MESSAGE)
    })?;
    let detached = directory_create(parent)?;
    fs::rename(directory.path(), detached.path()).map_err(|error| {
        SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error))
    })?;
    // Only after rename succeeds does the old name lose ownership; it is never reused.
    let previous = std::mem::replace(directory, detached);
    let _ = previous.keep();
    #[cfg(test)]
    if let Some(checkpoint) = checkpoint {
        checkpoint.pause_after_detach(directory.path());
    }
    for _ in 0..4 {
        match fs::remove_dir_all(directory.path()) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => continue,
            Err(error) => {
                return Err(SnapshotError::new_with_cause(
                    "Git write cleanup failed",
                    SafeIo(error),
                ));
            }
        }
    }
    Err(SnapshotError::new_with_cause(
        "Git write cleanup failed",
        SafeIo(io::Error::from(io::ErrorKind::DirectoryNotEmpty)),
    ))
}

fn directory_create(root: &Path) -> Result<TempDir, SnapshotError> {
    tempfile::Builder::new()
        .prefix(".letopis-stage-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(root)
        .map_err(|error| SnapshotError::new_with_cause("Git write cleanup failed", SafeIo(error)))
}

#[cfg(test)]
#[path = "cleanup_test.rs"]
mod tests;
