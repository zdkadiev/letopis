use super::output::response_encode;
use crate::service::{
    BackgroundRunner, CheckReport, CommandProblem, CommandResult, Configuration, Inspection,
    SearchLink, StatusReport, Traversal,
};
use std::{io::Write, sync::Mutex};

/// Typed application results before X-1 transport rendering.
#[derive(Debug)]
pub(crate) enum ResponseResult {
    Initialized,
    Status(StatusReport),
    Config(Option<Configuration>),
    Reindex(u64),
    Check(CheckReport),
    Inspect(Inspection),
    Search(Vec<SearchLink>),
    Traverse(Traversal),
    Drained,
    /// Plain-text help or version output; the only result not rendered as JSON.
    Help(String),
}
#[derive(Debug)]
pub(crate) struct CommandResponse {
    pub result: ResponseResult,
    pub warnings: Vec<CommandProblem>,
}
impl CommandResponse {
    /// A response without warnings.
    pub(crate) fn new(result: ResponseResult) -> Self {
        Self {
            result,
            warnings: Vec::new(),
        }
    }
}

/// Start background work after a successful foreground command.
/// A launch warning is attached to the response; a readiness failure replaces it.
pub(crate) fn finish_foreground(
    result: ResponseResult,
    background: &mut dyn BackgroundRunner,
) -> CommandResult<CommandResponse> {
    let warning = background.start_if_needed()?;
    Ok(CommandResponse {
        result,
        warnings: warning.into_iter().collect(),
    })
}

/// Owns the single X-1 response of one executable invocation.
///
/// The completion state records the winning exit code. A caller that loses the
/// race receives that code and never attempts a second output, including after
/// an earlier write failure.
#[derive(Debug, Default)]
pub(crate) struct ResponseWriter {
    completion_mutex: Mutex<Option<u8>>,
}

impl ResponseWriter {
    /// Serialize one complete response and write it if output is still unclaimed.
    ///
    /// Encoding finishes before the output guard is taken. The guard covers both
    /// writing and flushing. Returns the winning response exit code, or 1 for a
    /// poisoned state, encoding failure or failed output. An encoding failure
    /// still claims the response, so a later caller never emits a second one.
    pub(crate) fn response_write(
        &self,
        output: &mut dyn Write,
        response: CommandResult<CommandResponse>,
    ) -> u8 {
        let encoded = response_encode(response);
        let Ok(mut completion) = self.completion_mutex.lock() else {
            return 1;
        };
        if let Some(exit) = *completion {
            return exit;
        }
        let exit = match encoded {
            Ok((bytes, exit))
                if output
                    .write_all(&bytes)
                    .and_then(|()| output.flush())
                    .is_ok() =>
            {
                exit
            }
            _ => 1,
        };
        *completion = Some(exit);
        exit
    }
}

#[cfg(test)]
mod tests {
    use super::ResponseWriter;
    use crate::{
        cli::{CommandResponse, ResponseResult},
        service::{CommandError, CommandErrorKind},
    };
    use std::{
        io::{self, Write},
        sync::{
            Arc, Mutex,
            mpsc::{self, Receiver, Sender},
        },
        thread,
        time::Duration,
    };

    const CANCELLED_MESSAGE: &str = "command cancelled";

