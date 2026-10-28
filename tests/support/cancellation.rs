use serde_json::Value;
use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, Read},
    net::Shutdown,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{MetadataExt, PermissionsExt},
            net::{UnixListener, UnixStream},
        },
    },
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

/// Own the Unix listener that receives a live wrapper acknowledgement.
#[derive(Debug)]
pub struct ObserverSocket {
    _directory: TempDir,
    listener: UnixListener,
}
impl ObserverSocket {
    /// Bind an owned observer socket in a short temporary path.
    ///
    /// # Panics
    /// Panics if temporary directory creation or socket binding fails.
    pub fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("tk29-observer-")
            .tempdir_in("/tmp")
            .expect("observer directory");
        let address = directory.path().join("control.sock");
        let listener = UnixListener::bind(&address).expect("observer listener");
        Self {
            _directory: directory,
            listener,
        }
    }

    pub fn get_path(&self) -> PathBuf {
        self._directory.path().join("control.sock")
    }

    pub fn get_listener(&self) -> &UnixListener {
        &self.listener
    }
}

/// Test-owned Git executable that proxies non-target argv and reports the live `ls-files` child.
#[derive(Debug)]
pub struct GitProxy {
    _directory: TempDir,
    path: OsString,
}
impl GitProxy {
    /// Create an executable Git proxy that acknowledges the targeted child.
    ///
    /// # Panics
    /// Panics if tools cannot be resolved or the executable cannot be created.
    pub fn new(observer: &Path) -> Self {
        let python = resolve_program("python3").display().to_string();
        let real_git = resolve_program("git");
        let directory = tempfile::Builder::new()
            .prefix("tk29-git-")
            .tempdir_in("/tmp")
            .expect("proxy directory");
        let script = directory.path().join("git");
        fs::write(
            &script,
            format!(
                "#!{python}\n\
                 import json, os, socket, sys\n\
                 REAL = {real_git:?}\n\
                 OBSERVER = {observer:?}\n\
                 arguments = sys.argv[1:]\n\
                 if \"ls-files\" not in arguments:\n\
                 \x20   os.execv(REAL, [REAL] + arguments)\n\
                 connection = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)\n\
                 connection.connect(OBSERVER)\n\
                 connection.sendall(json.dumps({{\"pid\": os.getpid(), \"argv\": arguments}}).encode() + b\"\\n\")\n\
                 while connection.recv(4096):\n\
                 \x20   pass\n"
            ),
        )
        .expect("write Git proxy");
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).expect("proxy mode");
        let mut entries = vec![directory.path().to_path_buf()];
        if let Some(existing) = std::env::var_os("PATH") {
            entries.extend(std::env::split_paths(&existing));
        }
        Self {
            _directory: directory,
            path: std::env::join_paths(entries).expect("proxy PATH"),
        }
    }

    pub fn get_path(&self) -> &OsStr {
        &self.path
    }
}

/// The acknowledged live Git wrapper and a guard that releases it on every exit path.
#[derive(Debug)]
pub struct WrapperConnection {
    stream: Option<UnixStream>,
    pid: i32,
}
impl WrapperConnection {
    /// Accept and validate one live ls-files acknowledgement within the watchdog.
    ///
    /// # Panics
    /// Panics on timeout, I/O failure or a malformed child acknowledgement.
    pub fn accept(listener: &UnixListener) -> Self {
        wait_readable(listener.as_raw_fd(), WAIT);
        let (stream, _) = listener.accept().expect("wrapper connection");
        stream
            .set_read_timeout(Some(WAIT))
            .expect("wrapper read timeout");
        let mut reader = BufReader::new(stream.try_clone().expect("wrapper stream"));
        let mut line = String::new();
        assert!(
            reader
                .read_line(&mut line)
                .expect("wrapper acknowledgement")
                > 0,
            "wrapper closed before acknowledgement"
        );
        let acknowledgement: Value = serde_json::from_str(&line).expect("wrapper JSON");
        let pid = i32::try_from(acknowledgement["pid"].as_u64().expect("wrapper pid"))
            .expect("wrapper pid range");
        let argv = acknowledgement["argv"].as_array().expect("wrapper argv");
        assert!(
            argv.iter().any(|value| value.as_str() == Some("ls-files")),
            "wrapper was not the target Git invocation"
        );
        Self {
            stream: Some(stream),
            pid,
        }
    }

    /// Assert that the acknowledged child still holds its observer connection.
    ///
    /// # Panics
    /// Panics if the stream is absent or has closed or unexpected pending data.
    pub fn assert_alive(&self) {
        let stream = self.stream.as_ref().expect("observer stream");
        let revents = poll_observer(stream, Duration::ZERO);
        assert_eq!(
            revents, 0,
            "abandoned Git wrapper {} closed its observer socket",
            self.pid
        );
    }

    /// Release the child and consume its observer EOF within the watchdog.
    ///
    /// # Panics
    /// Panics on I/O failure, timeout or unexpected child output.
    pub fn release_and_wait(mut self) {
        let stream = self.stream.take().expect("observer stream");
        stream.shutdown(Shutdown::Write).expect("wrapper release");
        let revents = wait_for_observer_close(&stream, WAIT);
        assert_ne!(revents & (libc::POLLIN | libc::POLLHUP | libc::POLLERR), 0);
        let mut reader = stream.try_clone().expect("wrapper reader");
        let mut byte = [0];
        assert_eq!(
            reader.read(&mut byte).expect("wrapper EOF"),
            0,
            "Git wrapper wrote unexpected data"
        );
    }
}
impl Drop for WrapperConnection {
    fn drop(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}

fn wait_readable(fd: i32, timeout: Duration) {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: `poll` points to one initialized entry for the live listener.
    let ready = unsafe {
        libc::poll(
            &raw mut poll,
            1,
            i32::try_from(timeout.as_millis()).expect("poll timeout"),
        )
    };
    assert_eq!(ready, 1, "observer connection timeout");
}

fn wait_for_observer_close(stream: &UnixStream, timeout: Duration) -> i16 {
    let deadline = Instant::now() + timeout;
    loop {
        let revents = poll_observer(stream, Duration::ZERO);
        if revents != 0 {
            return revents;
        }
        assert!(
            Instant::now() < deadline,
            "Git wrapper did not close its observer socket"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

fn poll_observer(stream: &UnixStream, timeout: Duration) -> i16 {
    let mut poll = libc::pollfd {
        fd: stream.as_raw_fd(),
        events: libc::POLLIN | libc::POLLHUP | libc::POLLERR,
        revents: 0,
    };
    // SAFETY: `poll` points to one initialized entry for the live wrapper socket.
    let ready = unsafe {
        libc::poll(
            &raw mut poll,
            1,
            i32::try_from(timeout.as_millis()).expect("poll timeout"),
        )
    };
    assert!(ready >= 0, "observer poll failed");
    if ready == 0 { 0 } else { poll.revents }
}

fn resolve_program(name: &str) -> PathBuf {
    let path = std::env::var_os("PATH").unwrap_or_default();
    for directory in std::env::split_paths(&path) {
        let candidate = directory.join(name);
        if candidate.is_file() {
            return candidate;
        }
    }
    panic!("required program {name} was not found on PATH");
}
