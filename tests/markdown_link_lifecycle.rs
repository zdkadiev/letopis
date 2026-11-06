use crate::support::storage::get_current;
use letopis::service::StorageError;
use letopis::service::{CacheDatabase, StorageRead, VectorDatabase};
mod support;

use letopis::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256,
        SourceUri, TreeOid,
    },
    markdown::file_parse,
    open_filesystem,
    service::{
        EmbeddingFailure, FileLocker, FileRow, KnowledgeMetadata, NodeMetadata, SourceMetadata,
        VectorOutcome,
    },
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, FileTimes},
    os::unix::fs::MetadataExt,
    process::Command,
    time::{Duration, UNIX_EPOCH},
};
use support::{
    documents::{State, build_whole},
    git::isolate_command,
    provenance::create_basis,
    reading::ReadingRepo,
};

const NOTE_PATH: &str = "letopis/cache/note.md";
const SAFE_MISMATCH: &str = "Markdown link path does not match letopis path";
const VALID_NOTE: &str =
    "[TargetToken](target.md#%ZZ \"letopis:type=supports;path=letopis/cache/target.md\")";
const INVALID_NOTE: &str = "[TargetToken](../../private-navigation.md#PrivateFragment \"letopis:type=supports;path=letopis/cache/target.md\")";
const BASIS: &str = "8533ec406e5b17e3e5b2164a3b324cac95ca97e4";
const SOURCE_BYTES: &[u8] = b"one\ntwo\nthree\n";
const OLD_NOTE: &str = "<!-- letopis: {\"created_at\":42,\"source_snapshot\":\"8533ec406e5b17e3e5b2164a3b324cac95ca97e4\"} -->\nStableToken [source](source.txt#L2-L3 \"letopis:type=external\")\n";
const NEW_NOTE: &str = "<!-- letopis: {\"created_at\":42,\"source_snapshot\":\"8533ec406e5b17e3e5b2164a3b324cac95ca97e4\"} -->\nStableToken [source](./source.txt#nonsense%ZZ \"letopis:type=external;path=source.txt#L2-L3\")\n";
const OLD_HASH: &str = "902e5cfa985e2bce74e39e607d043b3e2dc5b8bcda6fb6d950ec5e4d9301674b";
const NEW_HASH: &str = "a23cb8fc6cdc5df208338011fef8360c881ac278e089901ec60ccaf71d87d8f7";
const SEARCH_HASH: &str = "cac8155f265636e95df1e02ed6c11e64577bae0aece4fa29ac7cc15b9662994b";
const CONFIG_IDENTITY: &str = "4f2c2e77179bda2b59bfb2eee5c1e53d75ffeace98fba901c97b65282c6d0cbb";
const FAILURE_HASH: &str = "ce7289886eae2d81587ee8a170e7ece5a3face8ad7be4527ff942877d0ab888d";

#[test]
fn status_reports_published_path_mismatch_until_a_refresh_repairs_it() {
    // Given: a valid nested note and its target have already published.
    let repo = ReadingRepo::new();
    repo.write(NOTE_PATH, build_whole(42, VALID_NOTE).as_bytes());
    repo.write(
        "letopis/cache/target.md",
        build_whole(7, "Target").as_bytes(),
    );
    assert_eq!(invoke(&repo, "check")["result"]["errors"], json!([]));
    let original = get_state(&repo);
    repo.write(NOTE_PATH, build_whole(42, INVALID_NOTE).as_bytes());
    repo.write("unrelated.md", build_whole(73, "UnrelatedToken").as_bytes());
    // When: status observes old publication, then check publishes the invalid file.
    assert_eq!(invoke(&repo, "status")["result"]["problems"], json!([]));
    assert_eq!(get_state(&repo), original);
    let checked = invoke(&repo, "check");
    let invalid = get_state(&repo);
    // Then: the exact safe finding persists, while old edges and unrelated data survive.
    let finding = json!({"code":"validation_failed","message":
        "Markdown link path does not match letopis path","details":{
        "path":"letopis/cache/note.md","reason":"link_path_mismatch"}});
    assert_eq!(checked["result"]["errors"], json!([finding]));
    assert!(!invalid.get_file(NOTE_PATH).valid);
    assert_eq!(
        invalid.get_file(NOTE_PATH).error.as_deref(),
        Some(SAFE_MISMATCH)
    );
    assert_eq!(invalid.get_node(NOTE_PATH), original.get_node(NOTE_PATH));
    assert_eq!(invalid.edges, original.edges);
    assert_eq!(get_knowledge(&repo, "unrelated.md").created_at, 73);
    let status = invoke(&repo, "status");
    assert_eq!(status["result"]["problems"], json!([finding]));
    assert_eq!(get_state(&repo), invalid);
    // When: only author bytes are repaired, a new status process still reports the finding.
    repo.write(NOTE_PATH, build_whole(42, VALID_NOTE).as_bytes());
    let after_edit = invoke(&repo, "status");
    // Then: observation cannot clear the error or advance the revision.
    assert_eq!(after_edit["result"]["problems"], json!([finding]));
    assert_eq!(after_edit["result"]["revision"], invalid.revision);
    assert_eq!(get_state(&repo), invalid);
    // When: successful refresh publishes the repaired file.
    assert_eq!(invoke(&repo, "check")["result"]["errors"], json!([]));
    // Then: its invalid state clears only with the new publication.
    let repaired = get_state(&repo);
    assert!(repaired.revision > invalid.revision);
    assert!(repaired.get_file(NOTE_PATH).valid);
    assert_eq!(repaired.get_file(NOTE_PATH).error, None);
    assert_eq!(repaired.get_node(NOTE_PATH), original.get_node(NOTE_PATH));
    assert_eq!(repaired.edges, original.edges);
    assert_eq!(
        repaired.get_node("unrelated.md"),
        invalid.get_node("unrelated.md")
    );
    assert_eq!(invoke(&repo, "status")["result"]["problems"], json!([]));
}

