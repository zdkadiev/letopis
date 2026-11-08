use letopis::service::VectorDatabase;
use support::storage::{Databases, open_storage};
mod support;

use letopis::{
    domain::{LineRange, Sha256, SourceUri},
    open_cache_storage,
    service::{
        CacheDatabase, DocumentErrorKind, Refresher, SourceAvailability, SourceReader, StorageRead,
        StorageResult,
    },
    wire_document_refresher, wire_source_reader,
};
use std::{fs, os::unix::fs::symlink, rc::Rc};
use support::documents::{DocumentRepo, build_external};

#[test]
fn observe_live_source_without_referrer_edits() {
    // Given: live source bytes and unchanged referrers.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", "<!-- letopis: {\"created_at\":3,\"source_snapshot\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"} -->\nReferrerToken [source](source.txt \"letopis:type=external;path=source.txt\")\n");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let before = repo.get_state();
    let storage = open_repo_storage(&repo);
    let vector_identity =
        Sha256::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    storage
        .vectors
        .reset_vectors(None, &vector_identity)
        .unwrap();
    let cache = open_repo_cache(&repo);
    let reader = wire_source_reader(repo.get_root(), Rc::clone(&cache)).unwrap();
    cache
        .with_read_tx(|| -> StorageResult<()> {
            let view = cache.as_ref();
            let uri = SourceUri::parse("source.txt").unwrap();
            let revision = view.project_get_by_id().unwrap();
            let identity = view.meta_get_by_id().unwrap();

            // When: the source is observed.
            let first = reader.get_source_by_uri(&uri).unwrap().unwrap();
            // Then: current bytes are visible without referrer edits.
            assert_eq!(first.availability, SourceAvailability::Available);
            assert_eq!(
                first.metadata.hash.unwrap().to_hex(),
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
            );
            // Given: the live source bytes change while referrers remain untouched.
            repo.write("source.txt", b"def");
            // When: the source is observed.
            let second = reader.get_source_by_uri(&uri).unwrap().unwrap();
            // Then: current bytes are visible without referrer edits.
            assert_eq!(
                second.metadata.hash.unwrap().to_hex(),
                "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
            );
            // Given: the declared source file is removed.
            repo.remove("source.txt");
            // When: the source is observed.
            let missing = reader.get_source_by_uri(&uri).unwrap().unwrap();
            // Then: the missing source has no observed content hash.
            assert_eq!(missing.availability, SourceAvailability::SourceNotFound);
            assert_eq!(missing.metadata.hash, None);
            // Given: the original source bytes are restored.
            repo.write("source.txt", b"abc");
            // When / Then: the source is observed; current bytes are visible without referrer edits.
            assert_eq!(reader.get_source_by_uri(&uri).unwrap(), Some(first));
            assert_eq!(view.project_get_by_id().unwrap(), revision);
            assert_eq!(view.meta_get_by_id().unwrap(), identity);
            assert_eq!(view.nodes_get().unwrap(), before.nodes);
            Ok(())
        })
        .unwrap();
    assert_eq!(repo.get_state(), before);
}

#[test]
fn retain_pinned_membership_while_observing_current_bytes() {
    // Given: a pinned source membership view.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "ReferrerToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh(&[]).unwrap();
    let cache = open_repo_cache(&repo);
    let reader = wire_source_reader(repo.get_root(), Rc::clone(&cache)).unwrap();
    cache
        .with_read_tx(|| -> StorageResult<()> {
            let old_view = cache.as_ref();
            let revision = old_view.project_get_by_id().unwrap();
            let uri = SourceUri::parse("source.txt").unwrap();
            repo.remove("a.md");
            refresher.refresh(&[]).unwrap();
            repo.write("source.txt", b"def");

            // When: source bytes change after pinning.
            let old = reader.get_source_by_uri(&uri).unwrap().unwrap();

            // Then: membership stays pinned while current bytes are observed.
            assert_eq!(old.metadata.created_at, 3);
            assert_eq!(
                old.metadata.hash.unwrap().to_hex(),
                "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
            );
            assert_eq!(old_view.project_get_by_id().unwrap(), revision);
            Ok(())
        })
        .unwrap();
    assert_eq!(cache.with_read_tx(|| cache.project_get_by_id()).unwrap(), 2);
    assert!(
        reader
            .get_source_by_uri(&SourceUri::parse("source.txt").unwrap())
            .unwrap()
            .is_none()
    );
}

