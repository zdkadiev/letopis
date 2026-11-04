use crate::service::{ProcessError, ProcessLauncher, ProcessResult};
use std::{
    ffi::OsString,
    io,
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};

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
mod tests {
    use super::DetachedProcess;
    use crate::service::ProcessLauncher;
    use std::{
        error::Error,
        fs,
        io::{self, ErrorKind},
        os::unix::fs::PermissionsExt,
        path::Path,
        thread,
        time::{Duration, Instant},
    };

    #[test]
    fn launches_literal_executable_with_generic_arguments_and_working_directory() {
        // Given: an executable with shell metacharacters records its literal invocation.
        let directory = tempfile::tempdir().unwrap();
        let working_directory = directory.path().join("working directory");
        fs::create_dir(&working_directory).unwrap();
        let executable = directory.path().join("executable ; literal");
        fs::write(
            &executable,
            "#!/bin/sh\nprintf '%s\\n' \"$0\" \"$@\" > argv\npwd -P > cwd\nprintf done > ready\n",
        )
        .unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        let arguments = [
            "literal argument",
            "$(touch INJECTED)",
            "; false",
            "",
            "--flag=value",
        ]
        .map(Into::into)
        .to_vec();
        let process =
            DetachedProcess::new(executable.clone(), arguments, working_directory.clone());
        // When: the configured executable runs without interpreting its arguments.
        process.process_spawn().unwrap();
        wait_for_file(&working_directory.join("ready"));
        // Then: executable, arbitrary arguments and cwd are preserved byte for byte.
        let expected = format!(
            "{}\nliteral argument\n$(touch INJECTED)\n; false\n\n--flag=value\n",
            executable.display()
        );
        assert_eq!(
            fs::read_to_string(working_directory.join("argv")).unwrap(),
            expected
        );
        assert_eq!(
            fs::read_to_string(working_directory.join("cwd")).unwrap(),
            format!(
                "{}\n",
                fs::canonicalize(&working_directory).unwrap().display()
            )
        );
        assert!(!working_directory.join("INJECTED").exists());
    }

    #[test]
    fn missing_executable_retains_private_cause() {
        // Given: the configured literal executable does not exist.
        let directory = tempfile::tempdir().unwrap();
        let process = DetachedProcess::new(
            directory.path().join("secret-executable"),
            Vec::new(),
            directory.path().to_path_buf(),
        );
        // When: the operating system rejects process creation.
        let error = process.process_spawn().unwrap_err();
        // Then: the typed launch failure preserves the private I/O cause and redacts its path.
        let cause = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
        assert_eq!(cause.kind(), ErrorKind::NotFound);
        assert_eq!(error.to_string(), "cannot spawn process");
        assert!(!format!("{error:?} {error}").contains("secret"));
    }
    #[test]
    fn removed_working_directory_is_reported() {
        // Given: the configured working directory disappears before launch.
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("secret-root");
        fs::create_dir(&root).unwrap();
        let process =
            DetachedProcess::new(std::env::current_exe().unwrap(), Vec::new(), root.clone());
        fs::remove_dir(root).unwrap();
        // When: process creation attempts to enter the removed directory.
        let error = process.process_spawn().unwrap_err();
        // Then: the working-directory failure remains typed and does not expose the path.
        let cause = error.source().unwrap().downcast_ref::<io::Error>().unwrap();
        assert_eq!(cause.kind(), ErrorKind::NotFound);
        assert!(!format!("{error:?} {error}").contains("secret"));
    }

    fn wait_for_file(path: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !path.exists() {
            assert!(
                Instant::now() < deadline,
                "child did not record its invocation"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}
