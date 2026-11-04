use super::{
    CacheDatabase, CommandError, CommandErrorKind, CommandProblem, CommandResult, Configuration,
    EmbeddingProvider, EmbeddingStatusPolicy, EmbeddingStatusSummary, GraphDiagnostics,
    SourceProvenance, StatusFiles, StatusReader, StatusReport, StorageError,
    utils::{CheckObservations, sort_problems},
};
use serde_json::Value;
use std::{collections::BTreeMap, fmt, rc::Rc};

const REASON: &str = "reason";
const INVALID_CONFIG: &str = "invalid_embedding_config";
const QUERY_FAILURE: &str = "query_embedding_failed";
const EXHAUSTED: &str = "embedding_attempts_exhausted";

/// Observes the last publication and live coordination without changing either.
pub(crate) struct StatusService<'a, C> {
    pub(crate) storage: Rc<C>,
    pub(crate) files: &'a dyn StatusFiles,
    pub(crate) graph: &'a dyn GraphDiagnostics,
    pub(crate) provenance: &'a dyn SourceProvenance,
    pub(crate) provider: &'a dyn EmbeddingProvider,
}

impl<C> fmt::Debug for StatusService<'_, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StatusService").finish_non_exhaustive()
    }
}

impl<C: CacheDatabase> StatusReader for StatusService<'_, C> {
    fn get_status(&mut self) -> CommandResult<StatusReport> {
        let (policy, config_problem) = self.get_config_policy();
        let published = self
            .storage
            .with_status_read_tx(|| {
                let observations = CheckObservations {
                    storage: self.storage.as_ref(),
                    graph: self.graph,
                    provenance: self.provenance,
                };
                let problems = observations.collect_check_problems(&[])?;
                self.get_published(&policy, problems)
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => map_storage_error(error),
            })?;
        let mut report = published.unwrap_or(StatusReport {
            revision: None,
            embedding_backlog: 0,
            embedding_failed: 0,
            locks: Vec::new(),
            problems: Vec::new(),
        });
        if let Some(problem) = config_problem {
            report.problems.push(problem);
        }
        report.locks = self.files.observe_locks()?;
        report.problems = sort_problems(report.problems);
        Ok(report)
    }
}

impl<C: CacheDatabase> StatusService<'_, C> {
    fn get_config_policy(&self) -> (EmbeddingStatusPolicy, Option<CommandProblem>) {
        let Ok(bytes) = self.files.get_config() else {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        };
        let Some(bytes) = bytes else {
            return (EmbeddingStatusPolicy::Unavailable, None);
        };
        let Ok(configuration) = Configuration::parse(&bytes) else {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        };
        if let Some(config) = configuration.get_embedding_config()
            && self.provider.validate_config(config).is_err()
        {
            return (
                EmbeddingStatusPolicy::Unavailable,
                Some(invalid_config_problem()),
            );
        }
        match configuration {
            Configuration::Enabled(config) => (
                EmbeddingStatusPolicy::Ready {
                    identity: config.get_identity(),
                    max_attempts: config.get_max_attempts(),
                },
                None,
            ),
            Configuration::Unconfigured { .. } | Configuration::Disabled(_) => {
                (EmbeddingStatusPolicy::Disabled, None)
            }
        }
    }

    fn get_published(
        &self,
        policy: &EmbeddingStatusPolicy,
        mut problems: Vec<CommandProblem>,
    ) -> CommandResult<StatusReport> {
        let revision = self
            .storage
            .project_get_by_id()
            .map_err(map_storage_error)?;
        let summary = self
            .storage
            .get_embedding_status(policy)
            .map_err(map_storage_error)?;
        if !matches!(policy, EmbeddingStatusPolicy::Disabled) {
            problems.extend(collect_exhausted(&summary));
            if self
                .storage
                .query_error_get_by_id()
                .map_err(map_storage_error)?
                .is_some()
            {
                problems.push(build_problem(
                    CommandErrorKind::EmbeddingFailed,
                    "query embedding failed",
                    QUERY_FAILURE,
                    None,
                ));
            }
        }
        Ok(StatusReport {
            revision: Some(revision),
            embedding_backlog: summary.backlog,
            embedding_failed: summary.failed,
            locks: Vec::new(),
            problems,
        })
    }
}

fn collect_exhausted(summary: &EmbeddingStatusSummary) -> Vec<CommandProblem> {
    summary
        .exhausted
        .iter()
        .map(|failure| {
            build_problem(
                CommandErrorKind::EmbeddingFailed,
                "embedding attempts exhausted",
                EXHAUSTED,
                Some(&failure.search_hash.to_hex()),
            )
        })
        .collect()
}

fn invalid_config_problem() -> CommandProblem {
    build_problem(
        CommandErrorKind::ValidationFailed,
        "invalid embedding configuration",
        INVALID_CONFIG,
        None,
    )
}

