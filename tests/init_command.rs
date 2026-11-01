mod support;

use letopis::service::Storage;
use serde_json::{Value, json};
use std::{
    fs,
    io::{Seek, Write},
    os::unix::fs::{MetadataExt, PermissionsExt, symlink},
    path::Path,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use support::{
    git::{OwnedChild, isolate_command},
    reading::ReadingRepo,
};

const GENERATED: &[u8] = b"{\n  \"embedding_model\": {\n    \"enabled\": false,\n    \"provider\": null,\n    \"model\": null,\n    \"parameters\": {},\n    \"max_retries\": 0\n  }\n}\n";
const SUCCESS: &[u8] = b"{\"ok\":true,\"result\":{}}\n";
const DISABLED: &[u8] = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://127.0.0.1:9/api"},"max_retries":2}}"#;

#[test]
fn init_creates_exact_private_configuration_and_usable_schemas() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("known.md", "searchable knowledge");

    // When
    let output = repo.invoke(&["init"], b"{}");

    // Then: exact config, no corpus indexing, and the next check uses the schemas.
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout, SUCCESS);
    let service = repo.get_root().join(".letopis");
    assert_eq!(fs::read(service.join("config.json")).unwrap(), GENERATED);
    assert_eq!(
        fs::metadata(service.join("config.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(&service).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(service.join("cache.db").is_file());
    assert!(service.join("vectors.db").is_file());
    let status = repo.invoke(&["status"], b"{}");
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["result"]["revision"], Value::Null);
    let checked = repo.invoke(&["check"], b"{}");
    assert!(checked.status.success(), "{checked:?}");
    let known = repo.invoke(&["search"], br#"{"query":"searchable","limit":5}"#);
    assert!(known.status.success(), "{known:?}");
}

#[test]
fn init_from_subdirectory_targets_the_git_root_only() {
    // Given
    let repo = ReadingRepo::new();

    // When
    let output = repo.invoke_in(repo.get_subdir(), &["init"], b"");

    // Then
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/config.json")).unwrap(),
        GENERATED
    );
    assert!(!repo.get_subdir().join(".letopis").exists());
}

#[test]
fn init_accepts_empty_repository_and_rejects_non_repository_or_bare() {
    // Given: an initialized work tree without commits.
    let empty = tempfile::tempdir().unwrap();
    run_git(empty.path(), &["init", "-q"]);
    // When / Then
    let output = invoke_at(empty.path(), &["init"], b"{}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(empty.path().join(".letopis/cache.db").is_file());

    // Given: a directory outside Git and a bare repository.
    for (root, bare) in [
        (tempfile::tempdir().unwrap(), false),
        (tempfile::tempdir().unwrap(), true),
    ] {
        if bare {
            run_git(root.path(), &["init", "-q", "--bare"]);
        }
        // When
        let output = invoke_at(root.path(), &["init"], b"{}");
        // Then
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            output.stdout,
            b"{\"ok\":false,\"error\":{\"code\":\"invalid_request\",\"message\":\"Git repository required\",\"details\":{}}}\n"
        );
        assert!(!root.path().join(".letopis").exists());
    }
}

#[test]
fn init_protects_a_preexisting_permissive_service_directory() {
    // Given: a precreated permissive service directory, with and without valid config.
    for config in [None, Some(DISABLED)] {
        let repo = ReadingRepo::new();
        let service = repo.get_root().join(".letopis");
        fs::create_dir(&service).unwrap();
        fs::set_permissions(&service, fs::Permissions::from_mode(0o755)).unwrap();
        if let Some(config) = config {
            let path = service.join("config.json");
            fs::write(&path, config).unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }

        // When
        let output = repo.invoke(&["init"], b"{}");

        // Then: initialization completes only after making the directory private.
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(
            fs::metadata(&service).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert!(service.join("cache.db").is_file());
        assert!(service.join("vectors.db").is_file());
    }
}

#[test]
fn init_preserves_an_edited_valid_configuration_and_logical_database_state() {
    // Given: a published cache with a valid configured-disabled file.
    let repo = ReadingRepo::new();
    repo.write_knowledge("known.md", "searchable knowledge");
    let checked = repo.invoke(&["check"], b"{}");
    assert!(checked.status.success(), "{checked:?}");
    repo.write(".letopis/config.json", DISABLED);
    let before_config = fs::read(repo.get_root().join(".letopis/config.json")).unwrap();
    let before_databases = logical_databases(repo.get_root());
    let before_inode = fs::metadata(repo.get_root().join(".letopis/config.json"))
        .unwrap()
        .ino();

    // When
    let first = repo.invoke(&["init"], b"{}");
    let second = repo.invoke(&["init"], b"{}");

    // Then
    for output in [first, second] {
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(output.stdout, SUCCESS);
    }
    let path = repo.get_root().join(".letopis/config.json");
    assert_eq!(fs::read(&path).unwrap(), before_config);
    assert_eq!(fs::metadata(&path).unwrap().ino(), before_inode);
    assert_eq!(logical_databases(repo.get_root()), before_databases);
}

#[test]
fn init_rejects_invalid_or_unsafe_config_before_database_creation() {
    // Given: legacy flat, malformed and symlinked configuration files.
    for (case, bytes) in [
        (
            "legacy",
            br#"{"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api"},"max_retries":0}"#.as_slice(),
        ),
        ("malformed", b"PRIVATE not json".as_slice()),
    ] {
        let repo = ReadingRepo::new();
        repo.write(".letopis/config.json", bytes);
        // When
        let output = repo.invoke(&["init"], b"{}");
        // Then
        assert_eq!(output.status.code(), Some(1), "{case}: {output:?}");
        assert!(!repo.get_root().join(".letopis/cache.db").exists(), "{case}");
        assert!(!repo.get_root().join(".letopis/vectors.db").exists(), "{case}");
        assert_eq!(fs::read(repo.get_root().join(".letopis/config.json")).unwrap(), bytes);
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE"));
        if case == "legacy" {
            assert_eq!(
                output.stdout,
                b"{\"ok\":false,\"error\":{\"code\":\"embedding_failed\",\"message\":\"embedding configuration requires migration\",\"details\":{}}}\n"
            );
        }
    }

    // Given: a config symlink pointing outside the repository.
    let repo = ReadingRepo::new();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("config.json");
    fs::write(&target, b"PRIVATE").unwrap();
    fs::create_dir(repo.get_root().join(".letopis")).unwrap();
    symlink(&target, repo.get_root().join(".letopis/config.json")).unwrap();
    // When / Then
    let output = repo.invoke(&["init"], b"{}");
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read(&target).unwrap(), b"PRIVATE");
    assert!(!repo.get_root().join(".letopis/cache.db").exists());
}

#[test]
fn init_preserves_incompatible_storage_and_retry_completes_after_repair() {
    // Given: an incompatible cache database and no configuration.
    let repo = ReadingRepo::new();
    let cache = repo.get_root().join(".letopis/cache.db");
    fs::create_dir_all(cache.parent().unwrap()).unwrap();
    fs::write(&cache, b"PRIVATE not sqlite").unwrap();

    // When
    let failed = repo.invoke(&["init"], b"{}");

    // Then
    assert_eq!(failed.status.code(), Some(1), "{failed:?}");
    assert_eq!(fs::read(&cache).unwrap(), b"PRIVATE not sqlite");
    assert!(!repo.get_root().join(".letopis/config.json").exists());
    assert!(!repo.get_root().join(".letopis/vectors.db").exists());

    // When: the operator removes the incompatible cache and retries.
    fs::remove_file(&cache).unwrap();
    let retried = repo.invoke(&["init"], b"{}");

    // Then: the retry completes the previously missing work.
    assert_eq!(retried.status.code(), Some(0), "{retried:?}");
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/config.json")).unwrap(),
        GENERATED
    );
    assert!(cache.is_file());
    assert!(repo.get_root().join(".letopis/vectors.db").is_file());
}

