use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandProblem, CommandResult, DiagnosticKind,
    FileRow, GraphDiagnostics, NodeMetadata, StorageError,
};
use crate::domain::{GraphProblem, KnowledgeId, RepoPath, find_graph_problems};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

const PATH: &str = "path";
const REASON: &str = "reason";
const ELEMENT_ID: &str = "element_id";
const TARGET: &str = "target";
const PARSER_KINDS: [DiagnosticKind; 10] = [
    DiagnosticKind::InvalidUtf8,
    DiagnosticKind::InvalidMetadata,
    DiagnosticKind::InvalidAnchor,
    DiagnosticKind::DuplicateAnchor,
    DiagnosticKind::OrphanMetadata,
    DiagnosticKind::NestedElement,
    DiagnosticKind::InvalidLinkTitle,
    DiagnosticKind::InvalidTarget,
    DiagnosticKind::LinkPathMismatch,
    DiagnosticKind::SelfLoop,
];

/// Derives safe current findings from a caller-owned read view.
#[derive(Debug)]
pub(crate) struct GraphDiagnosticsService<C> {
    storage: std::rc::Rc<C>,
}

impl<C> GraphDiagnosticsService<C> {
    pub(crate) fn new(storage: std::rc::Rc<C>) -> Self {
        Self { storage }
    }
}

impl<C: CacheDatabase> GraphDiagnostics for GraphDiagnosticsService<C> {
    fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
        self.storage
            .with_read_tx(|| {
                let nodes = self.storage.nodes_get().map_err(map_read_error)?;
                let edges = self
                    .storage
                    .edges_get_by_source_id(None)
                    .map_err(map_read_error)?;
                let files = self.storage.files_get().map_err(map_read_error)?;
                let knowledge: BTreeSet<KnowledgeId> = nodes
                    .into_iter()
                    .filter_map(|node| match node.metadata {
                        NodeMetadata::Knowledge(metadata) => Some(metadata.id),
                        NodeMetadata::Source(_) => None,
                    })
                    .collect();
                let edges = edges.into_iter().collect();
                let mut problems: Vec<CommandProblem> = files
                    .iter()
                    .filter(|file| !file.valid)
                    .map(format_invalid_file)
                    .collect();
                for problem in find_graph_problems(&knowledge, &edges) {
                    problems.extend(format_graph_problem(&problem));
                }
                let selected: BTreeSet<&str> = paths.iter().map(RepoPath::as_str).collect();
                problems.retain(|problem| {
                    selected.is_empty() || selected.contains(get_detail(problem, PATH))
                });
                problems.sort_by(|left, right| get_sort_key(left).cmp(&get_sort_key(right)));
                Ok(problems)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => map_read_error(error),
            })
    }
}

fn map_read_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read graph diagnostics",
        cause,
    )
}

fn format_invalid_file(file: &FileRow) -> CommandProblem {
    let kind = classify_cached_message(file.error.as_deref());
    let (reason, message) = kind.map_or(("invalid_markup", "invalid knowledge file"), |kind| {
        (kind.get_code(), kind.get_message())
    });
    CommandProblem {
        code: CommandErrorKind::ValidationFailed,
        message,
        details: BTreeMap::from([
            (PATH.into(), Value::from(file.path.as_str())),
            (REASON.into(), Value::from(reason)),
        ]),
    }
}

fn classify_cached_message(message: Option<&str>) -> Option<DiagnosticKind> {
    PARSER_KINDS
        .into_iter()
        .find(|kind| Some(kind.get_message()) == message)
}

fn format_graph_problem(problem: &GraphProblem) -> Vec<CommandProblem> {
    match problem {
        GraphProblem::DanglingTarget { source, target } => vec![CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "knowledge target is missing",
            details: BTreeMap::from([
                (PATH.into(), Value::from(get_owner_path(source))),
                (REASON.into(), Value::from("dangling_target")),
                (ELEMENT_ID.into(), Value::from(source.as_str())),
                (TARGET.into(), Value::from(target.as_str())),
            ]),
        }],
        GraphProblem::IncludesCycle { members } => format_cycle(members),
    }
}

