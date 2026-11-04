mod support;

use letopis::{
    domain::{EdgeTarget, KnowledgeId, RepoPath, Sha256},
    open_filesystem, open_storage,
    service::{
        EmbedLocker, EmbeddingConfig, EmbeddingFailure, QueryError, Storage, StorageErrorKind,
        StorageRead, VectorOutcome,
    },
};
use rusqlite::{Connection, OpenFlags, types::Value as SqlValue};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::MetadataExt, path::Path, process::Output};
use support::{
    embedding::{FixtureReply, HttpFixture},
    reading::ReadingRepo,
};

const LEGACY_CACHE: &str = include_str!("fixtures/storage/legacy-cache.sql");
const OLD_CACHE: &str = include_str!("fixtures/storage/cache-v2.sql");
const SCHEMA_THREE_CACHE: &str = include_str!("fixtures/storage/cache-v3.sql");
const CURRENT_CACHE: &str = include_str!("../src/storage/cache.sql");
// Independent SHA-256 literals of the visible texts, frozen across the cache upgrade.
const ALPHA_HASH: &str = "541ccbfe8401458c34bd84afc2a20f695b987d0d56f7ba2bf488924331ea585c";
const BRAVO_HASH: &str = "7a8c8f38be53273d985057a67b574300e39b5e722aa4efdd3531cc166584a6d7";
const CANONICAL_PATHS: &[&str] = &[
    "a.md",
    "z.md",
    "invalid.md",
    "live.md",
    ".letopis/config.json",
    ".letopis/index.lock",
    ".letopis/embed.lock",
];
const SERVICE_FILES: &[&str] = &[
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
enum OldRows {
    Valid,
    UnequalIdentity,
    DuplicateUri,
}

#[derive(Debug, PartialEq)]
struct FileState {
    path: String,
    inode: Option<u64>,
    bytes: Option<Vec<u8>>,
}

#[test]
fn old_cache_is_rejected_without_vector_or_file_changes() {
    for (version, rows) in [
        (1, OldRows::Valid),
        (2, OldRows::Valid),
        (2, OldRows::UnequalIdentity),
        (2, OldRows::DuplicateUri),
    ] {
        for populated in [true, false] {
            // Given: closed schema-1/2 fixtures include layouts the current writer cannot create.
            let repo = ReadingRepo::new();
            write_canonical_files(&repo);
            let config = repo.write_config("ollama", "http://127.0.0.1:1/api", "upgrade-fixture");
            if populated {
                seed_vectors(&repo, &config);
            }
            remove_fixture_cache(&repo);
            create_old_cache(&repo, version, rows);
            let before = capture_service_files(&repo);
            let canonical = capture_canonical_files(&repo);

            // When / Then: ordinary open, status and reindex all fail before changing any sentinel.
            assert_rejected(&repo);
            assert_rejection_files(&repo, &before, "all entries");
            assert_eq!(capture_canonical_files(&repo), canonical);
            if !populated {
                assert!(!repo.get_root().join(".letopis/vectors.db").exists());
            }
        }
    }
}

#[test]
fn schema_three_cache_is_rejected_without_vector_or_file_changes() {
    for populated in [true, false] {
        // Given: the frozen schema-3 cache has numeric nodes and textual edge endpoints.
        let repo = ReadingRepo::new();
        write_canonical_files(&repo);
        let config = repo.write_config("ollama", "http://127.0.0.1:1/api", "upgrade-fixture");
        let vectors = if populated {
            seed_vectors(&repo, &config);
            Some(capture_vector_records(&repo))
        } else {
            None
        };
        remove_fixture_cache(&repo);
        create_schema_three_cache(&repo);
        let before = capture_service_files(&repo);
        let canonical = capture_canonical_files(&repo);

        // When / Then: all entry points reject schema 3, allowing only new cache-reader sidecars.
        assert_rejected(&repo);
        assert_rejection_files(&repo, &before, "all entries");
        assert_eq!(capture_canonical_files(&repo), canonical);
        if let Some(vectors) = vectors {
            assert_eq!(capture_vector_records(&repo), vectors);
        } else {
            assert!(!repo.get_root().join(".letopis/vectors.db").exists());
        }
    }
}

#[test]
fn malformed_current_cache_is_rejected_before_vector_access() {
    for (original, malformed) in get_schema_corruptions() {
        for populated in [true, false] {
            // Given: exactly one schema-4 endpoint or unresolved-table contract is broken.
            let repo = ReadingRepo::new();
            write_canonical_files(&repo);
            let config = repo.write_config("ollama", "http://127.0.0.1:1/api", "upgrade-fixture");
            if populated {
                seed_vectors(&repo, &config);
            }
            remove_fixture_cache(&repo);
            create_malformed_cache(&repo, original, malformed);
            let before = capture_service_files(&repo);
            let canonical = capture_canonical_files(&repo);

            // When / Then: the current version cannot authorize repair or vector-file changes.
            assert_rejected(&repo);
            assert_rejection_files(&repo, &before, "all entries");
            assert_eq!(capture_canonical_files(&repo), canonical);
            if !populated {
                assert!(!repo.get_root().join(".letopis/vectors.db").exists());
            }
        }
    }
}

#[test]
fn cache_rebuild_reuses_vectors_independently_of_node_keys() {
    // Given: every valid current hash already has a vector; any inference is a regression.
    let repo = ReadingRepo::new();
    write_canonical_files(&repo);
    let no_call = HttpFixture::start(FixtureReply::Status {
        status: 500,
        body: "unexpected inference".to_owned(),
    });
    let config = repo.write_config("ollama", &no_call.get_base_url(), "upgrade-fixture");
    seed_vectors(&repo, &config);
    let vector_path = repo.get_root().join(".letopis/vectors.db");
    let vector_inode = fs::metadata(&vector_path).unwrap().ino();
    let vectors = capture_vector_records(&repo);
    remove_fixture_cache(&repo);
    create_schema_three_cache(&repo);
    let canonical = capture_canonical_files(&repo);
    let closed = capture_service_files(&repo);

    // When / Then: old cache open is rejected; only a quiescent, explicit cache removal upgrades it.
    assert_rejected(&repo);
    assert_rejection_files(&repo, &closed, "all entries");
    assert_eq!(capture_canonical_files(&repo), canonical);
    remove_fixture_cache(&repo);
    let files = open_filesystem(repo.get_root()).unwrap();
    let lock = files.try_lock_embed().unwrap().unwrap();
    let rebuilt = get_response(&repo.invoke(&["reindex"], b"{}"), 0);
    assert_eq!(rebuilt, json!({"ok":true,"result":{"revision":1}}));

    // Then: new graph and FTS keys differ from old and vector keys; all vector records survive.
    assert_rebuilt_projection(&repo, &config);
    assert_invalid_file_status(&repo);
    assert_eq!(capture_vector_records(&repo), vectors);
    assert_eq!(fs::metadata(&vector_path).unwrap().ino(), vector_inode);
    assert_eq!(capture_canonical_files(&repo), canonical);
    assert!(
        no_call.take_requests().is_empty(),
        "reindex requested document inference"
    );

    // When: a synchronous drain actively checks the rebuilt hashes with no held embed lock.
    drop(lock);
    assert_eq!(
        get_response(&repo.invoke(&["embed", "--drain"], b""), 0),
        json!({"ok":true,"result":{}})
    );
    // Then: cached hashes require zero provider calls, preserving sentinels through normal reopen.
    assert!(
        no_call.take_requests().is_empty(),
        "drain requested document inference"
    );
    assert_eq!(capture_vector_records(&repo), vectors);
    assert_eq!(fs::metadata(vector_path).unwrap().ino(), vector_inode);
    assert_eq!(capture_canonical_files(&repo), canonical);
}

fn write_canonical_files(repo: &ReadingRepo) {
    repo.write_knowledge("z.md", "AlphaToken");
    repo.write_knowledge(
        "a.md",
        "BravoToken\n[z](z.md \"letopis:type=supports;path=z.md\")",
    );
    repo.write(
        "invalid.md",
        b"<!-- letopis: PRIVATE_INVALID_METADATA -->\n",
    );
    repo.write(".letopis/index.lock", b"index lock sentinel\n");
    repo.write(".letopis/embed.lock", b"embed lock sentinel\n");
}

fn seed_vectors(repo: &ReadingRepo, config: &EmbeddingConfig) {
    let mut storage = repo.open_storage();
    let identity = config.get_identity();
    assert_eq!(
        storage.reset_vectors(None, &identity).unwrap(),
        VectorOutcome::Current(true)
    );
    let mut write = unwrap_current(storage.begin_vector_write_tx(&identity).unwrap());
    // Vector keys 1/2 intentionally have the opposite order from rebuilt a.md/z.md.
    for (hash, vector, key) in [
        (ALPHA_HASH, [1.0, 0.0, 0.0], 1),
        (BRAVO_HASH, [0.0, 1.0, 0.0], 2),
    ] {
        assert_eq!(
            write
                .store_vector(&Sha256::parse(hash).unwrap(), &vector, 3)
                .unwrap()
                .row_id,
            key
        );
    }
    let hash = Sha256::parse(ALPHA_HASH).unwrap();
    assert!(write.reserve_attempt(&hash, 4).unwrap());
    assert!(write.reserve_attempt(&hash, 4).unwrap());
    assert!(
        write
            .record_failure(&hash, "preserved failure sentinel")
            .unwrap()
    );
    write
        .set_query_error(42, "preserved query-error sentinel")
        .unwrap();
    write.commit().unwrap();
}

fn create_old_cache(repo: &ReadingRepo, version: i64, rows: OldRows) {
    let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    db.execute_batch("pragma journal_mode=wal").unwrap();
    db.execute_batch(if version == 1 {
        LEGACY_CACHE
    } else {
        OLD_CACHE
    })
    .unwrap();
    db.pragma_update(None, "user_version", version).unwrap();
    // SQL input deliberately bypasses the current writer. The old constraints allowed these identities.
    db.execute(
        "insert into nodes(row_id,id,kind,uri,hash,created_at,line_start,line_end,search_hash)
        values(100,'z.md','vertex','z.md',?1,1,1,2,?1)",
        [ALPHA_HASH],
    )
    .unwrap();
    db.execute(
        "insert into nodes(row_id,id,kind,uri,hash,created_at,line_start,line_end,search_hash)
        values(200,'a.md','vertex','a.md',?1,1,1,3,?1)",
        [BRAVO_HASH],
    )
    .unwrap();
    db.execute(
        "insert into nodes(row_id,id,kind,uri,hash,created_at,line_start,line_end,search_hash)
        values(300,'invalid.md','vertex','invalid.md',?1,1,1,2,?1)",
        [ALPHA_HASH],
    )
    .unwrap();
    db.execute_batch("insert into edges values('a.md','z.md','supports');
        insert into text_index(rowid,text) values(100,'AlphaToken'),(200,'BravoToken z'),(300,'OldInvalidToken');
        update project set revision=7;").unwrap();
    match rows {
        OldRows::Valid => (),
        OldRows::UnequalIdentity => {
            db.execute("update nodes set uri='different.md' where id='z.md'", [])
                .unwrap();
        }
        OldRows::DuplicateUri => {
            db.execute("update nodes set uri='z.md' where id='a.md'", [])
                .unwrap();
        }
    }
    assert_eq!(
        get_rows(&db, "select row_id from nodes order by row_id"),
        vec![
            vec![SqlValue::Integer(100)],
            vec![SqlValue::Integer(200)],
            vec![SqlValue::Integer(300)]
        ]
    );
    db.execute_batch("pragma wal_checkpoint(truncate)").unwrap();
}

fn create_schema_three_cache(repo: &ReadingRepo) {
    let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    db.execute_batch("pragma journal_mode=wal").unwrap();
    db.execute_batch(SCHEMA_THREE_CACHE).unwrap();
    db.execute_batch("pragma user_version=3").unwrap();
    for (id, uri, hash) in [
        (100, "z.md", ALPHA_HASH),
        (200, "a.md", BRAVO_HASH),
        (300, "invalid.md", ALPHA_HASH),
    ] {
        db.execute(
            "insert into nodes(id,kind,uri,hash,created_at,line_start,line_end,search_hash)
             values(?1,'vertex',?2,?3,1,1,2,?3)",
            rusqlite::params![id, uri, hash],
        )
        .unwrap();
    }
    db.execute_batch(
        "insert into edges values('a.md','z.md','supports');
         insert into text_index(rowid,text) values(100,'AlphaToken'),(200,'BravoToken z'),(300,'OldInvalidToken');
         update project set revision=7;
         pragma wal_checkpoint(truncate)",
    )
    .unwrap();
}

fn create_malformed_cache(repo: &ReadingRepo, original: &str, malformed: &str) {
    assert_eq!(CURRENT_CACHE.matches(original).count(), 1, "{original}");
    let schema = CURRENT_CACHE.replace(original, malformed);
    let db = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    db.execute_batch("pragma journal_mode=wal").unwrap();
    db.execute_batch(&schema).unwrap();
    db.execute_batch("pragma user_version=4; pragma wal_checkpoint(truncate)")
        .unwrap();
}

fn get_schema_corruptions() -> &'static [(&'static str, &'static str)] {
    &[
        (
            "create table edges(\n source_id integer",
            "create table edges(\n source_id text",
        ),
        ("target_id integer not null", "target_id text not null"),
        (
            "create table edges(\n source_id integer not null references nodes(id)",
            "create table edges(\n source_id integer not null",
        ),
        (
            "target_id integer not null references nodes(id)",
            "target_id integer not null",
        ),
        (
            "create table unresolved_edges(\n source_id integer not null references nodes(id)",
            "create table unresolved_edges(\n source_id integer not null",
        ),
        (
            "create table unresolved_edges(\n source_id integer",
            "create table unresolved_edges(\n source_id text",
        ),
        ("target_uri text not null", "target_uri text"),
        (
            "primary key(source_id,target_uri,type)",
            "primary key(source_id,target_uri)",
        ),
        (
            "create index unresolved_edges_target_uri on unresolved_edges(target_uri);",
            "",
        ),
        (
            "create index nodes_search_hash on nodes(search_hash) where search_hash is not null;",
            "",
        ),
    ]
}

fn assert_rejected(repo: &ReadingRepo) {
    let before = capture_service_files(repo);
    let error = open_storage(
        &repo.get_root().join(".letopis/cache.db"),
        &repo.get_root().join(".letopis/vectors.db"),
    )
    .err()
    .expect("old cache accepted");
    assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
    assert_eq!(error.get_message(), "incompatible storage database schema");
    assert_rejection_files(repo, &before, "open");
    for command in ["status", "reindex"] {
        let response = get_response(&repo.invoke(&[command], b"{}"), 1);
        assert_rejection_files(repo, &before, command);
        assert_eq!(response["ok"], false);
        assert_eq!(response["error"]["code"], "internal_error");
        assert!(response.get("result").is_none());
        let visible = response.to_string();
        for secret in [
            "PRIVATE_INVALID_METADATA",
            "AlphaToken",
            "upgrade-fixture",
            "fixture-secret",
        ] {
            assert!(!visible.contains(secret), "unsafe diagnostic: {visible}");
        }
    }
}

fn assert_rejection_files(repo: &ReadingRepo, before: &[FileState], command: &str) {
    let after = capture_service_files(repo);
    for (actual, expected) in after.iter().zip(before) {
        // A native read-only WAL schema read may create only these cache-reader artifacts.
        // Never remove them here: another process may already be using the shared index.
        let reader_sidecar_size = match actual.path.as_str() {
            ".letopis/cache.db-wal" => Some(0),
            ".letopis/cache.db-shm" => Some(32768),
            _ => None,
        };
        if expected.bytes.is_none()
            && let Some(size) = reader_sidecar_size
        {
            if actual.bytes.is_none() {
                continue;
            }
            assert!(actual.inode.is_some(), "missing {}", actual.path);
            assert_eq!(
                actual.bytes.as_ref().unwrap().len(),
                size,
                "{}",
                actual.path
            );
            continue;
        }
        if actual.path == ".letopis/cache.db-shm" && expected.inode.is_some() {
            assert_eq!(actual.inode, expected.inode, "SHM identity changed");
            assert_eq!(
                actual.bytes.as_ref().map(Vec::len),
                expected.bytes.as_ref().map(Vec::len)
            );
            continue;
        }
        assert!(actual == expected, "{command} changed {}", actual.path);
    }
}

fn assert_rebuilt_projection(repo: &ReadingRepo, config: &EmbeddingConfig) {
    let mut storage = repo.open_storage();
    let mut read = storage.begin_read_tx().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.get_config_identity(), Some(config.get_identity()));
    for (id, token, key, hash, vector) in [
        ("a.md", "BravoToken", 1, BRAVO_HASH, vec![0.0, 1.0, 0.0]),
        ("z.md", "AlphaToken", 2, ALPHA_HASH, vec![1.0, 0.0, 0.0]),
    ] {
        let node = read
            .get_node_by_id(&EdgeTarget::Knowledge(KnowledgeId::parse(id).unwrap()))
            .unwrap()
            .unwrap();
        assert_eq!(node.row_id, key);
        assert!(![100, 200, 300].contains(&node.row_id));
        let matches = read.get_text_matches_by_expression(token).unwrap();
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].id.as_str(), id);
        assert_eq!(matches[0].row_id, key);
        assert_eq!(
            unwrap_current(
                read.get_vector_by_search_hash(
                    &config.get_identity(),
                    &Sha256::parse(hash).unwrap()
                )
                .unwrap()
            ),
            Some(vector)
        );
    }
    assert_vector_state(&mut *read, config);
    assert!(
        read.get_text_matches_by_expression("OldInvalidToken")
            .unwrap()
            .is_empty()
    );
    let invalid = read
        .get_file_by_path(&RepoPath::parse("invalid.md").unwrap())
        .unwrap()
        .unwrap();
    assert!(!invalid.valid);
    assert_eq!(invalid.error.as_deref(), Some("invalid letopis metadata"));
    drop(read);
    drop(storage);
    assert_rebuilt_sql(repo);
}