    mod signals {
        use super::{CANCELLED_ENVELOPE, CANCELLED_MESSAGE, ResponseWriter, SUCCESS_ENVELOPE};
        use crate::{
            cli::{CommandResponse, ResponseResult},
            domain::{KnowledgeId, LineRange, RepoPath},
            process::SigintListener,
            service::{CommandError, CommandErrorKind, SearchLink},
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
            let _listener = SigintListener::signal_listener_install(Box::new(move || {
                let mut output = GatedOutput {
                    stream,
                    marker: Some(callback_marker),
                };
                let _ = writer.response_write(
                    &mut output,
                    Err(CommandError::new(
                        CommandErrorKind::Cancelled,
                        CANCELLED_MESSAGE,
                    )),
                );
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
            let _listener = SigintListener::signal_listener_install(Box::new(move || {
                let mut output = signal_output;
                let _ = signal_writer.response_write(
                    &mut output,
                    Err(CommandError::new(
                        CommandErrorKind::Cancelled,
                        CANCELLED_MESSAGE,
                    )),
                );
            }))
            .unwrap();
            let mut output = output;
            assert_eq!(
                writer.response_write(
                    &mut output,
                    Ok(CommandResponse {
                        result: ResponseResult::Drained,
                        warnings: Vec::new()
                    })
                ),
                0
            );
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
            let _listener = SigintListener::signal_listener_install(Box::new(move || {
                let mut output = signal_output;
                let _ = signal_writer.response_write(
                    &mut output,
                    Err(CommandError::new(
                        CommandErrorKind::Cancelled,
                        CANCELLED_MESSAGE,
                    )),
                );
            }))
            .unwrap();
            let mut output = PressureOutput {
                stream: output,
                marker,
            };
            let exit = writer.response_write(&mut output, Ok(large_successful_response()));
            assert_eq!(exit, 0);
            // Then: remain alive for the parent to observe the handled exit.
            loop {
                thread::park();
            }
        }

        fn large_successful_response() -> CommandResponse {
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
            CommandResponse {
                result: ResponseResult::Search(links),
                warnings: Vec::new(),
            }
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
    }

    const WAIT: Duration = Duration::from_secs(10);
    const SHORT_WAIT: Duration = Duration::from_millis(200);
    const CANCELLED_ENVELOPE: &[u8] =
        b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";
    const SUCCESS_ENVELOPE: &[u8] = b"{\"ok\":true,\"result\":{}}\n";

    #[test]
    fn cancellation_winner_prevents_success_exit() {
        // Given: a cancellation response pauses while it owns output.
        let writer = Arc::new(ResponseWriter::default());
        let cancellation_writer = Arc::clone(&writer);
        let PausedOutput {
            writer: mut output,
            bytes_mutex,
            entered,
            resume,
        } = PausedOutput::new(Pause::Write);
        let cancellation = thread::spawn(move || {
            cancellation_writer.response_write(
                &mut output,
                Err(CommandError::new(
                    CommandErrorKind::Cancelled,
                    CANCELLED_MESSAGE,
                )),
            )
        });
        assert_eq!(entered.recv_timeout(WAIT).unwrap(), Pause::Write);
        // When: a successful response races for the same single output.
        let success_writer = Arc::clone(&writer);
        let (done, completed) = mpsc::channel();
        let normal = thread::spawn(move || {
            let mut output = Vec::new();
            let exit = success_writer.response_write(
                &mut output,
                Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                }),
            );
            done.send((exit, output)).unwrap();
        });
        assert!(completed.recv_timeout(SHORT_WAIT).is_err());
        resume.send(()).unwrap();
        let cancellation_exit = cancellation.join().unwrap();
        let (normal_exit, normal_output) = completed.recv_timeout(WAIT).unwrap();
        normal.join().unwrap();
        // Then: the cancellation envelope is the only output, and its exit code wins.
        assert_eq!(cancellation_exit, 1);
        assert_eq!(&*bytes_mutex.lock().unwrap(), CANCELLED_ENVELOPE);
        assert_eq!(normal_exit, 1);
        assert!(normal_output.is_empty());
    }

    #[test]
    fn normal_winner_prevents_second_json() {
        // Given: a response writer and output for a normal response.
        let writer = ResponseWriter::default();
        let mut first_output = Vec::new();
        // When: a normal response completes, then cancellation uses the same owner.
        let first_exit = writer.response_write(
            &mut first_output,
            Ok(CommandResponse {
                result: ResponseResult::Drained,
                warnings: Vec::new(),
            }),
        );
        let mut second_output = Vec::new();
        let second_exit = writer.response_write(
            &mut second_output,
            Err(CommandError::new(
                CommandErrorKind::Cancelled,
                CANCELLED_MESSAGE,
            )),
        );
        // Then: exactly one success envelope exists and the loser receives its exit code.
        assert_eq!(first_exit, 0);
        assert_eq!(first_output, SUCCESS_ENVELOPE);
        assert_eq!(second_exit, 0);
        assert!(second_output.is_empty());
    }

    #[test]
    fn write_failure_never_retries_output() {
        // Given: a response writer and an output that will fail on write.
        let writer = ResponseWriter::default();
        let attempts_mutex = Arc::new(Mutex::new(0));
        let mut failing = FailingWriter {
            attempts_mutex: Arc::clone(&attempts_mutex),
        };
        // When: the first write fails, then cancellation uses the same owner.
        let first_exit = writer.response_write(
            &mut failing,
            Ok(CommandResponse {
                result: ResponseResult::Drained,
                warnings: Vec::new(),
            }),
        );
        let mut second_output = Vec::new();
        let second_exit = writer.response_write(
            &mut second_output,
            Err(CommandError::new(
                CommandErrorKind::Cancelled,
                CANCELLED_MESSAGE,
            )),
        );
        // Then: no second output is attempted after the failed one.
        assert_eq!(first_exit, 1);
        assert_eq!(*attempts_mutex.lock().unwrap(), 1);
        assert_eq!(second_exit, 1);
        assert!(second_output.is_empty());
    }

    #[test]
    fn guard_covers_flush() {
        // Given: a success response is paused inside flush while holding the guard.
        let writer = Arc::new(ResponseWriter::default());
        let first_writer = Arc::clone(&writer);
        let PausedOutput {
            writer: mut first_output,
            bytes_mutex,
            entered,
            resume,
        } = PausedOutput::new(Pause::Flush);
        let first = thread::spawn(move || {
            first_writer.response_write(
                &mut first_output,
                Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                }),
            )
        });
        assert_eq!(entered.recv_timeout(WAIT).unwrap(), Pause::Flush);
        // When: a cancellation response races while the first flush is still pending.
        let second_writer = Arc::clone(&writer);
        let (done, completed) = mpsc::channel();
        let second = thread::spawn(move || {
            let mut output = Vec::new();
            let exit = second_writer.response_write(
                &mut output,
                Err(CommandError::new(
                    CommandErrorKind::Cancelled,
                    CANCELLED_MESSAGE,
                )),
            );
            done.send((exit, output)).unwrap();
        });
        assert!(completed.recv_timeout(SHORT_WAIT).is_err());
        resume.send(()).unwrap();
        let first_exit = first.join().unwrap();
        let (second_exit, second_output) = completed.recv_timeout(WAIT).unwrap();
        second.join().unwrap();
        // Then: flush completes before the loser observes the winning exit code.
        assert_eq!(first_exit, 0);
        assert_eq!(&*bytes_mutex.lock().unwrap(), SUCCESS_ENVELOPE);
        assert_eq!(second_exit, 0);
        assert!(second_output.is_empty());
    }