fn format_cycle(members: &[KnowledgeId]) -> Vec<CommandProblem> {
    let mut representatives = BTreeMap::new();
    // Domain components are sorted, so the first member of each file is its minimum.
    for member in members {
        representatives
            .entry(get_owner_path(member))
            .or_insert(member);
    }
    representatives
        .into_iter()
        .map(|(path, member)| CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "knowledge participates in an includes cycle",
            details: BTreeMap::from([
                (PATH.into(), Value::from(path)),
                (REASON.into(), Value::from("includes_cycle")),
                (ELEMENT_ID.into(), Value::from(member.as_str())),
            ]),
        })
        .collect()
}

fn get_owner_path(id: &KnowledgeId) -> &str {
    id.as_str()
        .split_once('#')
        .map_or(id.as_str(), |(path, _)| path)
}

fn get_sort_key(problem: &CommandProblem) -> [&str; 4] {
    [PATH, REASON, ELEMENT_ID, TARGET].map(|key| get_detail(problem, key))
}

fn get_detail<'a>(problem: &'a CommandProblem, key: &str) -> &'a str {
    problem
        .details
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
}

#[cfg(test)]
mod tests {

    use super::{GraphDiagnosticsService, format_invalid_file, map_read_error};
    use crate::{
        domain::{
            Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256,
        },
        service::{
            CacheDatabase, CommandErrorKind, DiagnosticKind, FileRow, GraphDiagnostics,
            KnowledgeMetadata, NodeMetadata, StorageError, StorageErrorKind,
        },
        storage::open_test_storage as open_storage,
    };
    use serde_json::json;
    use std::{error::Error, io};
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
                serde_json::from_value(json!({"path":"bad.md", "reason":"invalid_markup"}))
                    .unwrap()
            );
            assert!(!format!("{problem:?}").contains("SECRET_RAW_BODY"));
        }
    }

    #[test]
    fn retains_private_storage_cause_without_public_payload() {
        // Given
        let cause = StorageError::new_with_cause(
            StorageErrorKind::InternalError,
            "storage read failed",
            io::Error::other("SECRET_SQL_CAUSE"),
        );

        // When
        let error = map_read_error(cause);

        // Then
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.to_string(), "cannot read graph diagnostics");
        assert!(!format!("{error:?}").contains("SECRET_SQL_CAUSE"));
        let storage = error
            .source()
            .unwrap()
            .downcast_ref::<StorageError>()
            .unwrap();
        assert_eq!(storage.source().unwrap().to_string(), "SECRET_SQL_CAUSE");
    }

    #[test]
    fn finds_cross_file_cycle_before_filtering_and_deduplicates_selectors() {
        // Given: a cross-file cycle and repeated selectors.
        let root = TempDir::new().unwrap();
        let storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        seed_graph(
            &storage,
            &["a.md", "b.md"],
            &[("a.md", "b.md"), ("b.md", "a.md")],
        );
        let cache = std::rc::Rc::new(storage.cache);
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
        let storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
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
        let cache = std::rc::Rc::new(storage.cache);

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
        let storage =
            open_storage(&root.path().join("cache"), &root.path().join("vectors")).unwrap();
        seed_graph(
            &storage,
            &["a.md#a", "a.md#b", "b.md"],
            &[("a.md#a", "z.md"), ("a.md#a", "x.md"), ("a.md#b", "x.md")],
        );
        storage
            .cache
            .with_write_tx(|| {
                let write = &storage.cache;
                write
                    .files_upsert_by_path(&build_invalid_file("a.md", Some("SECRET_RAW_BODY")))
                    .unwrap();
                write
                    .files_upsert_by_path(&build_invalid_file("z.md", Some("invalid graph target")))
                    .unwrap();

                write.get_publication_revision()
            })
            .unwrap();
        let cache = std::rc::Rc::new(storage.cache);

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
            .cache
            .with_write_tx(|| {
                let write = &storage.cache;
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

                write.get_publication_revision()
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
}
