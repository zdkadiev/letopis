//! Compatibility-gate implementation; production wiring remains in the process adapter.
use super::{constants::SNAPSHOT_REF_PREFIX, hashing::ObjectHasher, process::make_io_error};
use crate::service::{SnapshotError, SnapshotErrorKind};
use gix::{
    Repository,
    hash::ObjectId,
    objs::{Kind, Write as ObjectWrite},
    refs::Target,
};
use std::{
    fs::{self, File},
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

/// Owns one private, completed loose object until no-clobber publication or drop.
pub(super) struct StagedObject {
    directory: TempDir,
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
    pub(super) fn new(repo: &Repository, kind: Kind, bytes: &[u8]) -> Result<Self, SnapshotError> {
        let object_root = repo.objects.store_ref().path().to_owned();
        let directory = tempfile::Builder::new()
            .prefix(".letopis-stage-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&object_root)
            .map_err(fail_io)?;
        let store = gix::odb::loose::Store::at(directory.path(), repo.object_hash());
        let oid = store.write_buf(kind, bytes).map_err(|_| fail_integrity())?;
        verify_identity(oid, kind, bytes)?;
        let staged = store.object_path(&oid);
        let permissions = read_shared_permissions(repo)?;
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
        gix::fs::dir::create::all(parent, Default::default(), self.shared_permissions)
            .map_err(fail_io)?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::ObjectPublish)?;
        match fs::hard_link(&self.staged, &self.destination) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => self.verify_existing()?,
            Err(error) => return Err(fail_io(error)),
        }
        // A racing winner can still be between its link and sync. Sync its inode too.
        File::open(&self.destination)
            .and_then(|file| file.sync_all())
            .map_err(fail_io)?;
        #[cfg(test)]
        tests::inject_failure(tests::FailurePoint::DirectorySync)?;
        sync_parent_chain(parent, &self.object_root)?;
        self.directory.close().map_err(fail_io)
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

/// Creates only the exact immutable direct ref, using an owned lock for pre-rename fsync.
/// The caller validates the root/closure first. Existing reflogs and user refs are untouched.
///
/// # Errors
/// Returns an internal error for invalid/conflicting refs or lock/write/sync failures.
pub(super) fn retain_ref(repo: &Repository, oid: ObjectId) -> Result<(), SnapshotError> {
    let name = format!("{SNAPSHOT_REF_PREFIX}{oid}");
    if has_exact_ref(repo, &name, oid)? {
        return Ok(());
    }
    let path = repo.common_dir().join(&name);
    let acquired = gix::lock::File::acquire_to_update_resource(
        &path,
        gix::lock::acquire::Fail::Immediately,
        Some(repo.common_dir().to_owned()),
        read_shared_permissions(repo)?,
    );
    let mut lock = match acquired {
        Ok(lock) => lock,
        Err(_) if has_exact_ref(repo, &name, oid)? => return Ok(()),
        Err(_) => return Err(fail_integrity()),
    };
    if has_exact_ref(repo, &name, oid)? {
        return Ok(());
    }
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefSync)?;
    lock.with_mut(|file| writeln!(file, "{oid}").and_then(|()| file.sync_all()))
        .map_err(fail_io)?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefPublish)?;
    lock.commit().map_err(|error| fail_io(error.error))?;
    #[cfg(test)]
    tests::inject_failure(tests::FailurePoint::RefDirectorySync)?;
    sync_parent_chain(path.parent().ok_or_else(fail_integrity)?, repo.common_dir())?;
    if !has_exact_ref(repo, &name, oid)? {
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

fn read_shared_permissions(repo: &Repository) -> Result<i32, SnapshotError> {
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
    for chunk in bytes.chunks(64 * 1024) {
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
