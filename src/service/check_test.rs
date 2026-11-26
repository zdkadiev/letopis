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
        SnapshotError, SnapshotState, SnapshotVerifier, SourceProvenance, SourceRevisionComparison,
        SourceRevisionRequest, StorageError, StorageErrorKind, StorageRead,
        utils::CheckObservations,
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
    is_read_failing: bool,
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

struct RepeatedDiagnostics(Vec<CommandProblem>);

struct ChangedSources;

struct Verifier {
    probes: Probes,
    result: SnapshotState,
    is_failing: bool,
}

#[test]
fn collection_groups_changed_sources_by_element_and_keeps_independent_issues() {
    // Given: two elements share an owner, one source is missing and two bases need rechecking.
    let directory = tempfile::tempdir().unwrap();
    let mut storage = open_storage(&directory);
    let mut missing = build_node("c.md", "a");
    let NodeMetadata::Knowledge(metadata) = &mut missing else {
        panic!("expected knowledge metadata");
    };
    metadata.source_snapshot = None;
    publish_nodes(
        &mut storage,
        &[
            build_node("a.md#one", "a"),
            build_node("a.md#two", "a"),
            build_node("b.md", "b"),
            missing,
        ],
    );
    publish_sources(&storage, "a.md#one", &["z.rs", "a.rs#L2-L4", "missing.rs"]);
    publish_sources(&storage, "a.md#two", &["sibling.rs"]);
    let cache = Rc::new(storage.cache_storage);
    let graph = GraphDiagnosticsService::new(Rc::clone(&cache));
    let observations = CheckObservations {
        storage_read: cache.as_ref(),
        graph: &graph,
        provenance: &ChangedSources,
    };

    // When: the shared collector observes the published bindings.
    let problems = cache
        .with_read_tx(|| observations.collect_check_problems(&[]))
        .unwrap();

    // Then: each element retains its addresses, and rechecking alone yields no change finding.
    let mut findings = Vec::new();
    for problem in problems {
        assert_eq!(problem.code, CommandErrorKind::ValidationFailed);
        if problem.details["reason"] == "source_changed" {
            assert_eq!(problem.message, "knowledge source changed");
        }
        findings.push(json!(problem.details));
    }
    assert_eq!(findings.len(), 5);
    for details in [
        json!({"path":"a.md","reason":"source_changed","element_id":"a.md#one","targets":["a.rs#L2-L4","z.rs"]}),
        json!({"path":"a.md","reason":"source_changed","element_id":"a.md#two","targets":["sibling.rs"]}),
        json!({"path":"a.md","reason":"source_not_found","element_id":"a.md#one","target":"missing.rs"}),
        json!({"path":"b.md","reason":"source_snapshot_unavailable","element_id":"b.md","target":"b".repeat(40)}),
        json!({"path":"c.md","reason":"missing_source_snapshot","element_id":"c.md"}),
    ] {
        assert!(findings.contains(&details), "missing finding {details}");
    }
}

#[test]
fn check_filters_owner_before_limiting_and_keeps_all_targets() {
    // Given: 100 earlier findings and a separate owner with 101 changed addresses.
    let directory = tempfile::tempdir().unwrap();
    let mut storage = open_storage(&directory);
    let mut nodes: Vec<_> = (0..100)
        .map(|index| build_node(&format!("f{index:03}.md"), "b"))
        .collect();
    nodes.push(build_node("z.md#part", "a"));
    publish_nodes(&mut storage, &nodes);
    let targets: Vec<_> = (0..101).map(|index| format!("file{index:03}.rs")).collect();
    let source_addresses: Vec<_> = targets.iter().map(String::as_str).collect();
    publish_sources(&storage, "z.md#part", &source_addresses);
    let cache = Rc::new(storage.cache_storage);
    let mut service = CheckService::new(
        Refresh(|| {
            Ok(RefreshOutcome {
                revision: 2,
                ..build_outcome()
            })
        }),
        Rc::clone(&cache),
        Box::new(GraphDiagnosticsService::new(cache)),
        ChangedSources,
    );

    // When: check observes the whole corpus, an absent owner, and the last owner.
    let whole = service.check(&[]).unwrap();
    let empty = service
        .check(&[RepoPath::parse("absent.md").unwrap()])
        .unwrap();
    let scoped = service.check(&[RepoPath::parse("z.md").unwrap()]).unwrap();

    // Then: the last owner is outside the full report cutoff and complete in the scoped report.
    assert_eq!(whole.problems.len(), 100);
    assert_eq!(whole.problems[0].details["element_id"], "f000.md");
    assert_eq!(whole.problems[99].details["element_id"], "f099.md");
    assert!(empty.problems.is_empty());
    assert_eq!(scoped.problems.len(), 1);
    assert_eq!(scoped.problems[0].details["element_id"], "z.md#part");
    assert_eq!(scoped.problems[0].details["targets"], json!(targets));
}

