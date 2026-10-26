mod support;

use letopis::service::{Storage, StorageRead};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};
use support::{
    git::isolate_command,
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

const CACHE_DATABASE: &str = "cache.db";
const VECTOR_DATABASE: &str = "vectors.db";
const CORRUPTION_CANARY: &str = "CORRUPTION_CANARY";
const CORRUPT_DATABASE_BYTES: &[u8] = b"not a sqlite database CORRUPTION_CANARY";

#[derive(Debug, PartialEq, Eq)]
struct UserFixtureState {
    head: Vec<u8>,
    index: Vec<u8>,
    git_config: Vec<u8>,
    user_refs: Vec<u8>,
    canonical_files: Vec<(String, Option<Vec<u8>>)>,
}

#[test]
fn corrupt_cache_and_vectors_fail_in_each_new_command_process() {
    for target in [CACHE_DATABASE, VECTOR_DATABASE] {
        // Arrange
        let repo = ReadingRepo::new();
        repo.write_knowledge(
            "known.md",
            "Known searchable knowledge with CORRUPTION_CANARY payload",
        );
        assert_successful_check(&repo);
        assert_revision(&repo, 1);
        let database_snapshot = capture_databases(&repo);
        let user_snapshot = capture_user_state(&repo, &["known.md"]);
        let target_path = get_database_path(&repo, target);
        replace_database(&target_path, CORRUPT_DATABASE_BYTES);

        // Act / Assert: every invocation starts a separately supervised CLI process.
        for command in ["check", "check", "status", "status"] {
            let output = repo.invoke(&[command], b"{}");
            assert_internal_failure(&output);
            assert_eq!(fs::read(&target_path).unwrap(), CORRUPT_DATABASE_BYTES);
            assert_undamaged_database(&repo, target, &database_snapshot);
            assert_eq!(capture_user_state(&repo, &["known.md"]), user_snapshot);
        }

        // Arrange / Act / Assert: exact test-fixture restoration enables health again.
        restore_database(&target_path, get_database_bytes(target, &database_snapshot));
        assert_eq!(capture_databases(&repo), database_snapshot);
        assert_successful_check(&repo);
        assert_successful_status(&repo, 1);
        assert_eq!(capture_user_state(&repo, &["known.md"]), user_snapshot);
    }
}

#[test]
fn corrupt_authored_tree_has_persistent_current_command_failure() {
    // Arrange
    let repo = ReadingRepo::new();
    let source = b"authored source payload CORRUPTION_CANARY\n";
    let basis = create_basis(repo.get_root(), &[("code.rs", source)]);
    repo.write("code.rs", source);
    repo.write(
        "note.md",
        build_note(Some(&basis), &["code.rs"], "Known knowledge").as_bytes(),
    );
    assert_successful_check(&repo);
    assert_revision(&repo, 1);
    let database_snapshot = capture_databases(&repo);
    let user_snapshot = capture_user_state(&repo, &["code.rs", "note.md"]);
    let object_path = get_object_path(repo.get_root(), basis.as_str());
    let original_object = damage_object(&object_path);
    let damaged_object = fs::read(&object_path).unwrap();

    // Act / Assert: corruption remains a current hard failure in every new process.
    for command in ["check", "status"] {
        for _ in 0..2 {
            let output = repo.invoke(&[command], b"{}");
            assert_internal_failure(&output);
            assert_eq!(fs::read(&object_path).unwrap(), damaged_object);
            assert_eq!(capture_databases(&repo), database_snapshot);
            assert_eq!(
                capture_user_state(&repo, &["code.rs", "note.md"]),
                user_snapshot
            );
            assert_revision(&repo, 1);
        }
    }

    // Arrange / Act / Assert: the same commands recover after exact fixture restore.
    fs::write(&object_path, original_object).unwrap();
    assert_successful_check(&repo);
    assert_successful_status(&repo, 1);
    assert_eq!(capture_databases(&repo), database_snapshot);
    assert_eq!(
        capture_user_state(&repo, &["code.rs", "note.md"]),
        user_snapshot
    );
}

fn assert_internal_failure(output: &Output) {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], false);
    assert_eq!(value["error"]["code"], "internal_error");
    assert!(value.get("result").is_none());
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let visible = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!visible.contains(CORRUPTION_CANARY));
    assert!(!visible.contains("letopis-git-"));
    assert!(!visible.contains("\"revision\":null"));
}

