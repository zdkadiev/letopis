//! Durable raw objects and exact private refs, with command-owned cancellation.
use super::{
    cleanup::{GitWriteCleanup, WriteLease},
    constants::HASH_CHUNK_BYTES,
    error::SafeIo,
    hashing::ObjectHasher,
};
use crate::service::{SnapshotError, SnapshotErrorKind};
use gix::{
    Repository,
    hash::{Kind as HashKind, ObjectId},
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

const DURABILITY_INTEGRITY_MESSAGE: &str = "Git durable write integrity failed";
pub(super) const SNAPSHOT_REF_PREFIX: &str = "refs/letopis/snapshots/";

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
    /// # Arguments
    ///
    /// * `repo` — repository whose object store and shared permissions are used.
    /// * `kind` — Git object type written to the store.
    /// * `bytes` — raw object payload written verbatim.
    ///
    /// # Errors
    /// Returns an internal error on identity, configuration, filesystem or sync failure.
    #[cfg(test)]
    pub(super) fn new(repo: &Repository, kind: Kind, bytes: &[u8]) -> Result<Self, SnapshotError> {
        Self::new_with_cleanup(repo, kind, bytes, &Arc::new(GitWriteCleanup::new()))
    }

    /// Writes, independently verifies and syncs raw bytes under the command's cleanup owner.
    ///
    /// # Arguments
    ///
    /// * `repo` — repository whose object store and shared permissions are used.
    /// * `kind` — Git object type written to the store.
    /// * `bytes` — raw object payload written verbatim.
    /// * `cleanup` — command-owned write resources that own the private stage.
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
        let (directory, path) = cleanup.stage_create(&object_root)?;
        let store = gix::odb::loose::Store::at(path, repo.object_hash());
        let oid = store.write_buf(kind, bytes).map_err(|_| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            )
        })?;
        object_verify_identity(oid, kind, bytes)?;
        let staged = store.object_path(&oid);
        let permissions = repository_get_shared_permissions(repo)?;
        gix::fs::set_shared_repository_permissions(&staged, permissions).map_err(|error| {
            SnapshotError::new_with_cause("Git durable write failed", SafeIo(error))
        })?;
        object_sync(&staged)?;
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

    pub(super) fn object_get_oid(&self) -> ObjectId {
        self.oid
    }

    /// Publishes without replacing an existing object, then syncs the live directories.
    /// The owned stage is removed on normal success or drop.
    ///
    /// # Errors
    /// Fails closed on invalid existing bytes or any write/sync failure; a complete orphan may
    /// remain.
    pub(super) fn object_publish(self) -> Result<(), SnapshotError> {
        let parent = self.destination.parent().ok_or_else(|| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            )
        })?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::ObjectPublish)?;
        if !self.directory.object_publish(
            &self.staged,
            &self.destination,
            self.shared_permissions,
        )? {
            self.object_verify_existing()?;
        }
        // A racing winner can still be between its link and sync. Sync its inode too.
        object_sync(&self.destination)?;
        directory_sync_parent_chain(parent, &self.object_root)?;
        self.directory.stage_close()
    }

    fn object_verify_existing(&self) -> Result<(), SnapshotError> {
        let store = gix::odb::loose::Store::at(&self.object_root, self.oid.kind());
        let mut bytes = Vec::new();
        let object = store
            .try_find(&self.oid, &mut bytes)
            .map_err(|_| {
                SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    DURABILITY_INTEGRITY_MESSAGE,
                )
            })?
            .ok_or_else(|| {
                SnapshotError::new(
                    SnapshotErrorKind::InternalError,
                    DURABILITY_INTEGRITY_MESSAGE,
                )
            })?;
        if object.kind != self.kind {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            ));
        }
        object_verify_identity(self.oid, object.kind, object.data)
    }
}

