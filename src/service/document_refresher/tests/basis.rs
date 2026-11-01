use super::{Candidates, CountingMarkdown, TestFiles, build_service};
use crate::{
    domain::{Edge, EdgeTarget, KnowledgeId, RepoPath, Sha256, TreeOid},
    service::{
        BasisRetainer, CacheWrite, CapturedFile, DocumentError, DocumentErrorKind,
        DocumentRefreshService, DocumentResult, FileRow, FileStat, FileSystem, NodeMetadata,
        ProvenanceObservation, ProvenanceRequest, Refresher, SnapshotError, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest, Storage, StorageRead, StorageResult,
        VectorOutcome,
    },
    storage::SqliteStorage,
};
use std::{cell::RefCell, collections::BTreeSet, rc::Rc};
use tempfile::TempDir;

type Events = Rc<RefCell<Vec<&'static str>>>;
type Service = DocumentRefreshService<
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
    batches: RefCell<Vec<usize>>,
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
fn preflight_batches_only_prepared_bindings_in_bounded_chunks() {
    // Given: one document with 257 independently marked knowledge sections.
    let (_dir, mut service, events) = build_boundary_service(None);
    let bytes: String = (0..257)
        .map(|n| {
            format!("## Item {{#item{n}}}\n<!-- letopis: {{\"created_at\":1}} -->\nKnowledge\n")
        })
        .collect();
    service.filesystem.inner.stat.update(|mut stat| {
        stat.size = bytes.len() as u64;
        stat
    });
    service.filesystem.inner.bytes.replace(bytes.into_bytes());
    // When: preflight validates the prepared projection before publishing it.
    let outcome = service.refresh().unwrap();
    // Then: each binding is observed in bounded batches, before retention and SQL write.
    assert!(outcome.published);
    assert_eq!(*service.provenance.batches.borrow(), [256, 1]);
    assert_eq!(
        events
            .borrow()
            .iter()
            .filter(|event| **event == "observe")
            .count(),
        257
    );
    let events = events.borrow();
    let retain = events.iter().position(|event| *event == "retain").unwrap();
    let begin = events.iter().position(|event| *event == "begin").unwrap();
    let last_observe = events
        .iter()
        .rposition(|event| *event == "observe")
        .unwrap();
    assert!(last_observe < retain);
    assert!(retain < begin);
}

#[test]
fn verified_unchanged_refresh_audits_without_sql_write() {
    // Given: an already published document and no file delta.
    let (_dir, mut service, events) = build_boundary_service(None);
    service.refresh().unwrap();
    events.borrow_mut().clear();
    let parses = service.markdown.0.get();
    // When: full-corpus check requests a verified incremental refresh.
    let outcome = service.refresh_verified().unwrap();
    // Then: audit runs under lock, with no SQL write or body parse.
    assert_eq!(*events.borrow(), ["lock", "verify"]);
    assert!(!outcome.published);
    assert_eq!(service.markdown.0.get(), parses);
}

#[test]
fn verified_refresh_failure_preserves_previous_publication() {
    // Given: a new projection whose source audit fails.
    let (_dir, mut service, events) = build_boundary_service(Some("verify"));
    // When: verification fails before any SQL write begins.
    assert!(service.refresh_verified().is_err());
    // Then: no partial graph is published.
    assert_eq!(
        *events.borrow(),
        ["lock", "read", "read", "observe", "verify"]
    );
    assert_eq!(
        service
            .storage
            .inner
            .begin_read_tx()
            .unwrap()
            .get_revision(),
        0
    );
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
    // Given: a capture that changes after preflight.
    let (_dir, mut service, events) = build_boundary_service(None);
    service.filesystem.inner.replace_on_read.set(Some(3));
    // When / Then: refresh rejects the inconsistent capture; no cache commit occurs.
    assert!(service.refresh().is_err());
    // Then: no cache commit occurs.
    assert_eq!(
        *events.borrow(),
        ["lock", "read", "read", "observe", "retain", "begin", "read"]
    );
    assert_eq!(
        service
            .storage
            .inner
            .begin_read_tx()
            .unwrap()
            .get_revision(),
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
            "lock", "read", "read", "observe", "verify", "begin", "read", "commit"
        ]
    );
    assert!(service.markdown.0.get() > parses);
}

fn build_boundary_service(failure: Option<&'static str>) -> (TempDir, Service, Events) {
    let (dir, service) = build_service();
    let events = Rc::new(RefCell::new(Vec::new()));
    let service = DocumentRefreshService {
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
            batches: RefCell::new(Vec::new()),
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
    fn get_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        self.events.borrow_mut().push("read");
        self.inner.get_file(path)
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
    fn observe_sources_batch(
        &self,
        requests: &[ProvenanceRequest<'_>],
    ) -> DocumentResult<Vec<ProvenanceObservation>> {
        self.batches.borrow_mut().push(requests.len());
        requests
            .iter()
            .map(|request| self.observe_sources(*request))
            .collect()
    }
    fn compare_revision(
        &self,
        _: SourceRevisionRequest<'_>,
    ) -> Result<SourceRevisionComparison, SnapshotError> {
        panic!("refresh must not compare requested revisions")
    }
}
impl BasisRetainer for BoundaryProvenance {
    fn verify_snapshots(&self, _: &BTreeSet<TreeOid>) -> DocumentResult<()> {
        self.events.borrow_mut().push("verify");
        if self.failure == Some("verify") {
            return Err(build_failure());
        }
        Ok(())
    }
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
    fn begin_read_tx(&mut self) -> StorageResult<Self::Read<'_>> {
        self.inner.begin_read_tx()
    }
    fn begin_cache_write_tx(&mut self) -> StorageResult<Self::CacheWrite<'_>> {
        self.events.borrow_mut().push("begin");
        Ok(BoundaryWrite {
            inner: self.inner.begin_cache_write_tx()?,
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
    fn begin_vector_write_tx(
        &mut self,
        expected: &Sha256,
    ) -> StorageResult<VectorOutcome<Self::VectorWrite<'_>>> {
        self.inner.begin_vector_write_tx(expected)
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
