mod support;

use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{ErrorKind, Seek, Write},
    net::TcpListener,
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    process::{Command, Output, Stdio},
};
use support::{git::isolate_command, reading::ReadingRepo};

const LITERAL_CONFIG: &[u8] = br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9","custom":{"enabled":true},"dimensions":3},"max_retries":2}"#;
const LITERAL_OUTPUT: &[u8] = b"{\"ok\":true,\"result\":{\"provider\":\"ollama\",\"model\":\"fixture\",\"parameters\":{\"base_url\":\"http://127.0.0.1:9\",\"custom\":{\"enabled\":true},\"dimensions\":3},\"max_retries\":2}}\n";

#[test]
fn config_without_file_returns_null_without_service_state() {
    // Given
    let repo = ReadingRepo::new();
    let service_dir = repo.get_root().join(".letopis");
    assert!(!service_dir.exists());

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, b"{\"ok\":true,\"result\":null}\n");
    assert!(!service_dir.exists());
}

#[test]
fn config_preserves_literal_values_without_service_side_effects() {
    // Given
    let repo = ReadingRepo::new();
    repo.write(".letopis/config.json", LITERAL_CONFIG);
    let before = capture_service_files(&repo);

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, LITERAL_OUTPUT);
    assert_eq!(capture_service_files(&repo), before);
}

#[test]
fn config_never_connects_to_the_configured_provider() {
    // Given
    let repo = ReadingRepo::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let url = format!("http://{}/api", listener.local_addr().unwrap());
    let config = serde_json::json!({
        "provider": "ollama",
        "model": "fixture",
        "parameters": {"base_url": url},
        "max_retries": 0
    })
    .to_string();
    repo.write(".letopis/config.json", config.as_bytes());

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"]["parameters"]["base_url"],
        url
    );
    assert!(matches!(listener.accept(), Err(error) if error.kind() == ErrorKind::WouldBlock));
    assert_eq!(
        service_entry_names(&repo),
        ["config.json"],
        "config must not start a drain or other service state"
    );
}

#[test]
fn config_rejects_malformed_config_with_safe_diagnostics() {
    // Given
    let repo = ReadingRepo::new();
    let cases: [&[u8]; 6] = [
        b"CONFIG_CANARY not json",
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0} null"#,
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0,"max_retries":0}"#,
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://one/api","base_url":"http://two/api"},"max_retries":0}"#,
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","api_key":"CONFIG_CANARY"},"max_retries":0}"#,
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","authorization":"CONFIG_CANARY"},"max_retries":0}"#,
    ];
    for raw in cases {
        // Given
        repo.write(".letopis/config.json", raw);

        // When
        let output = repo.invoke(&["config"], b"{}");

        // Then
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(
            output.stdout,
            b"{\"ok\":false,\"error\":{\"code\":\"embedding_failed\",\"message\":\"invalid embedding configuration\",\"details\":{}}}\n"
        );
        assert!(!String::from_utf8_lossy(&output.stdout).contains("CONFIG_CANARY"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains("CONFIG_CANARY"));
    }
}

#[test]
fn config_rejects_symlink_and_unreadable_config_with_safe_diagnostics() {
    // Given: a symlinked configuration outside the service directory.
    let repo = ReadingRepo::new();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("config-target.json");
    fs::write(&target, b"CONFIG_CANARY").unwrap();
    fs::create_dir_all(repo.get_root().join(".letopis")).unwrap();
    symlink(&target, repo.get_root().join(".letopis/config.json")).unwrap();

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_internal_configuration_error(&output);
    assert_eq!(fs::read(&target).unwrap(), b"CONFIG_CANARY");

    // Given: an unreadable regular configuration file.
    let repo = ReadingRepo::new();
    let path = repo.get_root().join(".letopis/config.json");
    repo.write(".letopis/config.json", b"CONFIG_CANARY");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    // SAFETY: geteuid has no preconditions and only reads this process's effective UID.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_internal_configuration_error(&output);
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn config_preserves_existing_databases_and_metadata() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("known.md", "Existing state");
    let checked = repo.invoke(&["check"], b"{}");
    assert_eq!(checked.status.code(), Some(0), "{checked:?}");
    repo.write(
        ".letopis/config.json",
        br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9"},"max_retries":1}"#,
    );
    let before = capture_service_files(&repo);
    assert!(before.contains_key("cache.db"));
    assert!(before.contains_key("vectors.db"));

    // When
    let output = repo.invoke(&["config"], b"{}");

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["result"]["max_retries"],
        1
    );
    assert_eq!(capture_service_files(&repo), before);
}