/// Verify and sync an existing loose object without staging another compressed copy.
/// Packed or absent objects use the ordinary publication path instead.
///
/// # Errors
/// Fails closed on cancellation, corrupt existing bytes, invalid configuration or sync failure.
pub(super) fn object_reuse_existing(
    repo: &Repository,
    kind: Kind,
    bytes: &[u8],
    cleanup: &GitWriteCleanup,
) -> Result<Option<ObjectId>, SnapshotError> {
    cleanup.writes_verify_active()?;
    let oid = object_compute_oid(repo.object_hash(), kind, bytes)?;
    let object_root = repo.objects.store_ref().path();
    let store = gix::odb::loose::Store::at(object_root, repo.object_hash());
    let mut buffer = Vec::new();
    let Some(object) = store.try_find(&oid, &mut buffer).map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        )
    })?
    else {
        return Ok(None);
    };
    if object.kind != kind {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    object_verify_identity(oid, object.kind, object.data)?;
    repository_get_shared_permissions(repo)?;
    let destination = store.object_path(&oid);
    object_sync(&destination)?;
    let parent = destination.parent().ok_or_else(|| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        )
    })?;
    directory_sync_parent_chain(parent, object_root)?;
    cleanup.writes_verify_active()?;
    Ok(Some(oid))
}

/// Returns whether the error is contention a command may retry through the full durable protocol.
/// Other I/O/integrity failures must not be converted to equality-only success.
///
/// # Arguments
///
/// * `error` — failure returned by the durable ref protocol.
pub(super) fn ref_lock_is_busy(error: &SnapshotError) -> bool {
    error.source().is_some_and(<dyn Error>::is::<RefLockBusy>)
}

/// Creates only the exact immutable direct ref, using an owned lock for pre-rename fsync.
/// The caller validates the root/closure first. Existing reflogs and user refs are untouched.
///
/// # Arguments
///
/// * `repo` — repository whose common directory and configuration are used.
/// * `oid` — object identity to retain through the exact snapshot ref.
/// * `cleanup` — command-owned write resources for locks and confirmation markers.
///
/// # Errors
/// Returns an internal error for invalid/conflicting refs or lock/write/sync failures.
/// Contention has a safe typed cause recognized by `ref_lock_is_busy`; retries must
/// reenter this protocol. No success path bypasses owned-lock durability confirmation.
pub(super) fn ref_retain_by_oid(
    repo: &Repository,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<(), SnapshotError> {
    let name = format!("{SNAPSHOT_REF_PREFIX}{oid}");
    #[cfg(test)]
    tests::publish_concurrent_ref();
    let path = repo.common_dir().join(&name);
    let (lock, mut file) = cleanup.ref_acquire_by_path(
        &path,
        repo.common_dir(),
        repository_get_shared_permissions(repo)?,
    )?;
    if ref_confirm_durable(repo, &name, oid, cleanup)? {
        return Ok(());
    }
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefSync)?;
    writeln!(file, "{oid}")
        .and_then(|()| file.sync_all())
        .map_err(|error| {
            SnapshotError::new_with_cause("Git durable write failed", SafeIo(error))
        })?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefPublish)?;
    lock.ref_commit()?;
    ref_confirm_published(repo, &name, oid, cleanup)
}

// Publication releases the write lock. Reacquire ownership before selecting the
// current backing: pack-refs may have moved this value into packed-refs meanwhile.

fn ref_confirm_published(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<(), SnapshotError> {
    let _lock = cleanup.marker_acquire_by_path(
        &repo.common_dir().join(name),
        repo.common_dir(),
        repository_get_shared_permissions(repo)?,
    )?;
    if !ref_confirm_durable(repo, name, oid, cleanup)? {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    Ok(())
}

// The caller holds the exact ref lock. Packed-only confirmation additionally
// holds packed-refs.lock so neither Git update-ref nor pack-refs can replace the backing file.

fn ref_confirm_durable(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
    cleanup: &Arc<GitWriteCleanup>,
) -> Result<bool, SnapshotError> {
    let loose = repo.common_dir().join(name);
    let packed_lock = match fs::symlink_metadata(&loose) {
        Ok(metadata) if metadata.file_type().is_file() => None,
        Ok(_) => {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            Some(cleanup.marker_acquire_by_path(
                &repo.refs.packed_refs_path(),
                repo.common_dir(),
                repository_get_shared_permissions(repo)?,
            )?)
        }
        Err(error) => {
            return Err(SnapshotError::new_with_cause(
                "Git durable write failed",
                SafeIo(error),
            ));
        }
    };
    if !is_exact_ref_target(repo, name, oid)? {
        return Ok(false);
    }
    let backing = if packed_lock.is_some() {
        repo.refs.packed_refs_path()
    } else {
        loose
    };
    ref_sync_existing(repo, name, oid, &backing)?;
    Ok(true)
}

/// Verifies both identity reads and syncs the backing file under the required locks.
///
/// # Arguments
///
/// * `repo` — repository whose reference and object state is read.
/// * `name` — exact reference name being confirmed.
/// * `oid` — object identity the reference must still name.
/// * `backing` — file holding the loose reference, synced once its bytes are verified.
fn ref_sync_existing(
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
        .map_err(|error| {
            SnapshotError::new_with_cause("Git durable write failed", SafeIo(error))
        })?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefDirectorySync)?;
    directory_sync_parent_chain(
        backing.parent().ok_or_else(|| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            )
        })?,
        repo.common_dir(),
    )?;
    if !is_exact_ref_target(repo, name, oid)? {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    Ok(())
}

