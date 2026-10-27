use serde_json::Value;
#[cfg(target_os = "macos")]
use std::process::{Command, Stdio};
use std::{
    ffi::{CString, OsStr, OsString},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader},
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

pub const WAIT: Duration = Duration::from_secs(20);
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

/// Wait until the process main thread is blocked reading the test FIFO.
///
/// # Panics
/// Panics when no native read observation appears before the watchdog expires.
#[cfg(target_os = "macos")]
pub fn observe_stdin_wait(pid: u32, _stdin: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let trace = sample_process(pid);
        if trace.contains("letopis::cli::input::parse_request")
            && trace.contains("Stdin")
            && has_native_frame(&trace, "read", "libsystem_kernel.dylib")
        {
            return format!("inside=stdin\n{trace}");
        }
        assert!(
            Instant::now() < deadline,
            "process never entered stdin read:\n{trace}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Wait until the process main thread is blocked reading the test FIFO.
///
/// # Panics
/// Panics when no matching syscall and descriptor identity appears before the watchdog expires.
#[cfg(target_os = "linux")]
pub fn observe_stdin_wait(pid: u32, stdin: &Path) -> String {
    wait_for_trace(
        || {
            let syscall = fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
            let fields = syscall.split_whitespace().collect::<Vec<_>>();
            let number = fields.first()?.parse::<libc::c_long>().ok()?;
            let descriptor = fields.get(1)?.strip_prefix("0x")?;
            if number != libc::SYS_read || u64::from_str_radix(descriptor, 16).ok()? != 0 {
                return None;
            }
            let observed = fs::read_link(format!("/proc/{pid}/fd/0")).ok()?;
            if observed.as_path() != stdin {
                return None;
            }
            Some(format!(
                "inside=stdin\n{}",
                serde_json::json!({
                    "pid": pid,
                    "syscall": syscall,
                    "stdin": observed,
                })
            ))
        },
        "stdin read",
    )
}

/// Wait until the process main thread is blocked in the real index lock syscall.
///
/// # Panics
/// Panics when no matching native observation appears before the watchdog expires.
#[cfg(target_os = "macos")]
pub fn observe_index_flock(pid: u32, _lock: &Path) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        let trace = sample_process(pid);
        if trace.contains("lock_index")
            && has_native_frame(&trace, "flock", "libsystem_kernel.dylib")
        {
            return format!("inside=flock\n{trace}");
        }
        assert!(
            Instant::now() < deadline,
            "process never entered blocking flock:\n{trace}"
        );
        thread::sleep(Duration::from_millis(5));
    }
}

/// Wait until the process main thread is blocked in the real index lock syscall.
///
/// # Panics
/// Panics when no matching syscall and descriptor identity appears before the watchdog expires.
#[cfg(target_os = "linux")]
pub fn observe_index_flock(pid: u32, lock: &Path) -> String {
    wait_for_trace(
        || {
            let syscall = fs::read_to_string(format!("/proc/{pid}/syscall")).ok()?;
            let fields = syscall.split_whitespace().collect::<Vec<_>>();
            let number = fields.first()?.parse::<libc::c_long>().ok()?;
            let descriptor = fields.get(1)?.strip_prefix("0x")?;
            let operation = fields.get(2)?.strip_prefix("0x")?;
            if number != libc::SYS_flock
                || i32::try_from(u64::from_str_radix(operation, 16).ok()?).ok()? != libc::LOCK_EX
            {
                return None;
            }
            let descriptor = u64::from_str_radix(descriptor, 16).ok()?;
            let path = PathBuf::from(format!("/proc/{pid}/fd/{descriptor}"));
            let observed = fs::metadata(&path).ok()?;
            let expected = fs::metadata(lock).ok()?;
            if observed.ino() != expected.ino() || observed.dev() != expected.dev() {
                return None;
            }
            Some(format!(
                "inside=flock\n{}",
                serde_json::json!({
                    "pid": pid,
                    "syscall": syscall,
                    "descriptor": path,
                    "device": observed.dev(),
                    "inode": observed.ino(),
                })
            ))
        },
        "blocking flock",
    )
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
pub fn lock_identity(path: &Path) -> (u64, u64) {
    let metadata = fs::metadata(path).expect("index lock metadata");
    (metadata.dev(), metadata.ino())
}

/// Own the Unix listener that receives a live wrapper acknowledgement.
pub struct ObserverSocket {
    _directory: TempDir,
    listener: UnixListener,
}
impl ObserverSocket {
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
pub struct GitProxy {
    _directory: TempDir,
    path: OsString,
}
impl GitProxy {
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
pub struct WrapperConnection {
    stream: Option<UnixStream>,
    pid: i32,
}
impl WrapperConnection {
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

    pub fn assert_alive(&self) {
        assert!(is_alive(self.pid), "abandoned Git wrapper exited early");
    }

    pub fn release_and_wait(mut self) {
        self.release();
        let deadline = Instant::now() + WAIT;
        while is_alive(self.pid) {
            assert!(Instant::now() < deadline, "Git wrapper did not exit");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn release(&mut self) {
        if let Some(stream) = self.stream.take() {
            let _ = stream.shutdown(Shutdown::Both);
        }
    }
}
impl Drop for WrapperConnection {
    fn drop(&mut self) {
        self.release();
        let deadline = Instant::now() + Duration::from_secs(2);
        while is_alive(self.pid) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
    }
}

#[cfg(target_os = "macos")]
fn sample_process(pid: u32) -> String {
    let trace = tempfile::NamedTempFile::new().expect("sample file");
    let status = Command::new("/usr/bin/sample")
        .arg(pid.to_string())
        .args(["1", "1", "-file"])
        .arg(trace.path())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("run sample");
    assert!(status.success(), "sample failed for pid {pid}");
    fs::read_to_string(trace.path()).expect("sample trace")
}

#[cfg(target_os = "macos")]
fn has_native_frame(trace: &str, symbol: &str, image: &str) -> bool {
    trace.lines().any(|line| {
        // Sample indents counted frames with tree glyphs such as `+`.
        let counted = line.trim_start_matches(|character: char| !character.is_ascii_digit());
        let mut fields = counted.split_whitespace();
        fields
            .next()
            .is_some_and(|count| count.parse::<u64>().is_ok())
            && fields.next() == Some(symbol)
            && line.contains(image)
    })
}

#[cfg_attr(
    target_os = "macos",
    expect(dead_code, reason = "Only Linux observers use the polling helper")
)]
fn wait_for_trace(mut observe: impl FnMut() -> Option<String>, waiting: &str) -> String {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(trace) = observe() {
            return trace;
        }
        assert!(Instant::now() < deadline, "process never entered {waiting}");
        thread::sleep(Duration::from_millis(5));
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

fn is_alive(pid: i32) -> bool {
    // SAFETY: signal 0 performs only an existence/permission probe.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}
