//! Discovery file failures and non-refreshing command isolation.
mod support;

use letopis::service::{CacheDatabase, FileRow, StorageRead};
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    process::Command,
};
use support::reading::ReadingRepo;

#[derive(Debug, PartialEq, Eq)]
struct PublishedState {
    revision: u64,
    files: Vec<FileRow>,
}

#[test]
fn ignore_file_observation_failures_preserve_publication() {
    // Given: each unsafe file kind starts from an independently published owner.
    for kind in ["symlink", "directory", "fifo", "permission"] {
        let repo = prepare_repository();
        let before = get_state(&repo);
        let outside = tempfile::tempdir().unwrap();
        let sentinel = outside.path().join("PRIVATE_SENTINEL");
        fs::write(&sentinel, b"external bytes").unwrap();
        let ignore = repo.get_root().join(".letopis/ignore");
        match kind {
            "symlink" => symlink(&sentinel, &ignore).unwrap(),
            "directory" => fs::create_dir(&ignore).unwrap(),
            "fifo" => assert!(
                Command::new("mkfifo")
                    .arg(&ignore)
                    .status()
                    .unwrap()
                    .success()
            ),
            _ => {
                fs::write(&ignore, b"keep.md\n").unwrap();
                fs::set_permissions(&ignore, fs::Permissions::from_mode(0o0)).unwrap();
                if fs::read(&ignore).is_ok() {
                    fs::set_permissions(&ignore, fs::Permissions::from_mode(0o600)).unwrap();
                    eprintln!(
                        "permission check unavailable: fixture process can read mode-000 files"
                    );
                    continue;
                }
            }
        }
        // When: the actual indexing process observes policy under its owned watchdog.
        let output = repo.invoke(&["index"], b"{}");
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        // Then: I/O failure is fatal without changing canonical external bytes or the index.
        assert_eq!(output.status.code(), Some(1), "{kind}: {output:?}");
        assert_eq!(response["error"]["code"], "internal_error");
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_SENTINEL"));
        assert_eq!(fs::read(sentinel).unwrap(), b"external bytes");
        assert_eq!(get_state(&repo), before);
        if kind == "permission" {
            fs::set_permissions(ignore, fs::Permissions::from_mode(0o600)).unwrap();
        }
    }
}

#[test]
fn unsafe_service_parent_never_follows_an_external_directory() {
    // Given: the private directory is replaced by an external symlink after publication.
    let repo = prepare_repository();
    let before = get_state(&repo);
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("ignore"), b"keep.md\n").unwrap();
    let service = repo.get_root().join(".letopis");
    let saved = repo.get_root().join("saved-service");
    fs::rename(&service, &saved).unwrap();
    symlink(outside.path(), &service).unwrap();
    // When: indexing tries to open its confined service resources.
    let output = repo.invoke(&["index"], b"{}");
    // Then: it fails, does not create external databases, and preserves the old publication.
    assert!(!output.status.success(), "{output:?}");
    assert!(!outside.path().join("cache.db").exists());
    assert!(!outside.path().join("vectors.db").exists());
    fs::remove_file(service).unwrap();
    fs::rename(saved, repo.get_root().join(".letopis")).unwrap();
    assert_eq!(get_state(&repo), before);
}

#[test]
fn malformed_discovery_is_a_safe_request_error_for_every_refresh_command() {
    // Given: a valid published corpus and malformed author configuration.
    let cases: &[(&[&str], &[u8])] = &[
        (&["check"], b"{}"),
        (&["search"], br#"{"query":"","limit":1}"#),
        (
            &["traverse"],
            br#"{"element_ids":["keep.md"],"mode":"both","direction":"both","depth":0,"limit":1}"#,
        ),
        (&["inspect"], br#"{"element_ids":["keep.md"]}"#),
        (&["index"], b"{}"),
        (&["index", "--verify"], b"{}"),
        (&["index", "--full"], b"{}"),
        (&["index", "--full", "--verify"], b"{}"),
    ];
    for (args, body) in cases {
        let repo = prepare_repository();
        let before = get_state(&repo);
        repo.write(".letopis/config.json", b"{PRIVATE_BROKEN_CONFIG}");
        // When: a valid command reaches discovery validation.
        let output = repo.invoke(args, body);
        let response: Value = serde_json::from_slice(&output.stdout).unwrap();
        // Then: error type and secrecy are consistent and the previous publication survives.
        assert_eq!(
            response["error"]["code"], "invalid_request",
            "{args:?}: {response}"
        );
        assert_eq!(output.status.code(), Some(1));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("PRIVATE_BROKEN_CONFIG"));
        assert_eq!(get_state(&repo), before);
    }
}

#[test]
fn all_help_forms_ignore_invalid_config_and_fifo_patterns() {
    // Given: malformed config and a FIFO that would block an unsafe policy reader.
    let repo = prepare_repository();
    repo.write(".letopis/config.json", b"{invalid}");
    assert!(
        Command::new("mkfifo")
            .arg(repo.get_root().join(".letopis/ignore"))
            .status()
            .unwrap()
            .success()
    );
    // When / Then: help and version never read policy inputs or start a provider.
    for command in [
        "init", "config", "status", "check", "search", "inspect", "traverse", "index",
    ] {
        let output = repo.invoke(&[command, "--help"], b"{}");
        assert!(output.status.success(), "{command}: {output:?}");
        assert!(!output.stdout.is_empty());
    }
    assert!(repo.invoke(&["--help"], b"{}").status.success());
    assert!(repo.invoke(&["--version"], b"{}").status.success());
}

fn prepare_repository() -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write_knowledge("keep.md", "Needle");
    let output = repo.invoke(&["index"], b"{}");
    assert!(output.status.success(), "{output:?}");
    repo
}

fn get_state(repo: &ReadingRepo) -> PublishedState {
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(
            || -> Result<PublishedState, letopis::service::StorageError> {
                Ok(PublishedState {
                    revision: storage.cache.project_get_by_id()?,
                    files: storage.cache.files_get()?,
                })
            },
        )
        .unwrap()
}
