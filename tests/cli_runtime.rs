#[expect(dead_code, reason = "Only the shared owned-process watchdog is needed")]
#[path = "support/cli.rs"]
mod cli;
use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
    time::Duration,
};
use tempfile::TempDir;

#[test]
fn early_rejection_keeps_its_error_receipt_after_stdin_closes() {
    // Given: invalid arguments let the product reject input before reading stdin.
    let root = make_repository();
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    command
        .arg("unknown")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cli::OwnedChild::spawn(&mut command);
    let output = child.collect_output(Duration::from_secs(10));
    // When: the parent writes after the child has definitively exited.
    child.send_input(b"{}");
    // Then: a closed stdin does not hide the safe error or create service state.
    assert_error(&output, "invalid_request");
    assert!(!root.path().join(".letopis").exists());
}

#[test]
fn malformed_requests_fail_before_service_directory_creation() {
    // Given
    let root = make_repository();
    for (args, input) in [
        (vec!["unknown"], "{}"),
        (vec!["check", "extra"], "{}"),
        (vec!["check"], "{\"paths\":null}"),
        (vec!["check"], "{\"extra\":1}"),
        (vec!["inspect"], "{}"),
        (vec!["search", "extra"], "{}"),
        (vec!["search"], r#"["x",1]"#),
        (vec!["search"], r#"{"query":"x","limit":1,"filters":[]}"#),
        (vec!["search"], r#"{"query":"x","limit":1,"query":"y"}"#),
        (vec!["search"], r#"{"query":"x","limit":1.0}"#),
        (vec!["search"], r#"{"query":"x","limit":1e0}"#),
        (
            vec!["search"],
            r#"{"query":"x","limit":18446744073709551616}"#,
        ),
        (vec!["search"], r#"{"query":"x","limit":1} {}"#),
        (vec!["traverse", "extra"], "{}"),
        (
            vec!["traverse"],
            r#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1,"limit":0}"#,
        ),
        (
            vec!["traverse"],
            r#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1e0,"limit":1}"#,
        ),
        (
            vec!["traverse"],
            r#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1,"limit":1,"edge_limit":null}"#,
        ),
        (vec!["embed", "--drain"], "{\"extra\":1}"),
    ] {
        // When
        let output = invoke(root.path(), &args, input.as_bytes(), false);
        // Then
        assert_error(&output, "invalid_request");
        assert!(!root.path().join(".letopis").exists());
    }
}
#[test]
fn early_argv_rejection_preserves_response_after_closed_stdin() {
    // Given: invalid argv can terminate while the caller still owns the input pipe.
    let root = make_repository();
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    command
        .arg("unknown")
        .current_dir(root.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = cli::OwnedChild::spawn(&mut command);

    // When: the child rejects argv before stdin is sent, with no scheduling delay.
    let output = child.collect_output(Duration::from_secs(10));
    child.send_input(b"{}");

    // Then: the safe error remains observable and no service directory is created.
    assert_error(&output, "invalid_request");
    assert!(!root.path().join(".letopis").exists());
}

#[test]
fn nested_invalid_requests_do_not_initialize_repository_or_child() {
    // Given
    let root = make_repository();
    let nested = root.path().join("nested");
    fs::create_dir(&nested).unwrap();
    for (args, input, code) in [
        (
            vec!["search"],
            r#"{"query":"x","limit":1,"filters":{"needs_recheck":null}}"#,
            "invalid_request",
        ),
        (
            vec!["search"],
            r#"{"query":"x","limit":0}"#,
            "invalid_request",
        ),
        (
            vec!["traverse"],
            r#"{"element_ids":["a.md"],"mode":"both","direction":"both","depth":1,"limit":0}"#,
            "invalid_request",
        ),
        (
            vec!["traverse"],
            r#"{"element_ids":[],"mode":"both","direction":"both","depth":0,"limit":1}"#,
            "invalid_request",
        ),
        (vec!["inspect"], "{}", "invalid_request"),
        (
            vec!["embed", "--drain"],
            r#"{"extra":1}"#,
            "invalid_request",
        ),
    ] {
        // When / Then
        assert_error(&invoke(&nested, &args, input.as_bytes(), false), code);
        assert!(!root.path().join(".letopis").exists());
        assert!(!nested.join(".letopis").exists());
    }
}

#[test]
fn check_preserves_corpus_index_and_refs_after_lost_response() {
    // Given
    let root = make_repository();
    let corpus = fs::read(root.path().join("knowledge.md")).unwrap();
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let refs = run_git(root.path(), &["show-ref"]);
    let lost = invoke(root.path(), &["check"], b"{}", true);
    // When / Then
    assert_eq!(lost.status.code(), Some(0));
    assert!(lost.stderr.is_empty());
    for _ in 0..2 {
        let output = invoke(root.path(), &["check"], b"{}", false);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stderr.is_empty());
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            response,
            serde_json::json!({"ok":true,"result":{"errors":[]}})
        );
    }
    assert_eq!(fs::read(root.path().join("knowledge.md")).unwrap(), corpus);
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(run_git(root.path(), &["show-ref"]), refs);
    assert!(root.path().join(".letopis/cache.db").is_file());
}

#[test]
fn missing_current_directory_uses_one_safe_envelope_after_input_validation() {
    for (body, code, message) in [
        ("{}", "internal_error", "cannot locate command directory"),
        ("null", "invalid_request", "invalid command input"),
    ] {
        // Given: keep the child blocked on stdin until its cwd is removed.
        let root = tempfile::tempdir().unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_letopis"))
            .arg("check")
            .current_dir(root.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        fs::remove_dir(root.path()).unwrap();
        // When: parsing happens first; composition failure uses the same writer.
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        // Then: neither the removed path nor a raw OS diagnostic escapes.
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stderr.is_empty());
        assert_eq!(
            output.stdout.iter().filter(|byte| **byte == b'\n').count(),
            1
        );
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(response["error"]["code"], code);
        assert_eq!(response["error"]["message"], message);
    }
}

fn make_repository() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    run_git(root.path(), &["init", "-q"]);
    fs::write(
        root.path().join("knowledge.md"),
        "private corpus sentinel\n",
    )
    .unwrap();
    run_git(root.path(), &["add", "knowledge.md"]);
    run_git(
        root.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.org",
            "commit",
            "-qm",
            "fixture",
        ],
    );
    root
}

fn run_git(root: &Path, args: &[&str]) -> Vec<u8> {
    let result = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(result.status.success());
    result.stdout
}

fn invoke(root: &Path, args: &[&str], input: &[u8], lost: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    command
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(if lost { Stdio::null() } else { Stdio::piped() })
        .stderr(Stdio::piped());
    let mut child = cli::OwnedChild::spawn(&mut command);
    child.send_input(input);
    child.collect_output(Duration::from_secs(10))
}

fn assert_error(output: &Output, code: &str) {
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], code);
    assert_eq!(response["error"]["details"], serde_json::json!({}));
    if code == "internal_error" {
        assert_eq!(
            response["error"]["message"],
            "operation unavailable in this build"
        );
    }
}
