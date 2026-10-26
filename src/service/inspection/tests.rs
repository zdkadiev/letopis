use super::InspectionService;
use crate::{
    domain::{BlobOid, RepoPath, Sha256, TreeOid},
    markdown::MarkdownParser,
    service::{
        CapturedFile, CommandErrorKind, CorpusLister, DocumentError, DocumentErrorKind,
        DocumentResult, ElementAddress, FileStat, FileSystem, GitSnapshots, Inspector,
        RefreshOutcome, Refresher, RetainedTree, SnapshotEntry, SnapshotError, SnapshotErrorKind,
        SnapshotFile,
    },
};
use std::{cell::RefCell, rc::Rc};

const MARKED_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](a.md#L2-L2 \"letopis:type=external\")\n";
const MARKED_WITH_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](source.txt \"letopis:type=external\")\n";
const UNMARKED_SOURCE: &[u8] = b"PRIVATE_SOURCE_BYTES\n";
const MARKED_REFERER: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nReferer\n[source](a.md \"letopis:type=external\")\n";
const CHANGED_MARKED_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nKnowledge\n[more](second.txt \"letopis:type=external\")\n";
type Events = Rc<RefCell<Vec<String>>>;
struct RecordingDocuments(Events);
struct RecordingGit(Events, Scenario);
struct RecordingFiles(Events, Scenario);
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Plain,
    Overlap,
    PinFailure,
    SourceIoFailure,
    UnmarkedToMarked,
    UnmarkedToInvalid,
    UnmarkedUnchanged,
    AbsentToMarked,
    AbsentUnchanged,
}
#[test]
fn inspect_rejects_unmarked_candidate_changed_to_marked_source() {
    // Given / When / Then: the helper prepares, exercises, and checks this scenario.
    assert_changed_candidate_is_rejected(Scenario::UnmarkedToMarked);
}

#[test]
fn inspect_rejects_unmarked_candidate_changed_to_invalid_source() {
    // Given / When / Then: the helper prepares, exercises, and checks this scenario.
    assert_changed_candidate_is_rejected(Scenario::UnmarkedToInvalid);
}

#[test]
fn inspect_rejects_absent_candidate_changed_to_marked_source() {
    // Given / When / Then: the helper prepares, exercises, and checks this scenario.
    assert_changed_candidate_is_rejected(Scenario::AbsentToMarked);
}

#[test]
fn inspect_accepts_unchanged_unmarked_candidate_as_source() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::UnmarkedUnchanged),
        RecordingFiles(events.clone(), Scenario::UnmarkedUnchanged),
        MarkdownParser::new(),
    );
    // When
    let result = service
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap();
    // Then
    assert_eq!(result.elements.len(), 1);
    assert_eq!(
        result.elements["a.md"].hash,
        Some(
            Sha256::parse("fd800c29fbc6251cd9c3ba805b5113832e21ae5e0e08853ddcbc53ea64339741")
                .unwrap()
        )
    );
    assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
}

#[test]
fn inspect_keeps_absent_candidate_source_without_a_hash() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::AbsentUnchanged),
        RecordingFiles(events.clone(), Scenario::AbsentUnchanged),
        MarkdownParser::new(),
    );
    // When
    let result = service
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap();
    // Then
    assert_eq!(result.elements.len(), 1);
    assert_eq!(result.elements["a.md"].hash, None);
    assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
}
#[test]
fn capture_writes_each_blob_before_reading_the_next_file() {
    // Given: sequencing at the I/O boundaries proves bounded capture.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::Plain),
        RecordingFiles(events.clone(), Scenario::Plain),
        MarkdownParser::new(),
    );
    // When
    let inspection = service
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap();
    // Then: unrequested marked files must also belong to the tree.
    assert_eq!(inspection.elements.len(), 1);
    assert_eq!(
        *events.borrow(),
        [
            "refresh",
            "list",
            "read a.md",
            "write a",
            "read b.md",
            "write b",
            "assemble",
            "pin"
        ]
    );
}

