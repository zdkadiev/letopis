mod configuration;

use super::StatusService;
use crate::{
    domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, TreeOid},
    service::{
        CacheWrite, CommandErrorKind, CommandProblem, CommandResult, DocumentError,
        DocumentErrorKind, DocumentResult, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind,
        EmbeddingProvider, EmbeddingResult, GraphDiagnostics, HeldLock, KnowledgeMetadata,
        NodeMetadata, ProvenanceIssue, ProvenanceObservation, ProvenanceRequest, SnapshotError,
        SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, StatusFiles,
        StatusReader, StatusStorage, Storage, StorageRead, StorageResult, VectorOutcome,
        VectorWrite,
    },
    storage::SqliteStorage,
};
use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};
use tempfile::TempDir;

const VALID_CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","dimensions":3},"max_retries":0}}"#;

struct Reads {
    inner: Option<SqliteStorage>,
    count: Rc<Cell<usize>>,
}
struct Files {
    config: std::cell::RefCell<Option<Vec<u8>>>,
    fail: bool,
    count: Cell<usize>,
}
mockall::mock! {
    Provider {}
    impl EmbeddingProvider for Provider {
        fn validate_config(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;
        fn embed<'a, 'b>(&self, config: &EmbeddingConfig, texts: &'a [&'b str], dimensions: Option<NonZeroUsize>) -> EmbeddingResult<Vec<Vec<f32>>>;
    }
}
struct Diagnostics;
struct Provenance {
    fail: bool,
}

#[test]
fn reports_null_revision_without_publication() {
    // Given
    let count = Rc::new(Cell::new(0));
    let files = Files {
        config: None.into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        None,
        count.clone(),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When
    let report = service.get_status().unwrap();

    // Then
    assert_eq!(report.revision, None);
    assert_eq!((report.embedding_backlog, report.embedding_failed), (0, 0));
    assert!(report.problems.is_empty());
    assert_eq!(count.get(), 1);
    assert_eq!(files.count.get(), 1);
}

#[test]
fn disabled_configuration_hides_execution_failures_and_reports_zero_counts() {
    // Given: stored exhausted work, a query error and a valid disabled document.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let identity = Sha256::digest(b"disabled fixture");
    storage.reset_vectors(None, &identity).unwrap();
    let hash = Sha256::digest(b"search");
    let VectorOutcome::Current(mut unit) = storage.begin_vector_write_tx(&identity).unwrap() else {
        panic!("expected current vector writer")
    };
    assert!(unit.reserve_attempt(&hash, 1).unwrap());
    unit.record_failure(&hash, "private failure").unwrap();
    unit.set_query_error(7, "safe query failure").unwrap();
    unit.commit().unwrap();
    let disabled = String::from_utf8(VALID_CONFIG.to_vec())
        .unwrap()
        .replace("\"enabled\":true", "\"enabled\":false");
    let files = Files {
        config: Some(disabled.into_bytes()).into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When: status is requested.
    let report = service.get_status().unwrap();

    // Then: graph/file observations remain, execution failures do not.
    assert_eq!((report.embedding_backlog, report.embedding_failed), (0, 0));
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "graph_fixture")
    );
    assert!(!report.problems.iter().any(|p| matches!(
        p.details["reason"].as_str(),
        Some("query_embedding_failed" | "embedding_attempts_exhausted")
    )));
}

#[test]
fn invalid_disabled_configuration_remains_a_visible_problem() {
    // Given: nested disabled settings with an endpoint rejected locally.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let invalid = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"fixture","parameters":{"base_url":"ftp://localhost/api"},"max_retries":0}}"#;
    let files = Files {
        config: Some(invalid.to_vec()).into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_rejecting_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When
    let report = service.get_status().unwrap();

    // Then: invalid configuration keeps unavailable-policy counting and a current problem.
    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "invalid_embedding_config")
    );
}

#[test]
fn matches_check_findings_in_one_view() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 7);
    let count = Rc::new(Cell::new(0));
    let files = Files {
        config: Some(VALID_CONFIG.to_vec()).into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        count.clone(),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When
    let report = service.get_status().unwrap();

    // Then
    assert_eq!(report.revision, Some(7));
    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert_eq!(report.problems.len(), 2);
    assert_eq!(report.problems[0].details["reason"], "graph_fixture");
    assert_eq!(
        report.problems[1].details["reason"],
        "missing_source_snapshot"
    );
    assert_eq!(count.get(), 1);
}

#[test]
fn reports_invalid_config_with_unavailable_counts() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let files = Files {
        config: Some(b"SECRET_CONFIG".to_vec()).into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count,
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When
    let report = service.get_status().unwrap();

    // Then
    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "invalid_embedding_config")
    );
    assert!(!format!("{report:?}").contains("SECRET_CONFIG"));
    assert_eq!(files.count.get(), 1);
}