#[test]
fn check_orders_and_deduplicates_exactly_one_hundred_findings() {
    // Given: exactly 100 distinct findings, reversed, with a duplicate near the cutoff.
    let directory = tempfile::tempdir().unwrap();
    let storage = open_storage(&directory);
    let mut problems: Vec<_> = (0..100)
        .rev()
        .map(|index| CommandProblem {
            code: CommandErrorKind::ValidationFailed,
            message: "graph fixture",
            details: [("path".into(), json!(format!("f{index:03}.md")))].into(),
        })
        .collect();
    problems.insert(0, problems[1].clone());
    let mut service = CheckService::new(
        Refresh(|| {
            Ok(RefreshOutcome {
                revision: 0,
                ..build_outcome()
            })
        }),
        Rc::new(storage.cache_storage),
        Box::new(RepeatedDiagnostics(problems)),
        ChangedSources,
    );

    // When: check reports the findings.
    let report = service.check(&[]).unwrap();

    // Then: the duplicate does not displace the last distinct finding.
    assert_eq!(report.problems.len(), 100);
    for (index, problem) in report.problems.iter().enumerate() {
        assert_eq!(problem.details["path"], format!("f{index:03}.md"));
    }
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
        Rc::new(storage.cache_storage),
        Box::new(FindingDiagnostics),
        Verifier {
            probes: Probes::default(),
            result: SnapshotState::Available,
            is_failing: false,
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
        Rc::new(storage.cache_storage),
        Box::new(FindingDiagnostics),
        Verifier {
            probes: Probes::default(),
            result: SnapshotState::Available,
            is_failing: false,
        },
    );
    // When: the selected corpus is checked.
    let report = service
        .check(&[RepoPath::parse("fresh.md").unwrap()])
        .unwrap();
    // Then: regular diagnostics are returned without an exhaustive audit.
    assert_eq!(report.problems.len(), 1);
}

