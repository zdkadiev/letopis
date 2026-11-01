mod support;

use letopis::service::{NodeMetadata, Storage, VectorOutcome};
use rustix::fs::{FlockOperation, flock};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    net::TcpListener,
    os::unix::fs::symlink,
    path::PathBuf,
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
        "[missing](missing.md \"letopis:type=supports;path=missing.md\")",
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
    let status = get_status(&repo);
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
        get_status(&repo)["result"]["problems"],
        status["result"]["problems"]
    );

    // When
    let refreshed = repo.invoke(&["check"], b"{}");
    // Then
    assert!(refreshed.status.success(), "{refreshed:?}");
    let refreshed: Value = serde_json::from_slice(&refreshed.stdout).unwrap();
    assert_eq!(refreshed["result"]["errors"], json!([]));
    assert_eq!(get_status(&repo)["result"]["problems"], json!([]));
}

#[test]
fn status_reports_rename_delete_and_restore_after_refresh() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "owner.md",
        "[target](target.md \"letopis:type=supports;path=target.md\")",
    );
    repo.write_knowledge("target.md", "Target");
    // When
    let initial = repo.invoke(&["check"], b"{}");
    // Then
    assert!(initial.status.success(), "{initial:?}");
    assert_eq!(get_status(&repo)["result"]["problems"], json!([]));

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
        get_status(&repo)["result"]["problems"][0]["details"]["target"],
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
    assert_eq!(get_status(&repo)["result"]["problems"], json!([]));

    // Given
    fs::remove_file(repo.get_root().join("target.md")).unwrap();
    // When
    let deleted = repo.invoke(&["check"], b"{}");
    // Then
    assert!(deleted.status.success(), "{deleted:?}");
    assert_eq!(
        get_status(&repo)["result"]["problems"][0]["details"]["target"],
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
    let status = get_status(&repo);

    // Then
    assert!(start.elapsed().as_secs() < 2);
    assert_eq!(
        status["result"]["locks"],
        json!([{"name":"index","pid":pid}])
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    flock(&owner, FlockOperation::Unlock).unwrap();
}

#[test]
fn drain_start_failure_warns_and_keeps_fts() {
    // Given: pending embedding work, a valid config, and an unsafe embed lock.
    let repo = ReadingRepo::new();
    let (_outside, canary, listener) = prepare_failed_drain(&repo);

    // When: a real product search runs and the drain cannot start.
    let output = repo.invoke(
        &["search"],
        json!({"query":"needle","limit":10}).to_string().as_bytes(),
    );

    // Then: FTS succeeds with one safe warning and no provider call.
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"].as_array().unwrap().len(), 1);
    assert_eq!(value["result"][0]["element_id"], "known.md");
    assert_eq!(
        value["warnings"],
        json!([{
            "code": "internal_error",
            "message": "cannot start background embedding drain",
            "details": {}
        }])
    );
    assert!(matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    assert_eq!(fs::read(&canary).unwrap(), b"DRAIN_CANARY");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("DRAIN_CANARY"));
}

#[test]
fn drain_start_failure_is_absent_from_status() {
    // Given: a search that could not start the drain.
    let repo = ReadingRepo::new();
    let (_outside, canary, _listener) = prepare_failed_drain(&repo);
    let output = repo.invoke(
        &["search"],
        json!({"query":"needle","limit":10}).to_string().as_bytes(),
    );
    assert!(output.status.success(), "{output:?}");

    // When: the lock fault is removed and status is read afresh.
    fs::remove_file(repo.get_root().join(".letopis/embed.lock")).unwrap();
    let status = get_status(&repo);

    // Then: no launch warning is persisted and the canary is untouched.
    assert_eq!(status["result"]["problems"], json!([]));
    assert!(
        !status
            .to_string()
            .contains("cannot start background embedding drain")
    );
    assert_eq!(fs::read(&canary).unwrap(), b"DRAIN_CANARY");
}

#[test]
fn disabled_configuration_reports_zero_and_hides_then_reveals_stored_failures() {
    // Given: current knowledge with an exhausted hash and a stored query failure.
    let repo = ReadingRepo::new();
    repo.write_knowledge("known.md", "needle");
    let checked = repo.invoke(&["check"], b"{}");
    assert!(checked.status.success(), "{checked:?}");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    let config = repo.write_config("ollama", &url, "fixture");
    let mut storage = repo.open_storage();
    storage.reset_vectors(None, &config.get_identity()).unwrap();
    let node = storage
        .begin_read_tx()
        .unwrap()
        .get_nodes()
        .unwrap()
        .into_iter()
        .find_map(|node| match node.metadata {
            NodeMetadata::Knowledge(node) => Some(node),
            NodeMetadata::Source(_) => None,
        })
        .unwrap();
    let VectorOutcome::Current(mut unit) = storage
        .begin_vector_write_tx(&config.get_identity())
        .unwrap()
    else {
        panic!("expected current identity")
    };
    assert!(unit.reserve_attempt(&node.search_hash, 1).unwrap());
    unit.record_failure(&node.search_hash, "safe failure")
        .unwrap();
    unit.set_query_error(7, "safe query failure").unwrap();
    unit.commit().unwrap();
    drop(storage);
    repo.write(
        ".letopis/config.json",
        format!(
            r#"{{"embedding_model":{{"enabled":false,"provider":"ollama","model":"fixture","parameters":{{"base_url":"{url}"}},"max_retries":0}}}}"#
        )
        .as_bytes(),
    );

    // When: status and a drain observe the disabled document.
    let status = get_status(&repo);
    let drained = repo.invoke(&["embed", "--drain"], b"{}");

    // Then: execution counters and failures are hidden and no work starts.
    assert_eq!(status["result"]["embedding_backlog"], 0);
    assert_eq!(status["result"]["embedding_failed"], 0);
    assert!(!status.to_string().contains("query_embedding_failed"));
    assert!(!status.to_string().contains("embedding_attempts_exhausted"));
    assert!(drained.status.success(), "{drained:?}");
    assert_eq!(drained.stdout, b"{\"ok\":true,\"result\":{}}\n");
    assert!(matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));

    // Given: the same settings are re-enabled without changing their identity.
    repo.write(
        ".letopis/config.json",
        format!(
            r#"{{"embedding_model":{{"enabled":true,"provider":"ollama","model":"fixture","parameters":{{"base_url":"{url}"}},"max_retries":0}}}}"#
        )
        .as_bytes(),
    );

    // When / Then: stored execution failures become visible under enabled semantics.
    let enabled = get_status(&repo);
    assert_eq!(enabled["result"]["embedding_failed"], 1);
    assert!(enabled.to_string().contains("embedding_attempts_exhausted"));
    assert!(enabled.to_string().contains("query_embedding_failed"));
}

