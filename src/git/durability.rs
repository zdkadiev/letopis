//! Durable raw objects and exact private refs, with command-owned cancellation.
use super::{
    cleanup::{GitWriteCleanup, WriteLease},
    constants::{HASH_CHUNK_BYTES, SNAPSHOT_REF_PREFIX},
    error::make_io_error,
    hashing::ObjectHasher,
};
use crate::service::{SnapshotError, SnapshotErrorKind};
use gix::{
    Repository,
    hash::ObjectId,
    objs::{Kind, Write as ObjectWrite},
    refs::Target,
};
use std::{
    error::Error,
    fmt,
    fs::{self, File},
    io::{self, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Debug)]
struct RefLockBusy;
impl fmt::Display for RefLockBusy {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Git ref lock is busy")
    }
}
impl Error for RefLockBusy {}

/// Owns one private, completed loose object until no-clobber publication or drop.
#[derive(Debug)]
pub(super) struct StagedObject {
    directory: WriteLease,
    staged: PathBuf,
    destination: PathBuf,
    object_root: PathBuf,
    oid: ObjectId,
    kind: Kind,
    shared_permissions: i32,
}

impl StagedObject {
    /// Writes, independently verifies and syncs an object in its private staging directory.
    ///
    /// # Errors
    /// Returns an internal error on identity, configuration, filesystem or sync failure.
    #[cfg(test)]
    pub(super) fn new(repo: &Repository, kind: Kind, bytes: &[u8]) -> Result<Self, SnapshotError> {
        Self::new_with_cleanup(repo, kind, bytes, &Arc::new(GitWriteCleanup::new()))
    }
    /// Stage, independently verify and sync raw bytes under the command's cleanup owner.
    ///
    /// # Errors
    /// Reports cancellation, invalid identity/configuration, or filesystem/sync failure.
    pub(super) fn new_with_cleanup(
        repo: &Repository,
        kind: Kind,
        bytes: &[u8],
        cleanup: &Arc<GitWriteCleanup>,
    ) -> Result<Self, SnapshotError> {
        let object_root = repo.objects.store_ref().path().to_owned();
        let (directory, path) = cleanup.create_stage(&object_root)?;
        let store = gix::odb::loose::Store::at(path, repo.object_hash());
        let oid = store.write_buf(kind, bytes).map_err(|_| fail_integrity())?;
        verify_identity(oid, kind, bytes)?;
        let staged = store.object_path(&oid);
        let permissions = get_shared_permissions(repo)?;
        gix::fs::set_shared_repository_permissions(&staged, permissions).map_err(fail_io)?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::ObjectSync)?;
        File::open(&staged)
            .and_then(|file| file.sync_all())
            .map_err(fail_io)?;
        let live = gix::odb::loose::Store::at(&object_root, repo.object_hash());
        Ok(Self {
            directory,
            staged,
            destination: live.object_path(&oid),
            object_root,
            oid,
            kind,
            shared_permissions: permissions,
        })
    }

    pub(super) fn get_oid(&self) -> ObjectId {
        self.oid
    }

    /// Publishes without replacing an existing object, then syncs the live directories.
    /// The owned stage is removed on normal success or drop.
    ///
    /// # Errors
    /// Fails closed on invalid existing bytes or any write/sync failure; a complete orphan may remain.
    pub(super) fn publish(self) -> Result<(), SnapshotError> {
        let parent = self.destination.parent().ok_or_else(fail_integrity)?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::ObjectPublish)?;
        if !self.directory.publish_object(
            &self.staged,
            &self.destination,
            self.shared_permissions,
        )? {
            self.verify_existing()?;
        }
        // A racing winner can still be between its link and sync. Sync its inode too.
        File::open(&self.destination)
            .and_then(|file| file.sync_all())
            .map_err(fail_io)?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::DirectorySync)?;
        sync_parent_chain(parent, &self.object_root)?;
        self.directory.close_stage()
    }

    fn verify_existing(&self) -> Result<(), SnapshotError> {
        let store = gix::odb::loose::Store::at(&self.object_root, self.oid.kind());
        let mut bytes = Vec::new();
        let object = store
            .try_find(&self.oid, &mut bytes)
            .map_err(|_| fail_integrity())?
            .ok_or_else(fail_integrity)?;
        if object.kind != self.kind {
            return Err(fail_integrity());
        }
        verify_identity(self.oid, object.kind, object.data)
    }
}

/// Identifies contention that a command may retry through the full durable protocol.
/// Other I/O/integrity failures must not be converted to equality-only success.
pub(super) fn is_ref_lock_busy(error: &SnapshotError) -> bool {
    error.source().is_some_and(<dyn Error>::is::<RefLockBusy>)
}

/// Creates only the exact immutable direct ref, using an owned lock for pre-rename fsync.
/// The caller validates the root/closure first. Existing reflogs and user refs are untouched.
///
/// # Errors
/// Returns an internal error for invalid/conflicting refs or lock/write/sync failures.
/// Contention has a safe typed cause recognized by `is_ref_lock_busy`; retries must
/// reenter this protocol. No success path bypasses owned-lock durability confirmation.
pub(super) fn retain_ref(
    repo: &Repository,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<(), SnapshotError> {
    let name = format!("{SNAPSHOT_REF_PREFIX}{oid}");
    #[cfg(test)]
    tests::publish_concurrent_ref();
    let path = repo.common_dir().join(&name);
    let (lock, mut file) =
        cleanup.acquire_ref(&path, repo.common_dir(), get_shared_permissions(repo)?)?;
    if confirm_durable_ref(repo, &name, oid, cleanup)? {
        return Ok(());
    }
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefSync)?;
    writeln!(file, "{oid}")
        .and_then(|()| file.sync_all())
        .map_err(fail_io)?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefPublish)?;
    lock.commit_ref()?;
    confirm_published_ref(repo, &name, oid, cleanup)
}

