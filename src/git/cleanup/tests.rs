use super::GitWriteCleanup;
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::MetadataExt,
    sync::{Arc, mpsc},
    thread,
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
        [r#"{"ok":false,"error":{"code":"cancelled","message":"command cancelled","details":{}}}"#]
    );
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign");
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
