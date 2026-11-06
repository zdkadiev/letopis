use super::utils::map_document_error;
use super::{
    CacheDatabase, CheckReport, Checker, CommandError, CommandErrorKind, CommandResult,
    GraphDiagnostics, Refresher, SourceProvenance, StorageError, utils::CheckObservations,
};
use crate::domain::RepoPath;
use std::{fmt, rc::Rc};

/// Refreshes and validates current knowledge through one pinned cache view.
pub(crate) struct CheckService<D, P, C> {
    refresher: D,
    storage: Rc<C>,
    diagnostics: Box<dyn GraphDiagnostics>,
    provenance: P,
}

impl<D, P, C> fmt::Debug for CheckService<D, P, C> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("CheckService")
            .finish_non_exhaustive()
    }
}

impl<D, P, C> CheckService<D, P, C> {
    /// Assemble validation collaborators without refreshing or opening a read view.
    pub(crate) fn new(
        refresher: D,
        storage: Rc<C>,
        diagnostics: Box<dyn GraphDiagnostics>,
        provenance: P,
    ) -> Self {
        Self {
            refresher,
            storage,
            diagnostics,
            provenance,
        }
    }
}

impl<D: Refresher, P: SourceProvenance, C: CacheDatabase> Checker for CheckService<D, P, C> {
    fn check(&mut self, paths: &[RepoPath]) -> CommandResult<CheckReport> {
        let outcome = if paths.is_empty() {
            self.refresher.refresh_verified(&[])
        } else {
            self.refresher.refresh(&[])
        }
        .map_err(|cause| map_document_error(cause, "cannot refresh corpus for check"))?;
        self.storage
            .with_read_tx(|| {
                let view: &dyn super::StorageRead = self.storage.as_ref();
                if paths.is_empty()
                    && view.project_get_by_id().map_err(map_storage_error)? != outcome.revision
                {
                    return Err(CommandError::new(
                        CommandErrorKind::InternalError,
                        "corpus changed during check; retry",
                    ));
                }
                let observations = CheckObservations {
                    storage: self.storage.as_ref(),
                    graph: self.diagnostics.as_ref(),
                    provenance: &self.provenance,
                };
                let errors = observations.collect_check_problems(paths)?;
                Ok(CheckReport { errors })
            })
            .map_err(|error| match error {
                super::TransactionError::Operation(error) => error,
                super::TransactionError::Storage(error) => map_storage_error(error),
            })
    }
}

fn map_storage_error(cause: StorageError) -> CommandError {
    CommandError::new_with_cause(
        CommandErrorKind::InternalError,
        "cannot read corpus for check",
        cause,
    )
}
#[cfg(test)]
mod tests {
    use crate::service::TransactionError;

    use super::CheckService;
    use crate::{
        domain::{
            Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256,
            SourceUri, TreeOid,
        },
        service::{
            CacheDatabase, Checker, CommandError, CommandErrorKind, CommandProblem, CommandResult,
            DocumentError, DocumentErrorKind, DocumentResult, GraphDiagnostics,
            GraphDiagnosticsService, KnowledgeMetadata, NodeMetadata, ProvenanceIssue,
            ProvenanceObservation, ProvenanceRequest, RefreshOutcome, Refresher, SearchFlag,
            SnapshotError, SnapshotState, SnapshotVerifier, SourceProvenance,
            SourceRevisionComparison, SourceRevisionRequest, StorageError, StorageErrorKind,
            StorageRead,
        },
        storage::TestDatabases,
    };
    use serde_json::{Value, json};
    use std::{cell::RefCell, collections::BTreeSet, error::Error, io, rc::Rc};
    use tempfile::TempDir;

    type Events = Rc<RefCell<Vec<&'static str>>>;
    type Probes = Rc<RefCell<Vec<String>>>;

    struct Refresh<F>(F);

    struct RecordingStorage<S> {
        inner: S,
        events: Events,
        fail_read: bool,
        active: std::cell::Cell<bool>,
    }

    struct RecordingDiagnostics {
        storage: Rc<dyn StorageRead>,
        inner: Box<dyn GraphDiagnostics>,
        events: Events,
    }

    struct PublishingDiagnostics {
        storage: Rc<dyn StorageRead>,
        inner: Box<dyn GraphDiagnostics>,
        writer: RefCell<TestDatabases>,
    }

