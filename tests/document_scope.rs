mod support;

use letopis::{
    domain::RepoPath,
    service::{NodeMetadata, RefreshOutcome, Refresher},
    wire_document_refresher,
};
use rusqlite::Connection;
use std::fs;
use support::documents::{DocumentRepo, build_external, build_whole};

#[test]
fn scoped_refresh_preserves_outside_owners() {
    // Given: two indexed directories and one subsequently removed outside owner.
    let repo = DocumentRepo::new();
    fs::create_dir_all(repo.get_root().join("notes")).unwrap();
    fs::create_dir_all(repo.get_root().join("other")).unwrap();
    repo.write("notes/a.md", build_whole(1, "OriginalInsideToken"));
    repo.write("other/b.md", build_whole(2, "OriginalOutsideToken"));
    repo.write("other/deleted.md", build_whole(3, "DeletedOutsideToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.write("notes/a.md", build_whole(1, "ChangedInsideToken"));
    repo.write("other/b.md", build_whole(2, "ChangedOutsideToken"));
    repo.remove("other/deleted.md");
    let paths = [RepoPath::parse("notes").unwrap()];

    // When: only the notes directory is refreshed.
    let outcome = service.refresh(&paths).unwrap();

    // Then: only the selected owner changes, and the other directory stays indexed.
    assert_eq!(
        outcome,
        RefreshOutcome {
            revision: 2,
            is_published: true,
            files_updated: 1,
            files_removed: 0,
            invalid_files_updated: 0,
        }
    );
    let after = repo.get_state();
    assert_eq!(after.get_file("other/b.md"), before.get_file("other/b.md"));
    assert_eq!(after.get_node("other/b.md"), before.get_node("other/b.md"));
    assert_eq!(
        after.get_file("other/deleted.md"),
        before.get_file("other/deleted.md")
    );
    assert_eq!(repo.search("OriginalOutsideToken"), ["other/b.md"]);
    assert_eq!(repo.search("DeletedOutsideToken"), ["other/deleted.md"]);
    assert_eq!(repo.search("ChangedInsideToken"), ["notes/a.md"]);

    // When: the same scope is refreshed without further edits.
    let unchanged = service.refresh(&paths).unwrap();

    // Then: there is no publication or revision change.
    assert_eq!(
        unchanged,
        RefreshOutcome {
            revision: 2,
            is_published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        }
    );
}

#[test]
fn scoped_initial_publication_excludes_prefix_siblings() {
    // Given: two directories whose names share a byte prefix.
    let repo = DocumentRepo::new();
    fs::create_dir_all(repo.get_root().join("notes/a")).unwrap();
    fs::create_dir_all(repo.get_root().join("notes/ab")).unwrap();
    repo.write("notes/a/one.md", build_whole(1, "SelectedToken"));
    repo.write("notes/ab/two.md", build_whole(2, "SiblingToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    let paths = [RepoPath::parse("notes/a").unwrap()];

    // When: the first publication selects notes/a.
    let outcome = service.refresh(&paths).unwrap();

    // Then: the sibling is left for a subsequent unscoped refresh.
    assert_eq!((outcome.revision, outcome.files_updated), (1, 1));
    assert_eq!(repo.search("SelectedToken"), ["notes/a/one.md"]);
    assert!(repo.search("SiblingToken").is_empty());
    let outcome = service.refresh(&[]).unwrap();
    assert_eq!((outcome.revision, outcome.files_updated), (2, 1));
    assert_eq!(repo.search("SiblingToken"), ["notes/ab/two.md"]);
}

#[test]
fn scoped_full_reparses_selected_file_and_leaves_other_file_untouched() {
    // Given: a published corpus with two marked owners.
    let repo = DocumentRepo::new();
    repo.write("selected.md", build_whole(1, "SelectedToken"));
    repo.write("outside.md", build_whole(2, "OutsideToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.write("outside.md", build_whole(2, "ChangedOutsideToken"));
    let selected = [RepoPath::parse("selected.md").unwrap()];

    // When: full mode runs twice for only the selected owner.
    let first = service.reindex(&selected).unwrap();
    let second = service.reindex(&selected).unwrap();

    // Then: both forced publications reparse one owner, retaining the outside row.
    assert_eq!((first.revision, first.files_updated), (2, 1));
    assert_eq!((second.revision, second.files_updated), (3, 1));
    assert!(first.is_published && second.is_published);
    assert_eq!(
        repo.get_state().get_file("outside.md"),
        before.get_file("outside.md")
    );
    assert_eq!(repo.search("OutsideToken"), ["outside.md"]);
    assert!(repo.search("ChangedOutsideToken").is_empty());
}

#[test]
fn unmatched_scoped_refresh_does_not_remove_cached_owners() {
    // Given: a cached owner and a valid selector that matches no candidates.
    let repo = DocumentRepo::new();
    repo.write("owner.md", build_whole(1, "KnownToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    repo.remove("owner.md");
    let paths = [RepoPath::parse("unmatched").unwrap()];

    // When: an incremental index is scoped to the unmatched path.
    let outcome = service.refresh(&paths).unwrap();

    // Then: the cached owner remains, and the revision does not advance.
    assert_eq!(
        (
            outcome.revision,
            outcome.is_published,
            outcome.files_removed
        ),
        (1, false, 0)
    );
    assert_eq!(repo.search("KnownToken"), ["owner.md"]);
}

#[test]
fn scoped_full_does_not_reconcile_unselected_owner_source() {
    // Given: a source declared only by an outside owner and a separate selected owner.
    let repo = DocumentRepo::new();
    repo.write("outside.md", build_external(1, "source.txt", "Outside"));
    repo.write("selected.md", build_whole(2, "Selected"));
    repo.write("source.txt", "original source\n");
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    let before = repo.get_state();
    repo.write("source.txt", "changed source\n");
    let selected = [RepoPath::parse("selected.md").unwrap()];

    // When: full mode rebuilds the selected owner only.
    let outcome = service.reindex(&selected).unwrap();

    // Then: the unrelated source retains its previous derived metadata.
    assert_eq!(outcome.files_updated, 1);
    assert_eq!(
        repo.get_state().get_node("source.txt"),
        before.get_node("source.txt")
    );
}

#[test]
fn scoped_refresh_removes_only_selected_missing_owner() {
    // Given: two cached owners that are both removed from the worktree.
    let repo = DocumentRepo::new();
    repo.write("selected.md", build_whole(1, "SelectedToken"));
    repo.write("outside.md", build_whole(2, "OutsideToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    repo.remove("selected.md");
    repo.remove("outside.md");

    // When: the missing selected owner is refreshed.
    let outcome = service
        .refresh(&[RepoPath::parse("selected.md").unwrap()])
        .unwrap();

    // Then: its removal is published while the outside owner is retained.
    assert_eq!((outcome.revision, outcome.files_removed), (2, 1));
    assert!(repo.search("SelectedToken").is_empty());
    assert_eq!(repo.search("OutsideToken"), ["outside.md"]);
}

#[test]
fn scoped_full_repairs_only_selected_full_text_entries() {
    // Given: a published corpus whose FTS entries were removed independently.
    let repo = DocumentRepo::new();
    repo.write("selected.md", build_whole(1, "SelectedToken"));
    repo.write("outside.md", build_whole(2, "OutsideToken"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    Connection::open(repo.get_root().join(".letopis/cache.db"))
        .unwrap()
        .execute("delete from text_index", [])
        .unwrap();

    // When: forced full indexing selects only one owner.
    let outcome = service
        .reindex(&[RepoPath::parse("selected.md").unwrap()])
        .unwrap();

    // Then: the selected entry returns without rewriting the other owner’s FTS row.
    assert_eq!((outcome.revision, outcome.files_updated), (2, 1));
    assert_eq!(repo.search("SelectedToken"), ["selected.md"]);
    assert!(repo.search("OutsideToken").is_empty());
}

#[test]
fn scoped_full_reconciles_shared_source_using_retained_outside_owner() {
    // Given: two owners reference a shared source, with the outside owner setting its minimum age.
    let repo = DocumentRepo::new();
    repo.write("source.txt", "source bytes");
    repo.write("outside.md", build_external(3, "source.txt", "Outside"));
    repo.write("selected.md", build_external(8, "source.txt", "Selected"));
    let mut service = wire_document_refresher(repo.get_root()).unwrap();
    service.refresh(&[]).unwrap();
    repo.write("selected.md", build_external(10, "source.txt", "Changed"));

    // When: full mode rebuilds only the selected owner.
    let outcome = service
        .reindex(&[RepoPath::parse("selected.md").unwrap()])
        .unwrap();

    // Then: the shared source still reflects the retained outside owner.
    assert_eq!((outcome.revision, outcome.files_updated), (2, 1));
    let state = repo.get_state();
    let NodeMetadata::Source(source) = &state.get_node("source.txt").metadata else {
        panic!("expected source metadata")
    };
    assert_eq!(source.created_at, 3);
    assert_eq!(repo.search("Outside"), ["outside.md"]);
    assert_eq!(repo.search("Changed"), ["selected.md"]);
}
