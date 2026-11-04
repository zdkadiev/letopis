use rustix::{
    fs::{FlockOperation, flock},
    process::{Pid, Signal, getegid, geteuid, kill_process_group},
};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

static NEXT: AtomicU64 = AtomicU64::new(0);
const CHILD_ROOT: &str = "LETOPIS_FS_PRIVATE_TEST_ROOT";
const CHILD_ACTION: &str = "LETOPIS_FS_PRIVATE_TEST_ACTION";
const CHILD_TARGET: &str = "LETOPIS_FS_PRIVATE_TEST_TARGET";
const DEADLINE: Duration = Duration::from_secs(10);

pub(super) struct Fixture(pub PathBuf);
impl Fixture {
    pub fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "letopis-fs-private-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(root)
    }
    pub fn spawn_mutator(&self, target: &str, action: &str) -> Mutator {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "filesystem::test_support::run_mutation_child",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &self.0)
            .env(CHILD_TARGET, target)
            .env(CHILD_ACTION, action)
            .stdin(Stdio::null());
        let child = OwnedProcess::spawn(&mut command, ProcessGroup::Inherited);
        Mutator {
            child,
            root: self.0.clone(),
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
pub(super) struct Mutator {
    child: OwnedProcess,
    root: PathBuf,
}
impl Mutator {
    pub fn resume(&self) {
        fs::write(self.root.join("resume"), b"go").unwrap();
        wait_for(&self.root.join("done"));
    }
    pub fn finish(&mut self) {
        self.child.wait_success(DEADLINE);
    }
}

#[derive(Debug, Clone, Copy)]
enum ProcessGroup {
    Inherited,
    Owned,
}

struct OwnedProcess {
    child: Child,
    output: fs::File,
    group: Option<Pid>,
}
impl OwnedProcess {
    fn spawn(command: &mut Command, mode: ProcessGroup) -> Self {
        let output = tempfile::tempfile().unwrap();
        if matches!(mode, ProcessGroup::Owned) {
            command.process_group(0);
        }
        let child = command
            .stdout(Stdio::from(output.try_clone().unwrap()))
            .stderr(Stdio::from(output.try_clone().unwrap()))
            .spawn()
            .unwrap();
        let group = matches!(mode, ProcessGroup::Owned)
            .then(|| Pid::from_raw(i32::try_from(child.id()).unwrap()).unwrap());
        Self {
            child,
            output,
            group,
        }
    }
    fn wait_success(&mut self, deadline: Duration) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                self.terminate_group();
                assert!(status.success(), "child failed: {}", self.get_capture());
                return;
            }
            if start.elapsed() >= deadline {
                self.terminate();
                panic!("child exceeded deadline: {}", self.get_capture());
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
    fn terminate_group(&mut self) {
        if let Some(group) = self.group.take() {
            let _ = kill_process_group(group, Signal::KILL);
        }
    }
    fn terminate(&mut self) {
        // Reexecuted parents own a group; mutators and external children inherit it.
        self.terminate_group();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
    fn get_capture(&mut self) -> String {
        self.output.seek(SeekFrom::Start(0)).unwrap();
        let mut bytes = Vec::new();
        (&mut self.output)
            .take(64 * 1024)
            .read_to_end(&mut bytes)
            .unwrap();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        self.terminate();
    }
}

/// Reexecutes potentially blocking syscalls under a parent deadline and reap guard.
pub(super) fn run_bounded(name: &str) -> bool {
    const CASE: &str = "LETOPIS_FS_BOUNDED_CASE";
    if std::env::var(CASE).ok().as_deref() == Some(name) {
        return false;
    }
    let mut child = OwnedProcess::spawn(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CASE, name)
            .stdin(Stdio::null()),
        ProcessGroup::Owned,
    );
    child.wait_success(DEADLINE);
    true
}

/// Supervise only this fixture as an unprivileged child when the parent is root.
/// The child creates its fixture under /tmp, away from a root-private TMPDIR/cwd.
pub(super) fn run_bounded_unprivileged(name: &str) -> bool {
    const CASE: &str = "LETOPIS_FS_UNPRIVILEGED_CASE";
    const UNPRIVILEGED_ID: u32 = 65534;
    if std::env::var(CASE).ok().as_deref() == Some(name) {
        assert_ne!(
            geteuid().as_raw(),
            0,
            "permission child retained root privileges"
        );
        assert_eq!(
            getegid().as_raw(),
            UNPRIVILEGED_ID,
            "permission child retained its group"
        );
        return false;
    }
    if geteuid().as_raw() != 0 {
        return false;
    }
    // Root-private build paths are not traversable after dropping credentials.
    let executable_directory = tempfile::Builder::new()
        .prefix("letopis-permission-")
        .tempdir_in("/tmp")
        .unwrap();
    fs::set_permissions(
        executable_directory.path(),
        fs::Permissions::from_mode(0o711),
    )
    .unwrap();
    let executable = executable_directory.path().join("test");
    fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o555)).unwrap();
    let mut child = OwnedProcess::spawn(
        Command::new(executable)
            .args(["--exact", name, "--nocapture"])
            .env(CASE, name)
            .env("TMPDIR", "/tmp")
            .current_dir("/tmp")
            .uid(UNPRIVILEGED_ID)
            .gid(UNPRIVILEGED_ID)
            .stdin(Stdio::null()),
        ProcessGroup::Owned,
    );
    child.wait_success(DEADLINE);
    true
}

