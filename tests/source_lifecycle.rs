mod support;

use letopis::{
    domain::{EdgeTarget, Sha256, SourceUri},
    open_storage,
    service::{
        DocumentErrorKind, EmbeddingFailure, NodeMetadata, Refresher, SourceMetadata, Storage,
        StorageRead, VectorOutcome, VectorWrite,
    },
    wire_document_refresher,
};
use rusqlite::{Connection, params};
use std::{fs, os::unix::fs::MetadataExt};
use support::documents::{DocumentRepo, State, build_external, build_whole};

const RANGE: &str = "source.txt#L2-L3";
const PRESERVED_PATHS: [&str; 6] = [
    "remote.md",
    ".git/HEAD",
    ".git/config",
    ".git/index",
    ".letopis/config.json",
    ".letopis/embed.lock",
];

#[test]
fn derive_minimum_from_effective_references() {
    // Arrange: equivalent spellings and duplicate links share one range identity.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\ntwo\nthree\n");
    let duplicate = build_external(3, "./source.txt#L02-L03", "EarlyToken");
    repo.write(
        "early.md",
        format!("{duplicate}[again]({RANGE} \"letopis:type=external\")\n"),
    );
    repo.write("late.md", build_external(8, RANGE, "LateToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    let first = repo.read_state();
    assert_eq!(get_source(&first, RANGE).uri.as_str(), "source.txt#L2-L3");
    assert_eq!(get_source(&first, RANGE).created_at, 3);
    assert_eq!(first.edges.len(), 2);

    // Act/Assert: removal recomputes time from the remaining effective edge.
    repo.remove("early.md");
    refresher.refresh().unwrap();
    assert_eq!(get_source(&repo.read_state(), RANGE).created_at, 8);
    repo.write("late.md", "<!-- letopis: {broken} -->\ninvalid\n");
    refresher.refresh().unwrap();
    let invalid = repo.read_state();
    assert!(!invalid.get_file("late.md").valid);
    assert_eq!(get_source(&invalid, RANGE).created_at, 8);
    assert_eq!(invalid.edges.len(), 1);
    assert_eq!(invalid.edges[0].get_source().as_str(), "late.md");
    assert_eq!(
        invalid.edges[0].get_target(),
        &EdgeTarget::Source(SourceUri::parse(RANGE).unwrap())
    );
    assert_eq!(repo.search("LateToken"), ["late.md"]);

    repo.remove("late.md");
    refresher.refresh().unwrap();
    assert!(repo.read_state().nodes.is_empty());
    repo.write("new.md", build_external(11, RANGE, "NewToken"));
    refresher.refresh().unwrap();
    assert_eq!(get_source(&repo.read_state(), RANGE).created_at, 11);
}

#[test]
fn recompute_minimum_after_earliest_referrer_edit() {
    let repo = DocumentRepo::new();
    repo.write("early.md", build_external(3, RANGE, "EarlyToken"));
    repo.write("late.md", build_external(8, RANGE, "LateToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();

    repo.write("early.md", build_whole(3, "EditedToken"));
    refresher.refresh().unwrap();

    let state = repo.read_state();
    assert_eq!(get_source(&state, RANGE).created_at, 8);
    assert_eq!(state.edges.len(), 1);
    assert_eq!(repo.search("EditedToken"), ["early.md"]);
}

#[test]
fn preserve_whole_and_range_nodes_when_source_file_disappears() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\ntwo\nthree\n");
    repo.write("whole.md", build_external(4, "source.txt", "WholeToken"));
    repo.write("range.md", build_external(8, RANGE, "RangeToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    let before = repo.read_state();
    assert_eq!(get_source(&before, "source.txt").created_at, 4);
    assert_eq!(get_source(&before, RANGE).created_at, 8);
    assert_eq!(before.nodes.len(), 4);

    // Touch referrers so projection observes the absent file too.
    repo.remove("source.txt");
    repo.write(
        "whole.md",
        build_external(4, "source.txt", "StillWholeToken"),
    );
    repo.write("range.md", build_external(8, RANGE, "StillRangeToken"));
    refresher.refresh().unwrap();

    let missing = repo.read_state();
    assert_eq!(missing.nodes.len(), 4);
    assert_eq!(get_source(&missing, "source.txt").hash, None);
    assert_eq!(get_source(&missing, RANGE).hash, None);
    assert_eq!(missing.edges, before.edges);
    assert_eq!(repo.search("StillWholeToken"), ["whole.md"]);
    assert_eq!(repo.search("StillRangeToken"), ["range.md"]);
}

#[test]
fn reject_source_knowledge_collision_without_publishing_or_changing_fts() {
    let repo = DocumentRepo::new();
    repo.write("knowledge.md", build_whole(1, "OriginalToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    let before = repo.read_state();
    repo.write(
        "referrer.md",
        build_external(2, "knowledge.md", "RejectedToken"),
    );

    let error = refresher.refresh().unwrap_err();

    assert_eq!(error.get_kind(), DocumentErrorKind::InvalidRequest);
    assert_eq!(repo.read_state(), before);
    assert_eq!(repo.search("OriginalToken"), ["knowledge.md"]);
    assert!(repo.search("RejectedToken").is_empty());
}

#[test]
fn rebuild_source_identity_from_canonical_files() {
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\ntwo\nthree\n");
    repo.write(
        "a.md",
        build_external(3, "./source.txt#L02-L03", "EarlyToken"),
    );
    repo.write("b.md", build_external(8, RANGE, "LaterToken"));
    repo.write("whole.md", build_external(5, "source.txt", "WholeToken"));
    repo.write(
        "remote.md",
        build_external(7, "https://Host/a", "RemoteToken"),
    );
    refresh_once(&repo);
    let before = collect_sources(&repo.read_state());
    let expected = vec![
        ("https://Host/a".to_owned(), 7),
        ("source.txt".to_owned(), 5),
        ("source.txt#L2-L3".to_owned(), 3),
    ];
    assert_eq!(before, expected);
    let vectors = fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap();

    // All connections/refresher/read views have left scope before removal.
    remove_fixture_cache(&repo);
    refresh_once(&repo);

    assert_eq!(collect_sources(&repo.read_state()), expected);
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap(),
        vectors
    );
    assert_eq!(repo.search("EarlyToken OR LaterToken"), ["a.md", "b.md"]);
}

#[test]
fn repair_legacy_url_only_after_explicit_cache_rebuild() {
    let repo = DocumentRepo::new();
    repo.write(
        "remote.md",
        build_external(7, "https://Host/a", "RemoteToken"),
    );
    repo.run_git(&["add", "remote.md"]);
    refresh_once(&repo);
    write_preserved_files(&repo);
    let canonical = read_preserved_files(&repo);
    let identity = Sha256::digest(b"existing vector configuration");
    let hash = Sha256::digest(b"durable vector");
    let failure_hash = Sha256::digest(b"attempt sentinel");
    seed_vector_sentinels(&repo, &identity, &hash, &failure_hash);
    seed_legacy_address(&repo);
    let vectors = fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap();

    // This explicit fixture-only upgrade is the compatibility boundary.
    let lock = read_index_lock(&repo);
    remove_fixture_cache(&repo);
    assert_eq!(read_index_lock(&repo), lock);
    refresh_once(&repo);
    assert_eq!(read_index_lock(&repo).0, lock.0);

    let state = repo.read_state();
    let remote = get_source(&state, "https://Host/a");
    assert_eq!(remote.uri.as_str(), "https://Host/a");
    assert_eq!(remote.created_at, 7);
    assert_eq!(collect_sources(&state), [("https://Host/a".to_owned(), 7)]);
    assert_eq!(read_preserved_files(&repo), canonical);
    assert_eq!(
        fs::read(repo.get_root().join(".letopis/vectors.db")).unwrap(),
        vectors
    );
    let mut storage = open_fixture_storage(&repo);
    let mut view = storage.begin_read().unwrap();
    let preserved_vector = unwrap_current(view.read_vector(&identity, &hash).unwrap());
    assert_eq!(preserved_vector, Some(vec![1.0, 0.0, 0.0]));
    assert_eq!(
        unwrap_current(view.read_failure(&identity, &failure_hash).unwrap()),
        Some(EmbeddingFailure {
            search_hash: failure_hash,
            attempts: 1,
            error: Some("preserved failure sentinel".to_owned()),
        })
    );
}

fn get_source<'a>(state: &'a State, address: &str) -> &'a SourceMetadata {
    let NodeMetadata::Source(source) = &state.get_node(address).metadata else {
        panic!("expected source metadata")
    };
    source
}

fn collect_sources(state: &State) -> Vec<(String, u64)> {
    let mut sources: Vec<_> = state
        .nodes
        .iter()
        .filter_map(|node| match &node.metadata {
            NodeMetadata::Source(source) => {
                Some((source.uri.as_str().to_owned(), source.created_at))
            }
            NodeMetadata::Knowledge(_) => None,
        })
        .collect();
    sources.sort();
    sources
}

fn refresh_once(repo: &DocumentRepo) {
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
}

fn remove_fixture_cache(repo: &DocumentRepo) {
    for name in [
        "cache.db",
        "cache.db-wal",
        "cache.db-shm",
        "cache.db-journal",
    ] {
        let path = repo.get_root().join(".letopis").join(name);
        if path.exists() {
            fs::remove_file(path).unwrap();
        }
    }
}

fn write_preserved_files(repo: &DocumentRepo) {
    repo.write(".letopis/config.json", b"{\"fixture\":\"preserve\"}\n");
    repo.write(".letopis/index.lock", b"fixture-index-lock\n");
    repo.write(".letopis/embed.lock", b"fixture-embed-lock\n");
}

fn read_preserved_files(repo: &DocumentRepo) -> Vec<Vec<u8>> {
    PRESERVED_PATHS
        .iter()
        .map(|path| fs::read(repo.get_root().join(path)).unwrap())
        .collect()
}

fn seed_vector_sentinels(
    repo: &DocumentRepo,
    identity: &Sha256,
    hash: &Sha256,
    failure_hash: &Sha256,
) {
    let mut storage = open_fixture_storage(repo);
    storage.reset_vectors(identity).unwrap();
    let mut unit = unwrap_current(storage.begin_vector_write(identity).unwrap());
    unit.store_vector(hash, &[1.0, 0.0, 0.0], 3).unwrap();
    assert!(unit.reserve_attempt(failure_hash, 3).unwrap());
    assert!(
        unit.record_failure(failure_hash, "preserved failure sentinel")
            .unwrap()
    );
    unit.commit().unwrap();
}

fn open_fixture_storage(repo: &DocumentRepo) -> impl Storage + use<> {
    open_storage(
        &repo.get_root().join(".letopis/cache.db"),
        &repo.get_root().join(".letopis/vectors.db"),
    )
    .unwrap()
}

fn unwrap_current<T>(outcome: VectorOutcome<T>) -> T {
    let VectorOutcome::Current(value) = outcome else {
        panic!("unexpected vector identity change")
    };
    value
}

fn seed_legacy_address(repo: &DocumentRepo) {
    let mut connection = Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "UPDATE nodes SET id=?1, uri=?1 WHERE id=?2",
            params!["https:/Host/a", "https://Host/a"],
        )
        .unwrap();
    transaction
        .execute(
            "UPDATE edges SET target_id=?1 WHERE target_id=?2",
            params!["https:/Host/a", "https://Host/a"],
        )
        .unwrap();
    transaction.commit().unwrap();
    let cached: String = connection
        .query_row("SELECT id FROM nodes WHERE kind=?1", ["source"], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(cached, "https:/Host/a");
}

fn read_index_lock(repo: &DocumentRepo) -> (u64, Vec<u8>) {
    let path = repo.get_root().join(".letopis/index.lock");
    (fs::metadata(&path).unwrap().ino(), fs::read(path).unwrap())
}