#[test]
fn legacy_titles_never_publish_into_a_fresh_cache() {
    // Given: a legacy declaration follows a valid new-form edge in the same fresh file.
    let repo = ReadingRepo::new();
    repo.write("target.md", build_whole(7, "TargetToken").as_bytes());
    repo.write("legacy.md", build_whole(42,
        "[new](target.md \"letopis:type=supports;path=target.md\")\n[old](target.md \"letopis:type=supports\")").as_bytes());
    // When: the actual binary creates a new cache with the upgraded parser.
    let checked = invoke(&repo, "check");
    // Then: no partial owner, FTS entry or edge is published from that invalid file.
    let expected = json!([{"code":"validation_failed","message":"invalid letopis link title",
        "details":{"path":"legacy.md","reason":"invalid_link_title"}}]);
    assert_eq!(checked["result"]["errors"], expected);
    assert_eq!(invoke(&repo, "status")["result"]["problems"], expected);
    let state = get_state(&repo);
    assert!(!state.get_file("legacy.md").valid);
    assert_eq!(
        state.get_file("legacy.md").error.as_deref(),
        Some("invalid letopis link title")
    );
    assert_eq!(state.nodes.len(), 1);
    assert_eq!(get_knowledge(&repo, "target.md").id.as_str(), "target.md");
    assert_eq!(get_knowledge(&repo, "target.md").created_at, 7);
    assert!(state.edges.is_empty());
    let storage = repo.open_storage();
    assert!(
        storage
            .cache
            .with_read_tx(|| storage.cache.text_index_get_by_expression("new OR old"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn reindex_reparses_unchanged_pre_upgrade_rows() {
    // Given: independently seeded old errors have exact current hashes and non-racy stats.
    let repo = ReadingRepo::new();
    let (valid, mismatch) = seed_pre_upgrade_errors(&repo);
    // When: ordinary refresh sees unchanged bytes and preserves pre-upgrade errors.
    let ordinary = invoke(&repo, "check");
    // Then: both seeded observations survive unchanged, with no new projection.
    assert_eq!(ordinary["result"]["errors"].as_array().unwrap().len(), 2);
    assert!(
        ordinary["result"]["errors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|problem| problem["details"]["reason"] == "invalid_link_title")
    );
    let incremental = get_state(&repo);
    assert_eq!(incremental.get_file("valid.md"), &valid);
    assert_eq!(incremental.get_file("mismatch.md"), &mismatch);
    assert!(incremental.nodes.is_empty());
    // When: explicit reindex reparses both candidates without any byte/stat change.
    let rebuilt = invoke(&repo, "reindex");
    let state = get_state(&repo);
    // Then: valid syntax clears the old error; mismatch gets its new precise classification.
    assert!(state.get_file("valid.md").valid);
    assert!(rebuilt["result"]["revision"].as_u64().unwrap() > incremental.revision);
    assert_eq!(state.get_file("valid.md").error, None);
    assert!(!state.get_file("mismatch.md").valid);
    assert_eq!(
        state.get_file("mismatch.md").error.as_deref(),
        Some(SAFE_MISMATCH)
    );
    assert_eq!(state.get_file("valid.md").hash, valid.hash);
    assert_eq!(state.get_file("valid.md").mtime_ns, valid.mtime_ns);
    assert_eq!(state.get_file("mismatch.md").hash, mismatch.hash);
    assert_eq!(state.edges.len(), 1);
    assert_eq!(get_knowledge(&repo, "valid.md").created_at, 42);
    assert_eq!(
        invoke(&repo, "status")["result"]["problems"],
        json!([
            {"code":"validation_failed","message":"Markdown link path does not match letopis path",
             "details":{"path":"mismatch.md","reason":"link_path_mismatch"}},
            {"code":"validation_failed","message":"knowledge target is missing",
             "details":{"path":"valid.md","reason":"dangling_target","element_id":"valid.md","target":"missing.md"}}
        ])
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "Keep the sequential transaction scenario and its assertions together"
)]
fn link_syntax_migration_reuses_vectors_and_preserves_source_basis() {
    // Given: literal old cache data and a real verified source tree, without an old parser.
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("source.txt", SOURCE_BYTES)]);
    assert_eq!(basis.as_str(), BASIS);
    assert_eq!(get_git_source(&repo, &basis), SOURCE_BYTES);
    repo.write("source.txt", SOURCE_BYTES);
    seed_legacy_projection(&repo);
    let original = get_knowledge(&repo, "note.md");
    assert_eq!(original.hash.to_hex(), OLD_HASH);
    assert_eq!(original.search_hash.to_hex(), SEARCH_HASH);
    let config = repo.write_config("ollama", "http://127.0.0.1:1", "fixture");
    let identity = Sha256::parse(CONFIG_IDENTITY).unwrap();
    repo.seed_vectors(&config, &[("note.md", &[1.0, 0.0, 0.0])]);
    seed_failure(&repo, &identity);
    let files = open_filesystem(repo.get_root()).unwrap();
    let _lock = files
        .lock_try_acquire_by_path(std::path::Path::new(".letopis/embed.lock"))
        .unwrap()
        .unwrap();
    let vectors = fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap();
    // When: only destination/title syntax changes and the actual CLI rebuilds.
    repo.write("note.md", NEW_NOTE.as_bytes());
    invoke(&repo, "reindex");
    // Then: independent literals pin the author time, hashes and honest source basis.
    let migrated = get_knowledge(&repo, "note.md");
    assert_eq!(migrated.id.as_str(), "note.md");
    assert_eq!(migrated.created_at, 42);
    assert_eq!(migrated.source_snapshot.as_ref().unwrap().as_str(), BASIS);
    assert_eq!(migrated.search_hash.to_hex(), SEARCH_HASH);
    assert_eq!(migrated.hash.to_hex(), NEW_HASH);
    assert_ne!(migrated.hash, original.hash);
    assert_eq!(get_git_source(&repo, &basis), SOURCE_BYTES);
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            let read = &storage.cache;
            assert_eq!(read.meta_get_by_id().unwrap(), Some(identity));
            assert_eq!(
                read.vector_keys_get_by_search_hash(&identity, &migrated.search_hash)
                    .unwrap(),
                VectorOutcome::Current(Some(vec![1.0, 0.0, 0.0]))
            );
            assert_eq!(
                read.embed_failures_get_by_search_hash(
                    &identity,
                    &Sha256::parse(FAILURE_HASH).unwrap()
                )
                .unwrap(),
                VectorOutcome::Current(Some(EmbeddingFailure {
                    search_hash: Sha256::parse(FAILURE_HASH).unwrap(),
                    attempts: 1,
                    error: Some("preserved failure sentinel".into())
                }))
            );
            assert_eq!(
                read.text_index_get_by_expression("StableToken").unwrap()[0]
                    .id
                    .as_str(),
                "note.md"
            );

            Ok::<_, StorageError>(())
        })
        .unwrap();

    assert_eq!(
        fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap(),
        vectors
    );
    assert_eq!(invoke(&repo, "check")["result"]["errors"], json!([]));
}

#[test]
fn syntax_migration_preserves_literal_search_text_and_updates_exact_bytes() {
    // Given: independent historical and migrated byte/hash literals share a visible label.
    let repo = ReadingRepo::new();
    let basis = create_basis(repo.get_root(), &[("source.txt", SOURCE_BYTES)]);
    assert_eq!(basis.as_str(), BASIS);
    assert_eq!(get_git_source(&repo, &basis), SOURCE_BYTES);
    repo.write("source.txt", SOURCE_BYTES);
    seed_legacy_projection(&repo);
    // When: new syntax is published by the actual reindex command.
    repo.write("note.md", NEW_NOTE.as_bytes());
    invoke(&repo, "reindex");
    let parsed = file_parse(&RepoPath::parse("note.md").unwrap(), NEW_NOTE.as_bytes())
        .unwrap()
        .unwrap();
    // Then: exact authored bytes change while the visible text and fingerprint stay literal.
    assert_eq!(parsed.elements[0].search_text, "StableToken source");
    assert_eq!(parsed.elements[0].search_hash.to_hex(), SEARCH_HASH);
    assert_eq!(get_knowledge(&repo, "note.md").hash.to_hex(), NEW_HASH);
    assert_eq!(
        fs::read(repo.get_root().join("note.md")).unwrap(),
        NEW_NOTE.as_bytes()
    );
    assert_eq!(
        fs::read(repo.get_root().join("source.txt")).unwrap(),
        SOURCE_BYTES
    );
}

#[test]
fn matching_missing_targets_keep_existing_diagnostics() {
    // Given: matching nonexistent paths and malformed navigation fragments are lexical matches.
    let repo = ReadingRepo::new();
    repo.write("missing-links.md", build_whole(42,
        "[knowledge](absent.md#%ZZ \"letopis:type=supports;path=absent.md#anchor\")\n[source](absent.rs#L0-Lbroken \"letopis:type=external;path=absent.rs\")").as_bytes());
    repo.write("mismatch.md", build_whole(73, INVALID_NOTE).as_bytes());
    // When: the binary performs parser validation followed by existing graph/source checks.
    let checked = invoke(&repo, "check");
    let status = invoke(&repo, "status");
    // Then: the independent global rules remain visible alongside the new file-level reason.
    let expected = json!([
        {"code":"validation_failed","message":"Markdown link path does not match letopis path",
         "details":{"path":"mismatch.md","reason":"link_path_mismatch"}},
        {"code":"validation_failed","message":"knowledge target is missing",
         "details":{"path":"missing-links.md","reason":"dangling_target","element_id":"missing-links.md","target":"absent.md#anchor"}},
        {"code":"validation_failed","message":"knowledge source snapshot is missing",
         "details":{"path":"missing-links.md","reason":"missing_source_snapshot","element_id":"missing-links.md"}},
        {"code":"validation_failed","message":"knowledge source is not found",
         "details":{"path":"missing-links.md","reason":"source_not_found","element_id":"missing-links.md","target":"absent.rs"}}
    ]);
    assert_eq!(checked["result"]["errors"], expected);
    assert_eq!(status["result"]["problems"], expected);
    let state = get_state(&repo);
    assert!(state.get_file("missing-links.md").valid);
    assert_eq!(state.get_file("missing-links.md").error, None);
    assert_eq!(state.edges.len(), 2);
    assert!(!repo.get_root().join("absent.md").exists());
    assert!(!repo.get_root().join("absent.rs").exists());
}

fn invoke(repo: &ReadingRepo, command: &str) -> Value {
    let args: &[&str] = if command == "reindex" {
        &["index", "--full"]
    } else {
        &[command]
    };
    let output = repo.invoke(args, b"{}");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(
        output.stdout.iter().filter(|byte| **byte == b'\n').count(),
        1
    );
    let response: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(response["ok"], true, "{response}");
    response
}

fn get_knowledge(repo: &ReadingRepo, id: &str) -> KnowledgeMetadata {
    let NodeMetadata::Knowledge(metadata) = get_state(repo).get_node(id).metadata.clone() else {
        panic!("expected knowledge node")
    };
    metadata
}

fn get_state(repo: &ReadingRepo) -> State {
    let storage = repo.open_storage();
    storage
        .cache
        .with_read_tx(|| {
            Ok::<_, StorageError>({
                let read = &storage.cache;
                State {
                    revision: read.project_get_by_id().unwrap(),
                    nodes: read.nodes_get().unwrap(),
                    edges: read.edges_get_by_source_id(None).unwrap(),
                    files: read.files_get().unwrap(),
                }
            })
        })
        .unwrap()
}

fn seed_pre_upgrade_errors(repo: &ReadingRepo) -> (FileRow, FileRow) {
    fs::create_dir_all(repo.get_root().join(".letopis")).unwrap();
    repo.write(
        "valid.md",
        build_whole(
            42,
            "[target](missing.md#%ZZ \"letopis:type=supports;path=missing.md\")",
        )
        .as_bytes(),
    );
    repo.write(
        "mismatch.md",
        build_whole(
            73,
            "[target](different.md \"letopis:type=supports;path=missing.md\")",
        )
        .as_bytes(),
    );
    let valid = capture_seed_file(repo, "valid.md", false, Some("invalid letopis link title"));
    let mismatch = capture_seed_file(
        repo,
        "mismatch.md",
        false,
        Some("invalid letopis link title"),
    );
    let storage = repo.open_storage();
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            unit.files_upsert_by_path(&valid).unwrap();
            unit.files_upsert_by_path(&mismatch).unwrap();

            unit.project_get_revision()
        })
        .unwrap();
    drop(storage);
    (valid, mismatch)
}

