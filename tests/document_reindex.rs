mod support;
use letopis::{
    domain::Sha256,
    service::{NodeMetadata, Refresher},
    wire_document_refresher,
};
use std::{fs, os::unix::fs::symlink};
use support::documents::{DocumentRepo, build_external, build_whole};

#[test]
fn reindex_preserves_invalid_owner_and_publishes_other_file() {
    // Given.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "source bytes");
    repo.write("a.md", build_external(1, "source.txt", "AlphaToken"));
    repo.write("b.md", build_whole(2, "BravoToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh().unwrap();
    let before = repo.read_state();
    repo.write("a.md", "<!-- letopis: SECRET_INVALID -->\nBadToken");
    repo.write("b.md", build_whole(2, "CharlieToken"));
    // When.
    let outcome = service.reindex().unwrap();
    // Then.
    assert_eq!((outcome.revision, outcome.invalid_files_updated), (2, 1));
    let after = repo.read_state();
    assert_eq!(after.get_node("a.md"), before.get_node("a.md"));
    assert_eq!(after.edges, before.edges);
    assert!(!after.get_file("a.md").valid);
    assert!(
        !after
            .get_file("a.md")
            .error
            .as_ref()
            .unwrap()
            .contains("SECRET_INVALID")
    );
    assert_eq!(repo.search("AlphaToken"), ["a.md"]);
    assert_eq!(repo.search("CharlieToken"), ["b.md"]);
    assert!(repo.search("BravoToken OR BadToken").is_empty());
    // A subsequent valid change must not be hidden by the invalid owner's fingerprint.
    repo.write("b.md", build_whole(2, "DeltaToken"));
    assert_eq!(service.refresh().unwrap().revision, 3);
    assert_eq!(repo.search("DeltaToken"), ["b.md"]);
}

#[test]
fn reindex_initializes_empty_and_invalid_corpus_without_inventing_history() {
    for invalid in [false, true] {
        // Given.
        let repo = DocumentRepo::new();
        if invalid {
            repo.write("a.md", "<!-- letopis: invalid -->");
            repo.write("b.md", build_whole(2, "BravoToken"));
        }
        let mut service = wire_document_refresher(repo.get_root()).unwrap();
        // When.
        let result = service.reindex().unwrap();
        // Then.
        assert_eq!(result.revision, 1);
        let state = repo.read_state();
        assert_eq!(state.nodes.len(), usize::from(invalid));
        assert_eq!(result.invalid_files_updated, usize::from(invalid));
        if invalid {
            assert!(!state.get_file("a.md").valid);
            assert_eq!(repo.search("BravoToken"), ["b.md"]);
        }
        assert!(!service.refresh().unwrap().published);
        assert_eq!(service.reindex().unwrap().revision, 2);
    }
}

#[test]
fn reindex_reconciles_unchanged_and_invalid_source_references() {
    // Given: only the source changes, so incremental refresh intentionally skips it.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "OldSource");
    repo.write("a.md", build_external(3, "source.txt", "AlphaToken"));
    repo.write("b.md", build_external(8, "source.txt", "BravoToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh().unwrap();
    repo.write("a.md", "<!-- letopis: invalid -->");
    service.refresh().unwrap();
    repo.write("b.md", "<!-- letopis: invalid -->");
    service.refresh().unwrap();
    repo.write("source.txt", "NewSource");
    // When.
    service.reindex().unwrap();
    // Then: even an invalid referrer contributes its old valid declaration.
    let state = repo.read_state();
    let NodeMetadata::Source(source) = &state.get_node("source.txt").metadata else {
        panic!()
    };
    assert_eq!(source.hash, Some(Sha256::digest(b"NewSource")));
    assert_eq!(source.created_at, 3);
    repo.remove("source.txt");
    service.reindex().unwrap();
    let missing = repo.read_state();
    let NodeMetadata::Source(source) = &missing.get_node("source.txt").metadata else {
        panic!()
    };
    assert_eq!(source.hash, None);
}

#[test]
fn reindex_removes_deleted_unmarked_and_orphaned_projections() {
    // Given.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "Source");
    repo.write("a.md", build_external(1, "source.txt", "AlphaToken"));
    repo.write("b.md", build_whole(2, "BravoToken"));
    repo.write("c.md", build_whole(3, "CharlieToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh().unwrap();
    repo.remove("a.md");
    repo.write("b.md", "ordinary Markdown");
    // When.
    let result = service.reindex().unwrap();
    // Then.
    assert_eq!((result.files_updated, result.files_removed), (2, 1));
    let state = repo.read_state();
    assert_eq!(state.nodes.len(), 1);
    assert!(state.edges.is_empty());
    assert!(state.get_file("b.md").valid);
    assert_eq!(repo.search("CharlieToken"), ["c.md"]);
    assert!(repo.search("AlphaToken OR BravoToken").is_empty());
}

#[test]
fn reindex_source_capture_error_rolls_back_every_file() {
    // Given: retain a source binding, then replace its source with an unsafe path.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "Source");
    repo.write("a.md", build_external(1, "source.txt", "AlphaToken"));
    repo.write("b.md", build_whole(2, "BravoToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh().unwrap();
    let before = repo.read_state();
    repo.remove("source.txt");
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("private.txt"), "SECRET_OUTSIDE").unwrap();
    symlink(
        outside.path().join("private.txt"),
        repo.get_root().join("source.txt"),
    )
    .unwrap();
    repo.write("b.md", build_whole(2, "ChangedToken"));
    // When.
    let error = service.reindex().unwrap_err();
    // Then.
    assert!(!format!("{error:?}").contains("SECRET_OUTSIDE"));
    assert_eq!(repo.read_state(), before);
    assert_eq!(repo.search("AlphaToken"), ["a.md"]);
    assert_eq!(repo.search("BravoToken"), ["b.md"]);
    assert!(repo.search("ChangedToken").is_empty());
}