    struct FindingDiagnostics;

    struct Verifier {
        probes: Probes,
        result: SnapshotState,
        fail: bool,
    }

    #[test]
    fn whole_check_requests_verified_refresh_and_rejects_a_newer_revision() {
        // Given: the audit completed at revision 0; another writer publishes revision 1.
        let directory = tempfile::tempdir().unwrap();
        let mut storage = open_storage(&directory);
        publish_nodes(&mut storage, &[build_node("fresh.md", "a")]);
        let mut refresher = crate::service::MockRefresher::new();
        refresher
            .expect_refresh_verified()
            .once()
            .returning(|paths| {
                assert!(paths.is_empty());
                Ok(RefreshOutcome {
                    revision: 0,
                    ..build_outcome()
                })
            });
        let mut service = CheckService::new(
            refresher,
            Rc::new(storage.cache),
            Box::new(FindingDiagnostics),
            Verifier {
                probes: Probes::default(),
                result: SnapshotState::Available,
                fail: false,
            },
        );
        // When: full check opens an unaudited view.
        let error = service.check(&[]).unwrap_err();
        // Then: it cannot report that revision as clean.
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(error.get_message().contains("changed"));
    }

    #[test]
    fn scoped_check_requests_routine_refresh() {
        // Given: a nonempty selection, even when it names every stored file.
        let directory = tempfile::tempdir().unwrap();
        let mut storage = open_storage(&directory);
        publish_nodes(&mut storage, &[build_node("fresh.md", "a")]);
        let mut refresher = crate::service::MockRefresher::new();
        refresher.expect_refresh().once().returning(|paths| {
            assert!(paths.is_empty());
            Ok(build_outcome())
        });
        let mut service = CheckService::new(
            refresher,
            Rc::new(storage.cache),
            Box::new(FindingDiagnostics),
            Verifier {
                probes: Probes::default(),
                result: SnapshotState::Available,
                fail: false,
            },
        );
        // When: the selected corpus is checked.
        let report = service
            .check(&[RepoPath::parse("fresh.md").unwrap()])
            .unwrap();
        // Then: regular diagnostics are returned without an exhaustive audit.
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn check_reports_missing_address_from_selected_knowledge() {
        // Given: selected knowledge whose source address is missing.
        let directory = tempfile::tempdir().unwrap();
        let mut storage = open_storage(&directory);
        publish_nodes(&mut storage, &[build_node("a.md#one", "a")]);
        storage
            .cache
            .with_write_tx(|| {
                let unit = &storage.cache;
                unit.edges_save_by_source_id_and_target_id_and_type(
                    &Edge::new(
                        KnowledgeId::parse("a.md#one").unwrap(),
                        EdgeTarget::Source(SourceUri::parse("gone.rs").unwrap()),
                        EdgeType::parse("external").unwrap(),
                    )
                    .unwrap(),
                )
                .unwrap();

                unit.get_publication_revision()
            })
            .unwrap();
        let mut service = CheckService::new(
            Refresh(|| Ok(build_outcome())),
            Rc::new(storage.cache),
            Box::new(FindingDiagnostics),
            Verifier {
                probes: Probes::default(),
                result: SnapshotState::Available,
                fail: false,
            },
        );

        // When: check runs for the selected file.
        let report = service.check(&[RepoPath::parse("a.md").unwrap()]).unwrap();

        // Then: the report includes the missing source finding.
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[1].details["reason"], "source_not_found");
        assert_eq!(report.errors[1].details["path"], "a.md");
        assert_eq!(report.errors[1].details["target"], "gone.rs");
    }

    #[test]
    fn check_refreshes_before_opening_one_read() {
        // Given: recorded refresh, read and diagnostic events.
        let directory = tempfile::tempdir().unwrap();
        let storage = open_storage(&directory);
        let writer = open_storage(&directory);
        let events = Events::default();
        let refresh_events = events.clone();
        let refresher = Refresh(move || {
            refresh_events.borrow_mut().push("refresh");
            writer
                .cache
                .with_write_tx(|| {
                    let unit = &writer.cache;
                    unit.nodes_upsert_by_id(&build_node("fresh.md", "a"))
                        .unwrap();

                    unit.get_publication_revision()
                })
                .unwrap();
            Ok(build_outcome())
        });
        let probes = Probes::default();
        let cache = Rc::new(RecordingStorage {
            active: std::cell::Cell::new(false),
            inner: storage.cache,
            events: events.clone(),
            fail_read: false,
        });
        let mut service = CheckService::new(
            refresher,
            Rc::clone(&cache),
            Box::new(RecordingDiagnostics::new(Rc::clone(&cache), events.clone())),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Missing,
                fail: false,
            },
        );

