use crate::service::ports::git::{SnapshotError, SnapshotErrorKind::InternalError};
use std::{
    env,
    error::Error,
    ffi::OsString,
    fmt,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    process::{Command, ExitStatus, Stdio},
    thread,
};

// Preserve the original cause privately; every diagnostic surface is redacted.
enum SafeCause {
    Io(io::Error),
    Exit(ExitStatus),
}
impl fmt::Debug for SafeCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for SafeCause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O failure ({:?})", error.kind()),
            Self::Exit(status) => write!(f, "Git exited ({status})"),
        }
    }
}
impl Error for SafeCause {}
fn io_error(message: &'static str, error: io::Error) -> SnapshotError {
    SnapshotError::with_cause(message, SafeCause::Io(error))
}
pub(super) fn is_exit_code(error: &SnapshotError, code: i32) -> bool {
    matches!(error.source().and_then(|cause|cause.downcast_ref::<SafeCause>()),Some(SafeCause::Exit(status)) if status.code()==Some(code))
}

pub(super) fn check_version(output: &[u8]) -> Result<(), SnapshotError> {
    let unsupported = || SnapshotError::new(InternalError, "Git 2.29 or newer is required");
    let text = std::str::from_utf8(output).map_err(|_| unsupported())?;
    let version = text
        .strip_prefix("git version ")
        .ok_or_else(unsupported)?
        .split_whitespace()
        .next()
        .ok_or_else(unsupported)?;
    let mut parts = version.split('.');
    let major = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(unsupported)?;
    let minor = parts
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .ok_or_else(unsupported)?;
    if (major, minor) < (2, 29) {
        return Err(unsupported());
    }
    Ok(())
}
pub(super) struct GitProcess {
    program: PathBuf,
}
impl GitProcess {
    pub(super) fn system() -> Self {
        Self::with_executable("git".into())
    }
    pub(super) fn with_executable(program: PathBuf) -> Self {
        Self { program }
    }
    pub(super) fn run(
        &self,
        cwd: &Path,
        args: &[OsString],
        input: Option<&[u8]>,
    ) -> Result<Vec<u8>, SnapshotError> {
        let mut command = Command::new(&self.program);
        command.current_dir(cwd);
        for (key, _) in env::vars_os() {
            if key.to_string_lossy().starts_with("GIT_") {
                command.env_remove(key);
            }
        }
        command
            .env("GIT_NO_REPLACE_OBJECTS", "1")
            .env("GIT_ALLOW_PROTOCOL", "")
            .env("GIT_NO_LAZY_FETCH", "1")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("LC_ALL", "C");
        command.args(["-c", "core.hooksPath=/dev/null"]);
        let writes = args.first().is_some_and(|a| {
            a == "mktree"
                || a == "update-ref"
                || (a == "hash-object" && args.iter().any(|a| a == "-w"))
        });
        if writes {
            command.args(["-c", "core.fsync=all", "-c", "core.fsyncMethod=fsync"]);
        }
        let mut child = command
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| io_error("Git spawn failed", e))?;
        let mut stdin = child.stdin.take().expect("piped stdin");
        let mut stdout = child.stdout.take().expect("piped stdout");
        let mut stderr = child.stderr.take().expect("piped stderr");
        // All pipes progress concurrently. Even a broken stdin is reported only after
        // draining both outputs and waiting for this owned child.
        thread::scope(|scope| {
            let writer = scope.spawn(move || -> io::Result<()> {
                if let Some(bytes) = input {
                    stdin.write_all(bytes)?;
                }
                Ok(())
            });
            let reader = scope.spawn(move || {
                let mut bytes = Vec::new();
                stdout.read_to_end(&mut bytes).map(|_| bytes)
            });
            let drainer = scope.spawn(move || io::copy(&mut stderr, &mut io::sink()));
            let status = child.wait();
            if status.is_err() {
                // A wait failure must not leave a live child holding the pipes.
                let _ = child.kill();
                let _ = child.wait();
            }
            let written = writer.join();
            let read = reader.join();
            let drained = drainer.join();
            let status = status.map_err(|e| io_error("Git wait failed", e))?;
            if !status.success() {
                return Err(SnapshotError::with_cause(
                    "Git command failed",
                    SafeCause::Exit(status),
                ));
            }
            written
                .map_err(|_| SnapshotError::new(InternalError, "Git stdin worker failed"))?
                .map_err(|e| io_error("Git stdin failed", e))?;
            drained
                .map_err(|_| SnapshotError::new(InternalError, "Git stderr worker failed"))?
                .map_err(|e| io_error("Git stderr failed", e))?;
            read.map_err(|_| SnapshotError::new(InternalError, "Git stdout worker failed"))?
                .map_err(|e| io_error("Git stdout failed", e))
        })
    }
}
