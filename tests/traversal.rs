mod support;
use letopis::{
    domain::LineRange,
    service::{
        CommandErrorKind, ElementAddress, Refresher, SearchFlag, TraversalDirection, TraversalMode,
        TraversalRequest, Traverser,
    },
    wire_document_refresher, wire_traverser,
};
use std::{collections::BTreeSet, num::NonZeroUsize};
use support::reading::ReadingRepo;

#[test]
fn traverses_without_configuration_with_source_urls_and_ranges() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "[child](b.md \"letopis:type=includes;path=b.md\") [code](code.rs#L2-L2 \"letopis:type=external;path=code.rs#L2-L2\") [url](https://Host/x?q=1#part \"letopis:type=external;path=https://Host/x?q=1#part\")");
    repo.write_knowledge("b.md", "child");
    repo.write("code.rs", b"one\ntwo\n");
    // When
    let output = wire_traverser(repo.get_root())
        .unwrap()
        .traverse(&make_request("a.md"))
        .unwrap();
    // Then
    assert_eq!(
        output
            .elements
            .iter()
            .map(|link| link.element_id.as_str())
            .collect::<Vec<_>>(),
        ["code.rs#L2-L2", "https://Host/x?q=1#part", "b.md"]
    );
    assert_eq!(output.elements[0].path.as_str(), "code.rs");
    assert_eq!(
        output.elements[0].lines,
        Some(LineRange::new(2, 2).unwrap())
    );
    assert_eq!(output.elements[1].path.as_str(), "https://Host/x?q=1#part");
    assert_eq!(output.elements[1].lines, None);
    assert_eq!(
        output.elements[2].lines,
        Some(LineRange::new(1, 2).unwrap())
    );
    assert!(
        output
            .elements
            .iter()
            .all(|link| link.revision == 1 && link.flags.is_empty())
    );
    assert!(output.edges.is_none());
}

#[test]
fn distinguishes_missing_source_membership_from_removed_final_reference() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[code](missing.rs \"letopis:type=external;path=missing.rs\")",
    );
    let mut service = wire_traverser(repo.get_root()).unwrap();
    let mut request = make_request("missing.rs");
    request.direction = TraversalDirection::Incoming;
    // When
    let output = service.traverse(&request).unwrap();
    // Then
    assert_eq!(output.elements[0].element_id.as_str(), "a.md");
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
    );
    // Given
    repo.write_knowledge("a.md", "reference removed");
    // When
    let error = service.traverse(&request).unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::NotFound);
}

#[test]
fn retains_last_valid_knowledge_as_stale_and_missing_source_flag() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[child](b.md \"letopis:type=includes;path=b.md\") [code](code.rs#L2-L3 \"letopis:type=external;path=code.rs#L2-L3\")",
    );
    repo.write_knowledge("b.md", "valid child");
    repo.write("code.rs", b"one\n");
    let mut service = wire_traverser(repo.get_root()).unwrap();
    service.traverse(&make_request("a.md")).unwrap();
    repo.write("b.md", b"<!-- letopis: {broken} -->\nSECRET");
    // When
    let output = service.traverse(&make_request("a.md")).unwrap();
    // Then
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::SourceNotFound])
    );
    assert_eq!(
        output.elements[1].flags,
        BTreeSet::from([SearchFlag::Stale])
    );
    assert_eq!(
        output.elements[1].lines,
        Some(LineRange::new(1, 2).unwrap())
    );
}

#[test]
fn propagates_unsafe_source_errors_without_private_paths_or_payloads() {
    // Given: a declared source whose parent becomes a symlink.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[code](dir/code.rs \"letopis:type=external;path=dir/code.rs\")",
    );
    repo.write("dir/code.rs", b"SECRET");
    let mut service = wire_traverser(repo.get_root()).unwrap();
    service.traverse(&make_request("a.md")).unwrap();
    std::fs::remove_dir_all(repo.get_root().join("dir")).unwrap();
    std::os::unix::fs::symlink("/private/tmp", repo.get_root().join("dir")).unwrap();
    // When: traversal observes the source.
    let error = service.traverse(&make_request("a.md")).unwrap_err();
    // Then: it reports a safe error without exposing the target.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("SECRET"));
    assert!(!error.to_string().contains("/private/tmp"));
}

#[test]
fn observes_late_source_binding_even_when_traversing_only_containment() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge("a.md", "[child](b.md \"letopis:type=includes;path=b.md\")");
    let mut body = String::new();
    for index in 0..65 {
        let source = format!("s{index:02}.rs");
        body.push_str(&format!(
            "[code]({source} \"letopis:type=external;path={source}\")\n"
        ));
        if index < 64 {
            repo.write(&source, b"live");
        }
    }
    repo.write_knowledge("b.md", &body);
    let mut request = make_request("a.md");
    request.mode = TraversalMode::Containment;
    request
        .edge_types
        .insert(letopis::domain::EdgeType::parse("includes").unwrap());
    // When
    let output = wire_traverser(repo.get_root())
        .unwrap()
        .traverse(&request)
        .unwrap();
    // Then
    assert_eq!(output.elements.len(), 1);
    assert_eq!(output.elements[0].element_id.as_str(), "b.md");
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::SourceChanged, SearchFlag::SourceNotFound])
    );
}

