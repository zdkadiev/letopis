use super::{Candidates, CountingMarkdown, TestFiles, build_service};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        BasisRetainer, CacheWrite, CapturedFile, DocumentError, DocumentErrorKind,
        DocumentRefresher, DocumentResult, FileRow, FileStat, FileSystem, NodeMetadata,
        ProvenanceObservation, ProvenanceRequest, Refresher, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead, StorageResult,
        VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use tempfile::TempDir;

type Events = Rc<RefCell<Vec<&'static str>>>;
type Service = DocumentRefresher<
    Candidates,
    BoundaryFiles,
    CountingMarkdown,
    BoundaryStorage,
    BoundaryProvenance,
>;
struct BoundaryFiles {
    inner: TestFiles,
    events: Events,
}
struct BoundaryProvenance {
    events: Events,
    failure: Option<&'static str>,
}
struct BoundaryStorage {
    inner: SqliteStorage,
    events: Events,
}
struct BoundaryWrite<W> {
    inner: W,
    events: Events,
}

#[test]
fn retain_under_index_lock_before_sql_publication() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(None);
    // When
    let outcome = service.refresh().unwrap();
    // Then
    assert!(outcome.published);
    assert_eq!(outcome.revision, 1);
    assert_eq!(
        *events.borrow(),
        [
            "lock", "read", "read", "observe", "retain", "begin", "read", "commit"
        ]
    );
}

#[test]
fn preflight_read_failure_never_opens_sql_write() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(None);
    service.filesystem.inner.fail_on_read.set(Some(2));
    // When / Then
    assert!(service.refresh().is_err());
    // Then
    assert_eq!(*events.borrow(), ["lock", "read", "read"]);
}

#[test]
fn observation_failure_never_opens_sql_write() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(Some("observe"));
    // When / Then
    assert!(service.refresh().is_err());
    // Then
    assert_eq!(*events.borrow(), ["lock", "read", "read", "observe"]);
}

#[test]
fn pin_failure_never_opens_sql_write() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(Some("retain"));
    // When / Then
    assert!(service.refresh().is_err());
    // Then
    assert_eq!(
        *events.borrow(),
        ["lock", "read", "read", "observe", "retain"]
    );
}

#[test]
fn changed_capture_after_preflight_rolls_back_without_commit() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(None);
    service.filesystem.inner.replace_on_read.set(Some(3));
    // When / Then
    assert!(service.refresh().is_err());
    // Then
    assert_eq!(
        *events.borrow(),
        ["lock", "read", "read", "observe", "retain", "begin", "read"]
    );
    assert_eq!(
        service.storage.inner.begin_read().unwrap().get_revision(),
        0
    );
}

#[test]
fn unchanged_basis_retention_skips_sql_write_and_body_parse() {
    // Given
    let (_dir, mut service, events) = build_boundary_service(None);
    let tree = TreeOid::parse(&"1".repeat(40)).unwrap();
    let note = format!(
        "<!-- letopis: {{\"created_at\":1,\"source_snapshot\":\"{}\"}} -->\nKnowledge\n",
        tree.as_str()
    );
    service
        .filesystem
        .inner
        .bytes
        .replace(note.as_bytes().to_vec());
    service.filesystem.inner.stat.update(|mut stat| {
        stat.size = note.len() as u64;
        stat
    });
    service.refresh().unwrap();
    events.borrow_mut().clear();
    let parses = service.markdown.0.get();
    // When
    let outcome = service.refresh().unwrap();
    // Then
    assert!(!outcome.published);
    assert_eq!(outcome.revision, 1);
    assert_eq!(service.markdown.0.get(), parses);
    assert_eq!(*events.borrow(), ["lock", "retain"]);
}

#[test]
fn initial_build_and_full_reindex_lock_before_capturing_and_validate_basis() {
    // Given.
    let (_dir, mut service, events) = build_boundary_service(None);
    // When: the first build follows the full path, not preliminary file capture.
    service.refresh().unwrap();
    // Then.
    assert_eq!(events.borrow().first(), Some(&"lock"));
    // Given
    events.borrow_mut().clear();
    let parses = service.markdown.0.get();
    // When
    service.reindex().unwrap();
    // Then
    assert_eq!(
        *events.borrow(),
        [
            "lock", "read", "read", "observe", "retain", "begin", "read", "commit"
        ]
    );
    assert!(service.markdown.0.get() > parses);
}