#[test]
fn inspect_reuses_captured_marked_source_after_live_edit() {
    // Given: the filesystem would return different bytes on a second read.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::Overlap),
        RecordingFiles(events.clone(), Scenario::Overlap),
        MarkdownParser::new(),
    );
    // When
    let result = service
        .inspect(&[ElementAddress::parse("a.md#L2-L2").unwrap()])
        .unwrap();
    // Then: one captured blob serves both knowledge and source identities.
    assert_eq!(result.elements.len(), 1);
    assert_eq!(
        result.elements["a.md#L2-L2"].hash,
        Some(
            Sha256::parse("421dc617d921c24f41441973d8476605718a14a5c2228b8344cc1d6d816e8d39")
                .unwrap()
        )
    );
    assert_eq!(
        *events.borrow(),
        [
            "refresh",
            "list",
            "read a.md",
            "write a",
            "assemble",
            "read blob a.md",
            "assemble",
            "pin"
        ]
    );
}

#[test]
fn inspect_fails_when_final_pin_fails() {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::PinFailure),
        RecordingFiles(events.clone(), Scenario::PinFailure),
        MarkdownParser::new(),
    );
    // When
    let error = service
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap_err();
    // Then
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(events.borrow().last().map(String::as_str), Some("pin"));
}

#[test]
fn inspect_fails_safely_when_declared_source_read_has_io_error() {
    // Given: corpus capture succeeds, then the source read fails with a private cause.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::SourceIoFailure),
        RecordingFiles(events.clone(), Scenario::SourceIoFailure),
        MarkdownParser::new(),
    );
    // When
    let error = service
        .inspect(&[
            ElementAddress::parse("a.md").unwrap(),
            ElementAddress::parse("source.txt").unwrap(),
        ])
        .unwrap_err();
    // Then: failure occurs before final assembly/pin, with no payload in diagnostics.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.get_message(), "cannot capture current elements");
    assert!(!error.to_string().contains("PRIVATE_SOURCE_BYTES"));
    assert_eq!(
        *events.borrow(),
        [
            "refresh",
            "list",
            "read a.md",
            "write a",
            "assemble",
            "read source.txt"
        ]
    );
}

fn assert_changed_candidate_is_rejected(scenario: Scenario) {
    // Given
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), scenario),
        RecordingFiles(events.clone(), scenario),
        MarkdownParser::new(),
    );
    // When
    let error = service
        .inspect(&[ElementAddress::parse("a.md").unwrap()])
        .unwrap_err();
    // Then: a changed candidate must never produce a partial retained tree.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.get_message(), "cannot capture current elements");
    assert!(!error.to_string().contains("PRIVATE_SOURCE_BYTES"));
    assert_eq!(
        events.borrow().last().map(String::as_str),
        Some("read a.md")
    );
}

impl Refresher for RecordingDocuments {
    fn reindex(&mut self) -> DocumentResult<RefreshOutcome> {
        panic!("this scenario must not trigger a full reindex")
    }

