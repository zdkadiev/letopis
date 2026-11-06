use super::SigintListener;
use crate::cli::ResponseWriter;
use crate::service::{CommandError, CommandErrorKind};
use signal_hook::{consts::SIGINT, iterator::Signals};
use std::{
    env,
    ffi::OsString,
    fs, io,
    path::Path,
    process::{Child, Command, Output, Stdio},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    thread,
    time::{Duration, Instant},
};

const WAIT: Duration = Duration::from_secs(10);
const CHILD_WAIT: Duration = Duration::from_secs(10);
const CHILD_POLL: Duration = Duration::from_millis(10);
const SHORT_WAIT: Duration = Duration::from_millis(200);
const CHILD_MARKER_ENV: &str = "LETOPIS_SIGINT_TEST_MARKER";
const CHILD_TEST: &str = "process::signal::tests::sigint_child_waits_for_owned_signal";
const CANCELLED_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"cancelled\",\"message\":\"command cancelled\",\"details\":{}}}\n";
const INTERNAL_ERROR_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"internal_error\",\"message\":\"cannot install command cancellation\",\"details\":{}}}\n";
const INVALID_INPUT_ENVELOPE: &[u8] =
    b"{\"ok\":false,\"error\":{\"code\":\"invalid_request\",\"message\":\"invalid command input\",\"details\":{}}}\n";

#[test]
fn ordinary_shutdown_closes_and_joins_without_signal() {
    // Given: an installed listener and a channel observing its teardown.
    let listener = SigintListener::signal_listener_install(Box::new(|| {})).unwrap();
    let (done, completed) = mpsc::channel();
    // When: the listener is dropped on another thread without receiving SIGINT.
    let dropper = thread::spawn(move || {
        drop(listener);
        let _ = done.send(());
    });
    // Then: closing the subscription wakes and joins its thread before the deadline.
    assert!(
        completed.recv_timeout(WAIT).is_ok(),
        "listener teardown blocked"
    );
    dropper.join().unwrap();
}

#[test]
fn spawned_listener_is_reaped_after_normal_teardown() {
    // Given: a listener whose thread stays alive until this test releases it.
    let signals = Signals::new([SIGINT]).unwrap();
    let (release, released) = mpsc::channel();
    let spawn = move |_listener| {
        Ok(thread::spawn(move || {
            let _ = released.recv();
        }))
    };
    let listener =
        SigintListener::signal_listener_start_with(signals, spawn, Box::new(|| {})).unwrap();
    let (done, completed) = mpsc::channel();
    // When: teardown starts while the spawned thread is still running.
    let dropper = thread::spawn(move || {
        drop(listener);
        let _ = done.send(());
    });
    // Then: teardown joins the thread instead of returning early.
    assert!(completed.recv_timeout(SHORT_WAIT).is_err());
    release.send(()).unwrap();
    assert!(completed.recv_timeout(WAIT).is_ok());
    dropper.join().unwrap();
}

#[test]
fn registration_or_spawn_failure_returns_error_without_listener() {
    // Given: a failing registration and an instrumented thread spawner.
    let spawns = Arc::new(AtomicUsize::new(0));
    let failed_registration = {
        let spawns = Arc::clone(&spawns);
        move |_listener| {
            spawns.fetch_add(1, Ordering::SeqCst);
            Err(io::Error::other("unexpected spawn"))
        }
    };
    // When: listener installation encounters the registration error.
    let registration = SigintListener::signal_listener_install_with(
        || Err(io::Error::other("registration failure")),
        failed_registration,
        Box::new(|| {}),
    );
    // Then: the error is returned without attempting to spawn a thread.
    assert!(registration.is_err());
    assert_eq!(spawns.load(Ordering::SeqCst), 0);

    // Given / When: registration succeeds but the injected thread spawner fails.
    let failed_spawn = SigintListener::signal_listener_install_with(
        || Signals::new([SIGINT]),
        |_listener| Err(io::Error::other("spawn failure")),
        Box::new(|| {}),
    );
    // Then: installation returns the spawn error without retaining a listener.
    assert!(failed_spawn.is_err());
}

#[test]
fn listener_installation_failure_returns_one_safe_internal_error() {
    // Given: a request body that must never be read.
    let mut output = Vec::new();
    // When
    let exit =
        crate::run_process_cli_with(&[OsString::from("check")], PanicOnRead, &mut output, |_| {
            Err(io::Error::other("SECRET_SIGNAL_FAILURE"))
        });
    // Then
    assert_eq!(exit, 1);
    assert_eq!(output, INTERNAL_ERROR_ENVELOPE);
    assert!(!String::from_utf8_lossy(&output).contains("SECRET_SIGNAL_FAILURE"));
}

