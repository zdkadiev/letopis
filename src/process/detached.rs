use crate::service::{CommandError, CommandErrorKind, CommandResult, DrainLauncher};
use std::{
    io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};

pub(crate) struct DetachedProcess {
    executable: PathBuf,
    root: PathBuf,
}
impl DetachedProcess {
    pub(crate) fn new(executable: PathBuf, root: PathBuf) -> Self {
        Self { executable, root }
    }
}
impl DrainLauncher for DetachedProcess {
    fn spawn_drain(&self) -> CommandResult<()> {
        let mut command = Command::new(&self.executable);
        command
            .args(["embed", "--drain"])
            .current_dir(&self.root)
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
        let child = command.spawn().map_err(|cause| {
            CommandError::new_with_cause(
                CommandErrorKind::InternalError,
                "cannot launch background embedding drain",
                cause,
            )
        })?;
        // The foreground never waits for inference; the short-lived child owns
        // its execution and is reparented when this foreground process exits.
        drop(child);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::DetachedProcess;
    use crate::service::{CommandErrorKind, DrainLauncher};
    use std::{error::Error, fs};

    #[test]
    fn missing_executable_retains_private_cause() {
        let directory = tempfile::tempdir().unwrap();
        let process = DetachedProcess::new(
            directory.path().join("secret-executable"),
            directory.path().to_path_buf(),
        );
        let error = process.spawn_drain().unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(error.source().is_some());
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
    #[test]
    fn removed_working_directory_is_reported() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("secret-root");
        fs::create_dir(&root).unwrap();
        let process = DetachedProcess::new(std::env::current_exe().unwrap(), root.clone());
        fs::remove_dir(root).unwrap();
        let error = process.spawn_drain().unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(error.source().is_some());
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
}