#[test]
fn skips_unselected_metadata_and_releases_view_after_selected_metadata_error() {
    // Given: two containment targets, with malformed metadata only on the later target.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[b](b.md \"letopis:type=includes;path=b.md\") [c](c.md \"letopis:type=includes;path=c.md\")",
    );
    repo.write_knowledge("b.md", "first");
    repo.write_knowledge("c.md", "second");
    wire_document_refresher(repo.get_root())
        .unwrap()
        .refresh()
        .unwrap();
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    assert_eq!(
        connection
            .execute(
                "update nodes set created_at = 'PRIVATE_INVALID' where uri = ?1",
                ["c.md"],
            )
            .unwrap(),
        1
    );
    let mut service = wire_traverser(repo.get_root()).unwrap();
    let mut request = make_request("a.md");
    request.limit = NonZeroUsize::new(1).unwrap();
    // When: traversal selects only the first target.
    let output = service.traverse(&request).unwrap();
    // Then: unselected malformed metadata does not prevent the bounded result.
    assert_eq!(output.elements[0].element_id.as_str(), "b.md");
    request.limit = NonZeroUsize::new(2).unwrap();
    // When: the larger limit selects the corrupted target.
    let error = service.traverse(&request).unwrap_err();
    // Then: traversal fails safely, releases its view and succeeds after metadata repair.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("PRIVATE_INVALID"));
    let busy: i64 = connection
        .query_row("pragma wal_checkpoint(truncate)", [], |row| row.get(0))
        .unwrap();
    assert_eq!(
        busy, 0,
        "failed traversal must release its read view while the service lives"
    );
    assert_eq!(
        connection
            .execute("update nodes set created_at = 3 where uri = ?1", ["c.md"])
            .unwrap(),
        1
    );
    assert_eq!(service.traverse(&request).unwrap().elements.len(), 2);
}

#[test]
fn resolves_markdown_shaped_source_by_stored_membership() {
    // Given
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[source](plain.md \"letopis:type=external;path=plain.md\")",
    );
    repo.write("plain.md", b"unmarked Markdown source");
    // When
    let output = wire_traverser(repo.get_root())
        .unwrap()
        .traverse(&make_request("a.md"))
        .unwrap();
    // Then
    assert_eq!(output.elements[0].element_id.as_str(), "plain.md");
    assert_eq!(output.elements[0].path.as_str(), "plain.md");
    assert_eq!(output.elements[0].lines, None);
    assert!(output.elements[0].flags.is_empty());
}

#[test]
fn nonregular_sources_use_missing_local_flags() {
    // Given: a missing declared local source.
    let repo = ReadingRepo::new();
    repo.write_knowledge(
        "a.md",
        "[code](code.rs \"letopis:type=external;path=code.rs\")",
    );
    let mut service = wire_traverser(repo.get_root()).unwrap();
    // When: traversal observes the source.
    let output = service.traverse(&make_request("a.md")).unwrap();
    // Then: it reports the source as missing.
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::SourceNotFound])
    );
    std::fs::create_dir(repo.get_root().join("code.rs")).unwrap();
    // When: the source path becomes a directory.
    let output = service.traverse(&make_request("a.md")).unwrap();
    // Then: it remains absent and releases its read lock.
    assert_eq!(
        output.elements[0].flags,
        BTreeSet::from([SearchFlag::SourceNotFound])
    );
    let connection = rusqlite::Connection::open(repo.get_root().join(".letopis/cache.db")).unwrap();
    let busy: i64 = connection
        .query_row("pragma wal_checkpoint(truncate)", [], |row| row.get(0))
        .unwrap();
    assert_eq!(busy, 0);
    std::fs::remove_dir(repo.get_root().join("code.rs")).unwrap();
    // Given: the source is restored as a regular file.
    repo.write("code.rs", b"now available");
    repo.write_knowledge(
        "a.md",
        "[code](code.rs \"letopis:type=external;path=code.rs\") changed",
    );
    // When / Then: traversal observes the restored source without a missing flag.
    assert!(
        service.traverse(&make_request("a.md")).unwrap().elements[0]
            .flags
            .is_empty()
    );
}

fn make_request(start: &str) -> TraversalRequest {
    TraversalRequest {
        element_ids: BTreeSet::from([ElementAddress::parse(start).unwrap()]),
        mode: TraversalMode::Both,
        direction: TraversalDirection::Outgoing,
        depth: 2,
        limit: NonZeroUsize::new(20).unwrap(),
        edge_types: BTreeSet::new(),
        edge_limit: None,
    }
}
