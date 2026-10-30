use super::{durability::make_lock_error, error::make_io_error};
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

/// Owns actual temporary resources independently of the foreground repository.
/// Admission/publication gates are short; payload writes and syncs never hold the gate.
#[derive(Debug, Default)]
pub(crate) struct GitWriteCleanup {
    cancelled: AtomicBool,
    state: Mutex<WriteState>,
}
#[derive(Debug, Default)]
struct WriteState {
    next_id: u64,
    resources: BTreeMap<u64, WriteResource>,
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
    /// Create an independent command lifetime with no registered resources.
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Reject future writes and release every owned resource before listener exit.
    /// Filesystem failures retain residual owners and do not skip other resources.
    ///
    /// # Errors
    /// Reports failed stage detachment/removal; arbitrary filesystem failures can leave residue.
    pub(crate) fn cancel_and_cleanup(&self) -> Result<(), SnapshotError> {
        self.cancelled.store(true, Ordering::SeqCst);
        let resources = std::mem::take(&mut self.lock_state().resources);
        let mut failure = None;
        for (id, resource) in resources {
            if let WriteResource::Stage(mut directory) = resource
                && let Err(error) = detach_and_remove(&mut directory)
            {
                failure = Some(error);
                self.lock_state()
                    .resources
                    .insert(id, WriteResource::Stage(directory));
            }
        }
        failure.map_or(Ok(()), Err)
    }

    /// Observe terminal cancellation without acquiring a repository or write lock.
    pub(crate) fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Create/register an outer stage atomically with respect to cancellation.
    pub(super) fn create_stage(
        self: &Arc<Self>,
        root: &Path,
    ) -> Result<(WriteLease, PathBuf), SnapshotError> {
        let mut state = self.admit()?;
        let directory = create_directory(root)?;
        let path = directory.path().to_owned();
        let lease = WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Stage(directory)),
        };
        Ok((lease, path))
    }

    /// Acquire/register the actual writable lock and clone its descriptor under the gate.
    pub(super) fn acquire_ref(
        self: &Arc<Self>,
        path: &Path,
        boundary: &Path,
        permissions: i32,
    ) -> Result<(WriteLease, File), SnapshotError> {
        let mut state = self.admit()?;
        let mut lock = gix::lock::File::acquire_to_update_resource(
            path,
            gix::lock::acquire::Fail::Immediately,
            Some(boundary.to_owned()),
            permissions,
        )
        .map_err(|error| make_lock_error(&error))?;
        let file = lock
            .with_mut(|file| file.try_clone())
            .map_err(fail_cleanup)?;
        let lease = WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Ref(lock)),
        };
        Ok((lease, file))
    }

    /// Register either exact-ref or packed-ref confirmation ownership under the gate.
    pub(super) fn acquire_marker(
        self: &Arc<Self>,
        path: &Path,
        boundary: &Path,
        permissions: i32,
    ) -> Result<WriteLease, SnapshotError> {
        let mut state = self.admit()?;
        let lock = gix::lock::Marker::acquire_to_hold_resource(
            path,
            gix::lock::acquire::Fail::Immediately,
            Some(boundary.to_owned()),
            permissions,
        )
        .map_err(|error| make_lock_error(&error))?;
        Ok(WriteLease {
            cleanup: Arc::clone(self),
            id: state.register(WriteResource::Marker(lock)),
        })
    }

    fn admit(&self) -> Result<MutexGuard<'_, WriteState>, SnapshotError> {
        let state = self.lock_state();
        if self.is_cancelled() {
            return Err(fail_cancelled());
        }
        Ok(state)
    }

    fn lock_state(&self) -> MutexGuard<'_, WriteState> {
        // A panic must not strand ownership; cleanup remains possible during unwinding.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

impl WriteLease {
    /// Remove a completed stage, reporting cleanup errors before returning write success.
    pub(super) fn close_stage(self) -> Result<(), SnapshotError> {
        let mut state = self.cleanup.admit()?;
        let Some(WriteResource::Stage(directory)) = state.resources.remove(&self.id) else {
            return Err(fail_cancelled());
        };
        directory.close().map_err(fail_cleanup)
    }

    /// Atomically publish an admitted complete object; false means a racing object already exists.
    pub(super) fn publish_object(
        &self,
        staged: &Path,
        destination: &Path,
        permissions: i32,
    ) -> Result<bool, SnapshotError> {
        let state = self.cleanup.admit()?;
        if !matches!(state.resources.get(&self.id), Some(WriteResource::Stage(_))) {
            return Err(fail_cancelled());
        }
        let parent = destination.parent().ok_or_else(fail_cancelled)?;
        gix::fs::dir::create::all(parent, Default::default(), permissions).map_err(fail_cleanup)?;
        match fs::hard_link(staged, destination) {
            Ok(()) => Ok(true),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(error) => Err(fail_cleanup(error)),
        }
    }

    /// Commit and unregister one actual ref lock without exposing a stale lock pathname.
    pub(super) fn commit_ref(self) -> Result<(), SnapshotError> {
        let mut state = self.cleanup.admit()?;
        let Some(WriteResource::Ref(lock)) = state.resources.remove(&self.id) else {
            return Err(fail_cancelled());
        };
        lock.commit().map_err(|error| fail_cleanup(error.error))?;
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
        let mut state = self.cleanup.lock_state();
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

fn detach_and_remove(directory: &mut TempDir) -> Result<(), SnapshotError> {
    let parent = directory.path().parent().ok_or_else(fail_cancelled)?;
    let detached = create_directory(parent)?;
    fs::rename(directory.path(), detached.path()).map_err(fail_cleanup)?;
    // Only after rename succeeds does the old name lose ownership; it is never reused.
    let previous = std::mem::replace(directory, detached);
    let _ = previous.keep();
    for _ in 0..4 {
        match fs::remove_dir_all(directory.path()) {
            Ok(()) => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => continue,
            Err(error) => return Err(fail_cleanup(error)),
        }
    }
    Err(fail_cleanup(io::Error::from(
        io::ErrorKind::DirectoryNotEmpty,
    )))
}

fn create_directory(root: &Path) -> Result<TempDir, SnapshotError> {
    tempfile::Builder::new()
        .prefix(".letopis-stage-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(root)
        .map_err(fail_cleanup)
}
fn fail_cleanup(error: io::Error) -> SnapshotError {
    make_io_error("Git write cleanup failed", error)
}
fn fail_cancelled() -> SnapshotError {
    SnapshotError::new(SnapshotErrorKind::InternalError, "Git write cancelled")
}

#[cfg(test)]
mod tests;