fn is_exact_ref_target(
    repo: &Repository,
    name: &str,
    oid: ObjectId,
) -> Result<bool, SnapshotError> {
    if fs::symlink_metadata(repo.common_dir().join(name))
        .is_ok_and(|metadata| metadata.file_type().is_symlink())
    {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    let packed = repo.refs.open_packed_buffer().map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        )
    })?;
    let found = repo
        .refs
        .try_find_packed(name, packed.as_ref())
        .map_err(|_| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            )
        })?;
    let Some(found) = found else {
        return Ok(false);
    };
    if found.name.as_bstr() != name.as_bytes() || found.target != Target::Object(oid) {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    Ok(true)
}

fn repository_get_shared_permissions(repo: &Repository) -> Result<i32, SnapshotError> {
    let config = repo.config_snapshot();
    let Some(value) = config.string("core.sharedRepository") else {
        return Ok(0);
    };
    gix::config::tree::Core::SHARED_REPOSITORY
        .try_into_shared_repository(Some(value))
        .map_err(|_| {
            SnapshotError::new(
                SnapshotErrorKind::InternalError,
                DURABILITY_INTEGRITY_MESSAGE,
            )
        })
}

fn object_verify_identity(oid: ObjectId, kind: Kind, bytes: &[u8]) -> Result<(), SnapshotError> {
    if object_compute_oid(oid.kind(), kind, bytes)? != oid {
        return Err(SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        ));
    }
    Ok(())
}

fn object_compute_oid(
    format: HashKind,
    kind: Kind,
    bytes: &[u8],
) -> Result<ObjectId, SnapshotError> {
    let kind = std::str::from_utf8(kind.as_bytes()).map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        )
    })?;
    let mut hash = ObjectHasher::new(format.len_in_hex(), kind, bytes.len() as u64)?;
    for chunk in bytes.chunks(HASH_CHUNK_BYTES) {
        hash.update(chunk);
    }
    ObjectId::from_hex(hash.finish()?.as_bytes()).map_err(|_| {
        SnapshotError::new(
            SnapshotErrorKind::InternalError,
            DURABILITY_INTEGRITY_MESSAGE,
        )
    })
}

fn object_sync(path: &Path) -> Result<(), SnapshotError> {
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::ObjectSync)?;
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|error| SnapshotError::new_with_cause("Git durable write failed", SafeIo(error)))
}

fn directory_sync_parent_chain(path: &Path, boundary: &Path) -> Result<(), SnapshotError> {
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::DirectorySync)?;
    for directory in path.ancestors() {
        File::open(directory)
            .and_then(|file| file.sync_all())
            .map_err(|error| {
                SnapshotError::new_with_cause("Git durable write failed", SafeIo(error))
            })?;
        if directory == boundary {
            return Ok(());
        }
    }
    Err(SnapshotError::new(
        SnapshotErrorKind::InternalError,
        DURABILITY_INTEGRITY_MESSAGE,
    ))
}

/// Returns a typed error preserving only lock contention and safe I/O classification.
///
/// # Arguments
///
/// * `error` — lock failure classified as typed contention or a safe I/O kind.
pub(super) fn lock_map_error(error: &gix::Error) -> SnapshotError {
    if error.is_retryable() {
        return SnapshotError::new_with_cause(
            "Git ref durability confirmation is contended",
            RefLockBusy,
        );
    }
    let kind = error
        .downcast_any_ref::<io::Error>()
        .map_or(io::ErrorKind::Other, io::Error::kind);
    SnapshotError::new_with_cause("Git durable write failed", SafeIo(io::Error::from(kind)))
}

#[cfg(test)]
#[path = "durability_test.rs"]
mod tests;