/// Seed the historical projection explicitly; no production parser accepts `OLD_NOTE`.
fn seed_legacy_projection(repo: &ReadingRepo) {
    fs::create_dir_all(repo.get_root().join(".letopis")).unwrap();
    repo.write("note.md", OLD_NOTE.as_bytes());
    let file = capture_seed_file(repo, "note.md", true, None);
    let id = KnowledgeId::parse("note.md").unwrap();
    let source = SourceUri::parse("source.txt#L2-L3").unwrap();
    let storage = repo.open_storage();
    storage
        .cache
        .with_write_tx(|| {
            let unit = &storage.cache;
            unit.nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
                id: id.clone(),
                uri: id.clone(),
                kind: ElementKind::Vertex,
                hash: Sha256::parse(OLD_HASH).unwrap(),
                created_at: 42,
                source_snapshot: Some(TreeOid::parse(BASIS).unwrap()),
                lines: LineRange::new(1, 2).unwrap(),
                search_hash: Sha256::parse(SEARCH_HASH).unwrap(),
            }))
            .unwrap();
            unit.nodes_upsert_by_id(&NodeMetadata::Source(SourceMetadata {
                uri: source.clone(),
                hash: Some(Sha256::digest(b"two\nthree\n")),
                created_at: 42,
                lines: Some(LineRange::new(2, 3).unwrap()),
            }))
            .unwrap();
            unit.edges_save_by_source_id_and_target_id_and_type(
                &Edge::new(
                    id.clone(),
                    EdgeTarget::Source(source),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap(),
            )
            .unwrap();
            unit.replace_text(&id, "StableToken source").unwrap();
            unit.files_upsert_by_path(&file).unwrap();

            unit.project_get_revision()
        })
        .unwrap();
}

