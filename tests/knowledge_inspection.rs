mod support;
use letopis::{
    domain::{LineRange, RepoPath, Sha256},
    open_git_snapshots,
    service::{CommandErrorKind, ElementAddress, GitSnapshots, Inspector, Refresher},
    wire_document_refresher, wire_inspector,
};
use support::reading::ReadingRepo;

#[test]
fn inspect_reads_current_knowledge_from_one_retained_tree() {
    // Given: fixed codec goldens exercise exact bytes and section boundaries.
    let repo = ReadingRepo::new();
    let fixtures = build_fixtures();
    for (path, bytes, ..) in fixtures {
        repo.write(path, bytes);
    }
    let mut service = wire_inspector(repo.get_root()).unwrap();
    let addresses = fixtures
        .iter()
        .map(|(_, _, id, ..)| ElementAddress::parse(id).unwrap())
        .collect::<Vec<_>>();
    // When
    let result = service.inspect(&addresses).unwrap();
    let git = open_git_snapshots(repo.get_root()).unwrap();
    // Then
    assert_eq!(result.elements.len(), 4);
    for (path, bytes, id, start, end, hash) in fixtures {
        assert_eq!(result.elements[id].path.as_str(), path);
        assert_eq!(
            result.elements[id].lines,
            Some(LineRange::new(start, end).unwrap())
        );
        assert_eq!(result.elements[id].hash, Some(Sha256::parse(hash).unwrap()));
        repo.write(path, b"live edit");
        assert_eq!(
            git.blob_get_by_tree_oid_and_path(&result.snapshot, &RepoPath::parse(path).unwrap())
                .unwrap(),
            bytes
        );
    }
    let status = std::process::Command::new("git")
        .current_dir(repo.get_root())
        .args(["gc", "--prune=now"])
        .status()
        .unwrap();
    assert!(status.success());
    assert_eq!(
        git.blob_get_by_tree_oid_and_path(&result.snapshot, &RepoPath::parse("crlf.md").unwrap())
            .unwrap(),
        fixtures[0].1
    );
}

#[test]
fn inspect_deduplicates_cross_file_ids_and_rejects_unknown_batch() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("a.md", b"<!-- letopis: {\"created_at\":1} -->\nalpha\n");
    repo.write("b.md", include_bytes!("fixtures/markdown/sections.md"));
    let mut service = wire_inspector(repo.get_root()).unwrap();
    // When
    let result = service
        .inspect(&["a.md", "./a.md", "b.md#a"].map(|id| ElementAddress::parse(id).unwrap()))
        .unwrap();
    // Then
    assert_eq!(result.elements.len(), 2);
    let error = service
        .inspect(&["a.md", "missing.md"].map(|id| ElementAddress::parse(id).unwrap()))
        .unwrap_err();
    assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
}

#[test]
fn inspect_rejects_invalid_current_file_instead_of_using_old_sql() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("a.md", b"<!-- letopis: {\"created_at\":1} -->\nold\n");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.write("a.md", b"<!-- letopis: {invalid} -->\ncurrent\n");
    // When
    let error = wire_inspector(repo.get_root())
        .unwrap()
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::ValidationFailed);
}

#[test]
fn inspect_distinguishes_referenced_sources_from_unknown_ids() {
    // Given
    let repo = ReadingRepo::new();
    repo.write("a.md", b"<!-- letopis: {\"created_at\":1} -->\n[source](src/data.rs \"letopis:type=external;path=src/data.rs\") [other](plain.md \"letopis:type=external;path=plain.md\")\n");
    repo.write("plain.md", b"unmarked Markdown");
    let mut service = wire_inspector(repo.get_root()).unwrap();
    for id in ["src/data.rs", "plain.md"] {
        // When
        let inspected = service
            .inspect(&[ElementAddress::parse(id).unwrap()])
            .unwrap();
        // Then
        assert_eq!(inspected.elements[id].path.as_str(), id);
        assert_eq!(inspected.elements[id].hash.is_some(), id == "plain.md");
    }
    for id in ["missing.rs", "missing.md"] {
        let error = service
            .inspect(&[ElementAddress::parse(id).unwrap()])
            .unwrap_err();
        assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
    }
    // Removing canonical declarations removes source membership despite old SQL.
    repo.write("a.md", b"unmarked");
    assert_eq!(
        service
            .inspect(&[ElementAddress::parse("src/data.rs").unwrap()])
            .unwrap_err()
            .get_kind(),
        CommandErrorKind::NotFound
    );
}

