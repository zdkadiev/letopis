use crate::service::{ProcessError, ProcessLauncher, ProcessResult};
use std::{
    ffi::OsString,
    io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};

/// Owning handle for a spawned process that the launcher completes without waiting.
pub(crate) struct DetachedProcess {
    executable: PathBuf,
    arguments: Vec<OsString>,
    working_directory: PathBuf,
}

impl DetachedProcess {
    pub(crate) fn new(
        executable: PathBuf,
        arguments: Vec<OsString>,
        working_directory: PathBuf,
    ) -> Self {
        Self {
            executable,
            arguments,
            working_directory,
        }
    }
}

impl ProcessLauncher for DetachedProcess {
    fn process_spawn(&self) -> ProcessResult<()> {
        let mut command = Command::new(&self.executable);
        command
            .args(&self.arguments)
            .current_dir(&self.working_directory)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        // SAFETY: the post-fork closure only calls async-signal-safe setsid and
        // captures errno on failure. It allocates nothing and touches no locks,
        // environment, database state, formatting or other runtime callbacks.
        unsafe {
            command.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn().map_err(ProcessError::new)?;
        // Dropping the handle does not wait; the child owns its execution and
        // is reparented when the launching process exits.
        drop(child);
        Ok(())
    }
}

#[cfg(test)]
#[path = "detached_test.rs"]
mod tests;