    #[test]
    fn cancel_output_wins_before_main_output() {
        // Given: the command thread pauses before it attempts any output.
        let writer = Arc::new(ResponseWriter::default());
        let (entered, main_entered) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let main_writer = Arc::clone(&writer);
        let main = thread::spawn(move || {
            entered.send(()).unwrap();
            released.recv().unwrap();
            let mut output = Vec::new();
            let exit = main_writer.response_write(
                &mut output,
                Ok(CommandResponse {
                    result: ResponseResult::Drained,
                    warnings: Vec::new(),
                }),
            );
            (exit, output)
        });
        main_entered.recv_timeout(WAIT).unwrap();
        // When: cancellation completes and flushes before main is released.
        let mut output = Vec::new();
        let cancellation_exit = writer.response_write(
            &mut output,
            Err(CommandError::new(
                CommandErrorKind::Cancelled,
                CANCELLED_MESSAGE,
            )),
        );
        release.send(()).unwrap();
        let (normal_exit, normal_output) = main.join().unwrap();
        // Then: main receives the winning exit code and appends no second JSON.
        assert_eq!(cancellation_exit, 1);
        assert_eq!(output, CANCELLED_ENVELOPE);
        assert_eq!(normal_exit, 1);
        assert!(normal_output.is_empty());
    }

    #[test]
    fn repeated_cancellation_responses_keep_one_envelope() {
        // Given: the first cancellation response owns the output.
        let writer = ResponseWriter::default();
        let mut output = Vec::new();
        let first_exit = writer.response_write(
            &mut output,
            Err(CommandError::new(
                CommandErrorKind::Cancelled,
                CANCELLED_MESSAGE,
            )),
        );
        // When: later callers offer the same cancellation response again.
        for _ in 0..2 {
            let mut repeated = Vec::new();
            let exit = writer.response_write(
                &mut repeated,
                Err(CommandError::new(
                    CommandErrorKind::Cancelled,
                    CANCELLED_MESSAGE,
                )),
            );
            assert_eq!(exit, 1);
            assert!(repeated.is_empty());
        }
        // Then: exactly one complete cancellation envelope exists.
        assert_eq!(first_exit, 1);
        assert_eq!(output, CANCELLED_ENVELOPE);
        assert_eq!(output.iter().filter(|byte| **byte == b'\n').count(), 1);
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Pause {
        Write,
        Flush,
    }

    struct PausingWriter {
        pause: Pause,
        bytes_mutex: Arc<Mutex<Vec<u8>>>,
        entered: Sender<Pause>,
        resume: Receiver<()>,
    }

    struct PausedOutput {
        writer: PausingWriter,
        bytes_mutex: Arc<Mutex<Vec<u8>>>,
        entered: Receiver<Pause>,
        resume: Sender<()>,
    }

    impl PausedOutput {
        fn new(pause: Pause) -> Self {
            let bytes_mutex = Arc::new(Mutex::new(Vec::new()));
            let (entered, entered_receiver) = mpsc::channel();
            let (resume, resume_receiver) = mpsc::channel();
            let writer = PausingWriter {
                pause,
                bytes_mutex: Arc::clone(&bytes_mutex),
                entered,
                resume: resume_receiver,
            };
            Self {
                writer,
                bytes_mutex,
                entered: entered_receiver,
                resume,
            }
        }
    }

    impl Write for PausingWriter {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            self.bytes_mutex.lock().unwrap().extend_from_slice(buffer);
            if self.pause == Pause::Write {
                self.pause_here();
            }
            Ok(buffer.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            if self.pause == Pause::Flush {
                self.pause_here();
            }
            Ok(())
        }
    }

    impl PausingWriter {
        fn pause_here(&mut self) {
            self.entered.send(self.pause).unwrap();
            self.resume.recv().unwrap();
        }
    }

    struct FailingWriter {
        attempts_mutex: Arc<Mutex<usize>>,
    }

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            *self.attempts_mutex.lock().unwrap() += 1;
            Err(io::Error::other("write failure"))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
