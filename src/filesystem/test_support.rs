use std::fs;
use std::os::unix::fs::symlink;
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
        let child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "filesystem::test_support::mutation_child",
                "--nocapture",
            ])
            .env(CHILD_ROOT, &self.0)
            .env(CHILD_TARGET, target)
            .env(CHILD_ACTION, action)
            .stdin(Stdio::null())
            .spawn()
            .unwrap();
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
    child: Child,
    root: PathBuf,
}
impl Mutator {
    pub fn resume(&self) {
        fs::write(self.root.join("resume"), b"go").unwrap();
        wait_for(&self.root.join("done"));
    }
    pub fn finish(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(start.elapsed() < DEADLINE, "mutator exceeded deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for Mutator {
    fn drop(&mut self) {
        let _ = self.child.kill();
        self.child.wait().unwrap();
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
fn mutation_child() {
    let Some(root) = std::env::var_os(CHILD_ROOT) else {
        return;
    };
    let root = Path::new(&root);
    let target = root.join(std::env::var(CHILD_TARGET).unwrap());
    wait_for(&root.join("resume"));
    let action = std::env::var(CHILD_ACTION).unwrap();
    if action == "write" {
        fs::write(&target, b"changed length and bytes").unwrap();
    } else {
        fs::rename(&target, root.join("retired")).unwrap();
        match action.as_str() {
            "regular" => fs::write(&target, b"new regular contents").unwrap(),
            "symlink" => symlink("/dev/null", &target).unwrap(),
            "fifo" => {
                let mut child = OwnedProcess(Command::new("mkfifo").arg(&target).spawn().unwrap());
                child.wait_success();
            }
            "directory" => fs::create_dir(&target).unwrap(),
            "delete" => {}
            _ => panic!("unknown mutation"),
        }
    }
    fs::write(root.join("done"), b"done").unwrap();
}

struct OwnedProcess(Child);
impl OwnedProcess {
    fn wait_success(&mut self) {
        let start = Instant::now();
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success());
                return;
            }
            assert!(start.elapsed() < DEADLINE, "child exceeded deadline");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}
impl Drop for OwnedProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.0.wait().unwrap();
    }
}

/// Reexecutes potentially blocking syscalls under a parent deadline and reap guard.
pub(super) fn run_bounded(name: &str) -> bool {
    const CASE: &str = "LETOPIS_FS_BOUNDED_CASE";
    if std::env::var(CASE).ok().as_deref() == Some(name) {
        return false;
    }
    let mut child = OwnedProcess(
        Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(CASE, name)
            .stdin(Stdio::null())
            .spawn()
            .unwrap(),
    );
    child.wait_success();
    true
}
