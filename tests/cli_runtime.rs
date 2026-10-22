use serde_json::Value;
use std::{
    fs,
    io::Write,
    path::Path,
    process::{Command, Output, Stdio},
};
use tempfile::TempDir;

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
    let mut child = Command::new(env!("CARGO_BIN_EXE_letopis"))
        .args(args)
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(if lost { Stdio::null() } else { Stdio::piped() })
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(input).unwrap();
    child.wait_with_output().unwrap()
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
#[test]
fn malformed_requests_fail_before_service_directory_creation() {
    let root = make_repository();
    for (args, input) in [
        (vec!["unknown"], "{}"),
        (vec!["check", "extra"], "{}"),
        (vec!["check"], ""),
        (vec!["check"], "{\"paths\":null}"),
        (vec!["check"], "{\"extra\":1}"),
        (vec!["inspect"], "{}"),
        (vec!["embed", "--drain"], "{\"extra\":1}"),
    ] {
        let output = invoke(root.path(), &args, input.as_bytes(), false);
        assert_error(&output, "invalid_request");
        assert!(!root.path().join(".letopis").exists());
    }
}
#[test]
fn unavailable_routes_preserve_corpus_index_and_refs_after_lost_response() {
    let root = make_repository();
    let corpus = fs::read(root.path().join("knowledge.md")).unwrap();
    let index = fs::read(root.path().join(".git/index")).unwrap();
    let refs = run_git(root.path(), &["show-ref"]);
    for (args, input) in [
        (vec!["check"], "{}"),
        (vec!["inspect"], "{\"element_ids\":[\"knowledge.md\"]}"),
        (vec!["embed", "--drain"], ""),
    ] {
        let lost = invoke(root.path(), &args, input.as_bytes(), true);
        assert_eq!(lost.status.code(), Some(1));
        assert!(lost.stderr.is_empty());
        for _ in 0..2 {
            assert_error(
                &invoke(root.path(), &args, input.as_bytes(), false),
                "internal_error",
            );
        }
    }
    assert_eq!(fs::read(root.path().join("knowledge.md")).unwrap(), corpus);
    assert_eq!(fs::read(root.path().join(".git/index")).unwrap(), index);
    assert_eq!(run_git(root.path(), &["show-ref"]), refs);
    assert!(!root.path().join(".letopis").exists());
}

#[test]
fn missing_current_directory_uses_one_safe_envelope_after_input_validation() {
    for (body, code, message) in [
        ("{}", "internal_error", "cannot locate command directory"),
        ("null", "invalid_request", "invalid command input"),
    ] {
        // Arrange: keep the child blocked on stdin until its cwd is removed.
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
        // Act: parsing happens first; composition failure uses the same writer.
        child
            .stdin
            .take()
            .unwrap()
            .write_all(body.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        // Assert: neither the removed path nor a raw OS diagnostic escapes.
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
