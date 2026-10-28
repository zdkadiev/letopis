#[expect(dead_code, reason = "Only the shared owned-process watchdog is needed")]
#[path = "support/cli.rs"]
mod cli;
use serde_json::Value;
use std::{
    fs,
    process::{Command, Output, Stdio},
    time::Duration,
};
use tempfile::TempDir;

const COMMANDS: [&str; 6] = [
    "check", "inspect", "search", "traverse", "reindex", "status",
];

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
        // When
        let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
        command
            .args(&args)
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cli::OwnedChild::spawn(&mut command);
        child.send_input(b"{}");
        let output = child.collect_output(Duration::from_secs(10));
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
