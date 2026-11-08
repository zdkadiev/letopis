use crate::service::{
    CapturedFile, FileError, FileErrorKind, FileIdentity, FileReader, FileResult, FileStat,
    MockFileReader,
};

use crate::service::StorageError;
use crate::service::TransactionError;
use crate::storage::get_current;

mod configuration {
    use crate::service::StorageError;

    use crate::storage::get_current;

    use super::{
        CommandErrorKind, CommandProblem, CommandResult, Diagnostics, Files, GraphDiagnostics,
        Provenance, Reads, RepoPath, Sha256, StatusReader, StatusService, TestDatabases,
        VALID_CONFIG, VectorDatabase, build_provider, build_service, open_storage, publish_node,
    };
    use crate::service::Configuration;
    use std::{cell::Cell, rc::Rc};
    use tempfile::TempDir;

    struct ChangingDiagnostics<'a>(&'a Files);

    #[test]
    fn keeps_captured_config_after_live_config_changes() {
        // Given: exhausted work and a diagnostic collaborator that removes the live config.
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        publish_node(&mut storage, 1);
        seed_exhausted(&mut storage);
        let files = build_files(Some(VALID_CONFIG.to_vec()));
        let provider = build_provider();
        let graph = ChangingDiagnostics(&files);
        let mut service = StatusService {
            cache: Rc::new(Reads {
                inner: storage.cache_storage,
                observations: Default::default(),
                count: Rc::new(Cell::new(0)),
            }),
            file_reader: &files,
            graph: &graph,
            provenance: &Provenance { is_failing: false },
            provider: &provider,
        };

        // When: status captures configuration before diagnostics change the file.
        let report = service.get_status().unwrap();

        // Then: the captured identity and threshold still report the exhausted hash after one config read.
        assert_eq!((report.embedding_backlog, report.embedding_failed), (0, 1));
        let exhausted = report
            .problems
            .iter()
            .find(|p| p.details["reason"] == "embedding_attempts_exhausted")
            .unwrap();
        assert_eq!(exhausted.code, CommandErrorKind::EmbeddingFailed);
        assert_eq!(
            exhausted.details["target"],
            "2419329067823cab5b4e5ac5dd18a6abf1f57f45e753f5fc934292f3085a3717"
        );
        assert_eq!(files.count.get(), 1);
        assert!(files.config.borrow().is_none());
    }

    #[test]
    fn mismatched_identity_keeps_exhausted_hash_in_backlog() {
        // Given
        let root = TempDir::new().unwrap();
        let mut storage = open_storage(&root);
        publish_node(&mut storage, 1);
        seed_exhausted(&mut storage);
        let other = String::from_utf8(VALID_CONFIG.to_vec())
            .unwrap()
            .replace("fixture", "other-model");
        let files = build_files(Some(other.into_bytes()));
        let provider = build_provider();
        let mut service = build_service(
            Some(storage),
            Rc::new(Cell::new(0)),
            &files,
            &provider,
            &Provenance { is_failing: false },
        );

        // When
        let report = service.get_status().unwrap();

        // Then
        assert_eq!((report.embedding_backlog, report.embedding_failed), (1, 0));
        assert!(
            !report
                .problems
                .iter()
                .any(|p| p.details["reason"] == "embedding_attempts_exhausted")
        );
    }

    impl GraphDiagnostics for ChangingDiagnostics<'_> {
        fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
            self.0.config.replace(None);
            Diagnostics.find_problems(paths)
        }
    }

    fn build_files(config: Option<Vec<u8>>) -> Files {
        Files {
            config: config.into(),
            is_failing: false,
            count: Cell::new(0),
        }
    }

    fn seed_exhausted(storage: &mut TestDatabases) {
        let config = Configuration::parse(VALID_CONFIG)
            .unwrap()
            .get_embedding_config()
            .unwrap()
            .clone();
        let identity = config.get_identity();
        storage
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        storage
            .vector_storage
            .with_write_tx(&identity, || {
                let writer = &storage.vector_storage;
                assert!(
                    writer
                        .embed_failures_upsert_by_search_hash_and_attempts(
                            &Sha256::digest(b"search"),
                            1
                        )
                        .unwrap()
                );
                assert!(
                    writer
                        .embed_failures_update_by_search_hash(
                            &Sha256::digest(b"search"),
                            "private failure"
                        )
                        .unwrap()
                );

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
    }
}

use super::StatusService;
use crate::{
    domain::{Edge, EdgeTarget, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, TreeOid},
    service::{
        CacheDatabase, CommandErrorKind, CommandProblem, CommandResult, Configuration,
        DocumentError, DocumentErrorKind, DocumentResult, EmbeddingConfig, EmbeddingError,
        EmbeddingErrorKind, EmbeddingFailure, EmbeddingProvider, EmbeddingResult, EmbeddingWork,
        FileRow, GraphDiagnostics, KnowledgeMetadata, NodeMetadata, ProvenanceIssue,
        ProvenanceObservation, ProvenanceRequest, QueryError, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, StatusReader, StorageRead, StorageResult,
        StoredNode, TextMatch, VectorDatabase, VectorMatch, VectorOutcome,
    },
    storage::TestDatabases,
};
use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};
use tempfile::TempDir;