fn assert_vector_state(read: &mut dyn StorageRead, config: &EmbeddingConfig) {
    let nearest = unwrap_current(
        read.get_nearest_vectors(&config.get_identity(), &[1.0, 0.0, 0.0], 3)
            .unwrap(),
    );
    assert_eq!(
        nearest
            .iter()
            .map(|v| (v.id.as_str(), v.search_hash.to_hex(), v.distance))
            .collect::<Vec<_>>(),
        [
            ("z.md", ALPHA_HASH.to_owned(), 0.0),
            ("a.md", BRAVO_HASH.to_owned(), 1.0)
        ]
    );
    assert_eq!(
        unwrap_current(
            read.get_failure_by_search_hash(
                &config.get_identity(),
                &Sha256::parse(ALPHA_HASH).unwrap()
            )
            .unwrap()
        ),
        Some(EmbeddingFailure {
            search_hash: Sha256::parse(ALPHA_HASH).unwrap(),
            attempts: 2,
            error: Some("preserved failure sentinel".to_owned()),
        })
    );
    assert_eq!(
        read.get_query_error().unwrap(),
        Some(QueryError {
            at: 42,
            error: "preserved query-error sentinel".to_owned()
        })
    );
}

fn assert_rebuilt_sql(repo: &ReadingRepo) {
    let db = open_read_only(&repo.get_root().join(".letopis/cache.db"));
    assert_eq!(
        get_rows(&db, "pragma user_version"),
        vec![vec![SqlValue::Integer(4)]]
    );
    assert_eq!(
        get_rows(&db, "select id,uri,search_hash from nodes order by uri"),
        vec![
            vec![
                SqlValue::Integer(1),
                SqlValue::Text("a.md".to_owned()),
                SqlValue::Text(BRAVO_HASH.to_owned())
            ],
            vec![
                SqlValue::Integer(2),
                SqlValue::Text("z.md".to_owned()),
                SqlValue::Text(ALPHA_HASH.to_owned())
            ],
        ]
    );
    assert_eq!(
        get_rows(&db, "select source_id,target_id,type from edges"),
        vec![vec![
            SqlValue::Integer(1),
            SqlValue::Integer(2),
            SqlValue::Text("supports".to_owned()),
        ]]
    );
    assert_eq!(
        get_rows(
            &db,
            "select s.uri,t.uri,e.type from edges e join nodes s on s.id=e.source_id join nodes t on t.id=e.target_id"
        ),
        vec![vec![
            SqlValue::Text("a.md".to_owned()),
            SqlValue::Text("z.md".to_owned()),
            SqlValue::Text("supports".to_owned())
        ]]
    );
    assert!(get_rows(&db, "select * from unresolved_edges").is_empty());
    assert!(get_rows(&db, "pragma foreign_key_check").is_empty());
    assert_eq!(
        get_rows(
            &db,
            "select n.uri,t.rowid from text_index t join nodes n on n.id=t.rowid where text_index match 'AlphaToken'"
        ),
        vec![vec![
            SqlValue::Text("z.md".to_owned()),
            SqlValue::Integer(2)
        ]]
    );
}