fn prepare_failed_drain(repo: &ReadingRepo) -> (tempfile::TempDir, PathBuf, TcpListener) {
    repo.write_knowledge("known.md", "needle");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    repo.write_config("ollama", &url, "fixture");
    let outside = tempfile::tempdir().unwrap();
    let canary = outside.path().join("embed-canary");
    fs::write(&canary, b"DRAIN_CANARY").unwrap();
    let lock = repo.get_root().join(".letopis/embed.lock");
    symlink(&canary, &lock).unwrap();
    (outside, canary, listener)
}

fn seed_diagnostic_files(repo: &ReadingRepo) {
    repo.write("bad.md", b"<!-- letopis: broken PRIVATE_BODY -->\n");
    repo.write_knowledge("owner.md", "[file](missing.md \"letopis:type=supports;path=missing.md\") [anchor](target.md#missing \"letopis:type=supports;path=target.md#missing\")");
    repo.write(
        "target.md",
        b"# Other {#other}\n<!-- letopis: {\"created_at\":1} -->\nTarget\n",
    );
    repo.write_knowledge(
        "loop.md",
        "[self](loop.md \"letopis:type=includes;path=loop.md\")",
    );
    repo.write_knowledge("a.md", "[b](b.md \"letopis:type=includes;path=b.md\")");
    repo.write_knowledge("b.md", "[a](a.md \"letopis:type=includes;path=a.md\")");
    repo.write_knowledge(
        "sources.md",
        "[source](code.rs \"letopis:type=external;path=code.rs\")",
    );
    repo.write("code.rs", b"source");
}

fn get_status(repo: &ReadingRepo) -> Value {
    let output = repo.invoke(&["status"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    serde_json::from_slice(&output.stdout).unwrap()
}