#[test]
fn distinguish_missing_file_and_incomplete_range_from_available_range() {
    // Given: missing files and complete or incomplete line ranges.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"one\r\ntwo\n");
    repo.write(
        "range.md",
        build_external(3, "source.txt#L2-L2", "RangeToken"),
    );
    repo.write(
        "short.md",
        build_external(4, "source.txt#L2-L3", "ShortToken"),
    );
    repo.write(
        "missing.md",
        build_external(5, "missing.txt", "MissingToken"),
    );
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let cache = open_repo_cache(&repo);
    let reader = wire_source_reader(repo.get_root(), Rc::clone(&cache)).unwrap();

    // When: each declared source range is observed.
    let range = reader
        .get_source_by_uri(&SourceUri::parse("source.txt#L2-L2").unwrap())
        .unwrap()
        .unwrap();
    // Then: missing, incomplete and available ranges remain distinct.
    assert_eq!(range.availability, SourceAvailability::Available);
    assert_eq!(range.metadata.lines, Some(LineRange::new(2, 2).unwrap()));
    assert_eq!(
        range.metadata.hash.unwrap().to_hex(),
        "27dd8ed44a83ff94d557f9fd0412ed5a8cbca69ea04922d88c01184a07300a5a"
    );
    for address in ["source.txt#L2-L3", "missing.txt"] {
        // When: each declared source range is observed.
        let observed = reader
            .get_source_by_uri(&SourceUri::parse(address).unwrap())
            .unwrap()
            .unwrap();
        // Then: missing, incomplete and available ranges remain distinct.
        assert_eq!(observed.availability, SourceAvailability::SourceNotFound);
        assert_eq!(observed.metadata.hash, None);
    }
}

#[test]
fn nonregular_sources_are_reported_as_missing() {
    // Given: symlink and nonregular source entries.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "Token"));
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let cache = open_repo_cache(&repo);
    let reader = wire_source_reader(repo.get_root(), Rc::clone(&cache)).unwrap();
    let uri = SourceUri::parse("source.txt").unwrap();
    repo.remove("source.txt");
    symlink("a.md", repo.get_root().join("source.txt")).unwrap();

    // When / Then: the source reader reports the nonregular entry as absent.
    assert_eq!(
        reader
            .get_source_by_uri(&uri)
            .unwrap()
            .unwrap()
            .availability,
        SourceAvailability::SourceNotFound
    );
    // Given: symlink and nonregular source entries.
    repo.remove("source.txt");
    fs::create_dir(repo.get_root().join("source.txt")).unwrap();
    // When / Then: the source reader reports the nonregular entry as absent.
    assert_eq!(
        reader
            .get_source_by_uri(&uri)
            .unwrap()
            .unwrap()
            .availability,
        SourceAvailability::SourceNotFound
    );
}

#[test]
fn propagate_failed_storage_read_before_source_capture() {
    // Given: storage that fails before source retrieval.
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "Token"));
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh(&[])
        .unwrap();
    let cache = open_repo_cache(&repo);
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection.execute_batch("drop table nodes").unwrap();
    let reader = wire_source_reader(repo.get_root(), Rc::clone(&cache)).unwrap();

    // When: the source reader requests stored metadata.
    let error = reader
        .get_source_by_uri(&SourceUri::parse("source.txt").unwrap())
        .unwrap_err();

    // Then: the storage error propagates before filesystem capture.
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!error.to_string().contains("nodes"));
    assert!(!format!("{error:?}").contains("drop table"));
}

#[test]
fn construct_reader_without_discovery_or_service_state() {
    // Given: source wiring receives a database located outside its target root.
    let root = tempfile::tempdir().unwrap();
    let database = tempfile::tempdir().unwrap();
    let cache = Rc::new(
        open_cache_storage(
            &database.path().join("cache.db"),
            &database.path().join("vectors.db"),
        )
        .unwrap(),
    );
    // When: the reader is constructed for a root with no Git or service state.
    let _reader = wire_source_reader(root.path(), cache).unwrap();
    // Then: constructing the reader leaves the target root empty.
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

fn open_repo_storage(
    repo: &DocumentRepo,
) -> Databases<impl CacheDatabase + use<>, impl VectorDatabase + use<>> {
    open_storage(
        &repo.get_root().join(".letopis/cache.db"),
        &repo.get_root().join(".letopis/vectors.db"),
    )
    .unwrap()
}

fn open_repo_cache(repo: &DocumentRepo) -> Rc<impl CacheDatabase + use<>> {
    Rc::new(
        open_cache_storage(
            &repo.get_root().join(".letopis/cache.db"),
            &repo.get_root().join(".letopis/vectors.db"),
        )
        .unwrap(),
    )
}
