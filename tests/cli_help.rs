#[expect(dead_code, reason = "Only the shared owned-process watchdog is needed")]
#[path = "support/cli.rs"]
mod cli;
use serde_json::Value;
use std::{
    fs::{self, File},
    os::fd::FromRawFd,
    process::{Command, Output, Stdio},
    time::Duration,
};
use tempfile::TempDir;

const COMMANDS: [&str; 8] = [
    "init", "check", "inspect", "search", "traverse", "reindex", "status", "config",
];

#[test]
fn open_pipe_without_input_uses_empty_request() {
    // Given: a Git repository and an open pipe whose writer sends nothing.
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    for (command, expected_code) in [("config", 0), ("search", 1)] {
        let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
        process
            .arg(command)
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // When: the writer remains open without sending input.
        let output = cli::OwnedChild::spawn(&mut process).collect_output(Duration::from_secs(2));
        // Then: the command completes with the result for an empty request.
        assert_eq!(output.status.code(), Some(expected_code), "{command}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        if command == "config" {
            assert_eq!(value, serde_json::json!({"ok": true, "result": null}));
        } else {
            assert_eq!(value["error"]["code"], "invalid_request");
        }
    }
}

#[test]
fn terminal_input_uses_empty_request_without_waiting() {
    // Given: a Git repository and an open terminal with no input.
    let root = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    for (command, expected_code) in [("config", 0), ("search", 1)] {
        let (master, slave) = open_terminal();
        let mut process = Command::new(env!("CARGO_BIN_EXE_letopis"));
        process
            .arg(command)
            .current_dir(root.path())
            .stdin(Stdio::from(slave))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        // When: the terminal stays open while the command runs.
        let output = cli::OwnedChild::spawn(&mut process).collect_output(Duration::from_secs(10));
        drop(master);
        // Then: config succeeds and search reports its missing required fields.
        assert_eq!(output.status.code(), Some(expected_code), "{command}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        if command == "config" {
            assert_eq!(value, serde_json::json!({"ok": true, "result": null}));
        } else {
            assert_eq!(value["error"]["code"], "invalid_request");
        }
    }
}

#[test]
fn help_and_version_work_without_repository_stdin_or_files() {
    // Given
    let root = tempfile::tempdir().unwrap();
    let mut invocations = vec![vec!["--help"], vec!["--version"]];
    invocations.extend(COMMANDS.map(|name| vec![name, "--help"]));
    for args in invocations {
        // When: stdin stays open and is never written, so any read would hang.
        let output = spawn_without_input(&root, &args);
        // Then
        assert_eq!(output.status.code(), Some(0), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.is_empty() && !stdout.starts_with('{'), "{args:?}");
        if args == ["--version"] {
            assert_eq!(stdout, "letopis 0.0.0\n");
        }
        assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0, "{args:?}");
    }
}

#[test]
fn unrecognized_help_forms_still_return_json_errors() {
    // Given
    let root = tempfile::tempdir().unwrap();
    for args in [
        vec![],
        vec!["--bogus"],
        vec!["--help", "search"],
        vec!["-h"],
    ] {
        // When: stdin stays open with no body or EOF while argv is rejected.
        let output = spawn_without_input(&root, &args);
        // Then
        assert_eq!(output.status.code(), Some(1), "{args:?}");
        assert!(output.stderr.is_empty(), "{args:?}");
        let value = serde_json::from_slice::<Value>(&output.stdout).unwrap();
        assert_eq!(value["ok"], false, "{args:?}");
        assert_eq!(value["error"]["code"], "invalid_request", "{args:?}");
    }
}

fn spawn_without_input(root: &TempDir, args: &[&str]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    command
        .args(args)
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cli::OwnedChild::spawn(&mut command).collect_output(Duration::from_secs(10))
}

fn open_terminal() -> (File, File) {
    let mut master = -1;
    let mut slave = -1;
    // SAFETY: both output pointers are valid; optional arguments are null.
    let result = unsafe {
        libc::openpty(
            &mut master,
            &mut slave,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    assert_eq!(
        result,
        0,
        "openpty failed: {}",
        std::io::Error::last_os_error()
    );
    // SAFETY: openpty returned two distinct owned descriptors on success.
    unsafe { (File::from_raw_fd(master), File::from_raw_fd(slave)) }
}
