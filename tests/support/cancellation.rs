use serde_json::Value;
use std::{
    ffi::CString,
    fs::{self, File, OpenOptions},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
    path::{Path, PathBuf},
    process::Output,
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

/// Native observations that distinguish real stdin and flock waits from startup.
#[path = "cancellation/observation.rs"]
pub(crate) mod observation;

/// Maximum wait for an owned process or readiness checkpoint.
pub const WAIT: Duration = Duration::from_secs(20);
/// Independent literal expected from handled cancellation.
pub const CANCELLED_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";

/// Assert one complete cancellation envelope and no leaked request text.
///
/// # Panics
/// Panics on any exit code other than 1, stderr output, extra bytes or a forbidden token.
pub fn assert_cancelled_output(output: &Output, context: &str, forbidden: &[&str]) {
    assert_eq!(output.status.code(), Some(1), "{context}: {output:?}");
    assert!(output.stderr.is_empty(), "{context}: {output:?}");
    assert_cancelled_bytes(&output.stdout, context);
    for token in forbidden {
        assert!(
            !String::from_utf8_lossy(&output.stdout).contains(token),
            "{context} leaked {token}"
        );
    }
}

/// Assert one complete cancellation envelope in an in-process child file.
///
/// # Panics
/// Panics unless the bytes are exactly the X-1 cancelled response.
pub fn assert_cancelled_bytes(bytes: &[u8], context: &str) {
    assert_eq!(bytes, CANCELLED_ENVELOPE, "{context}");
    assert_eq!(
        bytes.iter().filter(|byte| **byte == b'\n').count(),
        1,
        "{context}"
    );
    let response: Value = serde_json::from_slice(bytes).expect("one JSON value");
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "cancelled");
    assert_eq!(response["error"]["details"], serde_json::json!({}));
}

/// Create a test-owned FIFO and hold both ends so the child blocks without EOF.
///
/// # Panics
/// Panics when the FIFO cannot be created or opened.
pub fn create_stdin_fifo(directory: &Path) -> (File, PathBuf) {
    let path = directory.join("stdin.fifo");
    let address = CString::new(path.as_os_str().as_bytes()).expect("FIFO path");
    // SAFETY: `address` is a live NUL-terminated path and the mode is a plain integer.
    assert_eq!(unsafe { libc::mkfifo(address.as_ptr(), 0o600) }, 0);
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .expect("open test FIFO");
    (file, path)
}

/// Assert that another open description cannot take the index lock.
///
/// # Panics
/// Panics when the lock file is missing or the exclusive lock can be acquired.
pub fn assert_index_lock_held(path: &Path) {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .expect("index lock file");
    assert!(
        rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive).is_err(),
        "index lock was not held"
    );
}

/// Wait until the kernel lock is free, proving no abandoned child kept its descriptor.
///
/// # Panics
/// Panics on unexpected I/O errors or when the lock is not released before the watchdog.
pub fn wait_until_index_lock_free(path: &Path) {
    let deadline = Instant::now() + WAIT;
    loop {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(path)
            .expect("index lock file");
        match rustix::fs::flock(&file, rustix::fs::FlockOperation::NonBlockingLockExclusive) {
            Ok(()) => return,
            Err(rustix::io::Errno::WOULDBLOCK) => {}
            Err(error) => panic!("unexpected lock probe failure: {error}"),
        }
        assert!(Instant::now() < deadline, "index lock stayed held");
        thread::sleep(Duration::from_millis(5));
    }
}

/// Capture the stable device/inode identity of the persistent lock file.
pub fn read_lock_identity(path: &Path) -> (u64, u64) {
    let metadata = fs::metadata(path).expect("index lock metadata");
    (metadata.dev(), metadata.ino())
}

/// Own a FIFO read by the real in-process Git ignore stack while index.lock is held.
#[derive(Debug)]
pub struct GitReadBarrier {
    directory: TempDir,
    file: File,
    path: PathBuf,
}
impl GitReadBarrier {
    /// Create an untracked directory whose ignore file cannot reach EOF until released.
    ///
    /// # Panics
    /// Panics if the private fixture directory/FIFO cannot be created.
    pub fn new(root: &Path) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("cancel-git-")
            .tempdir_in(root)
            .unwrap();
        let (file, temporary) = create_stdin_fifo(directory.path());
        let path = directory.path().join(".gitignore");
        fs::rename(temporary, &path).unwrap();
        Self {
            directory,
            file,
            path,
        }
    }
    /// Require native evidence that this process reached the actual ignore-file read.
    pub fn observe(&self, pid: u32) {
        let trace = observation::observe_git_read(pid, &self.path);
        assert!(trace.starts_with("inside=git-read"), "{trace}");
    }
    /// Check cancellation did not remove or replace this independently owned FIFO.
    pub fn assert_owned(&self) {
        let held = self.file.metadata().unwrap();
        let named = fs::metadata(&self.path).unwrap();
        assert_eq!((held.dev(), held.ino()), (named.dev(), named.ino()));
    }
    /// Release and remove the fixture before retrying the normal command.
    pub fn release(self) {
        drop(self.file);
        self.directory.close().unwrap();
    }
}
