mod support;

use letopis::{
    domain::{LineRange, Sha256, SourceUri},
    open_storage,
    service::{
        DocumentErrorKind, Refresher, SourceAvailability, SourceReader, Storage, StorageRead,
    },
    wire_document_refresher, wire_source_reader,
};
use std::{fs, os::unix::fs::symlink};
use support::documents::{DocumentRepo, build_external};

#[test]
fn observe_live_source_without_referrer_edits() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", "<!-- letopis: {\"created_at\":3,\"source_snapshot\":\"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb\"} -->\nReferrerToken [source](source.txt \"letopis:type=external\")\n");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let before = repo.read_state();
    let reader = wire_source_reader(repo.get_root()).unwrap();
    let mut storage = open_repo_storage(&repo);
    let vector_identity =
        Sha256::parse("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa").unwrap();
    storage.reset_vectors(None, &vector_identity).unwrap();
    let mut view = storage.begin_read().unwrap();
    let uri = SourceUri::parse("source.txt").unwrap();
    let revision = view.get_revision();
    let identity = view.get_config_identity();

    // When
    let first = reader.read_source(&mut view, &uri).unwrap().unwrap();
    // Then
    assert_eq!(first.availability, SourceAvailability::Available);
    assert_eq!(
        first.metadata.hash.unwrap().to_hex(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    // Given
    repo.write("source.txt", b"def");
    // When
    let second = reader.read_source(&mut view, &uri).unwrap().unwrap();
    // Then
    assert_eq!(
        second.metadata.hash.unwrap().to_hex(),
        "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
    );
    // Given
    repo.remove("source.txt");
    // When
    let missing = reader.read_source(&mut view, &uri).unwrap().unwrap();
    // Then
    assert_eq!(missing.availability, SourceAvailability::SourceNotFound);
    assert_eq!(missing.metadata.hash, None);
    // Given
    repo.write("source.txt", b"abc");
    // When / Then
    assert_eq!(reader.read_source(&mut view, &uri).unwrap(), Some(first));
    assert_eq!(view.get_revision(), revision);
    assert_eq!(view.get_config_identity(), identity);
    assert_eq!(view.list_nodes().unwrap(), before.nodes);
    drop(view);
    assert_eq!(repo.read_state(), before);
}

#[test]
fn retain_pinned_membership_while_observing_current_bytes() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "ReferrerToken"));
    let mut refresher = wire_document_refresher(repo.get_root()).unwrap();
    refresher.refresh().unwrap();
    let reader = wire_source_reader(repo.get_root()).unwrap();
    let mut storage = open_repo_storage(&repo);
    let mut old_view = storage.begin_read().unwrap();
    let revision = old_view.get_revision();
    let uri = SourceUri::parse("source.txt").unwrap();
    repo.remove("a.md");
    refresher.refresh().unwrap();
    repo.write("source.txt", b"def");

    // When
    let old = reader.read_source(&mut old_view, &uri).unwrap().unwrap();
    let mut current = open_repo_storage(&repo);
    let mut new_view = current.begin_read().unwrap();

    // Then
    assert_eq!(old.metadata.created_at, 3);
    assert_eq!(
        old.metadata.hash.unwrap().to_hex(),
        "cb8379ac2098aa165029e3938a51da0bcecfc008fd6795f401178647f96c5b34"
    );
    assert_eq!(old_view.get_revision(), revision);
    assert!(new_view.get_revision() > revision);
    assert!(reader.read_source(&mut new_view, &uri).unwrap().is_none());
}

#[test]
fn distinguish_missing_file_and_incomplete_range_from_available_range() {
    // Given
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
        .refresh()
        .unwrap();
    let reader = wire_source_reader(repo.get_root()).unwrap();
    let mut storage = open_repo_storage(&repo);
    let mut view = storage.begin_read().unwrap();

    // When
    let range = reader
        .read_source(&mut view, &SourceUri::parse("source.txt#L2-L2").unwrap())
        .unwrap()
        .unwrap();
    // Then
    assert_eq!(range.availability, SourceAvailability::Available);
    assert_eq!(range.metadata.lines, Some(LineRange::new(2, 2).unwrap()));
    assert_eq!(
        range.metadata.hash.unwrap().to_hex(),
        "27dd8ed44a83ff94d557f9fd0412ed5a8cbca69ea04922d88c01184a07300a5a"
    );
    for address in ["source.txt#L2-L3", "missing.txt"] {
        // When
        let observed = reader
            .read_source(&mut view, &SourceUri::parse(address).unwrap())
            .unwrap()
            .unwrap();
        // Then
        assert_eq!(observed.availability, SourceAvailability::SourceNotFound);
        assert_eq!(observed.metadata.hash, None);
    }
}

#[test]
fn reject_symlink_and_nonregular_sources_through_confined_filesystem() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "Token"));
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let reader = wire_source_reader(repo.get_root()).unwrap();
    let mut storage = open_repo_storage(&repo);
    let mut view = storage.begin_read().unwrap();
    let uri = SourceUri::parse("source.txt").unwrap();
    repo.remove("source.txt");
    symlink("a.md", repo.get_root().join("source.txt")).unwrap();

    // When / Then
    assert_eq!(
        reader.read_source(&mut view, &uri).unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
    // Given
    repo.remove("source.txt");
    fs::create_dir(repo.get_root().join("source.txt")).unwrap();
    // When / Then
    assert_eq!(
        reader.read_source(&mut view, &uri).unwrap_err().get_kind(),
        DocumentErrorKind::InvalidRequest
    );
}

#[test]
fn propagate_failed_storage_read_before_source_capture() {
    // Given
    let repo = DocumentRepo::new();
    repo.write("source.txt", b"abc");
    repo.write("a.md", build_external(3, "source.txt", "Token"));
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let mut storage = open_repo_storage(&repo);
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    connection
        .pragma_update(None, "foreign_keys", false)
        .unwrap();
    connection.execute_batch("DROP TABLE nodes").unwrap();
    let mut view = storage.begin_read().unwrap();
    let reader = wire_source_reader(repo.get_root()).unwrap();

    // When
    let error = reader
        .read_source(&mut view, &SourceUri::parse("source.txt").unwrap())
        .unwrap_err();

    // Then
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!error.to_string().contains("nodes"));
    assert!(!format!("{error:?}").contains("DROP TABLE"));
}

#[test]
fn construct_reader_without_discovery_or_service_state() {
    // Given
    let root = tempfile::tempdir().unwrap();

    let _reader = wire_source_reader(root.path()).unwrap();

    // When / Then
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
}

fn open_repo_storage(repo: &DocumentRepo) -> impl Storage + use<> {
    open_storage(
        &repo.get_root().join(".letopis/cache.db"),
        &repo.get_root().join(".letopis/vectors.db"),
    )
    .unwrap()
}
