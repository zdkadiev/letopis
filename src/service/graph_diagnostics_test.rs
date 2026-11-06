use super::{GraphDiagnosticsService, format_invalid_file};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256},
    service::{
        CacheDatabase, CommandErrorKind, DiagnosticKind, FileRow, GraphDiagnostics,
        KnowledgeMetadata, NodeMetadata, StorageError, StorageErrorKind,
    },
    storage::open_test_storage as open_storage,
};
use serde_json::json;
use std::error::Error;
use tempfile::TempDir;

const PARSER_CASES: [(DiagnosticKind, &str, &str); 10] = [
    (
        DiagnosticKind::InvalidUtf8,
        "Markdown is not UTF-8",
        "invalid_utf8",
    ),
    (
        DiagnosticKind::InvalidMetadata,
        "invalid letopis metadata",
        "invalid_metadata",
    ),
    (
        DiagnosticKind::InvalidAnchor,
        "invalid explicit anchor",
        "invalid_anchor",
    ),
    (
        DiagnosticKind::DuplicateAnchor,
        "duplicate explicit anchor",
        "duplicate_anchor",
    ),
    (
        DiagnosticKind::OrphanMetadata,
        "metadata does not immediately follow an anchored heading",
        "orphan_metadata",
    ),
    (
        DiagnosticKind::NestedElement,
        "knowledge elements cannot be nested",
        "nested_element",
    ),
    (
        DiagnosticKind::InvalidLinkTitle,
        "invalid letopis link title",
        "invalid_link_title",
    ),
    (
        DiagnosticKind::InvalidTarget,
        "invalid graph target",
        "invalid_target",
    ),
    (
        DiagnosticKind::LinkPathMismatch,
        "Markdown link path does not match letopis path",
        "link_path_mismatch",
    ),
    (
        DiagnosticKind::SelfLoop,
        "knowledge cannot link to itself",
        "self_loop",
    ),
];

#[test]
fn classifies_every_known_cached_message_without_changing_parser_diagnostics() {
    // Given
    for (kind, message, code) in PARSER_CASES {
        let row = build_invalid_file("bad.md", Some(message));

        // When
        let problem = format_invalid_file(&row);

        // Then
        assert_eq!(kind.get_message(), message);
        assert_eq!(kind.get_code(), code);
        assert_eq!(problem.code, CommandErrorKind::ValidationFailed);
        assert_eq!(problem.message, message);
        assert_eq!(
            problem.details,
            serde_json::from_value(json!({"path":"bad.md", "reason":code})).unwrap()
        );
    }
}

#[test]
fn hides_unknown_and_absent_cached_reasons() {
    // Given
    for reason in [
        Some("SECRET_RAW_BODY"),
        None,
        Some("invalid graph target SECRET_RAW_BODY"),
    ] {
        let row = build_invalid_file("bad.md", reason);

        // When
        let problem = format_invalid_file(&row);

        // Then
        assert_eq!(problem.message, "invalid knowledge file");
        assert_eq!(
            problem.details,
            serde_json::from_value(json!({"path":"bad.md", "reason":"invalid_markup"})).unwrap()
        );
        assert!(!format!("{problem:?}").contains("SECRET_RAW_BODY"));
    }
}

#[test]
fn retains_private_storage_cause_without_public_payload() {
    // Given: a cache database that lost its node table after the service opened it.
    let directory = TempDir::new().unwrap();
    let cache = directory.path().join("cache.db");
    let vectors = directory.path().join("vectors.db");
    let storage = open_storage(&cache, &vectors).unwrap();
    let service = GraphDiagnosticsService::new(std::rc::Rc::new(storage.cache_storage));
    rusqlite::Connection::open(&cache)
        .unwrap()
        .execute("drop table nodes", [])
        .unwrap();

    // When: diagnostics read the missing node table.
    let error = service.find_problems(&[]).unwrap_err();

    // Then: callers receive the safe command classification and keep the storage cause private.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.to_string(), "cannot read graph diagnostics");
    assert!(!format!("{error:?}").contains("no such table"));
    let storage_error = error
        .source()
        .unwrap()
        .downcast_ref::<StorageError>()
        .unwrap();
    assert_eq!(storage_error.get_kind(), StorageErrorKind::InternalError);
    assert!(storage_error.source().is_some());
}

#[test]
fn finds_cross_file_cycle_before_filtering_and_deduplicates_selectors() {
    // Given: a cross-file cycle and repeated selectors.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    seed_graph(
        &storage,
        &["a.md", "b.md"],
        &[("a.md", "b.md"), ("b.md", "a.md")],
    );
    let cache = std::rc::Rc::new(storage.cache_storage);
    let selected = RepoPath::parse("a.md").unwrap();

    // When: graph diagnostics run with the selected owners.
    let problems = GraphDiagnosticsService::new(cache)
        .find_problems(&[selected.clone(), selected])
        .unwrap();

    // Then: the cycle is found once before owner filtering.
    assert_eq!(problems.len(), 1);
    assert_eq!(problems[0].code, CommandErrorKind::ValidationFailed);
    assert_eq!(
        problems[0].message,
        "knowledge participates in an includes cycle"
    );
    assert_eq!(
        problems[0].details,
        serde_json::from_value(
            json!({"path":"a.md", "reason":"includes_cycle", "element_id":"a.md"})
        )
        .unwrap()
    );
}

