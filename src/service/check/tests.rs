use super::CheckService;
use crate::{
    domain::{
        Edge, EdgeTarget, EdgeType, ElementKind, KnowledgeId, LineRange, RepoPath, Sha256,
        SourceUri, TreeOid,
    },
    service::{
        CacheWrite, Checker, CommandError, CommandErrorKind, CommandProblem, CommandResult,
        CurrentGraphDiagnostics, DocumentError, DocumentErrorKind, DocumentResult,
        GraphDiagnostics, KnowledgeMetadata, NodeMetadata, ProvenanceIssue, ProvenanceObservation,
        ProvenanceRequest, RefreshOutcome, Refresher, SearchFlag, SnapshotError, SnapshotState,
        SnapshotVerifier, SourceProvenance, SourceRevisionComparison, SourceRevisionRequest,
        Storage, StorageError, StorageErrorKind, StorageRead, StorageResult, VectorOutcome,
    },
    storage::SqliteStorage,
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
}

struct RecordingDiagnostics {
    events: Events,
}

struct PublishingDiagnostics {
    writer: RefCell<SqliteStorage>,
}

struct FindingDiagnostics;

struct Verifier {
    probes: Probes,
    result: SnapshotState,
    fail: bool,
}

#[test]
fn check_reports_missing_address_from_selected_knowledge() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let mut storage = open_storage(&directory);
    publish_nodes(&mut storage, &[build_node("a.md#one", "a")]);
    let mut unit = storage.begin_cache_write().unwrap();
    unit.insert_edge(
        &Edge::new(
            KnowledgeId::parse("a.md#one").unwrap(),
            EdgeTarget::Source(SourceUri::parse("gone.rs").unwrap()),
            EdgeType::parse("external").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    unit.commit().unwrap();
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        storage,
        FindingDiagnostics,
        Verifier {
            probes: Probes::default(),
            result: SnapshotState::Available,
            fail: false,
        },
    );

    // When
    let report = service.check(&[RepoPath::parse("a.md").unwrap()]).unwrap();

    // Then
    assert_eq!(report.errors.len(), 2);
    assert_eq!(report.errors[1].details["reason"], "source_not_found");
    assert_eq!(report.errors[1].details["path"], "a.md");
    assert_eq!(report.errors[1].details["target"], "gone.rs");
}

#[test]
fn check_refreshes_before_opening_one_read() {
    // Given
    let directory = tempfile::tempdir().unwrap();
    let storage = open_storage(&directory);
    let mut writer = open_storage(&directory);
    let events = Events::default();
    let refresh_events = events.clone();
    let refresher = Refresh(move || {
        refresh_events.borrow_mut().push("refresh");
        let mut unit = writer.begin_cache_write().unwrap();
        unit.upsert_node(&build_node("fresh.md", "a")).unwrap();
        unit.commit().unwrap();
        Ok(build_outcome())
    });
    let probes = Probes::default();
    let mut service = CheckService::new(
        refresher,
        RecordingStorage {
            inner: storage,
            events: events.clone(),
            fail_read: false,
        },
        RecordingDiagnostics {
            events: events.clone(),
        },
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Missing,
            fail: false,
        },
    );

    // When
    let report = service.check(&[]).unwrap();

    // Then
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
    let diagnostics = PublishingDiagnostics {
        writer: RefCell::new(writer),
    };
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        open_storage(&directory),
        diagnostics,
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
        storage,
        FindingDiagnostics,
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
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        storage,
        CurrentGraphDiagnostics::new(),
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
        .execute("DROP TABLE nodes", [])
        .unwrap();
    let mut service = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        storage,
        FindingDiagnostics,
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
    fn reindex(&mut self) -> DocumentResult<RefreshOutcome> {
        panic!("this scenario must not trigger a full reindex")
    }

    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        (self.0)()
    }
}

impl<S: Storage> Storage for RecordingStorage<S> {
    type Read<'a>
        = S::Read<'a>
    where
        Self: 'a;
    type CacheWrite<'a>
        = S::CacheWrite<'a>
    where
        Self: 'a;
    type VectorWrite<'a>
        = S::VectorWrite<'a>
    where
        Self: 'a;
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        self.events.borrow_mut().push("read");
        if self.fail_read {
            return Err(StorageError::new_with_cause(
                StorageErrorKind::InternalError,
                "safe storage",
                io::Error::other("SECRET_STORAGE"),
            ));
        }
        self.inner.begin_read()
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.inner.begin_cache_write()
    }
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write(expected)
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        identity: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, identity)
    }
}

impl GraphDiagnostics for RecordingDiagnostics {
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        self.events.borrow_mut().push("diagnostics");
        assert_eq!(view.get_revision(), 1);
        CurrentGraphDiagnostics::new().find_problems(view, paths)
    }
}

impl GraphDiagnostics for PublishingDiagnostics {
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
        paths: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        let revision = view.get_revision();
        let findings = CurrentGraphDiagnostics::new().find_problems(view, paths)?;
        publish_nodes(
            &mut self.writer.borrow_mut(),
            &[build_node("a.md", "b"), build_node("b.md", "b")],
        );
        assert_eq!(view.get_revision(), revision);
        Ok(findings)
    }
}

impl GraphDiagnostics for FindingDiagnostics {
    fn find_problems<R: StorageRead>(
        &self,
        _: &mut R,
        _: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
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
            let state = self.verify_snapshot(snapshot).map_err(|cause| {
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
    fn verify_snapshot(&self, oid: &TreeOid) -> Result<SnapshotState, SnapshotError> {
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
    let mut refresh_failure = CheckService::new(
        Refresh(|| {
            Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "safe refresh",
                io::Error::other("SECRET_REFRESH"),
            ))
        }),
        RecordingStorage {
            inner: open_storage(directory),
            events: events.clone(),
            fail_read: false,
        },
        RecordingDiagnostics {
            events: events.clone(),
        },
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
    let mut storage_failure = CheckService::new(
        Refresh(|| Ok(build_outcome())),
        RecordingStorage {
            inner: open_storage(directory),
            events: events.clone(),
            fail_read: true,
        },
        RecordingDiagnostics {
            events: events.clone(),
        },
        Verifier {
            probes: probes.clone(),
            result: SnapshotState::Available,
            fail: false,
        },
    );
    assert_private_failure(&storage_failure.check(&[]).unwrap_err());
    assert_eq!(*events.borrow(), ["read"]);
}

fn open_storage(directory: &TempDir) -> SqliteStorage {
    SqliteStorage::open(
        &directory.path().join("cache.db"),
        &directory.path().join("vectors.db"),
    )
    .unwrap()
}
fn publish_nodes(storage: &mut SqliteStorage, nodes: &[NodeMetadata]) {
    let mut unit = storage.begin_cache_write().unwrap();
    for node in nodes {
        unit.upsert_node(node).unwrap();
    }
    unit.commit().unwrap();
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

mod pinned;