#[test]
fn config_reads_valid_openai_configuration_without_credentials() {
    // Given
    let repo = ReadingRepo::new();
    repo.write(
        ".letopis/config.json",
        br#"{"provider":"openai","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9"},"max_retries":0}"#,
    );

    // When
    let output = invoke_without_openai_key(&repo);

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"{\"ok\":true,\"result\":{\"provider\":\"openai\",\"model\":\"fixture\",\"parameters\":{\"base_url\":\"http://127.0.0.1:9\"},\"max_retries\":0}}\n"
    );
}

#[test]
fn config_rejects_nonempty_request_before_repository_io() {
    // Given
    let repo = ReadingRepo::new();
    let service_dir = repo.get_root().join(".letopis");
    for (args, input) in [
        (&["config", "extra"][..], b"{}".as_slice()),
        (&["config"][..], br#"{"provider":"ollama"}"#.as_slice()),
        (&["config"][..], b"{} {}".as_slice()),
        (&["config"][..], b"null".as_slice()),
    ] {
        // When
        let output = repo.invoke(args, input);

        // Then
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["ok"], false);
        assert_eq!(value["error"]["code"], "invalid_request");
        assert_eq!(value["error"]["details"], json!({}));
        assert_eq!(
            output.stdout.iter().filter(|byte| **byte == b'\n').count(),
            1
        );
        assert!(!service_dir.exists());
    }
}

#[derive(Debug, PartialEq, Eq)]
struct FileState {
    bytes: Vec<u8>,
    len: u64,
    modified: (i64, i64),
    inode: u64,
    mode: u32,
}

fn capture_service_files(repo: &ReadingRepo) -> BTreeMap<String, FileState> {
    service_entries(repo)
        .into_iter()
        .map(|(name, path)| {
            let metadata = fs::metadata(&path).unwrap();
            (
                name,
                FileState {
                    bytes: fs::read(&path).unwrap(),
                    len: metadata.len(),
                    modified: (metadata.mtime(), metadata.mtime_nsec()),
                    inode: metadata.ino(),
                    mode: metadata.mode(),
                },
            )
        })
        .collect()
}

fn service_entry_names(repo: &ReadingRepo) -> Vec<String> {
    service_entries(repo).into_keys().collect()
}

fn service_entries(repo: &ReadingRepo) -> BTreeMap<String, std::path::PathBuf> {
    let directory = repo.get_root().join(".letopis");
    fs::read_dir(&directory)
        .unwrap()
        .map(|entry| {
            let entry = entry.unwrap();
            (
                entry.file_name().to_string_lossy().into_owned(),
                entry.path(),
            )
        })
        .collect()
}

fn assert_internal_configuration_error(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"{\"ok\":false,\"error\":{\"code\":\"internal_error\",\"message\":\"cannot read embedding configuration\",\"details\":{}}}\n"
    );
    assert!(!String::from_utf8_lossy(&output.stdout).contains("CONFIG_CANARY"));
}

fn invoke_without_openai_key(repo: &ReadingRepo) -> Output {
    let mut stdin = tempfile::tempfile().unwrap();
    stdin.write_all(b"{}").unwrap();
    stdin.rewind().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    command
        .current_dir(repo.get_root())
        .args(["config"])
        .env_remove("OPENAI_API_KEY")
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    command.output().unwrap()
}