#[test]
fn status_config_and_drain_skip_cancellation_registration() {
    for args in [vec!["status"], vec!["config"], vec!["embed", "--drain"]] {
        // Given
        let installs = Arc::new(AtomicUsize::new(0));
        let mut output = Vec::new();
        let installer = {
            let installs = Arc::clone(&installs);
            move |_listener| {
                installs.fetch_add(1, Ordering::SeqCst);
                Err(io::Error::other("must not install"))
            }
        };
        // When
        let parsed_args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let exit = crate::run_process_cli_with(&parsed_args, &b"null"[..], &mut output, installer);
        // Then
        assert_eq!(exit, 1);
        assert_eq!(installs.load(Ordering::SeqCst), 0);
        assert_eq!(output, INVALID_INPUT_ENVELOPE);
    }
}

#[test]
fn handled_sigint_writes_one_cancelled_envelope_and_exits_one() {
    // Given: an owned child that installs the real listener and waits.
    let directory = tempfile::tempdir().unwrap();
    let marker = directory.path().join("ready");
    let mut command = Command::new(env::current_exe().unwrap());
    command
        .args(["--exact", CHILD_TEST, "--nocapture", "--test-threads=1"])
        .env(CHILD_MARKER_ENV, &marker)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = OwnedChild::spawn(command);
    wait_for_file(&marker);
    // When: only the owned child receives SIGINT.
    let pid = i32::try_from(child.get_id()).unwrap();
    // SAFETY: the PID names the live child spawned above and the signal is SIGINT.
    let delivered = unsafe { libc::kill(pid, libc::SIGINT) };
    assert_eq!(delivered, 0);
    let output = child.wait_with_output();
    // Then: one complete cancellation envelope and the cancellation exit code.
    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(stdout.matches("\"code\":\"cancelled\"").count(), 1);
    let start = stdout.find("{\"ok\":false").unwrap();
    assert_eq!(
        &stdout[start..],
        String::from_utf8_lossy(CANCELLED_ENVELOPE)
    );
}

#[test]
fn owned_child_wait_fails_after_a_bounded_deadline() {
    // Given: a child that never observes the cancellation protocol.
    let mut command = Command::new("sleep");
    command
        .arg("30")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = OwnedChild::spawn(command);
    // When: the bounded wait expires and the child is killed and reaped.
    let error = child
        .try_wait_with_output_within(Duration::from_millis(200))
        .expect_err("a live child must time out");
    // Then: the failure is readable instead of an unbounded wait.
    assert!(error.contains("did not exit within"), "{error}");
}

#[test]
fn sigint_child_waits_for_owned_signal() {
    // This test is meaningful only in the owned child of the parent above.
    let Some(marker) = env::var_os(CHILD_MARKER_ENV) else {
        return;
    };
    // Given: the owned child response writer shared with its signal callback.
    let writer = Arc::new(ResponseWriter::default());
    let callback_writer = Arc::clone(&writer);
    // When: install the actual process listener.
    let _listener = SigintListener::signal_listener_install(Box::new(move || {
        let response = Err(CommandError::new(
            CommandErrorKind::Cancelled,
            "command cancelled",
        ));
        let mut output = io::stdout();
        let _ = callback_writer.response_write(&mut output, response);
    }))
    .unwrap();
    // Then: advertise registration and remain alive for the owned signal.
    fs::write(marker, b"ready").unwrap();
    loop {
        thread::park();
    }
}

struct PanicOnRead;

impl io::Read for PanicOnRead {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        panic!("request body must not be read");
    }
}

struct OwnedChild {
    child: Option<Child>,
}

impl OwnedChild {
    fn spawn(mut command: Command) -> Self {
        Self {
            child: Some(command.spawn().unwrap()),
        }
    }

    fn get_id(&self) -> u32 {
        self.child.as_ref().unwrap().id()
    }

    fn wait_with_output(self) -> Output {
        match self.try_wait_with_output_within(CHILD_WAIT) {
            Ok(output) => output,
            Err(message) => panic!("{message}"),
        }
    }

    fn try_wait_with_output_within(mut self, wait: Duration) -> Result<Output, String> {
        let mut child = self.child.take().unwrap();
        let deadline = Instant::now() + wait;
        loop {
            match child.try_wait() {
                Ok(Some(_)) => return Ok(child.wait_with_output().unwrap()),
                Ok(None) if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let output = child.wait_with_output().unwrap();
                    return Err(format!(
                        "owned child did not exit within {wait:?}; stdout: {}; stderr: {}",
                        String::from_utf8_lossy(&output.stdout),
                        String::from_utf8_lossy(&output.stderr)
                    ));
                }
                Ok(None) => thread::sleep(CHILD_POLL),
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("cannot observe owned child: {error}"));
                }
            }
        }
    }
}

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn wait_for_file(path: &Path) {
    let deadline = Instant::now() + WAIT;
    while !path.is_file() {
        assert!(Instant::now() < deadline, "child did not install listener");
        thread::sleep(Duration::from_millis(5));
    }
}