fn assert_invalid_file_status(repo: &ReadingRepo) {
    let status = get_response(&repo.invoke(&["status"], b"{}"), 0);
    assert_eq!(status["result"]["revision"], 1);
    assert!(
        status["result"]["problems"]
            .as_array()
            .unwrap()
            .contains(&json!({
                "code":"validation_failed", "message":"invalid letopis metadata",
                "details":{"path":"invalid.md","reason":"invalid_metadata"}
            }))
    );
    assert!(!status.to_string().contains("PRIVATE_INVALID_METADATA"));
}

fn capture_vector_records(repo: &ReadingRepo) -> Vec<Vec<Vec<SqlValue>>> {
    // Read-only SQL records, not file bytes: normal reopen may change WAL representation.
    let db = open_read_only(&repo.get_root().join(".letopis/vectors.db"));
    assert_eq!(
        get_rows(&db, "pragma user_version"),
        vec![vec![SqlValue::Integer(1)]]
    );
    assert_eq!(
        get_rows(&db, "select rowid,vector from vector_index order by rowid"),
        vec![
            vec![
                SqlValue::Integer(1),
                SqlValue::Blob(vec![0, 0, 128, 63, 0, 0, 0, 0, 0, 0, 0, 0])
            ],
            vec![
                SqlValue::Integer(2),
                SqlValue::Blob(vec![0, 0, 0, 0, 0, 0, 128, 63, 0, 0, 0, 0])
            ],
        ]
    );
    assert_eq!(
        get_rows(
            &db,
            "select row_id,search_hash from vector_keys order by row_id"
        ),
        vec![
            vec![SqlValue::Integer(1), SqlValue::Text(ALPHA_HASH.to_owned())],
            vec![SqlValue::Integer(2), SqlValue::Text(BRAVO_HASH.to_owned())],
        ]
    );
    [
        "select * from meta order by id",
        "select * from vector_keys order by row_id",
        "select rowid,vector from vector_index order by rowid",
        "select * from embed_failures order by search_hash",
        "select * from query_error order by id",
    ]
    .iter()
    .map(|sql| get_rows(&db, sql))
    .collect()
}

