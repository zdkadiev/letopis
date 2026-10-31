use super::{
    CANCELLED_ENVELOPE, ResponseWriter, SUCCESS_ENVELOPE, cancelled_response, successful_response,
};
use crate::{
    cli::{CommandResponse, ResponseResult},
    domain::{KnowledgeId, LineRange, RepoPath},
    process::SigintListener,
    service::{CommandResult, SearchLink},
};
use serde_json::Value;
use std::{
    collections::BTreeSet,
    env, fs,
    io::{self, Read, Write},
    net::Shutdown,
    os::{
        fd::AsRawFd,
        unix::net::{UnixListener, UnixStream},
    },
    path::PathBuf,
    process::{Child, Command, ExitStatus, Stdio},
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

const CHILD_WAIT: Duration = Duration::from_secs(20);
const RESPONSE_CHILD_SOCKET: &str = "LETOPIS_RESPONSE_CHILD_SOCKET";
const RESPONSE_CHILD_MARKER: &str = "LETOPIS_RESPONSE_CHILD_MARKER";
const LATE_CHILD: &str = "cli::response::tests::signals::late_sigint_child";
const BACKPRESSURE_CHILD: &str = "cli::response::tests::signals::backpressure_sigint_child";
const REPEATED_CHILD: &str = "cli::response::tests::signals::repeated_sigint_child";
const LARGE_LINK_COUNT: usize = 16_000;

#[test]
fn success_output_survives_late_sigint() {
    // Given: a private child has written its one success response and stays alive.
    let mut child = SignalChild::spawn(LATE_CHILD);
    child.wait_marker(b"written");
    // When: the real listener receives SIGINT after output ownership is complete.
    child.send_sigint();
    let status = child.wait_exit();
    let response = child.get_response();
    // Then: the success envelope survives unchanged and the handled exit is 1.
    assert_eq!(status.code(), Some(1), "{status:?}");
    assert_eq!(response, SUCCESS_ENVELOPE);
}

#[test]
fn sigint_during_backpressure_finishes_one_response() {
    // Given: the child has observed WouldBlock while holding the response guard.
    let mut child = SignalChild::spawn(BACKPRESSURE_CHILD);
    child.wait_marker(b"would-block");
    // When: SIGINT arrives while the original response is still incomplete.
    child.send_sigint();
    let response = child.get_response();
    let status = child.wait_exit();
    // Then: the original response completes exactly once without a replacement.
    assert_eq!(status.code(), Some(1), "{status:?}");
    assert_eq!(response.iter().filter(|byte| **byte == b'\n').count(), 1);
    assert_eq!(response.last(), Some(&b'\n'));
    let parsed: Value = serde_json::from_slice(&response).unwrap();
    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["result"].as_array().unwrap().len(), LARGE_LINK_COUNT);
    assert!(!String::from_utf8_lossy(&response).contains("cancelled"));
}

#[test]
fn repeated_sigint_keeps_one_response() {
    // Given: a real listener and a cancellation write held before its first byte.
    let mut child = SignalChild::spawn(REPEATED_CHILD);
    child.wait_marker(b"ready");
    child.send_sigint();
    child.wait_marker(b"cancellation-writing");
    // When: more OS signals arrive while the first cancellation still owns output.
    child.send_sigint();
    child.send_sigint();
    child.stream.write_all(b"x").unwrap();
    let response = child.get_response();
    let status = child.wait_exit();
    // Then: only one complete cancellation envelope is emitted, with exit 1.
    assert_eq!(status.code(), Some(1), "{status:?}");
    assert_eq!(response, CANCELLED_ENVELOPE);
}

#[test]
#[ignore = "private child fixture for repeated_sigint_keeps_one_response"]
fn repeated_sigint_child() {
    let Some(address) = env::var_os(RESPONSE_CHILD_SOCKET) else {
        return;
    };
    // Given: a parent-owned output socket and an explicit checkpoint.
    let marker = PathBuf::from(env::var_os(RESPONSE_CHILD_MARKER).unwrap());
    let stream = UnixStream::connect(PathBuf::from(address)).unwrap();
    let writer = ResponseWriter::default();
    let callback_marker = marker.clone();
    // When: the actual listener handles OS signals through the product writer.
    let _listener = SigintListener::install(Box::new(move || {
        let mut output = GatedOutput {
            stream,
            marker: Some(callback_marker),
        };
        let _ = writer.write_response(&mut output, cancelled_response());
    }))
    .unwrap();
    // Then: keep the callback alive until the parent releases its first write.
    fs::write(marker, b"ready").unwrap();
    loop {
        thread::park();
    }
}

#[test]
#[ignore = "private child fixture for success_output_survives_late_sigint"]
fn late_sigint_child() {
    let Some(address) = env::var_os(RESPONSE_CHILD_SOCKET) else {
        return;
    };
    // Given: a parent-owned output socket and an explicit checkpoint.
    let marker = PathBuf::from(env::var_os(RESPONSE_CHILD_MARKER).unwrap());
    let output = UnixStream::connect(PathBuf::from(address)).unwrap();
    let signal_output = output.try_clone().unwrap();
    let writer = Arc::new(ResponseWriter::default());
    let signal_writer = Arc::clone(&writer);
    // When: the actual listener handles OS signals through the product writer.
    let _listener = SigintListener::install(Box::new(move || {
        let mut output = signal_output;
        let _ = signal_writer.write_response(&mut output, cancelled_response());
    }))
    .unwrap();
    let mut output = output;
    assert_eq!(writer.write_response(&mut output, successful_response()), 0);
    fs::write(marker, b"written").unwrap();
    // Then: remain alive for the parent to observe the handled exit.
    loop {
        thread::park();
    }
}