        // When: check runs over the repository.
        let report = service.check(&[]).unwrap();

        // Then: refresh precedes one pinned read and its diagnostics.
        assert_eq!(*events.borrow(), ["refresh", "read", "diagnostics"]);
        assert_eq!(*probes.borrow(), ["a".repeat(40)]);
        assert_eq!(
            get_findings(&report.errors),
            json!([
                {"path":"fresh.md","reason":"source_snapshot_unavailable","element_id":"fresh.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
            ])
        );
    }

    #[test]
    fn check_reuses_same_view_for_each_knowledge_observation() {
        // Given
        let directory = tempfile::tempdir().unwrap();
        let mut writer = open_storage(&directory);
        publish_nodes(
            &mut writer,
            &[build_node("a.md", "a"), build_node("b.md", "a")],
        );
        let probes = Probes::default();
        let cache = Rc::new(open_storage(&directory).cache);
        let diagnostics = PublishingDiagnostics {
            storage: cache.clone(),
            inner: Box::new(GraphDiagnosticsService::new(Rc::clone(&cache))),
            writer: RefCell::new(writer),
        };
        let mut revision = 0;
        let mut service = CheckService::new(
            Refresh(move || {
                revision += 1;
                Ok(RefreshOutcome {
                    revision,
                    ..build_outcome()
                })
            }),
            Rc::clone(&cache),
            Box::new(diagnostics),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Invalid,
                fail: false,
            },
        );

        // When
        let report = service.check(&[]).unwrap();

        // Then
        assert_eq!(*probes.borrow(), ["a".repeat(40), "a".repeat(40)]);
        assert_eq!(
            get_findings(&report.errors),
            json!([
                {"path":"a.md","reason":"source_snapshot_unavailable","element_id":"a.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},
                {"path":"b.md","reason":"source_snapshot_unavailable","element_id":"b.md","target":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}
            ])
        );
        // When: a new invocation observes the newly committed metadata.
        let report = service.check(&[]).unwrap();
        // Then
        assert_eq!(
            *probes.borrow(),
            [
                "a".repeat(40),
                "a".repeat(40),
                "b".repeat(40),
                "b".repeat(40)
            ]
        );
        assert_eq!(report.errors.len(), 2);
        assert_eq!(report.errors[0].details["target"], "b".repeat(40));
    }

    #[test]
    fn check_propagates_refresh_storage_and_git_failures_without_partial_report() {
        // Given
        let directory = tempfile::tempdir().unwrap();
        let events = Events::default();
        let probes = Probes::default();
        // When / Then
        assert_refresh_failure(&directory, &events, &probes);
        assert_storage_failure(&directory, &events, &probes);
        // Given
        let mut storage = open_storage(&directory);
        publish_nodes(&mut storage, &[build_node("a.md", "a")]);
        let mut git_failure = CheckService::new(
            Refresh(|| Ok(build_outcome())),
            Rc::new(storage.cache),
            Box::new(FindingDiagnostics),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Available,
                fail: true,
            },
        );

        // When / Then
        assert_private_failure(&git_failure.check(&[]).unwrap_err());

