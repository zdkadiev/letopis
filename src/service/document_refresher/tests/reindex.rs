use super::{INITIAL, build_service, read_matches};
use crate::service::{Refresher, Storage, StorageRead};
use rusqlite::Connection;

#[test]
fn reindex_rebuilds_unchanged_fts_and_advances_once() {
    // Given: FTS is deliberately missing while file fingerprints are unchanged.
    let (dir, mut service) = build_service();
    service.refresh().unwrap();
    let db = Connection::open(dir.path().join("cache.db")).unwrap();
    db.execute("DELETE FROM text_index", []).unwrap();
    assert!(!service.refresh().unwrap().published);
    assert!(read_matches(&mut service, "AlphaToken").is_empty());
    // When.
    let rebuilt = service.reindex().unwrap();
    // Then: this must not be an incremental no-op.
    assert_eq!(
        (rebuilt.revision, rebuilt.published, rebuilt.files_updated),
        (2, true, 1)
    );
    assert_eq!(read_matches(&mut service, "AlphaToken"), ["note.md"]);
    assert_eq!(service.reindex().unwrap().revision, 3);
}

#[test]
fn reindex_reparses_identical_valid_unmarked_and_invalid_bytes() {
    for bytes in [
        INITIAL,
        &b"ordinary markdown\n"[..],
        &b"<!-- letopis: invalid -->"[..],
    ] {
        // Given.
        let (_dir, mut service) = build_service();
        service.filesystem.bytes.replace(bytes.to_vec());
        service.filesystem.stat.update(|mut stat| {
            stat.size = bytes.len() as u64;
            stat
        });
        service.refresh().unwrap();
        let parses = service.markdown.0.get();
        service.filesystem.reset_counts();
        // When.
        let outcome = service.reindex().unwrap();
        // Then.
        assert_eq!(outcome.revision, 2);
        assert!(service.markdown.0.get() > parses);
        assert_eq!(service.filesystem.locks.get(), 1);
        assert_eq!(outcome.files_updated, 1);
    }
}

#[test]
fn reindex_capture_failure_preserves_complete_publication() {
    // Given: fail the publication capture after detection and preflight.
    let (_dir, mut service) = build_service();
    service.refresh().unwrap();
    let before = service.storage.begin_read().unwrap().list_files().unwrap();
    service.filesystem.reset_counts();
    service.filesystem.fail_on_read.set(Some(3));
    // When.
    assert!(service.reindex().is_err());
    // Then.
    let mut read = service.storage.begin_read().unwrap();
    assert_eq!(read.get_revision(), 1);
    assert_eq!(read.list_files().unwrap(), before);
    assert_eq!(
        read.match_text("AlphaToken").unwrap()[0].id.as_str(),
        "note.md"
    );
}
