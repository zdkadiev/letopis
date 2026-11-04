mod support;

use letopis::{
    domain::Sha256,
    open_storage,
    service::{Storage, StorageErrorKind, VectorOutcome},
};
use rusqlite::{Connection, OpenFlags, config::DbConfig};
use serde_json::Value;
use std::{fs, os::unix::fs::MetadataExt, path::Path};
use support::reading::ReadingRepo;
use tempfile::TempDir;

const OLD_SCHEMA: &str = include_str!("fixtures/storage/cache-v3.sql");
const CURRENT_SCHEMA: &str = include_str!("../src/storage/cache.sql");
const FILES: &[&str] = &[
    "cache.db",
    "cache.db-wal",
    "cache.db-shm",
    "cache.db-journal",
    "vectors.db",
    "vectors.db-wal",
    "vectors.db-shm",
    "vectors.db-journal",
    "config.json",
    "index.lock",
    "embed.lock",
];

#[derive(Clone, Copy, Debug)]
enum Entry {
    Direct,
    Status,
    Reindex,
}

#[derive(Clone, Copy)]
enum Journal {
    Delete,
    RetainedWal,
    LiveWal,
}

#[derive(Debug, PartialEq)]
struct FileState {
    inode: u64,
    bytes: Vec<u8>,
}

#[test]
fn closed_delete_cache_is_rejected_without_persistent_changes() {
    for entry in [Entry::Status, Entry::Direct, Entry::Reindex] {
        for populated in [false, true] {
            // Given: an independent closed schema-3 cache in DELETE mode for each entry.
            let repo = create_repo(populated);
            create_cache(&repo, Journal::Delete);
            let before = get_files(&repo);
            // When: the public entry rejects the old schema.
            assert_rejected(&repo, entry);
            // Then: main database, vectors, config and locks retain bytes and identities.
            assert_preserved(&repo, &before);
            assert_committed_rows(&repo, 3);
        }
    }
}

#[test]
fn retained_wal_cache_is_rejected_without_checkpoint_or_removal() {
    for entry in [Entry::Status, Entry::Direct, Entry::Reindex] {
        for populated in [false, true] {
            // Given: a closed writer retained committed WAL over a supported main-file schema.
            let repo = create_repo(populated);
            create_cache(&repo, Journal::RetainedWal);
            assert_main_is_current(&repo);
            let before = get_files(&repo);
            assert!(!before[1].as_ref().unwrap().bytes.is_empty());
            // When: recognition must use the incompatible schema committed in WAL.
            assert_rejected(&repo, entry);
            // Then: the retained WAL and main file survive unchanged with their committed row.
            assert_preserved(&repo, &before);
            assert_committed_rows(&repo, 3);
        }
    }
}

#[test]
fn live_wal_rejection_preserves_persistent_files_and_shm_identity() {
    for entry in [Entry::Status, Entry::Direct, Entry::Reindex] {
        for populated in [false, true] {
            // Given: a live writer with committed old schema and existing WAL/SHM sidecars.
            let repo = create_repo(populated);
            let writer = create_cache(&repo, Journal::LiveWal).unwrap();
            assert_main_is_current(&repo);
            let before = get_files(&repo);
            assert!(!before[1].as_ref().unwrap().bytes.is_empty());
            assert!(before[2].is_some());
            // When: another connection observes the committed WAL before refusing the cache.
            assert_rejected(&repo, entry);
            // Then: persistent bytes and all identities survive; SHM reader marks may change.
            assert_preserved(&repo, &before);
            assert_committed_rows(&repo, 3);
            drop(writer);
        }
    }
}

#[test]
fn malformed_current_delete_cache_is_rejected_before_journal_configuration() {
    for entry in [Entry::Status, Entry::Direct, Entry::Reindex] {
        for populated in [false, true] {
            // Given: current version but a missing required index in a closed DELETE database.
            let repo = create_repo(populated);
            let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
            db.execute_batch(CURRENT_SCHEMA).unwrap();
            db.execute_batch("pragma user_version=4; drop index unresolved_edges_target_uri")
                .unwrap();
            drop(db);
            let before = get_files(&repo);
            // When: exact schema recognition rejects the malformed current layout.
            assert_rejected(&repo, entry);
            // Then: version-only preflight cannot hide persistent journal changes.
            assert_preserved(&repo, &before);
        }
    }
}