fn build_boundary_service(failure: Option<&'static str>) -> (TempDir, Service, Events) {
    let (dir, service) = build_service();
    let events = Rc::new(RefCell::new(Vec::new()));
    let service = DocumentRefresher {
        git: service.git,
        filesystem: BoundaryFiles {
            inner: service.filesystem,
            events: Rc::clone(&events),
        },
        markdown: service.markdown,
        storage: BoundaryStorage {
            inner: service.storage,
            events: Rc::clone(&events),
        },
        provenance: BoundaryProvenance {
            events: Rc::clone(&events),
            failure,
        },
    };
    (dir, service, events)
}
impl FileSystem for BoundaryFiles {
    type IndexLock = ();
    fn stat_file(&self, path: &RepoPath) -> DocumentResult<Option<FileStat>> {
        self.inner.stat_file(path)
    }
    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        self.events.borrow_mut().push("read");
        self.inner.read_file(path)
    }
    fn get_time_ns(&self) -> DocumentResult<i64> {
        self.inner.get_time_ns()
    }
    fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
        self.events.borrow_mut().push("lock");
        self.inner.lock_index()
    }
}
impl SourceProvenance for BoundaryProvenance {
    fn validate_revisions(&self, _: &BTreeSet<TreeOid>) -> Result<(), SnapshotError> {
        panic!("this scenario does not validate requested revisions")
    }
    fn observe_sources(&self, _: ProvenanceRequest<'_>) -> DocumentResult<ProvenanceObservation> {
        self.events.borrow_mut().push("observe");
        if self.failure == Some("observe") {
            return Err(build_failure());
        }
        Ok(ProvenanceObservation {
            flags: BTreeSet::new(),
            issues: BTreeSet::new(),
        })
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("refresh must not compare requested revisions")
    }
}
impl BasisRetainer for BoundaryProvenance {
    fn retain_snapshots(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        self.events.borrow_mut().push("retain");
        if self.failure == Some("retain") {
            return Err(build_failure());
        }
        Ok(())
    }
}
impl Storage for BoundaryStorage {
    type Read<'a> = <SqliteStorage as Storage>::Read<'a>;
    type CacheWrite<'a> = BoundaryWrite<<SqliteStorage as Storage>::CacheWrite<'a>>;
    type VectorWrite<'a> = <SqliteStorage as Storage>::VectorWrite<'a>;
    fn begin_read(&mut self) -> StorageResult<Self::Read<'_>> {
        self.inner.begin_read()
    }
    fn begin_cache_write(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.events.borrow_mut().push("begin");
        Ok(BoundaryWrite {
            inner: self.inner.begin_cache_write()?,
            events: Rc::clone(&self.events),
        })
    }
    fn reset_vectors(
        &mut self,
        observed: Option<&Sha256>,
        identity: &Sha256,
    ) -> StorageResult<VectorOutcome<bool>> {
        self.inner.reset_vectors(observed, identity)
    }
    fn begin_vector_write(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write(expected)
    }
}
impl<W: CacheWrite> CacheWrite for BoundaryWrite<W> {
    fn upsert_node(&mut self, node: &NodeMetadata) -> StorageResult<i64> {
        self.inner.upsert_node(node)
    }
    fn insert_edge(&mut self, edge: &Edge) -> StorageResult<()> {
        self.inner.insert_edge(edge)
    }
    fn delete_outgoing(&mut self, id: &KnowledgeId) -> StorageResult<()> {
        self.inner.delete_outgoing(id)
    }
    fn delete_node(&mut self, id: &EdgeTarget) -> StorageResult<()> {
        self.inner.delete_node(id)
    }
    fn replace_text(&mut self, id: &KnowledgeId, text: &str) -> StorageResult<()> {
        self.inner.replace_text(id, text)
    }
    fn upsert_file(&mut self, file: &FileRow) -> StorageResult<()> {
        self.inner.upsert_file(file)
    }
    fn delete_file(&mut self, path: &RepoPath) -> StorageResult<()> {
        self.inner.delete_file(path)
    }
    fn commit(self) -> StorageResult<u64> {
        self.events.borrow_mut().push("commit");
        self.inner.commit()
    }
}
fn build_failure() -> DocumentError {
    DocumentError::new(
        DocumentErrorKind::InternalError,
        "injected provenance failure",
    )
}