#[test]
fn check_reports_missing_address_from_selected_knowledge() {
    // Given: selected knowledge whose source address is missing.
    let directory = tempfile::tempdir().unwrap();
    let mut storage = open_storage(&directory);
    publish_nodes(&mut storage, &[build_node("a.md#one", "a")]);
    storage
        .cache_storage
        .with_write_tx(|| {
            let unit = &storage.cache_storage;
            unit.edges_save_by_source_id_and_target_id_and_type(
                &Edge::new(
                    KnowledgeId::parse("a.md#one").unwrap(),
                    EdgeTarget::Source(SourceUri::parse("gone.rs").unwrap()),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap(),
            )
            .unwrap();

            unit.project_get_revision()
        })
        .unwrap();
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        Rc::new(storage.cache_storage),
        Box::new(FindingDiagnostics),
        Verifier {
            probes: Probes::default(),
            result: SnapshotState::Available,
            is_failing: false,
        },
    );

    // When: check runs for the selected file.
    let report = service.check(&[RepoPath::parse("a.md").unwrap()]).unwrap();

    // Then: the report includes the missing source finding.
    assert_eq!(report.problems.len(), 2);
    assert_eq!(report.problems[1].details["reason"], "source_not_found");
    assert_eq!(report.problems[1].details["path"], "a.md");
    assert_eq!(report.problems[1].details["target"], "gone.rs");
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
            .cache_storage
            .with_write_tx(|| {
                let unit = &writer.cache_storage;
                unit.nodes_upsert_by_id(&build_node("fresh.md", "a"))
                    .unwrap();

                unit.project_get_revision()
            })
            .unwrap();
        Ok(build_outcome())
    });
    let probes = Probes::default();
    let cache = Rc::new(RecordingStorage {
        active: std::cell::Cell::new(false),
        inner: storage.cache_storage,
        events: events.clone(),
        is_read_failing: false,
    });
    let mut service = CheckService::new(
        refresher,
        Rc::clone(&cache),
        Box::new(RecordingDiagnostics::new(Rc::clone(&cache), events.clone())),
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Missing,
            is_failing: false,
        },
    );

    // When: check runs over the repository.
    let report = service.check(&[]).unwrap();

    // Then: refresh precedes one pinned read and its diagnostics.
    assert_eq!(*events.borrow(), ["refresh", "read", "diagnostics"]);
    assert_eq!(*probes.borrow(), ["a".repeat(40)]);
    assert_eq!(
        get_findings(&report.problems),
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
    let cache = Rc::new(open_storage(&directory).cache_storage);
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
            is_failing: false,
        },
    );

    // When
    let report = service.check(&[]).unwrap();

    // Then
    assert_eq!(*probes.borrow(), ["a".repeat(40), "a".repeat(40)]);
    assert_eq!(
        get_findings(&report.problems),
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
    assert_eq!(report.problems.len(), 2);
    assert_eq!(report.problems[0].details["target"], "b".repeat(40));
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
        Rc::new(storage.cache_storage),
        Box::new(FindingDiagnostics),
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Available,
            is_failing: true,
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
    let cache = Rc::new(storage.cache_storage);
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        Rc::clone(&cache),
        Box::new(GraphDiagnosticsService::new(cache)),
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Missing,
            is_failing: true,
        },
    );

    // When / Then
    assert!(
        service
            .check(&[RepoPath::parse("absent.md").unwrap()])
            .unwrap()
            .problems
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
        Rc::new(storage.cache_storage),
        Box::new(FindingDiagnostics),
        Verifier {
            probes: Probes::default(),
            result: SnapshotState::Available,
            is_failing: false,
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

use crate::service::cache_test_support::{
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
        if self.is_read_failing {
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

    fn with_status_read_tx<T, E, F>(&self, operation: F) -> Result<Option<T>, TransactionError<E>>
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

impl GraphDiagnostics for RepeatedDiagnostics {
    fn find_problems(&self, _: &[RepoPath]) -> CommandResult<Vec<CommandProblem>> {
        Ok(self.0.clone())
    }
}

impl SourceProvenance for ChangedSources {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("check does not validate requested revisions")
    }

    fn observe_sources(
        &self,
        request: ProvenanceRequest<'_>,
    ) -> DocumentResult<ProvenanceObservation> {
        let mut observation = ProvenanceObservation {
            flags: BTreeSet::from([SearchFlag::SourceChanged]),
            issues: BTreeSet::new(),
            changed_sources: BTreeSet::new(),
        };
        match request.source_snapshot {
            None => {
                observation.issues.insert(ProvenanceIssue::MissingSnapshot);
            }
            Some(snapshot) if snapshot.as_str().starts_with('b') => {
                observation
                    .issues
                    .insert(ProvenanceIssue::SnapshotUnavailable {
                        snapshot: snapshot.clone(),
                    });
            }
            Some(_) => {
                for source in request.sources {
                    if source.as_str() == "missing.rs" {
                        observation.issues.insert(ProvenanceIssue::SourceNotFound {
                            source: source.clone(),
                        });
                    } else {
                        observation.changed_sources.insert(source.clone());
                    }
                }
            }
        }
        Ok(observation)
    }

    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("check does not compare requested revisions")
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
            changed_sources: BTreeSet::new(),
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
        if self.is_failing {
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
        inner: open_storage(directory).cache_storage,
        events: events.clone(),
        is_read_failing: false,
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
            is_failing: false,
        },
    );
    assert_private_failure(&refresh_failure.check(&[]).unwrap_err());
    assert!(events.borrow().is_empty());
}

fn assert_storage_failure(directory: &TempDir, events: &Events, probes: &Probes) {
    let cache = Rc::new(RecordingStorage {
        active: std::cell::Cell::new(false),
        inner: open_storage(directory).cache_storage,
        events: events.clone(),
        is_read_failing: true,
    });
    let mut storage_failure = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        Rc::clone(&cache),
        Box::new(RecordingDiagnostics::new(Rc::clone(&cache), events.clone())),
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Available,
            is_failing: false,
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
        .cache_storage
        .with_write_tx(|| {
            let unit = &storage.cache_storage;
            for node in nodes {
                unit.nodes_upsert_by_id(node).unwrap();
            }

            unit.project_get_revision()
        })
        .unwrap();
}

fn publish_sources(storage: &TestDatabases, id: &str, sources: &[&str]) {
    let id = KnowledgeId::parse(id).unwrap();
    storage
        .cache_storage
        .with_write_tx(|| {
            for source in sources {
                let edge = Edge::new(
                    id.clone(),
                    EdgeTarget::Source(SourceUri::parse(source).unwrap()),
                    EdgeType::parse("external").unwrap(),
                )
                .unwrap();
                storage
                    .cache_storage
                    .edges_save_by_source_id_and_target_id_and_type(&edge)?;
            }
            Ok::<_, StorageError>(())
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
        is_published: false,
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
            inner: open_storage(&directory).cache_storage,
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
        assert!(report.problems.is_empty());
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
                    .cache_storage
                    .with_read_tx(|| {
                        Ok::<_, StorageError>(scope_databases.cache_storage.project_get_by_id())
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
                changed_sources: BTreeSet::new(),
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
    use crate::service::cache_test_support::{
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
            .cache_storage
            .with_write_tx(|| {
                let unit = &storage.cache_storage;
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

                unit.project_get_revision()
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
    fn check_batches_selected_owners_without_losing_findings() {
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
                is_failing: false,
            },
            sizes: RefCell::new(Vec::new()),
        };
        let cache = std::rc::Rc::new(storage.cache_storage);
        // When: common check/status problem collection observes these owners.
        let diagnostics = GraphDiagnosticsService::new(std::rc::Rc::clone(&cache));
        let observations = CheckObservations {
            storage_read: cache.as_ref(),
            graph: &diagnostics,
            provenance: &provenance,
        };
        let problems = cache
            .with_read_tx(|| observations.collect_check_problems(&[]))
            .unwrap();
        // Then: bounded batches preserve every owner finding.
        assert_eq!(*provenance.sizes.borrow(), [256, 1]);
        assert_eq!(problems.len(), 257);
        let reported_ids: BTreeSet<_> = problems
            .iter()
            .map(|problem| problem.details["element_id"].as_str().unwrap().to_owned())
            .collect();
        let expected_ids = (0..257).map(|index| format!("f{index:03}.md")).collect();
        assert_eq!(reported_ids, expected_ids);
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