        // Then
        assert_eq!(*probes.borrow(), ["a".repeat(40)]);
    }

    #[test]
    fn check_filters_snapshot_owners_before_probing_git() {
        // Given
        let directory = tempfile::tempdir().unwrap();
        let mut storage = open_storage(&directory);
        publish_nodes(
            &mut storage,
            &[build_node("a.md#one", "a"), build_node("b.md", "b")],
        );
        let probes = Probes::default();
        let cache = Rc::new(storage.cache);
        let mut service = CheckService::new(
            Refresh(|| Ok(build_outcome())),
            Rc::clone(&cache),
            Box::new(GraphDiagnosticsService::new(cache)),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Missing,
                fail: true,
            },
        );

        // When / Then
        assert!(
            service
                .check(&[RepoPath::parse("absent.md").unwrap()])
                .unwrap()
                .errors
                .is_empty()
        );
        // Then
        assert!(probes.borrow().is_empty());
    }

    #[test]
    fn check_propagates_metadata_read_failure_after_graph_findings() {
        // Given
        let directory = tempfile::tempdir().unwrap();
        let storage = open_storage(&directory);
        rusqlite::Connection::open(directory.path().join("cache.db"))
            .unwrap()
            .execute("drop table nodes", [])
            .unwrap();
        let mut service = CheckService::new(
            Refresh(|| {
                Ok(RefreshOutcome {
                    revision: 0,
                    ..build_outcome()
                })
            }),
            Rc::new(storage.cache),
            Box::new(FindingDiagnostics),
            Verifier {
                probes: Probes::default(),
                result: SnapshotState::Available,
                fail: false,
            },
        );

        // When
        let error = service.check(&[]).unwrap_err();

        // Then
        assert_private_failure(&error);
        assert_eq!(error.get_message(), "cannot read corpus for check");
    }

    impl<F: FnMut() -> DocumentResult<RefreshOutcome>> Refresher for Refresh<F> {
        fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            (self.0)()
        }
        fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            panic!("this scenario must not trigger a full reindex")
        }

        fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
            (self.0)()
        }
    }

    use crate::service::utils::cache_test_support::{
        delegate_cache_reads, delegate_cache_writes, delegate_graph_reads,
    };
    impl<S: crate::service::CacheDatabase> crate::service::StorageRead for RecordingStorage<S> {
        delegate_cache_reads!();
    }
    impl<S: crate::service::CacheDatabase> crate::service::GraphRead for RecordingStorage<S> {
        delegate_graph_reads!();
    }
    impl<S: crate::service::CacheDatabase> crate::service::CacheDatabase for RecordingStorage<S> {
        delegate_cache_writes!();
        fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
        where
            F: FnOnce() -> Result<T, E>,
        {
            let nested = self.active.get();
            if !nested {
                self.events.borrow_mut().push("read");
            }
            if self.fail_read {
                return Err(TransactionError::Storage(StorageError::new_with_cause(
                    StorageErrorKind::InternalError,
                    "safe storage",
                    io::Error::other("SECRET_STORAGE"),
                )));
            }
            self.active.set(true);
            let result = self.inner.with_read_tx(operation);
            self.active.set(nested);
            result
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
            self.inner.with_status_read_tx(operation)
        }
    }

    impl RecordingDiagnostics {
        fn new<C: crate::service::CacheDatabase + 'static>(storage: Rc<C>, events: Events) -> Self {
            Self {
                storage: storage.clone(),
                inner: Box::new(GraphDiagnosticsService::new(storage)),
                events,
            }
        }
    }
    impl GraphDiagnostics for RecordingDiagnostics {
        fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
            self.events.borrow_mut().push("diagnostics");
            assert_eq!(self.storage.project_get_by_id().unwrap(), 1);
            self.inner.find_problems(paths)
        }
    }

    impl GraphDiagnostics for PublishingDiagnostics {
        fn find_problems(&self, paths: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
            let revision = self.storage.project_get_by_id().unwrap();
            let findings = self.inner.find_problems(paths)?;
            publish_nodes(
                &mut self.writer.borrow_mut(),
                &[build_node("a.md", "b"), build_node("b.md", "b")],
            );
            assert_eq!(self.storage.project_get_by_id().unwrap(), revision);
            Ok(findings)
        }
    }

    impl GraphDiagnostics for FindingDiagnostics {
        fn find_problems(&self, _: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
            Ok(vec![CommandProblem {
                code: CommandErrorKind::ValidationFailed,
                message: "knowledge target is missing",
                details: Default::default(),
            }])
        }
    }

    impl SourceProvenance for Verifier {
        fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
            panic!("this scenario does not validate requested revisions")
        }
        fn observe_sources(
            &self,
            request: ProvenanceRequest<'_>,
        ) -> DocumentResult<ProvenanceObservation> {
            let mut issues = BTreeSet::new();
            if let Some(snapshot) = request.source_snapshot {
                let state = self.tree_verify_by_oid(snapshot).map_err(|cause| {
                    DocumentError::new_with_cause(
                        DocumentErrorKind::InternalError,
                        "safe provenance",
                        cause,
                    )
                })?;
                if state != SnapshotState::Available {
                    issues.insert(ProvenanceIssue::SnapshotUnavailable {
                        snapshot: snapshot.clone(),
                    });
                }
            }
            for source in request.sources {
                issues.insert(ProvenanceIssue::SourceNotFound {
                    source: source.clone(),
                });
            }
            Ok(ProvenanceObservation {
                flags: BTreeSet::from([SearchFlag::SourceNotFound]),
                issues,
            })
        }
        fn compare_revision(
            &self,
            _: SourceRevisionRequest<'_>,
        ) -> Result<SourceRevisionComparison, SnapshotError> {
            panic!("check does not compare requested revisions")
        }
    }

    impl SnapshotVerifier for Verifier {
        fn tree_verify_by_oid(&self, oid: &TreeOid) -> Result<SnapshotState, SnapshotError> {
            self.probes.borrow_mut().push(oid.as_str().into());
            if self.fail {
                return Err(SnapshotError::new_with_cause(
                    "safe Git",
                    io::Error::other("SECRET_GIT"),
                ));
            }
            Ok(self.result)
        }
    }
    fn assert_refresh_failure(directory: &TempDir, events: &Events, probes: &Probes) {
        let cache = Rc::new(RecordingStorage {
            active: std::cell::Cell::new(false),
            inner: open_storage(directory).cache,
            events: events.clone(),
            fail_read: false,
        });
        let mut refresh_failure = CheckService::new(
            Refresh(|| {
                Err(DocumentError::new_with_cause(
                    DocumentErrorKind::InternalError,
                    "safe refresh",
                    io::Error::other("SECRET_REFRESH"),
                ))
            }),
            Rc::clone(&cache),
            Box::new(RecordingDiagnostics::new(Rc::clone(&cache), events.clone())),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Available,
                fail: false,
            },
        );
        assert_private_failure(&refresh_failure.check(&[]).unwrap_err());
        assert!(events.borrow().is_empty());
    }

    fn assert_storage_failure(directory: &TempDir, events: &Events, probes: &Probes) {
        let cache = Rc::new(RecordingStorage {
            active: std::cell::Cell::new(false),
            inner: open_storage(directory).cache,
            events: events.clone(),
            fail_read: true,
        });
        let mut storage_failure = CheckService::new(
            Refresh(|| Ok(build_outcome())),
            Rc::clone(&cache),
            Box::new(RecordingDiagnostics::new(Rc::clone(&cache), events.clone())),
            Verifier {
                probes: probes.clone(),
                result: SnapshotState::Available,
                fail: false,
            },
        );
        assert_private_failure(&storage_failure.check(&[]).unwrap_err());
        assert_eq!(*events.borrow(), ["read"]);
    }

    fn open_storage(directory: &TempDir) -> TestDatabases {
        crate::storage::open_test_storage(
            &directory.path().join("cache.db"),
            &directory.path().join("vectors.db"),
        )
        .unwrap()
    }
    fn publish_nodes(storage: &mut TestDatabases, nodes: &[NodeMetadata]) {
        storage
            .cache
            .with_write_tx(|| {
                let unit = &storage.cache;
                for node in nodes {
                    unit.nodes_upsert_by_id(node).unwrap();
                }

                unit.get_publication_revision()
            })
            .unwrap();
    }
    fn build_node(id: &str, oid: &str) -> NodeMetadata {
        let id = KnowledgeId::parse(id).unwrap();
        NodeMetadata::Knowledge(KnowledgeMetadata {
            uri: id.clone(),
            id,
            kind: ElementKind::Vertex,
            hash: Sha256::digest(b"content"),
            created_at: 42,
            source_snapshot: Some(TreeOid::parse(&oid.repeat(40)).unwrap()),
            lines: LineRange::new(1, 2).unwrap(),
            search_hash: Sha256::digest(b"search"),
        })
    }
    fn build_outcome() -> RefreshOutcome {
        RefreshOutcome {
            revision: 1,
            published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        }
    }
    fn get_findings(problems: &[CommandProblem]) -> Value {
        Value::Array(
            problems
                .iter()
                .map(|problem| {
                    assert_eq!(problem.code, CommandErrorKind::ValidationFailed);
                    assert_eq!(problem.message, "knowledge source snapshot is unavailable");
                    json!(problem.details)
                })
                .collect(),
        )
    }
    fn assert_private_failure(error: &CommandError) {
        assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
        assert!(error.source().is_some());
        assert!(!format!("{error} {error:?}").contains("SECRET_"));
    }

    mod pinned {
        use crate::service::StorageError;
        use crate::service::TransactionError;

        use super::{Refresh, build_node, build_outcome, open_storage, publish_nodes};
        use crate::{
            domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, RepoPath, SourceUri, TreeOid},
            service::{
                CacheDatabase, Checker, CommandProblem, CommandResult, DocumentResult,
                GraphDiagnostics, ProvenanceObservation, ProvenanceRequest, SnapshotError,
                SourceProvenance, SourceRevisionComparison, SourceRevisionRequest, StorageRead,
                check::CheckService,
            },
            storage::TestDatabases,
        };
        use std::{
            cell::{Cell, RefCell},
            collections::BTreeSet,
            rc::Rc,
        };

        struct PublishingBindings(RefCell<TestDatabases>, Rc<dyn StorageRead>);
        type RecordedSources = Vec<(Option<TreeOid>, BTreeSet<SourceUri>)>;
        struct Observations(Rc<RefCell<RecordedSources>>);
        struct OneRead {
            inner: crate::storage::CacheStorage,
            count: Rc<Cell<usize>>,
        }

        #[test]
        fn check_observes_metadata_and_bindings_from_original_pinned_view() {
            // Given: an original publication and a concurrent writer.
            let directory = tempfile::tempdir().unwrap();
            let mut writer = open_storage(&directory);
            publish_bindings(&mut writer, "a", "old.rs");
            let observations = Rc::new(RefCell::new(Vec::new()));
            let count = Rc::new(Cell::new(0));
            let cache = Rc::new(OneRead {
                inner: open_storage(&directory).cache,
                count: count.clone(),
            });
            let mut service = CheckService::new(
                Refresh(|| {
                    Ok(crate::service::RefreshOutcome {
                        revision: 2,
                        ..build_outcome()
                    })
                }),
                Rc::clone(&cache),
                Box::new(PublishingBindings(RefCell::new(writer), cache)),
                Observations(observations.clone()),
            );

            // When: check observes metadata and bindings.
            let report = service.check(&[]).unwrap();

            // Then: all observations come from the original pinned view.
            assert!(report.errors.is_empty());
            assert_eq!(count.get(), 1);
            assert_eq!(
                *observations.borrow(),
                vec![(
                    Some(TreeOid::parse(&"a".repeat(40)).unwrap()),
                    BTreeSet::from([SourceUri::parse("old.rs").unwrap()])
                )]
            );
            assert_eq!(
                {
                    let scope_databases = open_storage(&directory);
                    scope_databases
                        .cache
                        .with_read_tx(|| {
                            Ok::<_, StorageError>(scope_databases.cache.project_get_by_id())
                        })
                        .unwrap()
                }
                .unwrap(),
                4
            );
        }

        impl GraphDiagnostics for PublishingBindings {
            fn find_problems(&self, _: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
                assert_eq!(self.1.project_get_by_id().unwrap(), 2);
                publish_bindings(&mut self.0.borrow_mut(), "b", "new.rs");
                Ok(Vec::new())
            }
        }
        impl SourceProvenance for Observations {
            fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
                panic!("this scenario does not validate requested revisions")
            }
            fn observe_sources(
                &self,
                request: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                self.0
                    .borrow_mut()
                    .push((request.source_snapshot.cloned(), request.sources.clone()));
                Ok(ProvenanceObservation {
                    flags: BTreeSet::new(),
                    issues: BTreeSet::new(),
                })
            }
            fn compare_revision(
                &self,
                _: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                panic!("check does not compare revisions")
            }
        }
        use crate::service::utils::cache_test_support::{
            delegate_cache_reads, delegate_cache_writes, delegate_graph_reads,
        };
        impl crate::service::StorageRead for OneRead {
            delegate_cache_reads!();
        }
        impl crate::service::GraphRead for OneRead {
            delegate_graph_reads!();
        }
        impl crate::service::CacheDatabase for OneRead {
            delegate_cache_writes!();
            fn with_read_tx<T, E, F>(&self, operation: F) -> Result<T, TransactionError<E>>
            where
                F: FnOnce() -> Result<T, E>,
            {
                self.count.set(self.count.get() + 1);
                assert_eq!(self.count.get(), 1);
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
                self.inner.with_status_read_tx(operation)
            }
        }

        fn publish_bindings(storage: &mut TestDatabases, oid: &str, source: &str) {
            publish_nodes(storage, &[build_node("a.md", oid)]);
            storage
                .cache
                .with_write_tx(|| {
                    let unit = &storage.cache;
                    let id = KnowledgeId::parse("a.md").unwrap();
                    unit.edges_delete_by_source_id(&id).unwrap();
                    unit.edges_save_by_source_id_and_target_id_and_type(
                        &Edge::new(
                            id,
                            EdgeTarget::Source(SourceUri::parse(source).unwrap()),
                            EdgeType::parse("external").unwrap(),
                        )
                        .unwrap(),
                    )
                    .unwrap();

                    unit.get_publication_revision()
                })
                .unwrap();
        }
    }

    mod batches {
        use super::{Probes, Verifier, build_node, open_storage, publish_nodes};
        use crate::{
            domain::TreeOid,
            service::{
                CacheDatabase, DocumentResult, GraphDiagnosticsService, ProvenanceObservation,
                ProvenanceRequest, SnapshotError, SnapshotState, SourceProvenance,
                SourceRevisionComparison, SourceRevisionRequest, utils::CheckObservations,
            },
        };
        use std::{cell::RefCell, collections::BTreeSet};

        struct Batches {
            verifier: Verifier,
            sizes: RefCell<Vec<usize>>,
        }

        #[test]
        fn check_batches_selected_owners_without_losing_diagnostic_order() {
            // Given: 257 owners with unavailable bases in the already pinned metadata.
            let directory = tempfile::tempdir().unwrap();
            let mut storage = open_storage(&directory);
            let nodes: Vec<_> = (0..257)
                .map(|n| build_node(&format!("f{n:03}.md"), "a"))
                .collect();
            publish_nodes(&mut storage, &nodes);
            let provenance = Batches {
                verifier: Verifier {
                    probes: Probes::default(),
                    result: SnapshotState::Missing,
                    fail: false,
                },
                sizes: RefCell::new(Vec::new()),
            };
            let cache = std::rc::Rc::new(storage.cache);
            // When: common check/status problem collection observes these owners.
            let diagnostics = GraphDiagnosticsService::new(std::rc::Rc::clone(&cache));
            let observations = CheckObservations {
                storage: cache.as_ref(),
                graph: &diagnostics,
                provenance: &provenance,
            };
            let problems = cache
                .with_read_tx(|| observations.collect_check_problems(&[]))
                .unwrap();
            // Then: bounded batches preserve all owner mappings and sorted diagnostics.
            assert_eq!(*provenance.sizes.borrow(), [256, 1]);
            assert_eq!(problems.len(), 257);
            assert_eq!(problems[0].details["element_id"], "f000.md");
            assert_eq!(problems[256].details["element_id"], "f256.md");
            assert_eq!(provenance.verifier.probes.borrow().len(), 257);
        }

        impl SourceProvenance for Batches {
            fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
                panic!("check has no requested revisions")
            }
            fn observe_sources(
                &self,
                request: ProvenanceRequest<'_>,
            ) -> DocumentResult<ProvenanceObservation> {
                self.verifier.observe_sources(request)
            }
            fn observe_sources_batch(
                &self,
                requests: &[ProvenanceRequest<'_>],
            ) -> DocumentResult<Vec<ProvenanceObservation>> {
                self.sizes.borrow_mut().push(requests.len());
                requests
                    .iter()
                    .map(|request| self.observe_sources(*request))
                    .collect()
            }
            fn compare_revision(
                &self,
                _: SourceRevisionRequest<'_>,
            ) -> Result<SourceRevisionComparison, SnapshotError> {
                panic!("check does not compare revisions")
            }
        }
    }
}