const VALID_CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","dimensions":3},"max_retries":0}}"#;

struct Reads {
    inner: crate::storage::CacheStorage,
    count: Rc<Cell<usize>>,
    observations: std::cell::RefCell<Vec<&'static str>>,
}

struct Files {
    config: std::cell::RefCell<Option<Vec<u8>>>,
    is_failing: bool,
    count: Cell<usize>,
}
mockall::mock! {
    Provider {}

    impl EmbeddingProvider for Provider {

        fn config_validate(&self, config: &EmbeddingConfig) -> EmbeddingResult<()>;

        fn vectors_compute_by_texts<'a, 'b>(&self, config: &EmbeddingConfig, texts: &'a [&'b str], dimensions: Option<NonZeroUsize>) -> EmbeddingResult<Vec<Vec<f32>>>;
    }
}
mockall::mock! {
    ScopeDiagnostics {}

    impl GraphDiagnostics for ScopeDiagnostics {

        fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>>;
    }
}

struct Diagnostics;

struct Provenance {
    is_failing: bool,
}

#[test]
fn selects_only_required_observations_for_each_configuration_mode() {
    // Given: one exhausted current hash and query failure under the configured identity.
    let config = crate::service::Configuration::parse(VALID_CONFIG).unwrap();
    let identity = config.get_embedding_config().unwrap().get_identity();
    let disabled = String::from_utf8(VALID_CONFIG.to_vec())
        .unwrap()
        .replace("true", "false");
    let mismatch = String::from_utf8(VALID_CONFIG.to_vec())
        .unwrap()
        .replace("fixture", "different");
    for (bytes, expected_counts, expected_reads, query_visible) in [
        (Some(disabled.into_bytes()), (0, 0), vec![], false),
        (None, (1, 0), vec!["total", "query_error"], true),
        (
            Some(mismatch.into_bytes()),
            (1, 0),
            vec!["total", "query_error"],
            true,
        ),
        (
            Some(VALID_CONFIG.to_vec()),
            (0, 1),
            vec!["total", "coverage", "failures", "query_error"],
            true,
        ),
    ] {
        let root = TempDir::new().unwrap();
        let storage = build_failed_storage(&root, &identity);
        let files = Files {
            config: bytes.into(),
            is_failing: false,
            count: Cell::new(0),
        };
        let provider = build_provider();
        let mut service = build_service(
            Some(storage),
            Rc::default(),
            &files,
            &provider,
            &Provenance { is_failing: false },
        );
        // When: the real service observes the publication using one captured configuration.
        let report = service.get_status().unwrap();
        // Then: modes request only their necessary observations and preserve query-error visibility.
        assert_eq!(
            (report.embedding_backlog, report.embedding_failed),
            expected_counts
        );
        assert_eq!(*service.cache.observations.borrow(), expected_reads);
        assert_eq!(
            report
                .problems
                .iter()
                .any(|p| p.details["reason"] == "query_embedding_failed"),
            query_visible
        );
        assert_eq!(files.count.get(), 1);
        assert_eq!(service.cache.count.get(), 1);
    }
}

#[test]
fn disabled_mode_still_rejects_invalid_scope_metadata_and_revision() {
    // Given: independently parsed disabled configuration and corrupted identity or revision metadata.
    let disabled_config = br#"{"embedding_model":{"enabled":false,"provider":"ollama","model":"disabled-scope-fixture","parameters":{"base_url":"http://localhost/api","dimensions":3},"max_retries":0}}"#;
    assert!(!Configuration::parse(disabled_config).unwrap().is_enabled());
    for (database, statement) in [
        ("vectors.db", "update meta set config_identity='malformed'"),
        ("cache.db", "update project set revision=-1"),
    ] {
        let root = TempDir::new().unwrap();
        let identity = Sha256::digest(b"model");
        let storage = build_failed_storage(&root, &identity);
        rusqlite::Connection::open(root.path().join(database))
            .unwrap()
            .execute_batch(statement)
            .unwrap();
        let files = Files {
            config: Some(disabled_config.to_vec()).into(),
            is_failing: false,
            count: Cell::new(0),
        };
        let provider = build_provider();
        let mut service = build_service(
            Some(storage),
            Rc::default(),
            &files,
            &provider,
            &Provenance { is_failing: false },
        );
        let mut graph = MockScopeDiagnostics::new();
        graph.expect_find_problems().times(0);
        service.graph = &graph;
        // When: status enters its normal read scope before suppressing embedding work.
        let error = service.get_status().unwrap_err();
        // Then: disabled mode rejects corrupt metadata before invoking the read callback.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert_eq!(error.get_message(), "cannot read published status");
        assert!(service.cache.observations.borrow().is_empty());
    }
}

