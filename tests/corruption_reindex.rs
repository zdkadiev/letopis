mod support;

use letopis::{domain::RepoPath, service::Storage};
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde_json::{Value, json};
use std::{
    fs,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::{Command, Output},
};
use support::{
    git::isolate_command,
    provenance::{build_note, create_basis},
    reading::ReadingRepo,
};

const SOURCE: &[u8] = b"authored source CORRUPTION_CANARY\n";
const INVALID: &[u8] = b"<!-- letopis: CORRUPTION_CANARY -->";
const CANONICAL_PATHS: &[&str] = &[
    "a.md",
    "b.md",
    "source.txt",
    "live.md",
    ".letopis/config.json",
    ".git/HEAD",
    ".git/index",
    ".git/config",
];

#[derive(Clone, Copy, Debug)]
enum RequiredObject {
    Root,
    Child,
}

#[derive(Debug, PartialEq, Eq)]
struct CanonicalState {
    files: Vec<Option<Vec<u8>>>,
    refs: Vec<u8>,
}

#[test]
fn reindex_rejects_corrupt_required_basis_without_advancing() {
    for object in [RequiredObject::Root, RequiredObject::Child] {
        // Given: the authored basis is required by a real external source edge.
        let repo = ReadingRepo::new();
        let basis = create_basis(repo.get_root(), &[("source.txt", SOURCE)]);
        repo.write("source.txt", SOURCE);
        repo.write(
            "a.md",
            build_note(Some(&basis), &["source.txt"], "AlphaToken").as_bytes(),
        );
        assert_eq!(invoke_reindex(&repo)["result"]["revision"], 1);
        assert_basis_projection(&repo, basis.as_str());
        let projection = capture_projection(&repo);
        let canonical = capture_canonical_state(&repo);
        let oid = match object {
            RequiredObject::Root => basis.as_str().to_owned(),
            RequiredObject::Child => String::from_utf8(run_git(
                &repo,
                &["rev-parse", &format!("{}:source.txt", basis.as_str())],
            ))
            .unwrap()
            .trim()
            .to_owned(),
        };
        let path = get_object_path(repo.get_root(), &oid);
        let original = damage_owned_object(repo.get_root(), &path);
        let damaged = fs::read(&path).unwrap();

        // When / Then: each command starts a fresh process and must fail closed.
        for _ in 0..2 {
            assert_safe_failure(&repo.invoke(&["reindex"], b"{}"));
            assert_eq!(capture_projection(&repo), projection);
            assert_eq!(capture_canonical_state(&repo), canonical);
            assert_eq!(fs::read(&path).unwrap(), damaged);
        }

        // Given / When / Then: exact object restoration is only a fixture control.
        fs::write(&path, original).unwrap();
        assert_eq!(invoke_reindex(&repo)["result"]["revision"], 2);
        assert_basis_projection(&repo, basis.as_str());
        assert_eq!(capture_canonical_state(&repo), canonical);
    }
}

#[test]
fn reindex_after_cache_loss_reports_invalid_file_and_publishes_valid_peer() {
    // Given: discard the sole last-valid projection after editing both files.
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "AlphaToken");
    repo.write_knowledge("b.md", "BravoToken");
    assert_eq!(invoke_reindex(&repo)["result"]["revision"], 1);
    repo.write("a.md", INVALID);
    repo.write_knowledge("b.md", "CharlieToken");
    let invalid_canonical = capture_canonical_state(&repo);
    remove_owned_cache(repo.get_root());

    // When: reindex builds a new counter from current canonical bytes.
    assert_eq!(invoke_reindex(&repo)["result"]["revision"], 1);

    // Then: invalid A has no invented old knowledge, while valid B is published.
    assert_invalid_projection(&repo);
    for _ in 0..2 {
        assert_invalid_status(&repo);
        assert_eq!(capture_canonical_state(&repo), invalid_canonical);
    }

    // Given / When: fix the author-owned file, then publish in another process.
    repo.write_knowledge("a.md", "DeltaToken");
    let fixed_canonical = capture_canonical_state(&repo);
    assert_eq!(invoke_reindex(&repo)["result"]["revision"], 2);

    // Then: repair removes the current error and publishes both current texts.
    assert_repaired_projection(&repo);
    let status = get_response(&repo.invoke(&["status"], b"{}"), 0);
    assert_eq!(status["result"]["revision"], 2);
    assert_eq!(status["result"]["problems"], json!([]));
    assert_eq!(capture_canonical_state(&repo), fixed_canonical);
}