/// Create a FIFO under the same deadline and cleanup guard as other fixture children.
pub(super) fn create_fifo(path: &Path) {
    let mut child = OwnedProcess::spawn(Command::new("mkfifo").arg(path), ProcessGroup::Inherited);
    child.wait_success(DEADLINE);
}

fn hold_descendant_lease(root: &Path) {
    let lease = fs::File::create(root.join("lease")).unwrap();
    flock(&lease, FlockOperation::LockExclusive).unwrap();
    let _descendant = OwnedProcess::spawn(
        Command::new("sleep")
            .arg("30")
            .stdin(Stdio::from(lease.try_clone().unwrap())),
        ProcessGroup::Inherited,
    );
    fs::write(root.join("nested-ready"), b"ready").unwrap();
    std::thread::sleep(Duration::from_secs(30));
}

struct CleanupGroup(Pid);
impl Drop for CleanupGroup {
    fn drop(&mut self) {
        let _ = kill_process_group(self.0, Signal::KILL);
    }
}

fn wait_for(path: &Path) {
    let start = Instant::now();
    while !path.exists() {
        assert!(start.elapsed() < DEADLINE, "barrier exceeded deadline");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn run_mutation_child() {
    // Given: read the parent fixture configuration.
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = Path::new(&root);
    if std::env::var(CHILD_ACTION).as_deref() == Ok("hang") {
        hold_descendant_lease(root);
        return;
    }
    let target = root.join(std::env::var(CHILD_TARGET).unwrap());
    wait_for(&root.join("resume"));
    let action = std::env::var(CHILD_ACTION).unwrap();
    // When: perform the coordinated filesystem mutation.
    if action == "write" {
        fs::write(&target, b"changed length and bytes").unwrap();
    } else {
        fs::rename(&target, root.join("retired")).unwrap();
        match action.as_str() {
            "regular" => fs::write(&target, b"new regular contents").unwrap(),
            "symlink" => symlink("/dev/null", &target).unwrap(),
            "fifo" => create_fifo(&target),
            "directory" => fs::create_dir(&target).unwrap(),
            "delete" => {}
            _ => panic!("unknown mutation"),
        }
    }
    // Then: signal completion; the parent asserts the observed result.
    fs::write(root.join("done"), b"done").unwrap();
}

#[test]
fn forced_watchdog_terminates_nested_descendants() {
    const CASE: &str = "LETOPIS_FS_FORCED_WATCHDOG";
    if let Some(root) = std::env::var_os(CASE) {
        let fixture = Fixture(PathBuf::from(root));
        let _mutator = fixture.spawn_mutator("unused", "hang");
        std::thread::sleep(Duration::from_secs(30));
        return;
    }
    // Given: a real mutator and its external descendant retain the same lease.
    let fixture = Fixture::new();
    let mut owned = OwnedProcess::spawn(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "filesystem::test_support::forced_watchdog_terminates_nested_descendants",
            ])
            .env(CASE, &fixture.0),
        ProcessGroup::Owned,
    );
    let _cleanup = CleanupGroup(owned.group.unwrap());
    wait_for(&fixture.0.join("nested-ready"));
    let lease = fs::File::open(fixture.0.join("lease")).unwrap();
    assert!(flock(&lease, FlockOperation::NonBlockingLockExclusive).is_err());
    // When: force the real watchdog, then run its owner cleanup.
    let timeout = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        owned.wait_success(Duration::from_millis(50));
    }));
    assert!(timeout.is_err());
    drop(owned);
    // Then: no descendant retains the inherited open description after cleanup.
    let start = Instant::now();
    while flock(&lease, FlockOperation::NonBlockingLockExclusive).is_err() {
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "descendant survived watchdog"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn selects_inherited_or_owned_process_group() {
    // Given: a parent process group and a child for each explicit ownership mode.
    let parent_group = rustix::process::getpgrp();
    for mode in [ProcessGroup::Inherited, ProcessGroup::Owned] {
        let mut command = Command::new("sleep");
        command.arg("30");

        // When: the guarded child is spawned with the requested process group.
        let child = OwnedProcess::spawn(&mut command, mode);
        let pid = Pid::from_raw(i32::try_from(child.child.id()).unwrap()).unwrap();
        let actual = rustix::process::getpgid(Some(pid)).unwrap();

        // Then: inherited children share the parent group; owned groups use the child PID.
        match mode {
            ProcessGroup::Inherited => {
                assert_eq!(actual, parent_group);
                assert!(child.group.is_none());
            }
            ProcessGroup::Owned => {
                assert_eq!(actual, pid);
                assert_eq!(child.group, Some(pid));
            }
        }
    }
}