fn build_problem(
    code: CommandErrorKind,
    message: &'static str,
    reason: &'static str,
    target: Option<&str>,
) -> CommandProblem {
    let mut details = BTreeMap::from([(REASON.into(), Value::from(reason))]);
    if let Some(target) = target {
        details.insert("target".into(), Value::from(target));
    }
    CommandProblem {
        code,
        message,
        details,
    }
}

fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read published status",
        cause,
    )
}

#[cfg(test)]
mod tests {
    use crate::service::StorageError;
    use crate::service::TransactionError;
    use crate::storage::get_current;

    mod configuration {
        use crate::service::StorageError;

        use crate::storage::get_current;

        use super::{
            CommandErrorKind, CommandProblem, CommandResult, Diagnostics, Files, GraphDiagnostics,
            Provenance, Reads, RepoPath, Sha256, StatusReader, StatusService, TestDatabases,
            VALID_CONFIG, VectorDatabase, build_provider, build_service, open_storage,
            publish_node,
        };
        use crate::service::Configuration;
        use std::{cell::Cell, rc::Rc};
        use tempfile::TempDir;

        struct ChangingDiagnostics<'a>(&'a Files);

        #[test]
        fn keeps_captured_config_after_live_config_changes() {
            // Given
            let root = TempDir::new().unwrap();
            let mut storage = open_storage(&root);
            publish_node(&mut storage, 1);
            seed_exhausted(&mut storage);
            let files = build_files(Some(VALID_CONFIG.to_vec()));
            let provider = build_provider();
            let graph = ChangingDiagnostics(&files);
            let mut service = StatusService {
                storage: Rc::new(Reads {
                    inner: storage.cache,
                    count: Rc::new(Cell::new(0)),
                }),
                files: &files,
                graph: &graph,
                provenance: &Provenance { fail: false },
                provider: &provider,
            };

            // When
            let report = service.get_status().unwrap();

            // Then
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
                &Provenance { fail: false },
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
                fail: false,
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
            storage.vectors.reset_vectors(None, &identity).unwrap();
            storage
                .vectors
                .with_write_tx(&identity, || {
                    let writer = &storage.vectors;
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
        domain::{ElementKind, KnowledgeId, LineRange, RepoPath, Sha256, TreeOid},
        service::{
            CacheDatabase, CommandErrorKind, CommandProblem, CommandResult, DocumentError,
            DocumentErrorKind, DocumentResult, EmbeddingConfig, EmbeddingError, EmbeddingErrorKind,
            EmbeddingProvider, EmbeddingResult, GraphDiagnostics, HeldLock, KnowledgeMetadata,
            NodeMetadata, ProvenanceIssue, ProvenanceObservation, ProvenanceRequest, SnapshotError,
            SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, StatusFiles,
            StatusReader, VectorDatabase,
        },
        storage::TestDatabases,
    };
    use std::{cell::Cell, collections::BTreeSet, num::NonZeroUsize, rc::Rc};
    use tempfile::TempDir;

    const VALID_CONFIG: &[u8] = br#"{"embedding_model":{"enabled":true,"provider":"ollama","model":"fixture","parameters":{"base_url":"http://localhost/api","dimensions":3},"max_retries":0}}"#;

    struct Reads {
        inner: crate::storage::CacheStorage,
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
        storage.vectors.reset_vectors(None, &identity).unwrap();
        let hash = Sha256::digest(b"search");
        storage
            .vectors
            .with_write_tx(&identity, || {
                let unit = &storage.vectors;
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
        storage.vectors.reset_vectors(None, &identity).unwrap();
        storage
            .vectors
            .with_write_tx(&identity, || {
                let writer = &storage.vectors;
                writer
                    .query_error_upsert_by_id(7, "safe query failure")
                    .unwrap();

                Ok::<_, StorageError>(())
            })
            .map(get_current)
            .unwrap();
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
        storage: Option<TestDatabases>,
        count: Rc<Cell<usize>>,
        files: &'a Files,
        provider: &'a MockProvider,
        provenance: &'a Provenance,
    ) -> StatusService<'a, Reads> {
        StatusService {
            storage: Rc::new(Reads {
                inner: storage
                    .map(|storage| storage.cache)
                    .unwrap_or_else(|| crate::storage::CacheStorage::new_observed(None)),
                count,
            }),
            files,
            graph: &Diagnostics,
            provenance,
            provider,
        }
    }

    use crate::service::utils::cache_test_support::{
        delegate_cache_reads, delegate_cache_writes, delegate_graph_reads,
    };
    impl crate::service::StorageRead for Reads {
        delegate_cache_reads!();
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
        fn with_status_read_tx<T, E, F>(
            &self,
            operation: F,
        ) -> Result<Option<T>, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            self.count.set(self.count.get() + 1);
            self.inner.with_status_read_tx(operation)
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
        fn observe_sources(
            &self,
            _: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
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
                .cache
                .with_write_tx(|| {
                    let writer = &storage.cache;
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

                    writer.get_publication_revision()
                })
                .unwrap();
        }
    }
}