#[test]
fn concurrent_init_calls_leave_one_complete_valid_state() {
    // Given: four independent processes starting at the same repository root.
    let repo = ReadingRepo::new();
    let mut children = Vec::new();
    for _ in 0..4 {
        let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
        isolate_command(&mut command);
        command
            .args(["init"])
            .current_dir(repo.get_root())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        children.push(OwnedChild::spawn(&mut command));
    }

    // When / Then
    for child in children {
        let output = child.wait(Instant::now() + Duration::from_secs(30));
        assert_eq!(output.status.code(), Some(0), "{output:?}");
        assert_eq!(output.stdout, SUCCESS);
    }
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/config.json")).unwrap(),
        GENERATED
    );
    let observed = repo.invoke(&["config"], b"{}");
    let value: Value = serde_json::from_slice(&observed.stdout).unwrap();
    assert_eq!(
        value,
        json!({"ok":true,"result":{"embedding_model":{
            "enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0
        }}})
    );
    let status = repo.invoke(&["status"], b"{}");
    assert!(status.status.success(), "{status:?}");
}

#[test]
fn init_parser_and_help_follow_existing_request_rules() {
    // Given: help must work outside Git without reading stdin or creating state.
    let root = tempfile::tempdir().unwrap();
    let help = invoke_at(root.path(), &["init", "--help"], b"ignored");
    let invalid = invoke_at(root.path(), &["init", "extra"], b"{}");
    let unknown = invoke_at(root.path(), &["init"], br#"{"extra":1}"#);

    // Then
    assert_eq!(help.status.code(), Some(0), "{help:?}");
    assert!(
        String::from_utf8(help.stdout)
            .unwrap()
            .contains("letopis init")
    );
    assert!(!root.path().join(".letopis").exists());
    for output in [invalid, unknown] {
        assert_eq!(output.status.code(), Some(1), "{output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["error"]["code"],
            "invalid_request"
        );
        assert!(!root.path().join(".letopis").exists());
    }
}

fn invoke_at(directory: &Path, args: &[&str], input: &[u8]) -> Output {
    let mut stdin = tempfile::tempfile().unwrap();
    stdin.write_all(input).unwrap();
    stdin.rewind().unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_letopis"));
    isolate_command(&mut command);
    command
        .current_dir(directory)
        .args(args)
        .stdin(Stdio::from(stdin))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    OwnedChild::spawn(&mut command).wait(Instant::now() + Duration::from_secs(30))
}

fn run_git(directory: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let status = command.args(args).current_dir(directory).status().unwrap();
    assert!(status.success(), "git {args:?}");
}

fn logical_databases(root: &Path) -> Vec<String> {
    let mut storage = letopis::open_storage(
        &root.join(".letopis/cache.db"),
        &root.join(".letopis/vectors.db"),
    )
    .unwrap();
    let mut read = storage.begin_read_tx().unwrap();
    let mut snapshot = vec![format!("revision:{}", read.get_revision())];
    snapshot.push(format!("identity:{:?}", read.get_config_identity()));
    let mut nodes = read
        .get_nodes()
        .unwrap()
        .into_iter()
        .map(|node| format!("{:?}", node.metadata))
        .collect::<Vec<_>>();
    nodes.sort();
    snapshot.extend(nodes);
    snapshot
}