fn capture_service_files(repo: &ReadingRepo) -> Vec<FileState> {
    SERVICE_FILES
        .iter()
        .map(|name| capture_file(repo.get_root(), &format!(".letopis/{name}")))
        .collect()
}

fn capture_canonical_files(repo: &ReadingRepo) -> Vec<FileState> {
    let mut paths: Vec<String> = CANONICAL_PATHS
        .iter()
        .map(|path| (*path).to_owned())
        .collect();
    collect_git_files(repo.get_root(), Path::new(".git"), &mut paths);
    paths.sort();
    paths
        .iter()
        .map(|path| {
            let mut state = capture_file(repo.get_root(), path);
            // Supported operations update lease owners; persistent lock inodes must survive.
            if matches!(path.as_str(), ".letopis/index.lock" | ".letopis/embed.lock") {
                state.bytes = None;
            }
            state
        })
        .collect()
}

fn collect_git_files(root: &Path, relative: &Path, paths: &mut Vec<String>) {
    for entry in fs::read_dir(root.join(relative)).unwrap() {
        let entry = entry.unwrap();
        let path = relative.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            collect_git_files(root, &path, paths);
        } else {
            paths.push(path.to_str().unwrap().to_owned());
        }
    }
}

fn capture_file(root: &Path, relative: &str) -> FileState {
    let path = root.join(relative);
    FileState {
        path: relative.to_owned(),
        inode: fs::metadata(&path).ok().map(|m| m.ino()),
        bytes: fs::read(path).ok(),
    }
}

fn remove_fixture_cache(repo: &ReadingRepo) {
    // Every caller has closed all storage/SQLite handles before this temporary-fixture operation.
    for name in [
        "cache.db",
        "cache.db-wal",
        "cache.db-shm",
        "cache.db-journal",
    ] {
        let path = repo.get_root().join(".letopis").join(name);
        if path.exists() {
            assert!(fs::symlink_metadata(&path).unwrap().is_file());
            fs::remove_file(path).unwrap();
        }
    }
}

fn open_read_only(path: &Path) -> Connection {
    Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap()
}

fn get_rows(db: &Connection, sql: &str) -> Vec<Vec<SqlValue>> {
    let mut query = db.prepare(sql).unwrap();
    let width = query.column_count();
    query
        .query_map([], |row| (0..width).map(|column| row.get(column)).collect())
        .unwrap()
        .map(Result::unwrap)
        .collect()
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    let VectorOutcome::Current(value) = outcome else {
        panic!("unexpected vector identity change")
    };
    value
}

fn get_response(output: &Output, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