fn invoke_reindex(repo: &ReadingRepo) -> Value {
    let response = get_response(&repo.invoke(&["reindex"], b"{}"), 0);
    assert_eq!(response["ok"], true);
    response
}

fn assert_basis_projection(repo: &ReadingRepo, basis: &str) {
    let db = open_cache_read_only(repo);
    let mut statement = db.prepare("select id from nodes order by id").unwrap();
    let ids: Vec<String> = statement
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(ids, ["a.md", "source.txt"]);
    let declared: String = db
        .query_row(
            "select source_snapshot from nodes where id='a.md'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(declared, basis);
    let edge: (String, String, String) = db
        .query_row("select source_id, target_id, type from edges", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .unwrap();
    assert_eq!(
        edge,
        (
            "a.md".to_owned(),
            "source.txt".to_owned(),
            "external".to_owned()
        )
    );
    assert_eq!(get_matching_ids(&db, "AlphaToken"), ["a.md"]);
    assert_eq!(
        get_rows(&db, "select path, valid, error from files order by path"),
        vec![
            vec![
                SqlValue::Text("a.md".to_owned()),
                SqlValue::Integer(1),
                SqlValue::Null
            ],
            vec![
                SqlValue::Text("live.md".to_owned()),
                SqlValue::Integer(1),
                SqlValue::Null
            ],
        ]
    );
}

/// Capture every persisted graph/file field and FTS term through a read-only handle.
fn capture_projection(repo: &ReadingRepo) -> Vec<Vec<Vec<SqlValue>>> {
    let db = open_cache_read_only(repo);
    db.execute_batch(
        "create virtual table temp.tokens using fts5vocab(main, text_index, instance);",
    )
    .unwrap();
    [
        "select * from project order by id",
        "select * from nodes order by id",
        "select * from edges order by source_id, target_id, type",
        "select * from files order by path",
        "select term, doc, col, offset from temp.tokens order by term, doc, col, offset",
    ]
    .into_iter()
    .map(|query| get_rows(&db, query))
    .collect()
}

fn assert_safe_failure(output: &Output) {
    let response = get_response(output, 1);
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "internal_error");
    assert!(response.get("result").is_none());
    let visible = String::from_utf8_lossy(&output.stdout);
    for secret in [
        "CORRUPTION_CANARY",
        "AlphaToken",
        "letopis-git-",
        "fixture-secret",
    ] {
        assert!(!visible.contains(secret), "unsafe diagnostic: {visible}");
    }
}

fn assert_invalid_projection(repo: &ReadingRepo) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.get_nodes().unwrap().len(), 1);
    assert!(read.get_edges_by_source_id(None).unwrap().is_empty());
    assert_eq!(
        read.get_text_matches_by_expression("CharlieToken").unwrap()[0]
            .id
            .as_str(),
        "b.md"
    );
    for token in ["AlphaToken", "BravoToken"] {
        assert!(
            read.get_text_matches_by_expression(token)
                .unwrap()
                .is_empty()
        );
    }
    let files = read.get_files().unwrap();
    let paths: Vec<&str> = files.iter().map(|file| file.path.as_str()).collect();
    assert_eq!(paths, ["a.md", "b.md", "live.md"]);
    let invalid = read
        .get_file_by_path(&RepoPath::parse("a.md").unwrap())
        .unwrap()
        .unwrap();
    assert!(!invalid.valid);
    assert_eq!(invalid.error.as_deref(), Some("invalid letopis metadata"));
    assert_eq!(invalid.size, u64::try_from(INVALID.len()).unwrap());
}