    fn refresh(&mut self) -> DocumentResult<RefreshOutcome> {
        self.0.borrow_mut().push("refresh".into());
        Ok(RefreshOutcome {
            revision: 1,
            published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    }
}
impl CorpusLister for RecordingGit {
    fn list_candidates(&self) -> DocumentResult<Vec<RepoPath>> {
        self.0.borrow_mut().push("list".into());
        let paths = if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure) {
            &['a'][..]
        } else {
            &[
                'a',
                if self.1.uses_candidate_source() {
                    'z'
                } else {
                    'b'
                },
            ]
        };
        Ok(paths
            .iter()
            .map(|letter| format!("{letter}.md"))
            .map(|path| RepoPath::parse(&path).unwrap())
            .collect())
    }
}
impl GitSnapshots for RecordingGit {
    fn create_tree(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        panic!("capture must not retain bytes for create_tree")
    }
    fn write_blob(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        let name = if self.1.uses_candidate_source() {
            if bytes == MARKED_REFERER { "d" } else { "a" }
        } else if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure)
            || bytes.ends_with(b"a")
        {
            "a"
        } else {
            "b"
        };
        self.0.borrow_mut().push(format!("write {name}"));
        Ok(BlobOid::parse(&name.repeat(40)).unwrap())
    }
    fn create_tree_from_blobs(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        if self.1.uses_candidate_source() {
            assert_eq!(
                entries.last().map(|entry| entry.path.as_str()),
                Some("z.md")
            );
            self.0.borrow_mut().push("assemble".into());
            return Ok(TreeOid::parse(&"c".repeat(40)).unwrap());
        }
        if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure) {
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path.as_str(), "a.md");
            self.0.borrow_mut().push("assemble".into());
            return Ok(TreeOid::parse(&"c".repeat(40)).unwrap());
        }
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.path.as_str())
                .collect::<Vec<_>>(),
            ["a.md", "b.md"]
        );
        assert_eq!(
            entries
                .iter()
                .map(|entry| entry.oid.as_str())
                .collect::<Vec<_>>(),
            ["a".repeat(40), "b".repeat(40)]
        );
        self.0.borrow_mut().push("assemble".into());
        Ok(TreeOid::parse(&"c".repeat(40)).unwrap())
    }
    fn read_blob(&self, _: &TreeOid, path: &RepoPath) -> Result<Vec<u8>, SnapshotError> {
        assert_eq!(self.1, Scenario::Overlap);
        self.0
            .borrow_mut()
            .push(format!("read blob {}", path.as_str()));
        Ok(MARKED_SOURCE.to_vec())
    }
    fn pin(&self, tree: &TreeOid) -> Result<RetainedTree, SnapshotError> {
        assert_eq!(tree.as_str(), "c".repeat(40));
        self.0.borrow_mut().push("pin".into());
        if self.1 == Scenario::PinFailure {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "pin failed",
            ));
        }
        Ok(RetainedTree::new(tree.clone()))
    }
}
impl FileSystem for RecordingFiles {
    type IndexLock = ();
    fn stat_file(&self, _: &RepoPath) -> DocumentResult<Option<FileStat>> {
        panic!("capture uses verified reads")
    }
    fn read_file(&self, path: &RepoPath) -> DocumentResult<Option<CapturedFile>> {
        if self.1 == Scenario::SourceIoFailure && path.as_str() == "source.txt" {
            self.0.borrow_mut().push("read source.txt".into());
            return Err(DocumentError::new_with_cause(
                DocumentErrorKind::InternalError,
                "cannot read source file",
                std::io::Error::other("PRIVATE_SOURCE_BYTES"),
            ));
        }
        let read_again =
            path.as_str() == "a.md" && self.0.borrow().iter().any(|event| event == "read a.md");
        self.0.borrow_mut().push(format!("read {}", path.as_str()));
        if matches!(self.1, Scenario::AbsentToMarked | Scenario::AbsentUnchanged)
            && (!read_again || self.1 == Scenario::AbsentUnchanged)
            && path.as_str() == "a.md"
        {
            return Ok(None);
        }
        let bytes = if read_again {
            match self.1 {
                Scenario::UnmarkedToMarked | Scenario::AbsentToMarked => {
                    CHANGED_MARKED_SOURCE.to_vec()
                }
                Scenario::UnmarkedToInvalid => {
                    b"<!-- letopis: {broken} -->\nPRIVATE_SOURCE_BYTES\n".to_vec()
                }
                Scenario::UnmarkedUnchanged => UNMARKED_SOURCE.to_vec(),
                _ => b"live edited after capture\n".to_vec(),
            }
        } else if self.1.uses_candidate_source() && path.as_str() == "a.md" {
            UNMARKED_SOURCE.to_vec()
        } else if self.1.uses_candidate_source() {
            MARKED_REFERER.to_vec()
        } else if self.1 == Scenario::Overlap {
            MARKED_SOURCE.to_vec()
        } else if self.1 == Scenario::SourceIoFailure {
            MARKED_WITH_SOURCE.to_vec()
        } else {
            format!(
                "<!-- letopis: {{\"created_at\":1}} -->\n{}",
                &path.as_str()[..1]
            )
            .into_bytes()
        };
        Ok(Some(CapturedFile {
            stat: FileStat {
                size: bytes.len() as u64,
                mtime_ns: 1,
                inode: 1,
                device: 1,
            },
            checked_at_ns: 1,
            bytes,
        }))
    }
    fn get_time_ns(&self) -> DocumentResult<i64> {
        panic!("capture must not own publication timing")
    }
    fn lock_index(&self) -> DocumentResult<()> {
        panic!("capture must not own publication lock")
    }
}

impl Scenario {
    fn uses_candidate_source(self) -> bool {
        matches!(
            self,
            Self::UnmarkedToMarked
                | Self::UnmarkedToInvalid
                | Self::UnmarkedUnchanged
                | Self::AbsentToMarked
                | Self::AbsentUnchanged
        )
    }
}