#[test]
#[ignore = "private child fixture for sigint_during_backpressure_finishes_one_response"]
fn backpressure_sigint_child() {
    let Some(address) = env::var_os(RESPONSE_CHILD_SOCKET) else {
        return;
    };
    // Given: a parent-owned output socket and an explicit checkpoint.
    let marker = PathBuf::from(env::var_os(RESPONSE_CHILD_MARKER).unwrap());
    let output = UnixStream::connect(PathBuf::from(address)).unwrap();
    let signal_output = output.try_clone().unwrap();
    let writer = Arc::new(ResponseWriter::default());
    let signal_writer = Arc::clone(&writer);
    // When: the actual listener handles OS signals through the product writer.
    let _listener = SigintListener::install(Box::new(move || {
        let mut output = signal_output;
        let _ = signal_writer.write_response(&mut output, cancelled_response());
    }))
    .unwrap();
    let mut output = PressureOutput {
        stream: output,
        marker,
    };
    let exit = writer.write_response(&mut output, large_successful_response());
    assert_eq!(exit, 0);
    // Then: remain alive for the parent to observe the handled exit.
    loop {
        thread::park();
    }
}

fn large_successful_response() -> CommandResult<CommandResponse> {
    let links = (0..LARGE_LINK_COUNT)
        .map(|index| {
            let id = format!("k{index}.md");
            SearchLink {
                element_id: KnowledgeId::parse(&id).unwrap(),
                path: RepoPath::parse(&id).unwrap(),
                lines: LineRange::new(1, 1).unwrap(),
                revision: 1,
                flags: BTreeSet::new(),
            }
        })
        .collect();
    Ok(CommandResponse {
        result: ResponseResult::Search(links),
        warnings: Vec::new(),
    })
}

struct SignalChild {
    child: Child,
    stream: UnixStream,
    marker: PathBuf,
    _directory: tempfile::TempDir,
}
impl SignalChild {
    fn spawn(test: &str) -> Self {
        let directory = tempfile::Builder::new()
            .prefix("tk29-response-")
            .tempdir_in("/tmp")
            .unwrap();
        let address = directory.path().join("output.sock");
        let marker = directory.path().join("ready");
        let listener = UnixListener::bind(&address).unwrap();
        let child = ChildGuard::new(
            Command::new(env::current_exe().unwrap())
                .args([
                    "--exact",
                    test,
                    "--ignored",
                    "--nocapture",
                    "--test-threads=1",
                ])
                .env(RESPONSE_CHILD_SOCKET, &address)
                .env(RESPONSE_CHILD_MARKER, &marker)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap(),
        );
        wait_readable(listener.as_raw_fd());
        let (stream, _) = listener.accept().unwrap();
        stream.set_read_timeout(Some(CHILD_WAIT)).unwrap();
        Self {
            child: child.into_child(),
            stream,
            marker,
            _directory: directory,
        }
    }

    fn wait_marker(&mut self, expected: &[u8]) {
        let deadline = Instant::now() + CHILD_WAIT;
        loop {
            if fs::read(&self.marker).ok().as_deref() == Some(expected) {
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                panic!("private response child exited early: {status:?}");
            }
            assert!(Instant::now() < deadline, "private response child timeout");
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn send_sigint(&mut self) {
        assert!(self.child.try_wait().unwrap().is_none(), "child exited");
        let pid = i32::try_from(self.child.id()).unwrap();
        // SAFETY: the unreaped child is owned here, so its PID cannot be reused
        // before collection; signal only that process, not its group.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
    }

    fn wait_exit(&mut self) -> ExitStatus {
        let deadline = Instant::now() + CHILD_WAIT;
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                return status;
            }
            if Instant::now() >= deadline {
                let _ = self.child.kill();
                let _ = self.child.wait();
                panic!("private response child exit timeout");
            }
            thread::sleep(Duration::from_millis(5));
        }
    }

    fn get_response(&mut self) -> Vec<u8> {
        let mut bytes = Vec::new();
        self.stream.read_to_end(&mut bytes).unwrap();
        bytes
    }
}
impl Drop for SignalChild {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        if !matches!(self.child.try_wait(), Ok(Some(_))) {
            let _ = self.child.kill();
        }
        let _ = self.child.wait();
    }
}

struct ChildGuard(Option<Child>);
impl ChildGuard {
    fn new(child: Child) -> Self {
        Self(Some(child))
    }

    fn into_child(mut self) -> Child {
        self.0.take().expect("guarded child")
    }
}
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn wait_readable(fd: i32) {
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
            i32::try_from(CHILD_WAIT.as_millis()).unwrap(),
        )
    };
    assert_eq!(ready, 1, "private response child did not connect");
}

struct GatedOutput {
    stream: UnixStream,
    marker: Option<PathBuf>,
}

impl Write for GatedOutput {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if let Some(marker) = self.marker.take() {
            fs::write(marker, b"cancellation-writing")?;
            self.stream.read_exact(&mut [0])?;
        }
        self.stream.write(buffer)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

/// Records actual socket pressure before retrying the same remaining response bytes.
/// The response guard is held across both attempts; the cloned signal descriptor
/// cannot write until this response finishes, so changing blocking mode is safe.
struct PressureOutput {
    stream: UnixStream,
    marker: PathBuf,
}

impl Write for PressureOutput {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.stream.set_nonblocking(true)?;
        let attempt = self.stream.write(buffer);
        self.stream.set_nonblocking(false)?;
        match attempt {
            Err(cause) if cause.kind() == io::ErrorKind::WouldBlock => {
                fs::write(&self.marker, b"would-block")?;
                self.stream.write(buffer)
            }
            result => result,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}
