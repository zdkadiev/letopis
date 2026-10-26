mod support;

use rustix::fs::{FlockOperation, flock};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    net::TcpListener,
    time::Instant,
};
use support::reading::ReadingRepo;

#[test]
fn invalid_status_transport_fails_before_repository_setup() {
    // Given
    let repo = ReadingRepo::new();
    let service_dir = repo.get_root().join(".letopis");

    // When / Then
    for (args, input) in [
        (&["status"][..], b"null".as_slice()),
        (&["status", "extra"][..], b"{}".as_slice()),
    ] {
        // When
        let output = repo.invoke(args, input);

        // Then
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "invalid_request");
        assert_eq!(
            output.stdout.iter().filter(|byte| **byte == b'\n').count(),
            1
        );
        assert!(!service_dir.exists());
    }
}

#[test]
fn status_without_publication_does_not_create_service_files() {
    // Given
    let repo = ReadingRepo::new();
    let service_dir = repo.get_root().join(".letopis");
    assert!(!service_dir.exists());

    // When
    let output = repo.invoke(&["status"], b"{}");

    // Then
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty());
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        json!({"ok":true,"result":{
            "revision":null,"embedding_backlog":0,"embedding_failed":0,
            "locks":[],"problems":[]
        }})
    );
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert!(!service_dir.exists());
}

#[test]
fn status_with_config_never_connects_or_starts_drain() {
    // Given
    let repo = ReadingRepo::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    repo.write_config("ollama", &url, "fixture");
    let service_dir = repo.get_root().join(".letopis");

    // When
    let output = repo.invoke(&["status"], b"{}");

    // Then
    assert!(output.status.success(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        value,
        json!({"ok":true,"result":{
            "revision":null,"embedding_backlog":0,"embedding_failed":0,
            "locks":[],"problems":[]
        }})
    );
    assert!(matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    assert_eq!(
        fs::read_dir(&service_dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect::<Vec<_>>(),
        ["config.json"]
    );
}

#[test]
fn status_reuses_published_check_findings_without_refreshing() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "owner.md",
        "[missing](missing.md \"letopis:type=supports\")",
    );
    // When
    let checked = repo.invoke(&["check"], b"{}");
    // Then
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    let expected = json!({"code":"validation_failed","message":"knowledge target is missing",
        "details":{"path":"owner.md","reason":"dangling_target","element_id":"owner.md","target":"missing.md"}});
    assert_eq!(checked["result"]["errors"], json!([expected]));
    let cache = repo.get_root().join(".letopis/cache.db");
    let before = fs::read(&cache).unwrap();
    let vectors = repo.get_root().join(".letopis/vectors.db");
    let before_vectors = fs::read(&vectors).unwrap();
    // Given
    repo.write_knowledge("missing.md", "Target repaired");
    repo.write_config("ollama", "http://localhost/api", "changed-config");

    // When
    let observed = repo.invoke(&["status"], b"{}");

    // Then
    assert!(observed.status.success(), "{observed:?}");
    let observed: Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(observed["result"]["problems"], json!([expected]));
    assert!(observed["result"]["revision"].as_u64().is_some());
    assert_eq!(observed["result"]["embedding_backlog"], 1);
    assert_eq!(observed["result"]["embedding_failed"], 0);
    assert_eq!(fs::read(&cache).unwrap(), before);
    assert_eq!(fs::read(&vectors).unwrap(), before_vectors);
}

#[test]
fn status_and_check_track_repairs_across_processes() {
    // Given
    let repo = ReadingRepo::new();
    seed_diagnostic_files(&repo);

    // When
    let checked = repo.invoke(&["check"], b"{}");
    // Then
    assert!(checked.status.success(), "{checked:?}");
    let checked: Value = serde_json::from_slice(&checked.stdout).unwrap();
    // When
    let status = read_status(&repo);
    let problems = status["result"]["problems"].as_array().unwrap();
    // Then
    assert_eq!(Value::Array(problems.clone()), checked["result"]["errors"]);
    let actual: Vec<_> = problems
        .iter()
        .map(|p| {
            (
                p["details"]["reason"].as_str().unwrap(),
                p["details"]["target"].as_str().unwrap_or(""),
            )
        })
        .collect();
    assert_eq!(
        actual,
        [
            ("includes_cycle", ""),
            ("includes_cycle", ""),
            ("invalid_metadata", ""),
            ("self_loop", ""),
            ("dangling_target", "missing.md"),
            ("dangling_target", "target.md#missing"),
            ("missing_source_snapshot", ""),
        ]
    );
    assert!(!status.to_string().contains("PRIVATE_BODY"));

    // Given
    repo.write_knowledge("missing.md", "Restored");
    repo.write(
        "target.md",
        b"# Missing {#missing}\n<!-- letopis: {\"created_at\":1} -->\nTarget\n",
    );
    repo.write_knowledge("loop.md", "No self link");
    repo.write_knowledge("b.md", "No cycle");
    repo.write_knowledge("sources.md", "No source");
    repo.write_knowledge("bad.md", "Valid metadata");
    // When / Then
    assert_eq!(
        read_status(&repo)["result"]["problems"],
        status["result"]["problems"]
    );

    // When
    let refreshed = repo.invoke(&["check"], b"{}");
    // Then
    assert!(refreshed.status.success(), "{refreshed:?}");
    let refreshed: Value = serde_json::from_slice(&refreshed.stdout).unwrap();
    assert_eq!(refreshed["result"]["errors"], json!([]));
    assert_eq!(read_status(&repo)["result"]["problems"], json!([]));
}

#[test]
fn status_reports_rename_delete_and_restore_after_refresh() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("owner.md", "[target](target.md \"letopis:type=supports\")");
    repo.write_knowledge("target.md", "Target");
    // When
    let initial = repo.invoke(&["check"], b"{}");
    // Then
    assert!(initial.status.success(), "{initial:?}");
    assert_eq!(read_status(&repo)["result"]["problems"], json!([]));

    // Given
    fs::rename(
        repo.get_root().join("target.md"),
        repo.get_root().join("renamed.md"),
    )
    .unwrap();
    // When
    let renamed = repo.invoke(&["check"], b"{}");
    // Then
    assert!(renamed.status.success(), "{renamed:?}");
    assert_eq!(
        read_status(&repo)["result"]["problems"][0]["details"]["target"],
        "target.md"
    );

    // Given
    fs::rename(
        repo.get_root().join("renamed.md"),
        repo.get_root().join("target.md"),
    )
    .unwrap();
    // When
    let restored = repo.invoke(&["check"], b"{}");
    // Then
    assert!(restored.status.success(), "{restored:?}");
    assert_eq!(read_status(&repo)["result"]["problems"], json!([]));

    // Given
    fs::remove_file(repo.get_root().join("target.md")).unwrap();
    // When
    let deleted = repo.invoke(&["check"], b"{}");
    // Then
    assert!(deleted.status.success(), "{deleted:?}");
    assert_eq!(
        read_status(&repo)["result"]["problems"][0]["details"]["target"],
        "target.md"
    );
}