fn create_repo(populated: bool) -> ReadingRepo {
    let repo = ReadingRepo::new();
    repo.write(
        ".letopis/config.json",
        br#"{"embedding_model":{"enabled":false,"provider":null,"model":null,"parameters":{},"max_retries":0}}"#,
    );
    repo.write(".letopis/index.lock", b"index sentinel");
    repo.write(".letopis/embed.lock", b"embed sentinel");
    if populated {
        let mut storage = repo.open_storage();
        let hash = Sha256::digest(b"retained vector");
        storage.reset_vectors(None, &hash).unwrap();
        let VectorOutcome::Current(mut write) = storage.begin_vector_write_tx(&hash).unwrap()
        else {
            panic!("identity changed")
        };
        write.store_vector(&hash, &[1.0, 0.0, 0.0], 3).unwrap();
        write.commit().unwrap();
        drop(storage);
        fs::remove_file(repo.get_root().join(".letopis/cache.db")).unwrap();
    }
    repo
}

fn create_cache(repo: &ReadingRepo, journal: Journal) -> Option<Connection> {
    let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    if !matches!(journal, Journal::Delete) {
        // A WAL-blind observer would see a supported empty cache, not the old schema.
        db.execute_batch(CURRENT_SCHEMA).unwrap();
        db.execute_batch(
            "pragma user_version=4; pragma journal_mode=wal;
            pragma wal_autocheckpoint=0; begin immediate;
            drop table unresolved_edges; drop table edges; drop table text_index;
            drop table files; drop table nodes; drop table project;",
        )
        .unwrap();
        db.set_db_config(DbConfig::SQLITE_DBCONFIG_NO_CKPT_ON_CLOSE, true)
            .unwrap();
    }
    db.execute_batch(OLD_SCHEMA).unwrap();
    db.execute_batch(
        "pragma user_version=3;
        insert into nodes(id,kind,uri,created_at) values(71,'source','wal-only.rs',7);
        update project set revision=7;",
    )
    .unwrap();
    if !matches!(journal, Journal::Delete) {
        db.execute_batch("commit").unwrap();
    }
    if matches!(journal, Journal::LiveWal) {
        Some(db)
    } else {
        None
    }
}

fn assert_main_is_current(repo: &ReadingRepo) {
    let copy = TempDir::new().unwrap();
    let path = copy.path().join("main-only.db");
    fs::copy(repo.get_root().join(".letopis/cache.db"), &path).unwrap();
    let db = open_read_only(&path);
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        4
    );
    assert_eq!(
        db.query_row("select count(*) from nodes", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn assert_committed_rows(repo: &ReadingRepo, version: i64) {
    let db = open_read_only(&repo.get_root().join(".letopis/cache.db"));
    assert_eq!(
        db.pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
            .unwrap(),
        version
    );
    let row = db
        .query_row("select id,uri from nodes", [], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap();
    assert_eq!(row, (71, "wal-only.rs".to_owned()));
    assert_eq!(
        db.query_row("select revision from project", [], |row| row
            .get::<_, i64>(0))
            .unwrap(),
        7
    );
}

fn assert_rejected(repo: &ReadingRepo, entry: Entry) {
    if matches!(entry, Entry::Direct) {
        let error = open_storage(
            &repo.get_root().join(".letopis/cache.db"),
            &repo.get_root().join(".letopis/vectors.db"),
        )
        .err()
        .expect("old schema accepted");
        assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
        assert_eq!(error.get_message(), "incompatible storage database schema");
        return;
    }
    let command = match entry {
        Entry::Status => "status",
        Entry::Reindex => "reindex",
        Entry::Direct => unreachable!(),
    };
    let output = repo.invoke(&[command], b"{}");
    assert_eq!(output.status.code(), Some(1), "{entry:?}: {output:?}");
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], false);
    assert_eq!(response["error"]["code"], "internal_error");
    assert!(response.get("result").is_none());
}

fn assert_preserved(repo: &ReadingRepo, before: &[Option<FileState>]) {
    for ((name, expected), actual) in FILES.iter().zip(before).zip(get_files(repo)) {
        if *name == "cache.db-shm"
            && let (Some(expected), Some(actual)) = (expected, &actual)
        {
            assert_eq!(actual.inode, expected.inode, "SHM replaced");
            assert_eq!(actual.bytes.len(), expected.bytes.len(), "SHM resized");
        } else {
            assert!(&actual == expected, "{name} changed");
        }
    }
}

fn get_files(repo: &ReadingRepo) -> Vec<Option<FileState>> {
    FILES
        .iter()
        .map(|name| {
            let path = repo.get_root().join(".letopis").join(name);
            path.exists().then(|| FileState {
                inode: fs::metadata(&path).unwrap().ino(),
                bytes: fs::read(&path).unwrap(),
            })
        })
        .collect()
}

fn open_read_only(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}
