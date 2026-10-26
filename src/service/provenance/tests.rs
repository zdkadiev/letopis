use super::ProvenanceService;
use crate::{
    domain::{BlobOid, RepoPath, SourceUri, TreeOid},
    service::{
        BasisRetainer, CapturedFile, DocumentError, DocumentErrorKind, DocumentResult, FileStat,
        FileSystem, GitSnapshots, ProvenanceRequest, RetainedTree, SnapshotEntry, SnapshotError,
        SnapshotErrorKind, SnapshotFile, SnapshotState, SnapshotVerifier, SourceProvenance,
        SourceRevisionComparison, SourceRevisionRequest,
    },
};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet},
    error::Error,
    io,
};

#[test]
fn leave_remote_unknown_without_file_reads_or_retention() {
    // Given
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let source = SourceUri::parse("https://host/path#L1-L2").unwrap();
    let sources = BTreeSet::from([source.clone()]);
    // When
    let observation = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap();
    let comparison = service
        .compare_revision(SourceRevisionRequest {
            source_snapshot: Some(&tree),
            source: &source,
            revision: &tree,
        })
        .unwrap();
    // Then
    assert!(observation.flags.is_empty());
    assert!(observation.issues.is_empty());
    assert_eq!(comparison, SourceRevisionComparison::Unfetched);
    assert_eq!(service.filesystem.reads.get(), 0);
    assert!(service.git.pins.borrow().is_empty());
}

#[test]
fn verify_each_root_once_and_never_pin_during_read() {
    // Given
    let service = create_service(GitFault::None);
    let tree = create_oid('a');
    let source = SourceUri::parse("a#L1-L1").unwrap();
    let sources = BTreeSet::from([source.clone(), SourceUri::parse("b").unwrap()]);
    // When
    service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap();
    assert_eq!(
        service.git.verified.borrow().as_slice(),
        std::slice::from_ref(&tree)
    );
    service.git.verified.borrow_mut().clear();
    let comparison = service
        .compare_revision(SourceRevisionRequest {
            source_snapshot: Some(&tree),
            source: &source,
            revision: &tree,
        })
        .unwrap();
    // Then
    assert_eq!(comparison, SourceRevisionComparison::Equal);
    assert_eq!(*service.git.verified.borrow(), [tree]);
    assert!(service.git.pins.borrow().is_empty());
}

#[test]
fn retain_each_unique_available_root_and_skip_missing_or_invalid() {
    // Given
    let mut service = create_service(GitFault::None);
    let available = create_oid('a');
    let missing = create_oid('b');
    let invalid = create_oid('c');
    service
        .git
        .states
        .insert(missing.clone(), SnapshotState::Missing);
    service
        .git
        .states
        .insert(invalid.clone(), SnapshotState::Invalid);
    let roots = BTreeSet::from([
        available.clone(),
        available.clone(),
        missing.clone(),
        invalid.clone(),
    ]);
    // When
    service.retain_snapshots(&roots).unwrap();
    // Then
    assert_eq!(
        *service.git.verified.borrow(),
        [available.clone(), missing, invalid]
    );
    assert_eq!(*service.git.pins.borrow(), [available]);
    assert_eq!(service.filesystem.reads.get(), 0);
}

#[test]
fn propagate_retention_and_verification_failures_with_private_causes() {
    for fault in [GitFault::Verify, GitFault::Pin] {
        // Given
        let service = create_service(fault);
        let roots = BTreeSet::from([create_oid('a')]);
        // When
        let error = service.retain_snapshots(&roots).unwrap_err();
        // Then
        assert_private_document_error(&error);
    }
}

#[test]
fn propagate_corruption_and_invalid_blob_instead_of_reporting_missing() {
    for fault in [GitFault::Verify, GitFault::Read, GitFault::ReadInvalid] {
        // Given
        let service = create_service(fault);
        let tree = create_oid('a');
        let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
        // When
        let error = service
            .observe_sources(ProvenanceRequest {
                source_snapshot: Some(&tree),
                sources: &sources,
            })
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
        assert!(error.source().is_some());
        assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
    }
}

#[test]
fn propagate_unsafe_live_capture_without_exposing_private_cause() {
    // Given
    let mut service = create_service(GitFault::None);
    service.filesystem.fail = true;
    let tree = create_oid('a');
    let sources = BTreeSet::from([SourceUri::parse("a").unwrap()]);
    // When
    let error = service
        .observe_sources(ProvenanceRequest {
            source_snapshot: Some(&tree),
            sources: &sources,
        })
        .unwrap_err();
    // Then
    assert_private_document_error(&error);
}

