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
    // Libtest may prefix its progress text on the same line before the exit.
    let cancellation =
        r#"{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}"#;
    assert_eq!(stdout.matches(cancellation).count(), 1, "{stdout:?}");
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
    let (_stage, path) = cleanup.stage_create(&stages).unwrap();
    fs::write(path.join("payload"), b"owned bytes").unwrap();
    let (_reference, _writer) = cleanup
        .ref_acquire_by_path(&root.join("snapshot"), root, 0)
        .unwrap();
    let _marker = cleanup
        .marker_acquire_by_path(&root.join("packed-refs"), root, 0)
        .unwrap();
    let foreign = root.join("foreign.lock");
    fs::write(&foreign, b"foreign bytes").unwrap();
    let inode = fs::metadata(&foreign).unwrap().ino();
    fs::rename(&stages, &moved).unwrap();
    // When: the first stage fails cleanup, without skipping later lock owners.
    assert!(cleanup.shutdown().is_err());
    // Then: residual ownership is retained, other owned locks are gone, foreign ownership survives.
    assert_eq!(cleanup.state_lock().resources.len(), 1);
    assert!(!root.join("snapshot.lock").exists());
    assert!(!root.join("packed-refs.lock").exists());
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign bytes");
    assert_eq!(fs::metadata(&foreign).unwrap().ino(), inode);
    assert!(cleanup.stage_create(root).is_err());
    // Given: restore availability of the original parent without replacing the owner.
    fs::rename(&moved, &stages).unwrap();
    assert_eq!(fs::read(path.join("payload")).unwrap(), b"owned bytes");
    // When: retry cleanup, then repeat it after all resources have gone.
    cleanup.shutdown().unwrap();
    cleanup.shutdown().unwrap();
    // Then: the terminal owner is empty and no spool or foreign-lock damage remains.
    assert!(cleanup.state_lock().resources.is_empty());
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
    let (lease, mut writer) = cleanup
        .ref_acquire_by_path(&path, directory.path(), 0)
        .unwrap();
    let (resume, wait) = mpsc::channel();
    let active = thread::spawn(move || {
        wait.recv().unwrap();
        writer.write_all(b"late original bytes\n").unwrap();
        writer.sync_all().unwrap();
        lease.ref_commit().is_err()
    });
    // When: cleanup runs before the active writer resumes; another owner acquires the name.
    cleanup.shutdown().unwrap();
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
    let (lease, stage) = cleanup.stage_create(directory.path()).unwrap();
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
    cleanup.shutdown().unwrap();
    resume.send(()).unwrap();
    let rejected = active.join().unwrap();
    let published = lease.object_publish(
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
    let (_lease, stage) = cleanup.stage_create(root).unwrap();
    fs::write(stage.join("payload"), b"pending object").unwrap();
    let parent = fs::File::open(&stage).unwrap();
    let reference = root.join("snapshot");
    let (ref_lease, mut old_writer) = cleanup.ref_acquire_by_path(&reference, root, 0).unwrap();
    let (detached, resume) = register_namespace_checkpoint(&cleanup);
    let cancelling = Arc::clone(&cleanup);
    let worker = thread::spawn(move || cancelling.shutdown());
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
    assert!(ref_lease.ref_commit().is_err());
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
    let _loose = cleanup
        .marker_acquire_by_path(&loose, directory.path(), 0)
        .unwrap();
    let _packed = cleanup
        .marker_acquire_by_path(&packed, directory.path(), 0)
        .unwrap();
    fs::write(directory.path().join("foreign.lock"), b"foreign").unwrap();
    // When: cancellation closes every actual owned marker.
    cleanup.shutdown().unwrap();
    // Then: new admission fails, owned markers disappear and foreign bytes survive.
    assert!(cleanup.writes_is_cancelled());
    assert!(!loose.with_extension("lock").exists());
    assert!(!packed.with_extension("lock").exists());
    assert!(cleanup.stage_create(directory.path()).is_err());
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
    cleanup.state_lock().after_stage_detach = Some(NamespaceCheckpoint {
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
        crate::process::SigintListener::signal_listener_install,
    )
    .unwrap();
    let (_stage, path) = cleanup.stage_create(root).unwrap();
    let _active = fs::File::create(path.join("active")).unwrap();
    let (_reference, _writer) = cleanup
        .ref_acquire_by_path(&root.join("snapshot"), root, 0)
        .unwrap();
    let _confirmed = cleanup
        .marker_acquire_by_path(&root.join("confirmed"), root, 0)
        .unwrap();
    let _packed = cleanup
        .marker_acquire_by_path(&root.join("packed-refs"), root, 0)
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