#[test]
fn observes_both_literal_lock_paths_in_order_and_preserves_labels() {
    // Given: two held owners and mocks allowing only the exact application paths.
    let mut files = MockFileReader::new();
    let mut sequence = mockall::Sequence::new();
    files
        .expect_lock_get_owner_by_path()
        .withf(|path| path == std::path::Path::new(".letopis/index.lock"))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_| Ok(std::num::NonZeroU32::new(101)));
    files
        .expect_lock_get_owner_by_path()
        .withf(|path| path == std::path::Path::new(".letopis/embed.lock"))
        .times(1)
        .in_sequence(&mut sequence)
        .returning(|_| Ok(std::num::NonZeroU32::new(202)));
    let provider = build_provider();
    let service = build_service(
        None,
        Rc::default(),
        &files,
        &provider,
        &Provenance { is_failing: false },
    );
    // When: status aggregates the two file owners.
    let locks = service.get_locks().unwrap();
    // Then: labels and PIDs retain the established index-before-embed ordering.
    assert_eq!(
        locks
            .iter()
            .map(|lock| (lock.name, lock.pid.get()))
            .collect::<Vec<_>>(),
        [
            (crate::service::LockName::Index, 101),
            (crate::service::LockName::Embed, 202)
        ]
    );
}

#[test]
fn malformed_owner_retains_safe_status_diagnostic() {
    // Given: the index owner record is malformed and must stop aggregation.
    let mut files = MockFileReader::new();
    files
        .expect_lock_get_owner_by_path()
        .withf(|path| path == std::path::Path::new(".letopis/index.lock"))
        .times(1)
        .returning(|_| Err(FileError::new(FileErrorKind::InvalidOwner)));
    let provider = build_provider();
    let service = build_service(
        None,
        Rc::default(),
        &files,
        &provider,
        &Provenance { is_failing: false },
    );
    // When: status observes coordination ownership.
    let error = service.get_locks().unwrap_err();
    // Then: callers receive the original safe command classification and message.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(
        error.get_message(),
        "cannot observe coordination lock owner"
    );
}