#[test]
fn keeps_two_components_in_one_file_with_minimum_local_representatives() {
    // Given: two graph components owned by one file.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    seed_graph(
        &storage,
        &["a.md#a", "a.md#b", "a.md#c", "b.md"],
        &[
            ("a.md#a", "a.md#b"),
            ("a.md#b", "a.md#a"),
            ("a.md#c", "b.md"),
            ("b.md", "a.md#c"),
        ],
    );
    let cache = std::rc::Rc::new(storage.cache_storage);

    // When: graph diagnostics inspect both components.
    let problems = GraphDiagnosticsService::new(cache)
        .find_problems(&[])
        .unwrap();

    let details: Vec<_> = problems.iter().map(|problem| &problem.details).collect();
    let expected: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        serde_json::from_value(json!([
            {"path":"a.md", "reason":"includes_cycle", "element_id":"a.md#a"},
            {"path":"a.md", "reason":"includes_cycle", "element_id":"a.md#c"},
            {"path":"b.md", "reason":"includes_cycle", "element_id":"b.md"}
        ]))
        .unwrap();
    // Then: each component uses its minimum local representative.
    assert_eq!(details, expected.iter().collect::<Vec<_>>());
}

#[test]
fn orders_file_graph_and_distinct_anchor_findings_by_safe_context() {
    // Given: file, graph and distinct anchor problems.
    let root = TempDir::new().unwrap();
    let storage = open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
    seed_graph(
        &storage,
        &["a.md#a", "a.md#b", "b.md"],
        &[("a.md#a", "z.md"), ("a.md#a", "x.md"), ("a.md#b", "x.md")],
    );
    storage
        .cache_storage
        .with_write_tx(|| {
            let write = &storage.cache_storage;
            write
                .files_upsert_by_path(&build_invalid_file("a.md", Some("SECRET_RAW_BODY")))
                .unwrap();
            write
                .files_upsert_by_path(&build_invalid_file("z.md", Some("invalid graph target")))
                .unwrap();

            write.project_get_revision()
        })
        .unwrap();
    let cache = std::rc::Rc::new(storage.cache_storage);

    // When: graph diagnostics collect the findings.
    let problems = GraphDiagnosticsService::new(cache)
        .find_problems(&[])
        .unwrap();

    let details: Vec<_> = problems.iter().map(|problem| &problem.details).collect();
    let expected: Vec<std::collections::BTreeMap<String, serde_json::Value>> =
        serde_json::from_value(json!([
            {"path":"a.md", "reason":"dangling_target", "element_id":"a.md#a", "target":"x.md"},
            {"path":"a.md", "reason":"dangling_target", "element_id":"a.md#a", "target":"z.md"},
            {"path":"a.md", "reason":"dangling_target", "element_id":"a.md#b", "target":"x.md"},
            {"path":"a.md", "reason":"invalid_markup"},
            {"path":"z.md", "reason":"invalid_target"}
        ]))
        .unwrap();
    // Then: findings use safe contexts in deterministic order.
    assert_eq!(details, expected.iter().collect::<Vec<_>>());
    assert!(
        problems
            .iter()
            .all(|problem| problem.code == CommandErrorKind::ValidationFailed)
    );
    assert_eq!(problems[0].message, "knowledge target is missing");
    assert!(!format!("{problems:?}").contains("SECRET_RAW_BODY"));
}

fn seed_graph(storage: &crate::storage::TestDatabases, nodes: &[&str], edges: &[(&str, &str)]) {
    storage
        .cache_storage
        .with_write_tx(|| {
            let write = &storage.cache_storage;
            for raw in nodes {
                let id = KnowledgeId::parse(raw).unwrap();
                write
                    .nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
                        uri: id.clone(),
                        id,
                        kind: ElementKind::Vertex,
                        hash: Sha256::digest(b"text"),
                        created_at: 1,
                        source_snapshot: None,
                        lines: LineRange::new(1, 1).unwrap(),
                        search_hash: Sha256::digest(b"text"),
                    }))
                    .unwrap();
            }
            for (source, target) in edges {
                write
                    .edges_save_by_source_id_and_target_id_and_type(
                        &Edge::new(
                            KnowledgeId::parse(source).unwrap(),
                            EdgeTarget::Knowledge(KnowledgeId::parse(target).unwrap()),
                            EdgeType::parse("includes").unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();
            }

            write.project_get_revision()
        })
        .unwrap();
}

fn build_invalid_file(path: &str, error: Option<&str>) -> FileRow {
    FileRow {
        path: RepoPath::parse(path).unwrap(),
        size: 0,
        mtime_ns: 1,
        inode: 1,
        hash: Sha256::digest(b""),
        checked_at_ns: 2,
        valid: false,
        error: error.map(str::to_owned),
    }
}