fn assert_successful_check(repo: &ReadingRepo) {
    let output = repo.invoke(&["check"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value, json!({"ok":true,"result":{"errors":[]}}));
}

fn assert_successful_status(repo: &ReadingRepo, revision: u64) {
    let output = repo.invoke(&["status"], b"{}");
    assert!(output.status.success(), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ok"], true);
    assert_eq!(value["result"]["revision"], revision);
}

fn assert_revision(repo: &ReadingRepo, expected: u64) {
    let mut storage = repo.open_storage();
    assert_eq!(storage.begin_read().unwrap().get_revision(), expected);
}

fn capture_databases(repo: &ReadingRepo) -> (Vec<u8>, Vec<u8>) {
    (
        fs::read(get_database_path(repo, CACHE_DATABASE)).unwrap(),
        fs::read(get_database_path(repo, VECTOR_DATABASE)).unwrap(),
    )
}

fn assert_undamaged_database(repo: &ReadingRepo, damaged: &str, snapshot: &(Vec<u8>, Vec<u8>)) {
    let healthy = if damaged == CACHE_DATABASE {
        VECTOR_DATABASE
    } else {
        CACHE_DATABASE
    };
    assert_eq!(
        fs::read(get_database_path(repo, healthy)).unwrap(),
        get_database_bytes(healthy, snapshot)
    );
}

fn get_database_bytes<'a>(target: &str, snapshot: &'a (Vec<u8>, Vec<u8>)) -> &'a [u8] {
    if target == CACHE_DATABASE {
        &snapshot.0
    } else {
        &snapshot.1
    }
}

fn get_database_path(repo: &ReadingRepo, name: &str) -> PathBuf {
    repo.get_root().join(".letopis").join(name)
}

fn restore_database(path: &Path, bytes: &[u8]) {
    replace_database(path, bytes);
}

fn replace_database(path: &Path, bytes: &[u8]) {
    remove_database_sidecars(path);
    fs::write(path, bytes).unwrap();
}

fn remove_database_sidecars(path: &Path) {
    for suffix in ["-wal", "-shm", "-journal"] {
        let sidecar = PathBuf::from(format!("{}{suffix}", path.display()));
        if sidecar.exists() {
            fs::remove_file(sidecar).unwrap();
        }
    }
}

fn capture_user_state(repo: &ReadingRepo, canonical_paths: &[&str]) -> UserFixtureState {
    UserFixtureState {
        head: fs::read(repo.get_root().join(".git/HEAD")).unwrap(),
        index: fs::read(repo.get_root().join(".git/index")).unwrap(),
        git_config: fs::read(repo.get_root().join(".git/config")).unwrap(),
        user_refs: read_user_refs(repo.get_root()),
        canonical_files: canonical_paths
            .iter()
            .chain([&"live.md", &".letopis/config.json"])
            .map(|path| {
                (
                    (*path).to_owned(),
                    fs::read(repo.get_root().join(path)).ok(),
                )
            })
            .collect(),
    }
}

fn read_user_refs(root: &Path) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(root)
        .args(["for-each-ref", "--format=%(refname) %(objectname)"])
        .stdin(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output
        .stdout
        .split_inclusive(|byte| *byte == b'\n')
        .filter(|line| !line.starts_with(b"refs/letopis/snapshots/"))
        .flatten()
        .copied()
        .collect()
}

fn get_object_path(root: &Path, oid: &str) -> PathBuf {
    root.join(".git")
        .join("objects")
        .join(&oid[..2])
        .join(&oid[2..])
}

fn damage_object(path: &Path) -> Vec<u8> {
    let original = fs::read(path).unwrap();
    fs::remove_file(path).unwrap();
    fs::write(path, &original[..original.len() / 2]).unwrap();
    original
}