fn assert_invalid_status(repo: &ReadingRepo) {
    let output = repo.invoke(&["status"], b"{}");
    let status = get_response(&output, 0);
    assert_eq!(status["ok"], true);
    assert_eq!(status["result"]["revision"], 1);
    assert_eq!(
        status["result"]["problems"],
        json!([{
            "code":"validation_failed", "message":"invalid letopis metadata",
            "details":{"path":"a.md", "reason":"invalid_metadata"}
        }])
    );
    let visible = String::from_utf8_lossy(&output.stdout);
    assert!(!visible.contains("CORRUPTION_CANARY"));
    assert!(!visible.contains("letopis-git-"));
}

fn assert_repaired_projection(repo: &ReadingRepo) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 2);
    assert_eq!(read.get_nodes().unwrap().len(), 2);
    assert!(read.get_edges_by_source_id(None).unwrap().is_empty());
    for (token, id) in [("DeltaToken", "a.md"), ("CharlieToken", "b.md")] {
        let found = read.get_text_matches_by_expression(token).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id.as_str(), id);
    }
    for token in ["AlphaToken", "BravoToken"] {
        assert!(
            read.get_text_matches_by_expression(token)
                .unwrap()
                .is_empty()
        );
    }
    let files = read.get_files().unwrap();
    assert_eq!(files.len(), 3);
    for file in files {
        assert!(file.valid);
        assert_eq!(file.error, None);
    }
}

/// Remove only the closed fixture cache and its explicit WAL/SHM siblings.
fn remove_owned_cache(root: &Path) {
    assert_eq!(fs::canonicalize(root).unwrap(), root);
    let parent = root.join(".letopis");
    assert_eq!(fs::canonicalize(&parent).unwrap(), parent);
    for name in ["cache.db", "cache.db-wal", "cache.db-shm"] {
        let path = parent.join(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                assert!(metadata.file_type().is_file());
                fs::remove_file(path).unwrap();
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => panic!("cannot inspect owned cache: {error}"),
        }
    }
}

fn get_object_path(root: &Path, oid: &str) -> PathBuf {
    root.join(".git/objects").join(&oid[..2]).join(&oid[2..])
}

fn damage_owned_object(root: &Path, path: &Path) -> Vec<u8> {
    assert_eq!(fs::canonicalize(root).unwrap(), root);
    assert!(fs::canonicalize(path).unwrap().starts_with(root));
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_file());
    let original = fs::read(path).unwrap();
    fs::remove_file(path).unwrap();
    fs::write(path, &original[..original.len() / 2]).unwrap();
    original
}

fn capture_canonical_state(repo: &ReadingRepo) -> CanonicalState {
    CanonicalState {
        files: CANONICAL_PATHS
            .iter()
            .map(|path| fs::read(repo.get_root().join(path)).ok())
            .collect(),
        refs: run_git(repo, &["for-each-ref", "--format=%(refname) %(objectname)"]),
    }
}

fn open_cache_read_only(repo: &ReadingRepo) -> Connection {
    Connection::open_with_flags(
        repo.get_root().join(".letopis/cache.db"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap()
}

fn get_matching_ids(db: &Connection, token: &str) -> Vec<String> {
    db.prepare("select id from nodes join text_index on row_id=text_index.rowid where text_index match ? order by id")
        .unwrap()
        .query_map([token], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn get_rows(db: &Connection, query: &str) -> Vec<Vec<SqlValue>> {
    let mut statement = db.prepare(query).unwrap();
    let width = statement.column_count();
    statement
        .query_map([], |row| (0..width).map(|index| row.get(index)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn run_git(repo: &ReadingRepo, args: &[&str]) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}

fn get_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    assert_eq!(output.stdout.last(), Some(&b'\n'));
    serde_json::from_slice(&output.stdout).unwrap()
}