#[test]
fn reports_null_revision_without_publication() {
    // Given
    let count = Rc::new(Cell::new(0));
    let files = Files {
        config: None.into(),
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        None,
        count.clone(),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
    storage
        .vector_storage
        .reset_vectors(None, &identity)
        .unwrap();
    let hash = Sha256::digest(b"search");
    storage
        .vector_storage
        .with_write_tx(&identity, || {
            let unit = &storage.vector_storage;
            assert!(
                unit.embed_failures_upsert_by_search_hash_and_attempts(&hash, 1)
                    .unwrap()
            );
            unit.embed_failures_update_by_search_hash(&hash, "private failure")
                .unwrap();
            unit.query_error_upsert_by_id(7, "safe query failure")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let disabled = String::from_utf8(VALID_CONFIG.to_vec())
        .unwrap()
        .replace("\"enabled\":true", "\"enabled\":false");
    let files = Files {
        config: Some(disabled.into_bytes()).into(),
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_rejecting_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        count.clone(),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count,
        &files,
        &provider,
        &Provenance { is_failing: false },
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
    storage
        .vector_storage
        .reset_vectors(None, &identity)
        .unwrap();
    storage
        .vector_storage
        .with_write_tx(&identity, || {
            let writer = &storage.vector_storage;
            writer
                .query_error_upsert_by_id(7, "safe query failure")
                .unwrap();

            Ok::<_, StorageError>(())
        })
        .map(get_current)
        .unwrap();
    let files = Files {
        config: None.into(),
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count.clone(),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: true,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_rejecting_provider();
    let mut service = build_service(
        Some(storage),
        Rc::new(Cell::new(0)),
        &files,
        &provider,
        &Provenance { is_failing: false },
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
        is_failing: false,
        count: Cell::new(0),
    };
    let provider = build_provider();
    let count = Rc::new(Cell::new(0));
    let mut service = build_service(
        Some(storage),
        count,
        &files,
        &provider,
        &Provenance { is_failing: true },
    );

    // When
    let error = service.get_status().unwrap_err();

    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert!(!format!("{error:?}").contains("SECRET_PROVENANCE"));
}

fn build_failed_storage(root: &TempDir, identity: &Sha256) -> TestDatabases {
    let mut storage = open_storage(root);
    publish_node(&mut storage, 1);
    storage
        .vector_storage
        .reset_vectors(None, identity)
        .unwrap();
    storage
        .vector_storage
        .with_write_tx(identity, || {
            storage
                .vector_storage
                .embed_failures_upsert_by_search_hash_and_attempts(&Sha256::digest(b"search"), 1)?;
            storage
                .vector_storage
                .query_error_upsert_by_id(7, "safe query failure")?;
            Ok::<_, StorageError>(())
        })
        .unwrap();
    storage
}

fn build_service<'a>(
    storage: Option<TestDatabases>,
    count: Rc<Cell<usize>>,
    files: &'a dyn FileReader,
    provider: &'a MockProvider,
    provenance: &'a Provenance,
) -> StatusService<'a, Reads> {
    StatusService {
        cache: Rc::new(Reads {
            inner: storage
                .map(|storage| storage.cache_storage)
                .unwrap_or_else(|| crate::storage::CacheStorage::new_observed(None)),
            observations: Default::default(),
            count,
        }),
        file_reader: files,
        graph: &Diagnostics,
        provenance,
        provider,
    }
}

use crate::service::cache_test_support::{delegate_cache_writes, delegate_graph_reads};

impl StorageRead for Reads {
    fn nodes_get_by_kind(&self) -> StorageResult<u64> {
        self.observations.borrow_mut().push("total");
        StorageRead::nodes_get_by_kind(&self.inner)
    }

    fn nodes_get_coverage_by_kind(&self) -> StorageResult<u64> {
        self.observations.borrow_mut().push("coverage");
        StorageRead::nodes_get_coverage_by_kind(&self.inner)
    }

    fn embed_failures_get_by_kind_and_search_hash_and_attempts(
        &self,
        max_attempts: u64,
    ) -> StorageResult<Vec<EmbeddingFailure>> {
        self.observations.borrow_mut().push("failures");
        StorageRead::embed_failures_get_by_kind_and_search_hash_and_attempts(
            &self.inner,
            max_attempts,
        )
    }

    fn nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
        &self,
        expected: &Sha256,
        max_attempts: u64,
        uri: Option<&KnowledgeId>,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingWork>>> {
        StorageRead::nodes_get_by_kind_and_search_hash_and_attempts_and_uri(
            &self.inner,
            expected,
            max_attempts,
            uri,
        )
    }

    fn vector_index_config_get_by_id(
        &self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<NonZeroUsize>>> {
        StorageRead::vector_index_config_get_by_id(&self.inner, expected)
    }

    fn nodes_get_by_uri(&self, uri: &EdgeTarget) -> StorageResult<Option<StoredNode>> {
        StorageRead::nodes_get_by_uri(&self.inner, uri)
    }

    fn nodes_get_by_source_snapshot(&self) -> StorageResult<BTreeSet<TreeOid>> {
        StorageRead::nodes_get_by_source_snapshot(&self.inner)
    }

    fn nodes_get(&self) -> StorageResult<Vec<StoredNode>> {
        StorageRead::nodes_get(&self.inner)
    }

    fn edges_get_by_source_id(&self, source_id: Option<&KnowledgeId>) -> StorageResult<Vec<Edge>> {
        StorageRead::edges_get_by_source_id(&self.inner, source_id)
    }

    fn files_get_by_path(&self, path: &RepoPath) -> StorageResult<Option<FileRow>> {
        StorageRead::files_get_by_path(&self.inner, path)
    }

    fn files_get(&self) -> StorageResult<Vec<FileRow>> {
        StorageRead::files_get(&self.inner)
    }

    fn text_index_get_by_expression(
        &self,
        expression: &str,
        limit: NonZeroUsize,
    ) -> StorageResult<Vec<TextMatch>> {
        StorageRead::text_index_get_by_expression(&self.inner, expression, limit)
    }

    fn vector_keys_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<Vec<f32>>>> {
        StorageRead::vector_keys_get_by_search_hash(&self.inner, expected, search_hash)
    }

    fn vector_index_get_by_vector(
        &self,
        expected: &Sha256,
        query: &[f32],
        dimensions: usize,
        limit: NonZeroUsize,
    ) -> StorageResult<VectorOutcome<Vec<VectorMatch>>> {
        StorageRead::vector_index_get_by_vector(&self.inner, expected, query, dimensions, limit)
    }

    fn embed_failures_get_by_search_hash(
        &self,
        expected: &Sha256,
        search_hash: &Sha256,
    ) -> StorageResult<VectorOutcome<Option<EmbeddingFailure>>> {
        StorageRead::embed_failures_get_by_search_hash(&self.inner, expected, search_hash)
    }

    fn query_error_get_by_id(&self) -> StorageResult<Option<QueryError>> {
        self.observations.borrow_mut().push("query_error");
        StorageRead::query_error_get_by_id(&self.inner)
    }

    fn project_get_by_id(&self) -> StorageResult<u64> {
        StorageRead::project_get_by_id(&self.inner)
    }

    fn meta_get_by_id(&self) -> StorageResult<Option<Sha256>> {
        StorageRead::meta_get_by_id(&self.inner)
    }
}

impl crate::service::GraphRead for Reads {
    delegate_graph_reads!();
}

impl crate::service::CacheDatabase for Reads {
    delegate_cache_writes!();

    fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.inner.with_read_tx(operation)
    }

    fn with_write_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.inner.with_write_tx(operation)
    }

    fn with_status_read_tx<T, E, F>(&self, operation: F) -> Result<Option<T>, TransactionError<E>>
    where
        F: FnOnce() -> Result<T, E>,
    {
        self.count.set(self.count.get() + 1);
        self.inner.with_status_read_tx(operation)
    }
}

impl FileReader for Files {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_by_path(&self, _: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        panic!("unexpected file_get_by_path")
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        assert_eq!(path, std::path::Path::new(".letopis/config.json"));
        self.count.set(self.count.get() + 1);
        if self.is_failing {
            return Err(FileError::new(FileErrorKind::Io));
        }
        Ok(self.config.borrow().clone())
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        panic!("unexpected file_get_stat_by_path")
    }

    fn file_get_identity_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileIdentity>> {
        panic!("unexpected file_get_identity_by_path")
    }

    fn file_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        panic!("unexpected file_is_exist")
    }

    fn file_verify_identity_by_path(
        &self,
        _: &std::path::Path,
        _: Option<FileIdentity>,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_identity_by_path")
    }

    fn file_verify_distinct_by_paths(
        &self,
        _: &std::path::Path,
        _: &std::path::Path,
    ) -> FileResult<()> {
        panic!("unexpected file_verify_distinct_by_paths")
    }

    fn directory_get_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::os::fd::OwnedFd>> {
        Ok(None)
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        Ok(None)
    }
}

fn build_provider() -> MockProvider {
    let mut provider = MockProvider::new();
    provider.expect_vectors_compute_by_texts().times(0);
    provider.expect_config_validate().returning(|_| Ok(()));
    provider
}

fn build_rejecting_provider() -> MockProvider {
    let mut provider = MockProvider::new();
    provider.expect_vectors_compute_by_texts().times(0);
    provider
        .expect_config_validate()
        .returning(|_| Err(EmbeddingError::new(EmbeddingErrorKind::InvalidConfig)));
    provider
}

impl GraphDiagnostics for Diagnostics {
    fn find_problems(&self, _: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
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
        if self.is_failing {
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

fn open_storage(root: &TempDir) -> TestDatabases {
    crate::storage::open_test_storage(
        &root.path().join("cache.db"),
        &root.path().join("vectors.db"),
    )
    .unwrap()
}

fn publish_node(storage: &mut TestDatabases, revision: usize) {
    let id = KnowledgeId::parse("a.md#one").unwrap();
    for _ in 0..revision {
        storage
            .cache_storage
            .with_write_tx(|| {
                let writer = &storage.cache_storage;
                writer
                    .nodes_upsert_by_id(&NodeMetadata::Knowledge(KnowledgeMetadata {
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

                writer.project_get_revision()
            })
            .unwrap();
    }
}
mod storage_status {
    use super::super::{EmbeddingStatusPolicy, EmbeddingStatusSummary, StatusService};
    use crate::{
        domain::Sha256,
        open_status_storage,
        service::{
            CacheDatabase, StorageError, StorageErrorKind, StorageRead, TransactionError,
            VectorDatabase, VectorOutcome,
        },
        storage::get_current,
    };
    use rusqlite::Connection;
    use std::{
        fs::{self, File},
        path::Path,
        rc::Rc,
    };
    use tempfile::TempDir;
    #[test]
    fn counts_distinct_current_hashes() {
        // Given: shared A, failed B, covered C, source and orphan D.
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(
            &writer,
            &[
                ("a.md", "A"),
                ("shared.md", "A"),
                ("b.md", "B"),
                ("c.md", "C"),
            ],
        );
        writer
            .cache_storage
            .with_write_tx(|| {
                let cache = &writer.cache_storage;
                cache
                    .nodes_upsert_by_id(&build_source("file://source", None, 7))
                    .unwrap();

                cache.project_get_revision()
            })
            .unwrap();
        store(&writer, &identity, "C");
        store(&writer, &identity, "D");
        fail(&writer, &identity, "B", 2);
        fail(&writer, &identity, "obsolete", 2);
        // When: status counts the current publication under unavailable and ready policies.
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;
                let unavailable =
                    get_embedding_status(read, &EmbeddingStatusPolicy::Unavailable).unwrap();
                let ready = get_embedding_status(read, &ready_policy(identity)).unwrap();
                // Then: independent literal counts and one exhausted live hash.
                assert_eq!((unavailable.backlog, unavailable.failed), (3, 0));
                assert_eq!((ready.backlog, ready.failed), (1, 1));
                assert_eq!(ready.exhausted.len(), 1);
                assert_eq!(ready.exhausted[0].search_hash, Sha256::digest(b"B"));
                assert_eq!(ready.exhausted[0].attempts, 2);

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();
    }

    #[test]
    fn respects_attempt_boundary() {
        // Given: a published corpus at the durable attempt boundary.
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
        fail(&writer, &identity, "A", 1);
        fail(&writer, &identity, "B", 2);
        fail(&writer, &identity, "obsolete", 2);
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        // When: status counts embedding work.
        let result = status
            .with_status_read_tx(|| {
                Ok::<_, StorageError>(get_embedding_status(&status, &ready_policy(identity)))
            })
            .unwrap()
            .unwrap()
            .unwrap();
        // Then: backlog and exhausted attempts remain distinct.
        assert_eq!((result.backlog, result.failed), (1, 1));
        assert_eq!(result.exhausted.len(), 1);
        assert_eq!(result.exhausted[0].search_hash, Sha256::digest(b"B"));
    }

    #[test]
    fn ignores_vectors_for_mismatched_identity() {
        // Given: stored vectors under an older identity.
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        let identity = Sha256::digest(b"old");
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
        store(&writer, &identity, "A");
        fail(&writer, &identity, "B", 2);
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;
                // When: status uses a different current identity.
                let result =
                    get_embedding_status(read, &ready_policy(Sha256::digest(b"new"))).unwrap();
                // Then: all current knowledge remains backlog.
                assert_eq!((result.backlog, result.failed), (3, 0));
                assert!(result.exhausted.is_empty());
                assert_eq!(read.meta_get_by_id().unwrap(), Some(identity));
                assert!(matches!(
                    read.vector_keys_get_by_search_hash(&identity, &Sha256::digest(b"A"))
                        .unwrap(),
                    VectorOutcome::Current(Some(_))
                ));

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();
    }

    #[test]
    fn opens_missing_files_without_creating_them() {
        // Given: a status reader with missing cache or vector files.
        let root = TempDir::new().unwrap();
        let pinned = File::open(root.path()).unwrap();
        let missing =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        // When / Then: status reads the available publication; missing database files are never created.
        assert!(
            missing
                .with_status_read_tx(|| Ok::<_, StorageError>(()))
                .unwrap()
                .is_none()
        );
        assert!(!root.path().join("cache.db").exists());
        assert!(!root.path().join("vectors.db").exists());
        drop(missing);
        let writer = open_writer(root.path());
        assert!(
            {
                let scope_databases =
                    open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap();
                scope_databases
                    .with_status_read_tx(|| Ok::<_, StorageError>(()))
                    .map_err(TransactionError::into_inner)
            }
            .unwrap()
            .is_none()
        );
        publish(&writer, &[("a.md", "A"), ("b.md", "A"), ("c.md", "B")]);
        drop(writer);
        // Given: a status reader with missing cache or vector files.
        fs::remove_file(root.path().join("vectors.db")).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        let summary = status
            .with_status_read_tx(|| {
                Ok::<_, StorageError>(get_embedding_status(
                    &status,
                    &ready_policy(Sha256::digest(b"model")),
                ))
            })
            .unwrap()
            .unwrap()
            .unwrap();
        // When / Then: status reads the available publication; missing database files are never created.
        assert_eq!((summary.backlog, summary.failed), (2, 0));
        assert!(!root.path().join("vectors.db").exists());
    }

    #[test]
    fn pins_status_revision_and_counts() {
        // Given: a status view pinned before a later publication.
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        let identity = Sha256::digest(b"model");
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A")]);
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;
                publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
                store(&writer, &identity, "A");
                // When: embedding status is counted.
                let summary = get_embedding_status(read, &ready_policy(identity)).unwrap();
                // Then: revision and counts remain from the pinned view.
                assert_eq!(read.project_get_by_id().unwrap(), 1);
                assert_eq!((summary.backlog, summary.failed), (1, 0));

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();
    }

    #[test]
    fn rejects_malformed_persisted_counts_and_read_failure() {
        // Given: malformed durable attempt counts or a failing status read.
        for attempts in ["-1", "0", "'invalid'", "1.5"] {
            let root = TempDir::new().unwrap();
            let writer = open_writer(root.path());
            let identity = Sha256::digest(b"model");
            writer
                .vector_storage
                .reset_vectors(None, &identity)
                .unwrap();
            publish(&writer, &[("a.md", "A")]);
            let connection = Connection::open(root.path().join("vectors.db")).unwrap();
            connection
                .execute_batch("pragma ignore_check_constraints=on")
                .unwrap();
            connection
                .execute(
                    &format!("insert into embed_failures values(?1,{attempts},null)"),
                    [Sha256::digest(b"A").to_hex()],
                )
                .unwrap();
            let pinned = File::open(root.path()).unwrap();
            let status = Rc::new(
                open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap(),
            );
            // When / Then: status decodes the stored state; malformed values and read failures propagate.
            assert!(
                status
                    .with_status_read_tx(|| Ok::<_, StorageError>(get_embedding_status(
                        &status,
                        &ready_policy(identity)
                    )))
                    .unwrap()
                    .unwrap()
                    .is_err(),
                "{attempts}"
            );
        }
        let root = TempDir::new().unwrap();
        let writer = open_writer(root.path());
        publish(&writer, &[("a.md", "A")]);
        let connection = Connection::open(root.path().join("cache.db")).unwrap();
        connection
            .execute_batch("update project set revision=-1")
            .unwrap();
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        // When / Then: status decodes the stored state; malformed values and read failures propagate.
        assert!(
            status
                .with_status_read_tx(|| Ok::<_, StorageError>(()))
                .map_err(TransactionError::into_inner)
                .is_err()
        );
        drop(status);
        connection.execute_batch("drop table project").unwrap();
        assert!(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).is_err());
    }

    #[test]
    fn preserves_files_and_orders_only_uncovered_current_failures() {
        // Given: known covered and missing hashes plus a durable query failure.
        let root = TempDir::new().unwrap();
        let identity = Sha256::digest(b"model");
        let writer = open_writer(root.path());
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A"), ("b.md", "B"), ("c.md", "C")]);
        store(&writer, &identity, "C");
        fail(&writer, &identity, "A", 2);
        fail(&writer, &identity, "B", 2);
        fail(&writer, &identity, "C", 2);
        writer
            .vector_storage
            .with_write_tx(&identity, || {
                let unit = &writer.vector_storage;
                unit.query_error_upsert_by_id(7, "safe query failure")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
        drop(writer);
        let before = [
            fs::read(root.path().join("cache.db")).unwrap(),
            fs::read(root.path().join("vectors.db")).unwrap(),
        ];
        // When: status reads embedding counts, failures and the query diagnostic.
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;
                let result = get_embedding_status(read, &ready_policy(identity)).unwrap();
                let query_error = read.query_error_get_by_id().unwrap().unwrap();
                // Then: hashes have known independent lexicographic order (A=559a..., B=df7e...).
                assert_eq!((result.backlog, result.failed), (0, 2));
                assert_eq!(
                    result
                        .exhausted
                        .iter()
                        .map(|f| f.search_hash)
                        .collect::<Vec<_>>(),
                    vec![Sha256::digest(b"A"), Sha256::digest(b"B")]
                );
                assert_eq!(query_error.at, 7);
                assert_eq!(query_error.error, "safe query failure");

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();

        drop(status);
        assert_eq!(fs::read(root.path().join("cache.db")).unwrap(), before[0]);
        assert_eq!(fs::read(root.path().join("vectors.db")).unwrap(), before[1]);
    }

    #[test]
    fn disabled_policy_returns_zero_without_touching_execution_state() {
        // Given: current knowledge, a broken executable vector row and stored failures.
        let root = TempDir::new().unwrap();
        let identity = Sha256::digest(b"model");
        let writer = open_writer(root.path());
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A"), ("b.md", "B")]);
        fail(&writer, &identity, "A", 2);
        writer
            .vector_storage
            .with_write_tx(&identity, || {
                let unit = &writer.vector_storage;
                unit.query_error_upsert_by_id(7, "safe query failure")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
        let connection = Connection::open(root.path().join("vectors.db")).unwrap();
        connection
            .execute(
                "insert into vector_keys(search_hash) values(?1)",
                [Sha256::digest(b"B").to_hex()],
            )
            .unwrap();
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;

                // When: embedding status is requested.
                let disabled =
                    get_embedding_status(read, &EmbeddingStatusPolicy::Disabled).unwrap();

                // Then: no coverage or stored-failure read can fail or leak into disabled counts.
                assert_eq!((disabled.backlog, disabled.failed), (0, 0));
                assert!(disabled.exhausted.is_empty());
                assert!(read.query_error_get_by_id().unwrap().is_some());

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();
    }

    #[test]
    fn rejects_current_vector_key_without_native_row() {
        // Given: a published current hash whose key has no corresponding native vector.
        let root = TempDir::new().unwrap();
        let identity = Sha256::digest(b"model");
        let hash = Sha256::digest(b"A");
        let writer = open_writer(root.path());
        writer
            .vector_storage
            .reset_vectors(None, &identity)
            .unwrap();
        publish(&writer, &[("a.md", "A")]);
        let connection = Connection::open(root.path().join("vectors.db")).unwrap();
        connection
            .execute(
                "insert into vector_keys(search_hash) values(?1)",
                [hash.to_hex()],
            )
            .unwrap();
        let pinned = File::open(root.path()).unwrap();
        let status =
            Rc::new(open_status_storage(&root.path().canonicalize().unwrap(), &pinned).unwrap());
        status
            .with_status_read_tx(|| {
                let read = &status;
                // When: status compares unavailable and ready coverage policies.
                let unavailable =
                    get_embedding_status(read, &EmbeddingStatusPolicy::Unavailable).unwrap();
                let error = get_embedding_status(read, &ready_policy(identity)).unwrap_err();
                let vector_error = read
                    .vector_keys_get_by_search_hash(&identity, &hash)
                    .unwrap_err();
                // Then: configuration-unavailable counting ignores old vectors, but ready coverage fails safely.
                assert_eq!((unavailable.backlog, unavailable.failed), (1, 0));
                assert_eq!(error.get_kind(), StorageErrorKind::InternalError);
                assert_eq!(error.get_message(), "invalid stored vector data");
                assert_eq!(vector_error.get_kind(), StorageErrorKind::InternalError);
                assert_eq!(vector_error.get_message(), "invalid stored vector data");

                Ok::<_, StorageError>(())
            })
            .unwrap()
            .unwrap();
    }

    fn open_writer(root: &Path) -> crate::storage::TestDatabases {
        crate::storage::open_test_storage(&root.join("cache.db"), &root.join("vectors.db")).unwrap()
    }
    fn ready_policy(identity: Sha256) -> EmbeddingStatusPolicy {
        EmbeddingStatusPolicy::Ready {
            identity,
            max_attempts: 2,
        }
    }
    fn publish(writer: &crate::storage::TestDatabases, rows: &[(&str, &str)]) {
        writer
            .cache_storage
            .with_write_tx(|| {
                let unit = &writer.cache_storage;
                for (id, text) in rows {
                    unit.nodes_upsert_by_id(&build_knowledge(id, text)).unwrap();
                }

                unit.project_get_revision()
            })
            .unwrap();
    }
    fn store(writer: &crate::storage::TestDatabases, identity: &Sha256, text: &str) {
        writer
            .vector_storage
            .with_write_tx(identity, || {
                let unit = &writer.vector_storage;
                unit.store_vector(&Sha256::digest(text.as_bytes()), &[1.0, 0.0, 0.0], 3)
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
    }
    fn fail(writer: &crate::storage::TestDatabases, identity: &Sha256, text: &str, attempts: u64) {
        writer
            .vector_storage
            .with_write_tx(identity, || {
                let unit = &writer.vector_storage;
                for _ in 0..attempts {
                    assert!(
                        unit.embed_failures_upsert_by_search_hash_and_attempts(
                            &Sha256::digest(text.as_bytes()),
                            2
                        )
                        .unwrap()
                    );
                }
                unit.embed_failures_update_by_search_hash(
                    &Sha256::digest(text.as_bytes()),
                    "safe failure",
                )
                .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
    }

    fn get_embedding_status<C: CacheDatabase>(
        storage: &Rc<C>,
        policy: &EmbeddingStatusPolicy,
    ) -> crate::service::StorageResult<EmbeddingStatusSummary> {
        let files = crate::service::MockFileReader::new();
        let provider = super::build_provider();
        let service = StatusService {
            cache: Rc::clone(storage),
            file_reader: &files,
            graph: &super::Diagnostics,
            provenance: &super::Provenance { is_failing: false },
            provider: &provider,
        };
        service.get_embedding_status_by_policy(policy)
    }
    fn build_knowledge(id: &str, text: &str) -> crate::service::NodeMetadata {
        use crate::{
            domain::{ElementKind, KnowledgeId, LineRange, Sha256},
            service::{KnowledgeMetadata, NodeMetadata},
        };
        NodeMetadata::Knowledge(KnowledgeMetadata {
            id: KnowledgeId::parse(id).unwrap(),
            uri: KnowledgeId::parse(id).unwrap(),
            kind: ElementKind::Vertex,
            hash: Sha256::digest(text.as_bytes()),
            created_at: 7,
            source_snapshot: None,
            lines: LineRange::new(1, 1).unwrap(),
            search_hash: Sha256::digest(text.as_bytes()),
        })
    }

    fn build_source(
        uri: &str,
        hash: Option<crate::domain::Sha256>,
        created_at: u64,
    ) -> crate::service::NodeMetadata {
        use crate::{
            domain::SourceUri,
            service::{NodeMetadata, SourceMetadata},
        };
        NodeMetadata::Source(SourceMetadata {
            uri: SourceUri::parse(uri).unwrap(),
            hash,
            created_at,
            lines: None,
        })
    }
}