#[test]
fn preserve_requested_revision_corruption_and_invalid_blob_classifications() {
    for (fault, expected) in [
        (GitFault::Verify, SnapshotErrorKind::InternalError),
        (GitFault::Read, SnapshotErrorKind::InternalError),
        (GitFault::ReadInvalid, SnapshotErrorKind::InvalidRequest),
    ] {
        // Given
        let service = create_service(fault);
        let tree = create_oid('a');
        let source = SourceUri::parse("a").unwrap();
        // When
        let error = service
            .compare_revision(SourceRevisionRequest {
                source_snapshot: Some(&tree),
                source: &source,
                revision: &tree,
            })
            .unwrap_err();
        // Then
        assert_eq!(error.get_kind(), expected);
        assert!(!error.get_message().contains("PRIVATE_PROVENANCE_CAUSE"));
        if expected == SnapshotErrorKind::InternalError {
            assert!(error.source().is_some());
        }
    }
}

#[derive(Clone, Copy)]
enum GitFault {
    None,
    Verify,
    Read,
    ReadInvalid,
    Pin,
}
struct RecordedGit {
    states: BTreeMap<TreeOid, SnapshotState>,
    verified: RefCell<Vec<TreeOid>>,
    pins: RefCell<Vec<TreeOid>>,
    fault: GitFault,
}
struct RecordedFiles {
    reads: Cell<usize>,
    fail: bool,
}

impl SnapshotVerifier for RecordedGit {
    fn verify_snapshot(&self, tree: &TreeOid) -> Result<SnapshotState, SnapshotError> {
        self.verified.borrow_mut().push(tree.clone());
        if matches!(self.fault, GitFault::Verify) {
            return Err(create_git_failure());
        }
        Ok(self
            .states
            .get(tree)
            .copied()
            .unwrap_or(SnapshotState::Available))
    }
}
impl GitSnapshots for RecordedGit {
    fn create_tree(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        panic!("provenance must not create trees")
    }
    fn write_blob(&self, _: &[u8]) -> Result<BlobOid, SnapshotError> {
        panic!("provenance must not write blobs")
    }
    fn create_tree_from_blobs(&self, _: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        panic!("provenance must not create trees")
    }
    fn read_blob(&self, _: &TreeOid, _: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
        match self.fault {
            GitFault::Read => Err(create_git_failure()),
            GitFault::ReadInvalid => Err(SnapshotError::new(
                SnapshotErrorKind::InvalidRequest,
                "not a blob",
            )),
            _ => Ok(b"basis\n".to_vec()),
        }
    }
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        self.pins.borrow_mut().push(tree.clone());
        if matches!(self.fault, GitFault::Pin) {
            return Err(create_git_failure());
        }
        Ok(RetainedTree::new(tree.clone()))
    }
}
impl FileSystem for RecordedFiles {
    type IndexLock = ();
    fn stat_file(&self, _: &RepoPath) -> DocumentResult<Option<FileStat>> {
        panic!("provenance must not stat sources")
    }
    fn read_file(&self, _: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        self.reads.set(self.reads.get() + 1);
        if self.fail {
            return Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot capture source",
                io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
            ));
        }
        Ok(Some(CapturedFile {
            bytes: b"basis\n".to_vec(),
            stat: FileStat {
                size: 6,
                mtime_ns: 1,
                inode: 1,
                device: 1,
            },
            checked_at_ns: 1,
        }))
    }
    fn get_time_ns(&self) -> DocumentResult<i64> {
        panic!("provenance must not sample the clock")
    }
    fn lock_index(&self) -> DocumentResult<Self::IndexLock> {
        panic!("caller owns the index lock")
    }
}
fn create_service(fault: GitFault) -> ProvenanceService<RecordedGit, RecordedFiles> {
    ProvenanceService::new(
        RecordedGit {
            states: BTreeMap::new(),
            verified: RefCell::new(Vec::new()),
            pins: RefCell::new(Vec::new()),
            fault,
        },
        RecordedFiles {
            reads: Cell::new(0),
            fail: false,
        },
    )
}
fn create_oid(digit: char) -> TreeOid {
    TreeOid::parse(&digit.to_string().repeat(40)).unwrap()
}
fn create_git_failure() -> SnapshotError {
    SnapshotError::new_with_cause(
        "cannot access provenance tree",
        io::Error::other("PRIVATE_PROVENANCE_CAUSE"),
    )
}
fn assert_private_document_error(error: &DocumentError) {
    assert_eq!(error.get_kind(), DocumentErrorKind::InternalError);
    assert!(!error.to_string().contains("PRIVATE_PROVENANCE_CAUSE"));
    assert!(!format!("{error:?}").contains("PRIVATE_PROVENANCE_CAUSE"));
    let mut cause = error.source();
    while let Some(current) = cause {
        if current.to_string().contains("PRIVATE_PROVENANCE_CAUSE") {
            return;
        }
        cause = current.source();
    }
    panic!("private cause was discarded");
}
