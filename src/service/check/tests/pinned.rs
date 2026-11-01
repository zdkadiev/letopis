use super::{Refresh, build_node, build_outcome, open_storage, publish_nodes};
use crate::{
    domain::{Edge, EdgeTarget, EdgeType, KnowledgeId, RepoPath, Sha256, SourceUri, TreeOid},
    service::{
        CacheWrite, Checker, CommandProblem, CommandResult, DocumentResult, GraphDiagnostics,
        ProvenanceObservation, ProvenanceRequest, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead, StorageResult,
        VectorOutcome, check::CheckService,
    },
    storage::SqliteStorage,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
    rc::Rc,
};

struct PublishingBindings(RefCell<SqliteStorage>);
type RecordedSources = Vec<(Option<TreeOid>, BTreeSet<SourceUri>)>;
struct Observations(Rc<RefCell<RecordedSources>>);
struct OneRead {
    inner: SqliteStorage,
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
    let mut service = CheckService::new(
        Refresh(|| {
            Ok(crate::service::RefreshOutcome {
                revision: 2,
                ..build_outcome()
            })
        }),
        OneRead {
            inner: open_storage(&directory),
            count: count.clone(),
        },
        PublishingBindings(RefCell::new(writer)),
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
        open_storage(&directory)
            .begin_read_tx()
            .unwrap()
            .get_revision(),
        4
    );
}

impl GraphDiagnostics for PublishingBindings {
    fn find_problems<R: StorageRead>(
        &self,
        view: &mut R,
        _: &[RepoPath],
    ) -> CommandResult<Vec<CommandProblem>> {
        assert_eq!(view.get_revision(), 2);
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
impl Storage for OneRead {
    type Read<'a>
        = <SqliteStorage as Storage>::Read<'a>
    where
        Self: 'a;
    type CacheWrite<'a>
        = <SqliteStorage as Storage>::CacheWrite<'a>
    where
        Self: 'a;
    type VectorWrite<'a>
        = <SqliteStorage as Storage>::VectorWrite<'a>
    where
        Self: 'a;
    fn begin_read_tx(&mut self) -> StorageResult<Self::Read<'_>> {
        self.count.set(self.count.get() + 1);
        assert_eq!(self.count.get(), 1, "check opened a second view");
        self.inner.begin_read_tx()
    }
    fn begin_cache_write_tx(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        panic!("check cannot publish")
    }
    fn begin_vector_write_tx(
        &mut self,
        _: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        panic!("check cannot write vectors")
    }
    fn reset_vectors(
        &mut self,
        _: Option<&Sha256>,
        _: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        panic!("check cannot reset vectors")
    }
}
fn publish_bindings(storage: &mut SqliteStorage, oid: &str, source: &str) {
    publish_nodes(storage, &[build_node("a.md", oid)]);
    let mut unit = storage.begin_cache_write_tx().unwrap();
    let id = KnowledgeId::parse("a.md").unwrap();
    unit.delete_outgoing(&id).unwrap();
    unit.insert_edge(
        &Edge::new(
            id,
            EdgeTarget::Source(SourceUri::parse(source).unwrap()),
            EdgeType::parse("external").unwrap(),
        )
        .unwrap(),
    )
    .unwrap();
    unit.commit().unwrap();
}
