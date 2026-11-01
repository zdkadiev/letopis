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
        #[cfg(test)]
        let mut checkpoint = self.lock_state().after_stage_detach.take();
        let mut failure = None;
        for (id, resource) in resources {
            if let WriteResource::Stage(mut directory) = resource
                && let Err(error) = detach_and_remove(
                    &mut directory,
                    #[cfg(test)]
                    checkpoint.take(),
                )
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

fn detach_and_remove(
    directory: &mut TempDir,
    #[cfg(test)] checkpoint: Option<tests::NamespaceCheckpoint>,
) -> Result<(), SnapshotError> {
    let parent = directory.path().parent().ok_or_else(fail_cancelled)?;
    let detached = create_directory(parent)?;
    fs::rename(directory.path(), detached.path()).map_err(fail_cleanup)?;
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
mod tests {
    use super::GitWriteCleanup;
    use std::{
        fs,
        io::{self, Read, Write},
        os::unix::fs::MetadataExt,
        path::{Path, PathBuf},
        sync::{Arc, mpsc},
        thread,
        time::Duration,
    };

    #[test]
    fn handled_sigint_cleans_actual_resources_before_process_exit() {
        use crate::process_test_support::OwnedChild;
        use std::{
            process::{Command, Stdio},
            time::{Duration, Instant},
        };
        // Given: an owned child whose foreground holds all resource types without running Drop.
        if let Some(root) = std::env::var_os("LETOPIS_SIGNAL_RESOURCES_ROOT") {
            run_signal_child(std::path::Path::new(&root));
        }
        let directory = tempfile::tempdir().unwrap();
        let foreign = directory.path().join("foreign.lock");
        fs::write(&foreign, b"foreign").unwrap();
        let inode = fs::metadata(&foreign).unwrap().ino();
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "git::cleanup::tests::handled_sigint_cleans_actual_resources_before_process_exit",
                "--nocapture",
            ])
            .env("LETOPIS_SIGNAL_RESOURCES_ROOT", directory.path())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // When: the real listener receives SIGINT after each resource is registered.
        let output = OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30));
        // Then: the callback cleans actual owners before exit1, leaving one safe cancellation response.
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        let envelopes: Vec<_> = stdout
            .lines()
            .filter(|line| line.starts_with('{'))
            .collect();
        assert_eq!(
            envelopes,
            [
                r#"{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}"#
            ]
        );
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
        assert_eq!(fs::metadata(&foreign).unwrap().ino(), inode);
    }

    #[test]
    fn failed_stage_cleanup_preserves_owner_and_releases_other_resources() {
        if supervise("failed_stage_cleanup_preserves_owner_and_releases_other_resources") {
            return;
        }
        // Given: a real missing parent makes detachment fail; other resources remain reachable.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let stages = root.join("stages");
        let moved = root.join("moved");
        fs::create_dir(&stages).unwrap();
        let cleanup = Arc::new(GitWriteCleanup::new());
        let (_stage, path) = cleanup.create_stage(&stages).unwrap();
        fs::write(path.join("payload"), b"owned bytes").unwrap();
        let (_reference, _writer) = cleanup
            .acquire_ref(&root.join("snapshot"), root, 0)
            .unwrap();
        let _marker = cleanup
            .acquire_marker(&root.join("packed-refs"), root, 0)
            .unwrap();
        let foreign = root.join("foreign.lock");
        fs::write(&foreign, b"foreign bytes").unwrap();
        let inode = fs::metadata(&foreign).unwrap().ino();
        fs::rename(&stages, &moved).unwrap();
        // When: the first stage fails cleanup, without skipping later lock owners.
        assert!(cleanup.cancel_and_cleanup().is_err());
        // Then: residual ownership is retained, other owned locks are gone, foreign ownership survives.
        assert_eq!(cleanup.lock_state().resources.len(), 1);
        assert!(!root.join("snapshot.lock").exists());
        assert!(!root.join("packed-refs.lock").exists());
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign bytes");
        assert_eq!(fs::metadata(&foreign).unwrap().ino(), inode);
        assert!(cleanup.create_stage(root).is_err());
        // Given: restore availability of the original parent without replacing the owner.
        fs::rename(&moved, &stages).unwrap();
        assert_eq!(fs::read(path.join("payload")).unwrap(), b"owned bytes");
        // When: retry cleanup, then repeat it after all resources have gone.
        cleanup.cancel_and_cleanup().unwrap();
        cleanup.cancel_and_cleanup().unwrap();
        // Then: the terminal owner is empty and no spool or foreign-lock damage remains.
        assert!(cleanup.lock_state().resources.is_empty());
        assert_eq!(fs::read_dir(&stages).unwrap().count(), 0);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign bytes");
        assert_eq!(fs::metadata(&foreign).unwrap().ino(), inode);
    }

    #[test]
    fn cancelled_ref_writer_cannot_publish_or_touch_successor_lock() {
        if supervise("cancelled_ref_writer_cannot_publish_or_touch_successor_lock") {
            return;
        }
        // Given: a writable registered ref with a descriptor held outside the owner.
        let directory = tempfile::tempdir().unwrap();
        let cleanup = Arc::new(GitWriteCleanup::new());
        let path = directory.path().join("snapshot");
        let (lease, mut writer) = cleanup.acquire_ref(&path, directory.path(), 0).unwrap();
        let (resume, wait) = mpsc::channel();
        let active = thread::spawn(move || {
            wait.recv().unwrap();
            writer.write_all(b"late original bytes\n").unwrap();
            writer.sync_all().unwrap();
            lease.commit_ref().is_err()
        });
        // When: cleanup runs before the active writer resumes; another owner acquires the name.
        cleanup.cancel_and_cleanup().unwrap();
        fs::write(path.with_extension("lock"), b"successor").unwrap();
        let successor_inode = fs::metadata(path.with_extension("lock")).unwrap().ino();
        resume.send(()).unwrap();
        let rejected = active.join().unwrap();
        // Then: the detached descriptor cannot publish or remove the successor's lock.
        assert!(rejected);
        assert!(!path.exists());
        assert_eq!(fs::read(path.with_extension("lock")).unwrap(), b"successor");
        assert_eq!(
            fs::metadata(path.with_extension("lock")).unwrap().ino(),
            successor_inode
        );
    }

    #[test]
    fn cancelled_stage_cannot_be_recreated_or_published_by_resumed_writer() {
        if supervise("cancelled_stage_cannot_be_recreated_or_published_by_resumed_writer") {
            return;
        }
        use gix::objs::{Kind, Write};
        // Given: an owned outer stage and an already-open payload descriptor.
        let directory = tempfile::tempdir().unwrap();
        let cleanup = Arc::new(GitWriteCleanup::new());
        let (lease, stage) = cleanup.create_stage(directory.path()).unwrap();
        let store = gix::odb::loose::Store::at(&stage, gix::hash::Kind::Sha1);
        let (entered, ready) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        let active = thread::spawn(move || {
            let mut bytes = SuspendedRead {
                entered: Some(entered),
                wait,
                bytes: b"late bytes".as_slice(),
            };
            store.write_stream(Kind::Blob, 10, &mut bytes).is_err()
        });
        ready.recv().unwrap();
        assert_eq!(fs::read_dir(&stage).unwrap().count(), 1);
        // When: cleanup detaches/removes while the writer remains live, then writing resumes.
        cleanup.cancel_and_cleanup().unwrap();
        resume.send(()).unwrap();
        let rejected = active.join().unwrap();
        let published = lease.publish_object(
            &stage.join("active"),
            &directory.path().join("aa/object"),
            0,
        );
        // Then: neither the old stage, a detached sibling, nor a published object reappears.
        assert!(rejected);
        assert!(published.is_err());
        assert!(!stage.exists());
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn resolved_parent_operations_remain_confined_to_detached_stage() {
        if supervise("resolved_parent_operations_remain_confined_to_detached_stage") {
            return;
        }
        // Given: a held dirfd models a namespace operation whose parent was resolved before cancellation.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path();
        let cleanup = Arc::new(GitWriteCleanup::new());
        let (_lease, stage) = cleanup.create_stage(root).unwrap();
        fs::write(stage.join("payload"), b"pending object").unwrap();
        let parent = fs::File::open(&stage).unwrap();
        let reference = root.join("snapshot");
        let (ref_lease, mut old_writer) = cleanup.acquire_ref(&reference, root, 0).unwrap();
        let (detached, resume) = register_namespace_checkpoint(&cleanup);
        let cancelling = Arc::clone(&cleanup);
        let worker = thread::spawn(move || cancelling.cancel_and_cleanup());
        // When: cancellation detaches the parent inode, then the pending mkdir/persist operations finish.
        let detached = detached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!stage.exists(), "active pathname is not isolated");
        let parent_inode = parent.metadata().unwrap().ino();
        assert_eq!(fs::metadata(&detached).unwrap().ino(), parent_inode);
        let fresh_lookup = fs::create_dir(stage.join("new-lookup")).unwrap_err();
        assert_eq!(fresh_lookup.kind(), io::ErrorKind::NotFound);
        complete_resolved_namespace(&parent).unwrap();
        assert_eq!(
            fs::read(detached.join("aa/object")).unwrap(),
            b"pending object"
        );
        resume.send(()).unwrap();
        worker.join().unwrap().unwrap();
        // Then: both stage names are gone; an unlinked parent rejects further additions.
        assert!(!stage.exists());
        assert!(!detached.exists());
        let late = rustix::fs::mkdirat(&parent, "late", rustix::fs::Mode::RWXU);
        assert_eq!(late.unwrap_err(), rustix::io::Errno::NOENT);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
        // When: an independent successor acquires the released lock and the old descriptor resumes.
        let lock = reference.with_extension("lock");
        let successor = acquire_successor(&lock);
        let inode = successor.metadata().unwrap().ino();
        old_writer.write_all(b"late old writer").unwrap();
        old_writer.sync_all().unwrap();
        // Then: neither the old publication lease nor late inode writes touch the successor.
        assert!(ref_lease.commit_ref().is_err());
        assert_eq!(fs::read(&lock).unwrap(), b"successor");
        assert_eq!(fs::metadata(&lock).unwrap().ino(), inode);
        assert!(!reference.exists());
    }

    #[test]
    fn cancellation_releases_both_confirmation_markers_but_preserves_foreign_lock() {
        if supervise("cancellation_releases_both_confirmation_markers_but_preserves_foreign_lock") {
            return;
        }
        // Given: actual owned loose/packed markers and an unrelated foreign lock.
        let directory = tempfile::tempdir().unwrap();
        let cleanup = Arc::new(GitWriteCleanup::new());
        let loose = directory.path().join("snapshot");
        let packed = directory.path().join("packed-refs");
        let _loose = cleanup.acquire_marker(&loose, directory.path(), 0).unwrap();
        let _packed = cleanup
            .acquire_marker(&packed, directory.path(), 0)
            .unwrap();
        fs::write(directory.path().join("foreign.lock"), b"foreign").unwrap();
        // When: cancellation closes every actual owned marker.
        cleanup.cancel_and_cleanup().unwrap();
        // Then: new admission fails, owned markers disappear and foreign bytes survive.
        assert!(cleanup.is_cancelled());
        assert!(!loose.with_extension("lock").exists());
        assert!(!packed.with_extension("lock").exists());
        assert!(cleanup.create_stage(directory.path()).is_err());
        assert_eq!(
            fs::read(directory.path().join("foreign.lock")).unwrap(),
            b"foreign"
        );
    }

    struct SuspendedRead<'a> {
        entered: Option<mpsc::Sender<()>>,
        wait: mpsc::Receiver<()>,
        bytes: &'a [u8],
    }

    /// An owned, test-only pause after detachment; no global hook or product input controls it.
    #[derive(Debug)]
    pub(super) struct NamespaceCheckpoint {
        detached: mpsc::Sender<PathBuf>,
        resume: mpsc::Receiver<()>,
    }
    impl NamespaceCheckpoint {
        pub(super) fn pause_after_detach(self, path: &Path) {
            self.detached.send(path.to_owned()).unwrap();
            self.resume.recv_timeout(Duration::from_secs(5)).unwrap();
        }
    }

    fn register_namespace_checkpoint(
        cleanup: &GitWriteCleanup,
    ) -> (mpsc::Receiver<PathBuf>, mpsc::Sender<()>) {
        let (detached, observed) = mpsc::channel();
        let (resume, admitted) = mpsc::channel();
        cleanup.lock_state().after_stage_detach = Some(NamespaceCheckpoint {
            detached,
            resume: admitted,
        });
        (observed, resume)
    }

    fn run_signal_child(root: &std::path::Path) -> ! {
        let cleanup = Arc::new(GitWriteCleanup::new());
        let writer = Arc::new(crate::cli::ResponseWriter::default());
        let _listener = crate::install_foreground_listener(
            &["check".into()],
            &writer,
            Arc::clone(&cleanup),
            crate::process::SigintListener::install,
        )
        .unwrap();
        let (_stage, path) = cleanup.create_stage(root).unwrap();
        let _active = fs::File::create(path.join("active")).unwrap();
        let (_reference, _writer) = cleanup
            .acquire_ref(&root.join("snapshot"), root, 0)
            .unwrap();
        let _confirmed = cleanup
            .acquire_marker(&root.join("confirmed"), root, 0)
            .unwrap();
        let _packed = cleanup
            .acquire_marker(&root.join("packed-refs"), root, 0)
            .unwrap();
        let (detached, resume) = register_namespace_checkpoint(&cleanup);
        thread::spawn(move || {
            detached.recv_timeout(Duration::from_secs(5)).unwrap();
            // Repeated delivery while cleanup is acknowledged but not complete.
            for _ in 0..16 {
                signal_hook::low_level::raise(signal_hook::consts::SIGINT).unwrap();
            }
            resume.send(()).unwrap();
        });
        signal_hook::low_level::raise(signal_hook::consts::SIGINT).unwrap();
        loop {
            thread::park();
        }
    }
    impl Read for SuspendedRead<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if let Some(entered) = self.entered.take() {
                entered.send(()).unwrap();
                self.wait.recv().unwrap();
            }
            self.bytes.read(buffer)
        }
    }
    fn supervise(case: &str) -> bool {
        if std::env::var("LETOPIS_CLEANUP_CASE").as_deref() == Ok(case) {
            return false;
        }
        crate::process_test_support::supervise_test(
            &format!("git::cleanup::tests::{case}"),
            &[("LETOPIS_CLEANUP_CASE", case)],
        );
        true
    }

    // A dirfd represents the parent already resolved by a pending path-based syscall.
    // These real *at operations model mkdir/persist; they do not pause gix inside the kernel.
    fn complete_resolved_namespace(parent: &fs::File) -> rustix::io::Result<()> {
        rustix::fs::mkdirat(parent, "aa", rustix::fs::Mode::RWXU)?;
        rustix::fs::renameat(parent, "payload", parent, "aa/object")
    }

    fn acquire_successor(path: &Path) -> fs::File {
        let mut file = fs::File::options()
            .create_new(true)
            .write(true)
            .open(path)
            .unwrap();
        file.write_all(b"successor").unwrap();
        file
    }
}