#[test]
fn keeps_query_error_without_mutation() {
    // Given: a persisted query error.
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let identity = Sha256::digest(b"query fixture");
    storage.reset_vectors(None, &identity).unwrap();
    let VectorOutcome::Current(mut writer) = storage.begin_vector_write_tx(&identity).unwrap()
    else {
        panic!("expected current vector writer")
    };
    writer.set_query_error(7, "safe query failure").unwrap();
    writer.commit().unwrap();
    let files = Files {
        config: None.into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count.clone(),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When: status is requested.
    let report = service.get_status().unwrap();

    // Then: the error remains stored and is reported without mutation.
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "query_embedding_failed")
    );
    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert_eq!(count.get(), 1);
}

#[test]
fn reports_unreadable_and_unsupported_config_without_payloads() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let files = Files {
        config: None.into(),
        fail: true,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { fail: false },
    );

    // When
    let report = service.get_status().unwrap();

    // Then
    assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "invalid_embedding_config")
    );
    assert_eq!(files.count.get(), 1);

    // Given
    let storage = open_storage(&root);
    let files = Files {
        config: Some(VALID_CONFIG.to_vec()).into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_rejecting_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { fail: false },
    );
    // When
    let report = service.get_status().unwrap();
    // Then
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.details["reason"] == "invalid_embedding_config")
    );
}

#[test]
fn returns_error_after_late_provenance_failure() {
    // Given
    let root = TempDir::new().unwrap();
    let mut storage = open_storage(&root);
    publish_node(&mut storage, 1);
    let files = Files {
        config: None.into(),
        fail: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count,
        &files,
        &provider,
        &Provenance { fail: true },
    );

    // When
    let error = service.get_status().unwrap_err();

    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("SECRET_PROVENANCE"));
}

fn build_service<'a>(
    storage: Option<SqliteStorage>,
    count: Rc<Cell<usize>>,
    files: &'a Files,
    provider: &'a MockProvider,
    provenance: &'a Provenance,
) -> StatusService<'a, Reads, Diagnostics> {
    StatusService {
        storage: Reads {
            inner: storage,
            count,
        },
        files,
        graph: &Diagnostics,
        provenance,
        provider,
    }
}

impl StatusStorage for Reads {
    type Read<'a> = <SqliteStorage as Storage>::Read<'a>;
    fn begin_status_read_tx(&mut self) -> StorageResult<Option<Self::Read<'_>>> {
        self.count.set(self.count.get() + 1);
        self.inner.as_mut().map(Storage::begin_read_tx).transpose()
    }
}
impl StatusFiles for Files {
    fn get_config(&self) -> EmbeddingResult<Option<Vec<u8>>> {
        self.count.set(self.count.get() + 1);
        if self.fail {
            return Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig));
        }
        Ok(self.config.borrow().clone())
    }
    fn observe_locks(&self) -> CommandResult<Vec<HeldLock>> {
        Ok(Vec::new())
    }
}
fn build_provider() -> MockProvider {
    let mut provider = MockProvider::new();
    provider.expect_embed().times(0);
    provider.expect_validate_config().returning(|_| Ok(()));
    provider
}
fn build_rejecting_provider() -> MockProvider {
    let mut provider = MockProvider::new();
    provider.expect_embed().times(0);
    provider
        .expect_validate_config()
        .returning(|_| Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)));
    provider
}
impl GraphDiagnostics for Diagnostics {
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
        _: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let _ = view;
        Ok(vec![CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "graph fixture",
            details: [
                ("path".into(), serde_json::Value::from("a.md")),
                ("reason".into(), serde_json::Value::from("graph_fixture")),
            ]
            .into(),
        }])
    }
}
impl SourceProvenance for Provenance {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("unused")
    }
    fn observe_sources(&self, _: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation> {
        if self.fail {
            return Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "safe provenance",
                std::io::Error::other("SECRET_PROVENANCE"),
            ));
        }
        Ok(ProvenanceObservation {
            flags: BTreeSet::new(),
            issues: BTreeSet::from([ProvenanceIssue::MissingSnapshot]),
        })
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("unused")
    }
}
fn open_storage(root: &TempDir) -> SqliteStorage {
    SqliteStorage::open(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap()
}
fn publish_node(storage: &mut SqliteStorage, revision: usize) {
    let id = KnowledgeId::parse("a.md#one").unwrap();
    for _ in 0..revision {
        let mut writer = storage.begin_cache_write_tx().unwrap();
        writer
            .upsert_node(&NodeMetadata::Knowledge(KnowledgeMetadata {
                uri: id.clone(),
                id: id.clone(),
                kind: ElementKind::Vertex,
                hash: Sha256::digest(b"content"),
                created_at: 1,
                source_snapshot: None,
                lines: LineRange::new(1, 2).unwrap(),
                search_hash: Sha256::digest(b"search"),
            }))
            .unwrap();
        writer.commit().unwrap();
    }
}
