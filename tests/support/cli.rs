use letopis::open_storage;
use letopis::service::FileLocker;
use serde_json::Value;
use std::{
    env, fs,
    io::{BufRead, BufReader, ErrorKind, Read, Write},
    os::fd::AsRawFd,
    os::unix::{
        fs::PermissionsExt,
        net::{UnixListener, UnixStream},
    },
    path::{Path, PathBuf},
    process::{Child, Command, Output, Stdio},
    sync::{OnceLock, mpsc},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;

const WAIT: Duration = Duration::from_secs(10);
static FIXTURE: OnceLock<PathBuf> = OnceLock::new();

pub fn get_fixture() -> &'static Path {
    FIXTURE.get_or_init(|| {
        if let Some(path) = env::var_os("LETOPIS_CLI_FIXTURE") {
            return verify_artifact(Path::new(&path));
        }
        build_fixture()
    })
}
fn build_fixture() -> PathBuf {
    let mut child = OwnedChild(
        Command::new(env!("CARGO"))
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .args([
                "build",
                "--locked",
                "--offline",
                "--example",
                "cli_runtime_fixture",
                "--message-format=json",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("build fixture"),
    );
    let output = child.collect_output(Duration::from_secs(120));
    assert!(
        output.status.success(),
        "fixture build failed: {}\n{}",
        String::from_utf8_lossy(&output.stderr),
        String::from_utf8_lossy(&output.stdout)
    );
    let artifact = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .find(|value| {
            value["reason"] == "compiler-artifact"
                && value["target"]["name"] == "cli_runtime_fixture"
                && value["executable"].is_string()
        })
        .expect("Cargo example artifact");
    verify_artifact(Path::new(artifact["executable"].as_str().unwrap()))
}
fn verify_artifact(path: &Path) -> PathBuf {
    let path = fs::canonicalize(path).expect("fixture artifact exists");
    let metadata = fs::metadata(&path).expect("fixture artifact metadata");
    assert!(metadata.is_file(), "fixture artifact is a regular file");
    assert_ne!(
        metadata.permissions().mode() & 0o111,
        0,
        "fixture artifact is executable"
    );
    path
}
pub struct OwnedChild(Child);
impl OwnedChild {
    /// Spawn a fixture process whose lifetime is owned by this guard.
    ///
    /// # Panics
    /// Panics when process setup or spawn fails.
    pub fn spawn(command: &mut Command) -> Self {
        Self(command.spawn().unwrap())
    }

    /// Write fixture input and close stdin, tolerating an already closed reader.
    ///
    /// A child may reject argv before reading stdin; callers must still verify
    /// its actual output and exit status.
    ///
    /// # Panics
    /// Panics if stdin was not piped, was consumed, or writing fails otherwise.
    pub fn send_input(&mut self, input: &[u8]) {
        if let Err(error) = self.0.stdin.take().unwrap().write_all(input) {
            assert_eq!(
                error.kind(),
                ErrorKind::BrokenPipe,
                "writing child input failed: {error}"
            );
        }
    }

    pub fn get_id(&self) -> u32 {
        self.0.id()
    }

    /// Deliver SIGINT to this live owned child, retaining ownership until reaping.
    ///
    /// # Panics
    /// Panics if the child already exited, its PID is invalid, or signaling fails.
    pub fn send_sigint(&mut self) {
        assert!(
            self.0.try_wait().unwrap().is_none(),
            "owned child already exited"
        );
        let pid = i32::try_from(self.0.id()).unwrap();
        // SAFETY: the unreaped child is owned by this guard, so its PID cannot
        // be reused before collection; signal only that process, not its group.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGINT) }, 0);
    }

    /// Kill and reap this still-running owned process without consulting PID files.
    ///
    /// # Panics
    /// Panics if the process already exited or termination/wait fails.
    pub fn kill(&mut self) {
        self.0.kill().unwrap();
        self.0.wait().unwrap();
    }

    /// Await owned process termination and captured pipe EOF within one watchdog.
    ///
    /// Null stdout is represented by empty bytes; captured stderr must be piped.
    /// # Panics
    /// Panics on process/I/O errors, missing stderr, timeout or inherited pipes.
    pub fn collect_output(&mut self, timeout: Duration) -> Output {
        let stdout = self.0.stdout.take();
        let stderr = self.0.stderr.take().unwrap();
        let out = stdout.map(start_reader);
        let err = start_reader(stderr);
        let deadline = Instant::now() + timeout;
        let status = loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                break status;
            }
            assert!(Instant::now() < deadline, "owned process exit timeout");
            thread::sleep(Duration::from_millis(5));
        };
        Output {
            status,
            stdout: out
                .map(|out| {
                    out.recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .expect("stdout EOF before child release")
                })
                .unwrap_or_default(),
            stderr: err
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("stderr EOF before child release"),
        }
    }
}
impl Drop for OwnedChild {
    fn drop(&mut self) {
        if !matches!(self.0.try_wait(), Ok(Some(_))) {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}
fn start_reader(mut input: impl Read + Send + 'static) -> mpsc::Receiver<Vec<u8>> {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).unwrap();
        let _ = send.send(bytes);
    });
    receive
}
pub struct Fixture {
    pub root: TempDir,
    _control_dir: TempDir,
    listener: UnixListener,
    socket_path: PathBuf,
}
impl Fixture {
    pub fn new() -> Self {
        let _ = get_fixture();
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join(".letopis")).unwrap();
        drop(
            open_storage(
                &root.path().join(".letopis/cache.db"),
                &root.path().join(".letopis/vectors.db"),
            )
            .unwrap(),
        );
        fs::write(
            root.path().join("knowledge.md"),
            "immutable corpus sentinel",
        )
        .unwrap();
        let control_dir = tempfile::Builder::new()
            .prefix("tk11-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket_path = control_dir.path().join("control.sock");
        let listener = UnixListener::bind(&socket_path).unwrap();
        Self {
            root,
            _control_dir: control_dir,
            listener,
            socket_path,
        }
    }
    pub fn spawn_foreground(&self, executable: Option<&Path>) -> OwnedChild {
        let mut command = Command::new(get_fixture());
        command
            .arg("foreground")
            .current_dir(self.root.path())
            .env("LETOPIS_FIXTURE_CONTROL", &self.socket_path)
            .env("LETOPIS_FIXTURE_SENTINEL", "safe inherited sentinel")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(executable) = executable {
            command.env("LETOPIS_FIXTURE_EXECUTABLE", executable);
        }
        OwnedChild(command.spawn().unwrap())
    }
    pub fn accept_drain(&self) -> Drain {
        let mut poll = libc::pollfd {
            fd: self.listener.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: poll points to one initialized entry and the listener is alive.
        let ready =
            unsafe { libc::poll(&raw mut poll, 1, i32::try_from(WAIT.as_millis()).unwrap()) };
        assert_eq!(ready, 1, "drain did not connect to owned observer");
        let (stream, _) = self.listener.accept().unwrap();
        stream.set_read_timeout(Some(WAIT)).unwrap();
        stream.set_write_timeout(Some(WAIT)).unwrap();
        Drain(BufReader::new(stream))
    }
    pub fn get_preserved_files(&self) -> Vec<Vec<u8>> {
        ["knowledge.md", ".letopis/cache.db", ".letopis/vectors.db"]
            .iter()
            .map(|path| fs::read(self.root.path().join(path)).unwrap())
            .collect()
    }
}
pub struct Drain(BufReader<UnixStream>);
impl Drain {
    pub fn get_event(&mut self) -> Value {
        let mut line = String::new();
        assert_ne!(
            self.0.read_line(&mut line).unwrap(),
            0,
            "expected child event"
        );
        let event: Value = serde_json::from_str(&line).unwrap();
        println!("CLI_OBSERVER {event}");
        event
    }
    pub fn send_command(&mut self, command: &str) {
        writeln!(self.0.get_mut(), "{command}").unwrap();
        self.0.get_mut().flush().unwrap();
    }
    pub fn expect_eof(&mut self) {
        let mut line = String::new();
        assert_eq!(self.0.read_line(&mut line).unwrap(), 0, "child exited");
    }
}
impl Drop for Drain {
    fn drop(&mut self) {
        // EOF cancels the owned fixture even if an assertion panics. Wait for
        // its control close with the existing socket timeout before teardown.
        let _ = self.0.get_mut().shutdown(std::net::Shutdown::Write);
        let _ = self.0.read_to_end(&mut Vec::new());
    }
}
pub fn finish_foreground(child: &mut OwnedChild) -> Value {
    child.0.stdin.take().unwrap().write_all(b"exit\n").unwrap();
    let output = child.collect_output(WAIT);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    println!("CLI_FOREGROUND {response}");
    response
}

// The socket may close before the dying process releases its lock descriptor.
// Only an acquired lease is a lock-release completion observation.
pub fn wait_for_embed_lock<F: FileLocker>(files: &F) -> F::Lock {
    let deadline = Instant::now() + WAIT;
    loop {
        if let Some(lease) = files
            .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
            .unwrap()
        {
            return lease;
        }
        assert!(Instant::now() < deadline, "embedding lock release timeout");
        thread::yield_now();
    }
}
