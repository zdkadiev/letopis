use crate::service::{FileError, FileErrorKind, FileIdentity, FileLocker, FileReader, FileResult};
use std::path::Path;

use super::InspectionService;
use crate::{
    domain::{BlobOid, RepoPath, Sha256, TreeOid},
    markdown::MarkdownParser,
    service::{
        CapturedFile, CommandErrorKind, DocumentResult, ElementAddress, FileStat, GitSnapshots,
        GitWorktreeListing, Inspector, RefreshOutcome, Refresher, SnapshotEntry, SnapshotError,
        SnapshotErrorKind, SnapshotFile,
    },
};
use std::{cell::RefCell, rc::Rc};

const MARKED_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](a.md#L2-L2 \"letopis:type=external;path=a.md#L2-L2\")\n";
const MARKED_WITH_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nBody\n[source](source.txt \"letopis:type=external;path=source.txt\")\n";
const UNMARKED_SOURCE: &[u8] = b"PRIVATE_SOURCE_BYTES\n";
const MARKED_REFERER: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nReferer\n[source](a.md \"letopis:type=external;path=a.md\")\n";
const CHANGED_MARKED_SOURCE: &[u8] =
    b"<!-- letopis: {\"created_at\":1} -->\nKnowledge\n[more](second.txt \"letopis:type=external;path=second.txt\")\n";
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
    DiscoveryChange,
    DiscoveryChangeAtPin,
    UnmarkedToMarked,
    UnmarkedToInvalid,
    UnmarkedUnchanged,
    AbsentToMarked,
    AbsentUnchanged,
}

#[test]
fn inspection_filters_raw_symlinks_before_decoding_and_normalizes_safe_results() {
    use crate::service::{MockFileReader, MockGitWorktreeListing};
    // Given: raw irrelevant/private names, a symlink and a normalized duplicate.
    let mut git = MockGitWorktreeListing::new();
    git.expect_worktree_paths_get_by_ignore_mode_and_patterns()
        .returning(|_, _| {
            Ok(b"bad\xff.txt\0.letopis/bad\xff.md\0link\xff.md\0./note.md\0note.md\0".to_vec())
        });
    let mut files = MockFileReader::new();
    files
        .expect_file_get_bytes_by_path()
        .returning(|_| Ok(None));
    files
        .expect_directory_get_by_path()
        .returning(|_| Ok(Some(std::fs::File::open(".").unwrap().into())));
    files.expect_symlink_is_exist().returning(|path| {
        use std::os::unix::ffi::OsStrExt;
        match path.as_os_str().as_bytes() {
            b"link\xff.md" => Ok(true),
            b"note.md" => Ok(false),
            other => panic!("unexpected raw probe: {other:?}"),
        }
    });
    let service = InspectionService::new((), git, files, ());
    // When: inspection selects candidates before capture.
    let paths = service.get_candidate_observation().unwrap().paths;
    // Then: invalid-byte symlinks and irrelevant names are excluded before decoding.
    assert_eq!(paths, [RepoPath::parse("note.md").unwrap()]);
}

#[test]
fn inspection_preserves_listing_error_context_and_cause() {
    use crate::service::{GitError, MockFileReader, MockGitWorktreeListing};
    // Given: an independent Git enumeration failure and no permitted filesystem calls.
    let mut git = MockGitWorktreeListing::new();
    git.expect_worktree_paths_get_by_ignore_mode_and_patterns()
        .returning(|_, _| Err(GitError::new(std::io::Error::other("private cause"))));
    let mut files = MockFileReader::new();
    files.expect_directory_get_by_path().returning(|_| Ok(None));
    files
        .expect_file_get_bytes_by_path()
        .returning(|_| Ok(None));
    let service = InspectionService::new((), git, files, ());
    // When: the selection error takes inspection's document-error mapping.
    let error = super::map_document_error_to_command_error(
        service.get_candidate_observation().unwrap_err(),
    );
    // Then: the public capture diagnostic and internal cause remain distinct.
    assert_eq!(error.get_kind(), CommandErrorKind::InternalError);
    assert_eq!(error.to_string(), "cannot capture current elements");
    assert!(std::error::Error::source(&error).is_some());
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
fn inspect_rejects_discovery_policy_changed_during_capture() {
    // Given: policy becomes exclusionary only after a knowledge file is captured.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::DiscoveryChange),
        RecordingFiles(events.clone(), Scenario::DiscoveryChange),
        MarkdownParser::new(),
    );
    // When: inspection would return a file its author excluded during capture.
    let result = service.inspect(&[ElementAddress::parse("a.md").unwrap()]);
    // Then: no successful snapshot can escape the changed discovery policy.
    assert_eq!(
        result.unwrap_err().get_kind(),
        CommandErrorKind::InternalError
    );
    assert!(!events.borrow().iter().any(|event| event == "pin"));
}

#[test]
fn inspect_rejects_discovery_policy_changed_during_pin() {
    // Given: policy changes while the snapshot's strong pin runs.
    let events = Rc::new(RefCell::new(Vec::new()));
    let mut service = InspectionService::new(
        RecordingDocuments(events.clone()),
        RecordingGit(events.clone(), Scenario::DiscoveryChangeAtPin),
        RecordingFiles(events.clone(), Scenario::DiscoveryChangeAtPin),
        MarkdownParser::new(),
    );
    // When: the final snapshot pin completes after the author excludes a captured owner.
    let result = service.inspect(&[ElementAddress::parse("a.md").unwrap()]);
    // Then: pin execution cannot bypass the final policy observation.
    assert_eq!(
        result.unwrap_err().get_kind(),
        CommandErrorKind::InternalError
    );
    assert!(events.borrow().iter().any(|event| event == "pin"));
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
    fn refresh_verified(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        panic!("routine reads must not request a full audit")
    }

    fn reindex(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        panic!("this scenario must not trigger a full reindex")
    }

    fn refresh(&mut self, _paths: &[RepoPath]) -> DocumentResult<RefreshOutcome> {
        self.0.borrow_mut().push("refresh".into());
        Ok(RefreshOutcome {
            revision: 1,
            is_published: false,
            files_updated: 0,
            files_removed: 0,
            invalid_files_updated: 0,
        })
    }
}

impl GitWorktreeListing for RecordingGit {
    fn worktree_paths_get_by_ignore_mode_and_patterns(
        &self,
        _: crate::service::GitIgnoreMode,
        _: &[u8],
    ) -> crate::service::GitResult<Vec<u8>> {
        self.0.borrow_mut().push("list".into());
        let paths = if matches!(self.1, Scenario::Overlap | Scenario::SourceIoFailure) {
            &['a'][..]
        } else {
            &[
                'a',
                if self.1.is_candidate_source_used() {
                    'z'
                } else {
                    'b'
                },
            ]
        };
        Ok(paths
            .iter()
            .map(|letter| format!("{letter}.md"))
            .flat_map(|path| path.bytes().chain(std::iter::once(0)).collect::<Vec<_>>())
            .collect())
    }
}

impl GitSnapshots for RecordingGit {
    fn ref_update_by_oid(&self, _: &TreeOid) -> Result<(), SnapshotError> {
        panic!("inspection must not retain author roots")
    }

    fn tree_create(&self, _: &[SnapshotFile<'_>]) -> Result<TreeOid, SnapshotError> {
        panic!("capture must not retain bytes for tree_create")
    }

    fn blob_write(&self, bytes: &[u8]) -> Result<BlobOid, SnapshotError> {
        let name = if self.1.is_candidate_source_used() {
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

    fn tree_assemble(&self, entries: &[SnapshotEntry]) -> Result<TreeOid, SnapshotError> {
        if self.1.is_candidate_source_used() {
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

    fn blob_get_by_tree_oid_and_path(
        &self,
        _: &TreeOid,
        path: &RepoPath,
    ) -> Result<Vec<u8>, SnapshotError> {
        assert_eq!(self.1, Scenario::Overlap);
        self.0
            .borrow_mut()
            .push(format!("read blob {}", path.as_str()));
        Ok(MARKED_SOURCE.to_vec())
    }

    fn tree_sync_by_oid(&self, tree: &TreeOid) -> Result<(), SnapshotError> {
        assert_eq!(tree.as_str(), "c".repeat(40));
        self.0.borrow_mut().push("pin".into());
        if self.1 == Scenario::PinFailure {
            return Err(SnapshotError::new(
                SnapshotErrorKind::InternalError,
                "pin failed",
            ));
        }
        Ok(())
    }
}

impl FileReader for RecordingFiles {
    fn symlink_is_exist(&self, _: &std::path::Path) -> FileResult<bool> {
        Ok(false)
    }

    fn file_get_stat_by_path(&self, _: &std::path::Path) -> FileResult<Option<FileStat>> {
        panic!("capture uses verified reads")
    }

    fn file_get_bytes_by_path(&self, path: &std::path::Path) -> FileResult<Option<Vec<u8>>> {
        assert!(matches!(
            path.to_str(),
            Some(".letopis/config.json" | ".letopis/ignore")
        ));
        let changed = match self.1 {
            Scenario::DiscoveryChange => self.0.borrow().iter().any(|event| event == "read a.md"),
            Scenario::DiscoveryChangeAtPin => self.0.borrow().iter().any(|event| event == "pin"),
            _ => false,
        };
        if changed && path == Path::new(".letopis/ignore") {
            return Ok(Some(b"a.md\n".to_vec()));
        }
        Ok(None)
    }

    fn file_get_by_path(&self, path: &std::path::Path) -> FileResult<Option<CapturedFile>> {
        if self.1 == Scenario::SourceIoFailure && path.to_str().unwrap() == "source.txt" {
            self.0.borrow_mut().push("read source.txt".into());
            return Err(FileError::new_with_cause(
                FileErrorKind::Io,
                std::io::Error::other("PRIVATE_SOURCE_BYTES"),
            ));
        }
        let read_again = path.to_str().unwrap() == "a.md"
            && self.0.borrow().iter().any(|event| event == "read a.md");
        self.0
            .borrow_mut()
            .push(format!("read {}", path.to_str().unwrap()));
        if matches!(self.1, Scenario::AbsentToMarked | Scenario::AbsentUnchanged)
            && (!read_again || self.1 == Scenario::AbsentUnchanged)
            && path.to_str().unwrap() == "a.md"
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
        } else if self.1.is_candidate_source_used() && path.to_str().unwrap() == "a.md" {
            UNMARKED_SOURCE.to_vec()
        } else if self.1.is_candidate_source_used() {
            MARKED_REFERER.to_vec()
        } else if self.1 == Scenario::Overlap {
            MARKED_SOURCE.to_vec()
        } else if self.1 == Scenario::SourceIoFailure {
            MARKED_WITH_SOURCE.to_vec()
        } else {
            format!(
                "<!-- letopis: {{\"created_at\":1}} -->\n{}",
                &path.to_str().unwrap()[..1]
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
        Ok(Some(std::fs::File::open(".").unwrap().into()))
    }

    fn lock_get_owner_by_path(
        &self,
        _: &std::path::Path,
    ) -> FileResult<Option<std::num::NonZeroU32>> {
        panic!("unexpected lock_get_owner_by_path")
    }
}

impl FileLocker for RecordingFiles {
    type Lock = ();

    fn lock_acquire_by_path(&self, _path: &std::path::Path) -> FileResult<()> {
        panic!("capture must not own publication lock")
    }

    fn lock_try_acquire_by_path(&self, _: &std::path::Path) -> FileResult<Option<Self::Lock>> {
        panic!("unexpected lock probe")
    }
}

impl Scenario {
    fn is_candidate_source_used(self) -> bool {
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