#[test]
fn inspect_skips_removed_tracked_candidates_and_rejects_removed_knowledge() {
    // Given: refresh an indexed file which subsequently disappears.
    let repo = ReadingRepo::new();
    repo.write("gone.md", b"<!-- letopis: {\"created_at\":1} -->\ngone");
    repo.write("kept.md", b"<!-- letopis: {\"created_at\":1} -->\nkept");
    let status = std::process::Command::new("git")
        .current_dir(repo.get_root())
        .args(["add", "gone.md"])
        .status()
        .unwrap();
    assert!(status.success());
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    std::fs::remove_file(repo.get_root().join("gone.md")).unwrap();
    let mut service = wire_inspector(repo.get_root()).unwrap();
    // When
    let current = service
        .inspect(&[ElementAddress::parse("kept.md").unwrap()])
        .unwrap();
    let removed = service
        .inspect(&[ElementAddress::parse("gone.md").unwrap()])
        .unwrap_err();
    // Then
    assert_eq!(current.elements.len(), 1);
    assert_eq!(removed.get_kind(), CommandErrorKind::NotFound);
    assert!(
        open_git_snapshots(repo.get_root())
            .unwrap()
            .blob_get_by_tree_oid_and_path(&current.snapshot, &RepoPath::parse("gone.md").unwrap())
            .is_err()
    );
}

#[test]
fn inspect_rejects_removed_indexed_anchor_in_mixed_batch() {
    // Given: keep the file marked and its source declaration current.
    let repo = ReadingRepo::new();
    repo.write(
        "sections.md",
        b"## Old {#old}\n<!-- letopis: {\"created_at\":1} -->\nOld.\n[source](source.txt \"letopis:type=external;path=source.txt\")\n",
    );
    repo.write("source.txt", b"available source");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    repo.write(
        "sections.md",
        b"## Renamed {#renamed}\n<!-- letopis: {\"created_at\":1} -->\nCurrent.\n[source](source.txt \"letopis:type=external;path=source.txt\")\n",
    );
    let mut service = wire_inspector(repo.get_root()).unwrap();
    let current = service
        .inspect(&[ElementAddress::parse("sections.md#renamed").unwrap()])
        .unwrap();
    assert_eq!(current.elements.len(), 1);
    // When
    let error = service
        .inspect(&[
            ElementAddress::parse("source.txt").unwrap(),
            ElementAddress::parse("sections.md#old").unwrap(),
        ])
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
}

type InspectionFixture = (
    &'static str,
    &'static [u8],
    &'static str,
    u64,
    u64,
    &'static str,
);
fn build_fixtures() -> [InspectionFixture; 4] {
    [
        (
            "crlf.md",
            include_bytes!("fixtures/markdown/crlf.md").as_slice(),
            "crlf.md#a",
            1,
            3,
            "eacc729e917594b0955d7ccaae6b41106a58c53cc7b9d58d7d186f957ad72864",
        ),
        (
            "unicode.md",
            include_bytes!("fixtures/markdown/unicode.md").as_slice(),
            "unicode.md#memory",
            3,
            5,
            "99f2ad88e68feb8a1341ea4aec0bf046d236ef8c25953a45c1ee33dad5113a11",
        ),
        (
            "sections.md",
            include_bytes!("fixtures/markdown/sections.md").as_slice(),
            "sections.md#a",
            3,
            8,
            "4ca8f91e2e8b4e7af4edf635b9ffa41afb9c8a0d58f3d08fa940a8df9b093bf1",
        ),
        (
            "no-final-newline.md",
            include_bytes!("fixtures/markdown/no-final-newline.md").as_slice(),
            "no-final-newline.md#e",
            1,
            3,
            "8ae8dc48443c3b9132365da3e83a1862fe8c1be55da7ee6a9bbf478703fe58d2",
        ),
    ]
}