fn seed_failure(repo: &ReadingRepo, identity: &Sha256) {
    let storage = repo.open_storage();
    storage
        .vectors
        .with_write_tx(identity, || {
            let unit = &storage.vectors;
            let hash = Sha256::parse(FAILURE_HASH).unwrap();
            assert!(
                unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 3)
                    .unwrap()
            );
            assert!(
                unit.embed_failures_update_by_search_hash(&hash, "preserved failure sentinel")
                    .unwrap()
            );

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
}
/// Capture explicit fixture cache fields with old, non-racy modification/check times.
fn capture_seed_file(repo: &ReadingRepo, path: &str, valid: bool, error: Option<&str>) -> FileRow {
    let full = repo.get_root().join(path);
    let file = File::options().write(true).open(&full).unwrap();
    file.set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(10)))
        .unwrap();
    let metadata = fs::metadata(&full).unwrap();
    FileRow {
        path: RepoPath::parse(path).unwrap(),
        size: metadata.len(),
        mtime_ns: metadata.mtime() * 1_000_000_000 + metadata.mtime_nsec(),
        inode: metadata.ino(),
        hash: Sha256::digest(&fs::read(full).unwrap()),
        checked_at_ns: 20_000_000_000,
        valid,
        error: error.map(str::to_owned),
    }
}

fn get_git_source(repo: &ReadingRepo, basis: &TreeOid) -> Vec<u8> {
    let mut command = Command::new("git");
    isolate_command(&mut command);
    let output = command
        .current_dir(repo.get_root())
        .args(["show", &format!("{}:source.txt", basis.as_str())])
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output.stdout
}