#[test]
fn status_reports_busy_index_lock_without_waiting_or_writing_pid() {
    // Given
    let repo = ReadingRepo::new();
    // When
    let checked = repo.invoke(&["check"], b"{}");
    // Then
    assert!(checked.status.success(), "{checked:?}");
    let path = repo.get_root().join(".letopis/index.lock");
    let mut owner = OpenOptions::new().write(true).open(&path).unwrap();
    let pid = std::process::id();
    owner.set_len(0).unwrap();
    writeln!(owner, "{pid}").unwrap();
    owner.flush().unwrap();
    flock(&owner, FlockOperation::LockExclusive).unwrap();
    let original = fs::read(&path).unwrap();

    let start = Instant::now();
    // When
    let status = read_status(&repo);

    // Then
    assert!(start.elapsed().as_secs() < 2);
    assert_eq!(
        status["result"]["locks"],
        json!([{"name":"index","pid":pid}])
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    flock(&owner, FlockOperation::Unlock).unwrap();
}

fn seed_diagnostic_files(repo: &ReadingRepo) {
    repo.write("bad.md", b"<!-- letopis: broken PRIVATE_BODY -->\n");
    repo.write_knowledge("owner.md", "[file](missing.md \"letopis:type=supports\") [anchor](target.md#missing \"letopis:type=supports\")");
    repo.write(
        "target.md",
        b"# Other {#other}\n<!-- letopis: {\"created_at\":1} -->\nTarget\n",
    );
    repo.write_knowledge("loop.md", "[self](loop.md \"letopis:type=includes\")");
    repo.write_knowledge("a.md", "[b](b.md \"letopis:type=includes\")");
    repo.write_knowledge("b.md", "[a](a.md \"letopis:type=includes\")");
    repo.write_knowledge("sources.md", "[source](code.rs \"letopis:type=external\")");
    repo.write("code.rs", b"source");
}

fn read_status(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["status"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