// Publication releases the write lock. Reacquire ownership before selecting the
// current backing: pack-refs may have moved this value into packed-refs meanwhile.
fn confirm_published_ref(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<(), SnapshotError> {
    let _lock = cleanup.acquire_marker(
        &repo.common_dir().join(name),
        repo.common_dir(),
        get_shared_permissions(repo)?,
    )?;
    if !confirm_durable_ref(repo, name, oid, cleanup)? {
        return Err(fail_integrity());
    }
    Ok(())
}

// The caller holds the exact ref lock. Packed-only confirmation additionally
// holds packed-refs.lock so neither Git update-ref nor pack-refs can replace the backing file.
fn confirm_durable_ref(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<bool, SnapshotError> {
    let loose = repo.common_dir().join(name);
    let packed_lock = match fs::symlink_metadata(&loose) {
        Ok(metadata) if metadata.file_type().is_file() => None,
        Ok(_) => return Err(fail_integrity()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Some(cleanup.acquire_marker(
            &repo.refs.packed_refs_path(),
            repo.common_dir(),
            get_shared_permissions(repo)?,
        )?),
        Err(error) => return Err(fail_io(error)),
    };
    if !has_exact_ref(repo, name, oid)? {
        return Ok(false);
    }
    let backing = if packed_lock.is_some() {
        repo.refs.packed_refs_path()
    } else {
        loose
    };
    sync_existing_ref(repo, name, oid, &backing)?;
    Ok(true)
}

// Both identity reads and the backing file's sync occur under the required locks.
fn sync_existing_ref(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
    backing: &Path,
) -> Result<(), SnapshotError> {
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefSync)?;
    #[cfg(test)]
    tests::observe_confirmation_locks(repo, name);
    File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(backing)
        .and_then(|file| file.sync_all())
        .map_err(fail_io)?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefDirectorySync)?;
    sync_parent_chain(
        backing.parent().ok_or_else(fail_integrity)?,
        repo.common_dir(),
    )?;
    if !has_exact_ref(repo, name, oid)? {
        return Err(fail_integrity());
    }
    Ok(())
}

fn has_exact_ref(repo: &Repository, name: &str, oid: ObjectId) -> Result<bool, SnapshotError> {
    if fs::symlink_metadata(repo.common_dir().join(name))
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(fail_integrity());
    }
    let packed = repo
        .refs
        .open_packed_buffer()
        .map_err(|_| fail_integrity())?;
    let found = repo
        .refs
        .try_find_packed(name, packed.as_ref())
        .map_err(|_| fail_integrity())?;
    let Some(found) = found else {
        return Ok(false);
    };
    if found.name.as_bstr() != name.as_bytes() || found.target != Target::Object(oid) {
        return Err(fail_integrity());
    }
    Ok(true)
}

fn get_shared_permissions(repo: &Repository) -> Result<i32, SnapshotError> {
    let config = repo.config_snapshot();
    let Some(value) = config.string("core.sharedRepository") else {
        return Ok(0);
    };
    gix::config::tree::Core::SHARED_REPOSITORY
        .try_into_shared_repository(Some(value))
        .map_err(|_| fail_integrity())
}

fn verify_identity(oid: ObjectId, kind: Kind, bytes: &[u8]) -> Result<(), SnapshotError> {
    let kind = std::str::from_utf8(kind.as_bytes()).map_err(|_| fail_integrity())?;
    let mut hash = ObjectHasher::new(oid.kind().len_in_hex(), kind, bytes.len() as u64)?;
    for chunk in bytes.chunks(HASH_CHUNK_BYTES) {
        hash.update(chunk);
    }
    if hash.finish()? != oid {
        return Err(fail_integrity());
    }
    Ok(())
}

fn sync_parent_chain(path: &Path, boundary: &Path) -> Result<(), SnapshotError> {
    for directory in path.ancestors() {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(fail_io)?;
        if directory == boundary {
            return Ok(());
        }
    }
    Err(fail_integrity())
}

/// Preserve only typed lock contention and safe I/O classifications for retry decisions.
pub(super) fn make_lock_error(error: &gix::Error) -> SnapshotError {
    if error.is_retryable() {
        return SnapshotError::new_with_cause(
            "Git ref durability confirmation is contended",
            RefLockBusy,
        );
    }
    let kind = error
        .downcast_any_ref::<io::Error>()
        .map_or(io::ErrorKind::Other, io::Error::kind);
    fail_io(io::Error::from(kind))
}

fn fail_io(error: io::Error) -> SnapshotError {
    make_io_error("Git durable write failed", error)
}
fn fail_integrity() -> SnapshotError {
    SnapshotError::new(
        SnapshotErrorKind::InternalError,
        "Git durable write integrity failed",
    )
}

#[cfg(test)]
mod tests;
